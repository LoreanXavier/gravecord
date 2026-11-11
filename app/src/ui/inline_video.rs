//! Silent GIFV playback: modules 265705/619517, with bounded native frame storage.
use super::{
    Root,
    nameplate_animation::{Clip, Playback},
};
use crate::net::Request;
use dc_state::{Embed, Id};
use gpui::{
    AnyElement, Context, Corners, IntoElement, RenderImage, SharedString, Window, canvas, div,
    prelude::*, px,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Instant,
};

pub(super) const PREFIX: &str = "gifv:";
const TOTAL_DECODED: usize = 48 * 1024 * 1024;
const TOTAL_ENCODED: usize = 16 * 1024 * 1024;
/// What a decode that is still running counts for against `TOTAL_DECODED` (its real size is only known when it
/// ends, and at most `MAX_DECODED`).
const DECODE_RESERVE: usize = 16 * 1024 * 1024;
/// A download that fails is tried once more the next time its embed comes into view.
const FETCH_ATTEMPTS: u8 = 2;
type PaintedFrame = (Arc<RenderImage>, Option<Arc<RenderImage>>);
type DecodeJob = (String, Arc<Vec<u8>>, (u32, u32));

struct Entry {
    url: String,
    size: (u32, u32),
    epoch: u64,
    fetched: bool,
    encoded: Option<Arc<Vec<u8>>>,
    decoding: bool,
    failed: bool,
    /// Downloads that failed, and whether the next view may try again.
    fetch_failures: u8,
    retry_on_view: bool,
    /// The encoded bytes did not fit under `TOTAL_ENCODED`: the download repeats when room appears.
    waiting: bool,
    clip: Option<Arc<Clip>>,
    bytes: usize,
    clock: Playback,
    frame: Option<(usize, Arc<RenderImage>)>,
}

pub(super) struct Cache {
    entries: HashMap<String, Entry>,
    seen: HashMap<String, bool>,
    visible: HashMap<String, bool>,
    epoch: u64,
    start: Instant,
    hovered: Option<String>,
    deadline: Option<Instant>,
    generation: u64,
    retired: u64,
    /// Encoded or decoded memory was released since the last `prepare`.
    room: bool,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            seen: HashMap::new(),
            visible: HashMap::new(),
            epoch: 0,
            start: Instant::now(),
            hovered: None,
            deadline: None,
            generation: 0,
            retired: 0,
            room: false,
        }
    }
}

#[derive(Default)]
struct Preparation {
    fetch: Vec<(String, String)>,
    decode: Vec<DecodeJob>,
    retired: Vec<Arc<RenderImage>>,
    deadline: Option<Instant>,
}

fn should_play(visible: bool, focused: bool, autoplay: bool, hovered: bool) -> bool {
    visible && focused && (autoplay || hovered)
}

fn supported_dimensions(width: u32, height: u32) -> bool {
    (width <= 6016 && height <= 3384) || (width <= 3384 && height <= 6016)
}

impl Cache {
    fn encoded_used(&self) -> usize {
        self.entries
            .values()
            .filter_map(|entry| entry.encoded.as_ref())
            .map(|bytes| bytes.len())
            .sum()
    }

    fn decoded_used(&self) -> usize {
        self.entries.values().map(|entry| entry.bytes).sum()
    }

    fn playing(&self, key: &str) -> bool {
        self.visible.get(key) == Some(&true) || self.seen.get(key) == Some(&true)
    }

    /// Drops the downloads of clips that are not on screen until `needed` more bytes fit.
    fn evict_encoded(&mut self, keep: &str, needed: usize) {
        let mut used = self.encoded_used();
        let keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                key.as_str() != keep
                    && entry.encoded.is_some()
                    && !entry.decoding
                    && !self.playing(key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if used.saturating_add(needed) <= TOTAL_ENCODED {
                break;
            }
            if let Some(entry) = self.entries.get_mut(&key)
                && let Some(bytes) = entry.encoded.take()
            {
                used -= bytes.len();
                entry.fetched = false;
                self.room = true;
            }
        }
    }

    /// Drops decoded clips that are not on screen until `needed` more bytes fit; their frames are returned for
    /// the caller to release.
    fn evict_clips(&mut self, keep: &str, needed: usize) -> Vec<Arc<RenderImage>> {
        let mut retired = Vec::new();
        let mut used = self.decoded_used();
        let keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                key.as_str() != keep && entry.clip.is_some() && !self.playing(key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if used.saturating_add(needed) <= TOTAL_DECODED {
                break;
            }
            if let Some(entry) = self.entries.get_mut(&key) {
                used -= entry.bytes;
                entry.clip = None;
                entry.bytes = 0;
                if let Some((_, image)) = entry.frame.take() {
                    retired.push(image);
                }
                self.room = true;
            }
        }
        self.retired += retired.len() as u64;
        retired
    }

    fn fetched(&mut self, key: &str, bytes: Vec<u8>) {
        if !self.entries.contains_key(key) {
            return;
        }
        if bytes.len() > crate::video_clip::MAX_ENCODED {
            // Too big to ever play: the poster stays.
            if let Some(entry) = self.entries.get_mut(key) {
                entry.failed = true;
            }
            return;
        }
        self.evict_encoded(key, bytes.len());
        let fits = self.encoded_used().saturating_add(bytes.len()) <= TOTAL_ENCODED;
        if let Some(entry) = self.entries.get_mut(key) {
            if fits {
                entry.encoded = Some(Arc::new(bytes));
                entry.waiting = false;
            } else {
                // Busy channel: this clip waits for room and downloads again then.
                entry.fetched = false;
                entry.waiting = true;
            }
        }
    }

    /// A download that did not arrive: the poster stays, and the next view of the embed asks once more.
    fn fetch_failed(&mut self, key: &str) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.fetch_failures += 1;
            if entry.fetch_failures >= FETCH_ATTEMPTS {
                entry.failed = true;
            } else {
                entry.retry_on_view = true;
            }
        }
    }

    fn register(&mut self, key: &str, url: &str, size: (u32, u32)) {
        self.entries
            .entry(key.to_owned())
            .or_insert_with(|| Entry {
                url: url.to_owned(),
                size,
                epoch: self.epoch,
                fetched: false,
                encoded: None,
                decoding: false,
                failed: false,
                fetch_failures: 0,
                retry_on_view: false,
                waiting: false,
                clip: None,
                bytes: 0,
                clock: Playback::default(),
                frame: None,
            })
            .epoch = self.epoch;
    }

    fn prepare(&mut self, focused: bool, autoplay: bool) -> Preparation {
        self.epoch += 1;
        let mut out = Preparation::default();
        self.visible = std::mem::take(&mut self.seen);
        self.entries.retain(|_, entry| {
            if entry.epoch + 1 >= self.epoch {
                true
            } else {
                if let Some((_, image)) = entry.frame.take() {
                    out.retired.push(image);
                }
                false
            }
        });
        if std::mem::take(&mut self.room) {
            for entry in self.entries.values_mut() {
                entry.waiting = false;
            }
        }
        let mut decoding = self.entries.values().filter(|entry| entry.decoding).count();
        let mut reserved = self
            .entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            + decoding * DECODE_RESERVE;
        let now = self.start.elapsed();
        for (key, entry) in &mut self.entries {
            let visible = self.visible.get(key);
            let play = should_play(
                visible == Some(&true),
                focused,
                autoplay,
                self.hovered.as_ref() == Some(key),
            );
            if let Some(clip) = &entry.clip {
                entry.clock.tick(now, play, true, clip.duration);
                if entry.clock.playing {
                    let deadline = Instant::now() + clip.next_delay(entry.clock.elapsed);
                    out.deadline = Some(out.deadline.map_or(deadline, |old| old.min(deadline)));
                }
            }
            if visible.is_none() {
                if std::mem::take(&mut entry.retry_on_view) {
                    entry.fetched = false;
                }
                if entry.bytes > 0 {
                    reserved -= entry.bytes;
                    self.room = true;
                }
                entry.clip = None;
                entry.bytes = 0;
                if let Some((_, image)) = entry.frame.take() {
                    out.retired.push(image);
                }
            }
        }
        // Started only once every clip that left the screen has given its room back: the map's order must not
        // decide whether a waiting clip starts on this frame or the next.
        for (key, entry) in &mut self.entries {
            let play = should_play(
                self.visible.get(key) == Some(&true),
                focused,
                autoplay,
                self.hovered.as_ref() == Some(key),
            );
            if play && !entry.failed {
                if !entry.fetched && !entry.waiting {
                    entry.fetched = true;
                    out.fetch.push((key.clone(), entry.url.clone()));
                }
                if entry.clip.is_none()
                    && !entry.decoding
                    && !entry.waiting
                    && decoding < 2
                    && reserved + DECODE_RESERVE <= TOTAL_DECODED
                    && let Some(encoded) = &entry.encoded
                {
                    entry.decoding = true;
                    decoding += 1;
                    reserved += DECODE_RESERVE;
                    out.decode.push((key.clone(), encoded.clone(), entry.size));
                }
            }
        }
        self.retired += out.retired.len() as u64;
        out
    }

    /// A finished decode. Clips that are not on screen make room first; a clip that still does not fit waits for
    /// room (its download stays), and only one that could never fit fails. The frames to release come back.
    fn decoded(
        &mut self,
        key: &str,
        decoded: Result<crate::video_clip::Clip, String>,
    ) -> Vec<Arc<RenderImage>> {
        let mut retired = Vec::new();
        let Some(entry) = self.entries.get_mut(key) else {
            return retired;
        };
        entry.decoding = false;
        match decoded {
            Ok(clip) if clip.bytes <= TOTAL_DECODED => {
                retired = self.evict_clips(key, clip.bytes);
                let fits = self.decoded_used().saturating_add(clip.bytes) <= TOTAL_DECODED;
                if let Some(entry) = self.entries.get_mut(key) {
                    if fits {
                        entry.bytes = clip.bytes;
                        entry.clip = Clip::from_frames(clip.frames);
                        entry.failed = entry.clip.is_none();
                    } else {
                        // Decoded again once something is released.
                        entry.waiting = true;
                    }
                }
            }
            _ => entry.failed = true,
        }
        retired
    }

    fn frame(&mut self, key: &str) -> Option<PaintedFrame> {
        let entry = self.entries.get_mut(key)?;
        let clip = entry.clip.as_ref()?;
        if !entry.clock.started {
            return None;
        }
        let index = clip.frame_at(entry.clock.elapsed);
        if let Some((known, image)) = &entry.frame
            && *known == index
        {
            return Some((image.clone(), None));
        }
        let mut buffer = clip.frames[index].as_ref().clone();
        for pixel in buffer.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        let retired = entry
            .frame
            .replace((index, image.clone()))
            .map(|(_, image)| image);
        self.retired += u64::from(retired.is_some());
        Some((image, retired))
    }
}

fn key(message: Id, index: usize, url: &str) -> String {
    let mut hash = std::hash::DefaultHasher::new();
    url.hash(&mut hash);
    format!("{PREFIX}{message}:{index}:{:x}", hash.finish())
}

impl Root {
    pub(super) fn inline_video_fetched(&mut self, key: &str, bytes: Vec<u8>) {
        self.inline_videos.borrow_mut().fetched(key, bytes);
    }

    pub(super) fn inline_video_failed(&mut self, key: &str) {
        self.inline_videos.borrow_mut().fetch_failed(key);
    }

    pub(super) fn prepare_inline_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let autoplay = self.pref_on("animate_gifs", true) && !crate::ui::motion::reduced();
        let work = self
            .inline_videos
            .borrow_mut()
            .prepare(window.is_window_active(), autoplay);
        for image in work.retired {
            cx.drop_image(image, Some(window));
        }
        for (key, url) in work.fetch {
            let _ = self.requests.send(Request::Image { key, url });
        }
        for (key, bytes, size) in work.decode {
            cx.spawn(async move |this, cx| {
                let decoded = cx
                    .background_executor()
                    .spawn(async move { crate::video_clip::decode(&bytes, size) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let retired = this.inline_videos.borrow_mut().decoded(&key, decoded);
                    for image in retired {
                        cx.drop_image(image, None);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        let scheduled = {
            let mut cache = self.inline_videos.borrow_mut();
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
                    let mut cache = this.inline_videos.borrow_mut();
                    if cache.generation == generation {
                        cache.deadline = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    pub(super) fn inline_video(
        &self,
        message: Id,
        index: usize,
        embed: &Embed,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if embed.kind != "gifv" {
            return None;
        }
        let video = embed.video.as_ref()?;
        if !supported_dimensions(video.width, video.height) {
            return None;
        }
        let url = video.proxy_url.as_deref().unwrap_or(&video.url);
        let (width, height) = Embed::fitted(embed.image.as_ref()?);
        let key = key(message, index, url);
        self.inline_videos.borrow_mut().register(
            &key,
            url,
            (width.round() as u32, height.round() as u32),
        );
        let cache = self.inline_videos.clone();
        let hovered = key.clone();
        let root = cx.entity().downgrade();
        Some(
            div()
                .id(SharedString::from(format!("hover-{key}")))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                    let mut cache = this.inline_videos.borrow_mut();
                    if *inside {
                        cache.hovered = Some(hovered.clone());
                    } else if cache.hovered.as_ref() == Some(&hovered) {
                        cache.hovered = None;
                    }
                    cx.notify();
                }))
                .child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, cx| {
                            let area = f32::from(bounds.size.width) * f32::from(bounds.size.height);
                            if area <= 0. {
                                return;
                            }
                            let visible = bounds.intersect(&window.content_mask().bounds);
                            let visible_area = f32::from(visible.size.width).max(0.)
                                * f32::from(visible.size.height).max(0.);
                            if visible_area <= 0. {
                                return;
                            }
                            // LazyImage's observer uses a 60% intersection threshold (619517).
                            let playable = visible_area / area >= 0.6;
                            let notify = {
                                let mut cache = cache.borrow_mut();
                                cache.seen.insert(key.clone(), playable);
                                let epoch = cache.epoch;
                                if let Some(entry) = cache.entries.get_mut(&key) {
                                    entry.epoch = epoch;
                                }
                                cache.visible.get(&key) != Some(&playable)
                            };
                            if notify {
                                let root = root.clone();
                                cx.defer(move |cx| {
                                    let _ = root.update(cx, |_, cx| cx.notify());
                                });
                            }
                            if let Some((image, retired)) = cache.borrow_mut().frame(&key) {
                                if let Some(retired) = retired {
                                    cx.drop_image(retired, Some(window));
                                }
                                let _ = window.paint_image(
                                    bounds,
                                    bounds,
                                    Corners::all(px(4.)),
                                    image,
                                    0,
                                    false,
                                );
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        )
    }

    pub(super) fn release_inline_videos(&mut self, cx: &mut gpui::App) {
        let images: Vec<_> = self
            .inline_videos
            .borrow_mut()
            .entries
            .drain()
            .filter_map(|(_, entry)| entry.frame.map(|(_, image)| image))
            .collect();
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }

    #[cfg(feature = "capture")]
    pub(super) fn inline_video_status(&self) -> String {
        let cache = self.inline_videos.borrow();
        let mut frames: Vec<_> = cache
            .entries
            .values()
            .filter_map(|entry| entry.frame.as_ref().map(|(index, _)| *index))
            .collect();
        frames.sort_unstable();
        format!(
            "decoded={} bytes={} playing={} frames={frames:?} retired_textures={}",
            cache
                .entries
                .values()
                .filter(|entry| entry.clip.is_some())
                .count(),
            cache
                .entries
                .values()
                .map(|entry| entry.bytes)
                .sum::<usize>(),
            cache
                .entries
                .values()
                .filter(|entry| entry.clock.playing)
                .count(),
            cache.retired
        )
    }
}

/// The downloads and decoded clips this cache holds, for the perf harness.
#[cfg(feature = "capture")]
impl Cache {
    pub(super) fn perf_size(&self) -> crate::perf::Size {
        let mut size = crate::perf::Size::default();
        size.add(
            self.entries.len(),
            self.encoded_used() + self.decoded_used(),
        );
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn clip() -> crate::video_clip::Clip {
        crate::video_clip::Clip {
            frames: vec![(Duration::from_secs(1), image::RgbaImage::new(2, 2))],
            bytes: 16,
        }
    }

    #[test]
    fn a_gifv_plays_by_setting_focus_and_hover() {
        // Module 619517: on screen and focused, and either "Play GIFs when Discord is focused" or the pointer.
        for visible in [false, true] {
            for focused in [false, true] {
                for autoplay in [false, true] {
                    for hovered in [false, true] {
                        assert_eq!(
                            should_play(visible, focused, autoplay, hovered),
                            visible && focused && (autoplay || hovered),
                            "visible={visible} focused={focused} autoplay={autoplay} hovered={hovered}"
                        );
                    }
                }
            }
        }
        // The cases that matter: setting on plays without the pointer, setting off needs it, a blurred window never.
        assert!(should_play(true, true, true, false));
        assert!(!should_play(true, true, false, false));
        assert!(should_play(true, true, false, true));
        assert!(!should_play(true, false, true, true));
    }

    #[test]
    fn gifv_fetches_lazily_holds_paused_frame_and_retires_offscreen_resources() {
        let mut cache = Cache::default();
        cache.register("clip", "https://example.com/clip.mp4", (2, 2));
        assert!(cache.prepare(true, true).fetch.is_empty());
        cache.register("clip", "https://example.com/clip.mp4", (2, 2));
        cache.seen.insert("clip".into(), true);
        assert_eq!(cache.prepare(true, true).fetch.len(), 1);
        cache.decoded("clip", Ok(clip()));
        cache.register("clip", "https://example.com/clip.mp4", (2, 2));
        cache.seen.insert("clip".into(), true);
        assert!(cache.prepare(true, true).fetch.is_empty());
        let first = cache.frame("clip").unwrap().0;
        cache.register("clip", "https://example.com/clip.mp4", (2, 2));
        cache.seen.insert("clip".into(), true);
        cache.prepare(false, true);
        assert!(!cache.entries["clip"].clock.playing);
        assert!(Arc::ptr_eq(&first, &cache.frame("clip").unwrap().0));
        cache.register("clip", "https://example.com/clip.mp4", (2, 2));
        let work = cache.prepare(true, true);
        assert_eq!(work.retired.len(), 1);
        assert!(cache.entries["clip"].clip.is_none());
        assert_eq!(cache.entries["clip"].bytes, 0);
        cache.prepare(true, true);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn gifv_decode_jobs_and_total_frame_memory_are_bounded() {
        let mut cache = Cache::default();
        for index in 0..3 {
            let key = index.to_string();
            cache.register(&key, "https://example.com/clip.mp4", (2, 2));
            cache.entries.get_mut(&key).unwrap().encoded = Some(Arc::new(vec![]));
            cache.seen.insert(key, true);
        }
        assert_eq!(cache.prepare(true, true).decode.len(), 2);
        cache.decoded("0", Ok(clip()));
        // Playing clips are kept: a clip that does not fit waits instead of failing.
        cache.seen.insert("0".into(), true);
        let mut big = clip();
        big.bytes = TOTAL_DECODED;
        cache.decoded("1", Ok(big));
        assert!(!cache.entries["1"].failed);
        assert!(cache.entries["1"].clip.is_none());
        assert!(cache.entries["1"].waiting);
        // A clip that could never fit fails.
        let mut huge = clip();
        huge.bytes = TOTAL_DECODED + 1;
        cache.decoded("1", Ok(huge));
        assert!(cache.entries["1"].failed);
    }

    #[test]
    fn gifv_decodes_in_flight_count_toward_the_cap_and_off_screen_clips_make_room() {
        let mut cache = Cache::default();
        for key in ["a", "b", "c"] {
            cache.register(key, "https://example.com/clip.mp4", (2, 2));
            cache.entries.get_mut(key).unwrap().encoded = Some(Arc::new(vec![]));
            cache.seen.insert(key.into(), true);
        }
        // Another clip already holds nearly everything: nothing new starts while it plays.
        cache.register("held", "https://example.com/clip.mp4", (2, 2));
        let held = cache.entries.get_mut("held").unwrap();
        held.bytes = TOTAL_DECODED - DECODE_RESERVE + 1;
        held.clip = Clip::from_frames(clip().frames);
        cache.seen.insert("held".into(), true);
        assert!(cache.prepare(true, true).decode.is_empty());
        // Scrolled off, its clip goes and the waiting clips start (two at a time).
        for key in ["a", "b", "c"] {
            cache.register(key, "https://example.com/clip.mp4", (2, 2));
            cache.seen.insert(key.into(), true);
        }
        cache.register("held", "https://example.com/clip.mp4", (2, 2));
        let work = cache.prepare(true, true);
        assert_eq!(work.decode.len(), 2);
        assert!(cache.entries["held"].clip.is_none());
        // A finished decode that overflows evicts clips that are off screen instead of failing.
        cache.entries.get_mut("a").unwrap().decoding = false;
        cache.entries.get_mut("b").unwrap().decoding = false;
        let mut first = clip();
        first.bytes = TOTAL_DECODED - 4;
        cache.decoded("a", Ok(first));
        cache.seen.clear();
        cache.visible.clear();
        cache.seen.insert("b".into(), true);
        let mut second = clip();
        second.bytes = 16;
        cache.decoded("b", Ok(second));
        assert!(cache.entries["a"].clip.is_none());
        assert!(cache.entries["b"].clip.is_some());
        assert!(!cache.entries["b"].failed);
    }

    #[test]
    fn gifv_downloads_wait_for_room_and_failed_ones_retry_on_the_next_view() {
        let mut cache = Cache::default();
        cache.register("old", "https://example.com/old.mp4", (2, 2));
        cache.entries.get_mut("old").unwrap().encoded = Some(Arc::new(vec![0; TOTAL_ENCODED - 10]));
        cache.seen.insert("old".into(), true);
        cache.register("new", "https://example.com/new.mp4", (2, 2));
        cache.entries.get_mut("new").unwrap().fetched = true;
        cache.fetched("new", vec![0; 100]);
        assert!(cache.entries["new"].encoded.is_none());
        assert!(cache.entries["new"].waiting && !cache.entries["new"].failed);
        // Once "old" is off screen its download makes room.
        cache.seen.clear();
        cache.visible.clear();
        cache.fetched("new", vec![0; 100]);
        assert!(cache.entries["new"].encoded.is_some());
        assert!(cache.entries["old"].encoded.is_none());
        // An oversize download shows its poster for good.
        cache.fetched("new", vec![0; crate::video_clip::MAX_ENCODED + 1]);
        assert!(cache.entries["new"].failed);
        // One failed download is tried again when the embed is next seen; the second failure is final.
        cache.register("gone", "https://example.com/gone.mp4", (2, 2));
        cache.entries.get_mut("gone").unwrap().fetched = true;
        cache.fetch_failed("gone");
        assert!(!cache.entries["gone"].failed && cache.entries["gone"].retry_on_view);
        cache.register("gone", "https://example.com/gone.mp4", (2, 2));
        cache.seen.insert("gone".into(), true);
        cache.prepare(true, true);
        assert!(
            cache.entries["gone"].fetched,
            "no retry while it stays on screen"
        );
        cache.register("gone", "https://example.com/gone.mp4", (2, 2));
        cache.prepare(true, true);
        assert!(!cache.entries["gone"].fetched);
        cache.fetch_failed("gone");
        assert!(cache.entries["gone"].failed);
    }

    #[test]
    fn gifv_only_plays_while_visible_and_focused_with_autoplay_or_hover() {
        assert!(supported_dimensions(6016, 3384));
        assert!(supported_dimensions(3384, 6016));
        assert!(!supported_dimensions(6017, 3384));
        assert!(!supported_dimensions(4000, 4000));
        assert!(should_play(true, true, true, false));
        assert!(should_play(true, true, false, true));
        assert!(!should_play(false, true, true, true));
        assert!(!should_play(true, false, true, true));
        assert!(!should_play(true, true, false, false));
        assert_ne!(
            key(1, 0, "https://example.com/a.mp4"),
            key(1, 0, "https://example.com/b.mp4")
        );
    }
}
