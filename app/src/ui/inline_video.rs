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
const TOTAL_DECODED: usize = 128 * 1024 * 1024;
const TOTAL_ENCODED: usize = 64 * 1024 * 1024;
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
    fn fetched(&mut self, key: &str, bytes: Vec<u8>) {
        let used: usize = self
            .entries
            .values()
            .filter_map(|entry| entry.encoded.as_ref())
            .map(|bytes| bytes.len())
            .sum();
        if let Some(entry) = self.entries.get_mut(key) {
            if bytes.len() <= crate::video_clip::MAX_ENCODED
                && used.saturating_add(bytes.len()) <= TOTAL_ENCODED
            {
                entry.encoded = Some(Arc::new(bytes));
            } else {
                entry.failed = true;
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
        let mut decoding = self.entries.values().filter(|entry| entry.decoding).count();
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
                entry.clip = None;
                entry.bytes = 0;
                if let Some((_, image)) = entry.frame.take() {
                    out.retired.push(image);
                }
            }
            if play && !entry.failed {
                if !entry.fetched {
                    entry.fetched = true;
                    out.fetch.push((key.clone(), entry.url.clone()));
                }
                if entry.clip.is_none()
                    && !entry.decoding
                    && decoding < 2
                    && let Some(encoded) = &entry.encoded
                {
                    entry.decoding = true;
                    decoding += 1;
                    out.decode.push((key.clone(), encoded.clone(), entry.size));
                }
            }
        }
        self.retired += out.retired.len() as u64;
        out
    }

    fn decoded(&mut self, key: &str, decoded: Result<crate::video_clip::Clip, String>) {
        let used: usize = self.entries.values().map(|entry| entry.bytes).sum();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.decoding = false;
            match decoded {
                Ok(clip) if used.saturating_add(clip.bytes) <= TOTAL_DECODED => {
                    entry.bytes = clip.bytes;
                    entry.clip = Clip::from_frames(clip.frames);
                    entry.failed = entry.clip.is_none();
                }
                _ => {
                    entry.failed = true;
                }
            }
        }
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

    pub(super) fn prepare_inline_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let autoplay = self.pref_on("animate_gifs", true) && !self.pref_on("reduced_motion", false);
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
                    this.inline_videos.borrow_mut().decoded(&key, decoded);
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
        let mut oversized = clip();
        oversized.bytes = TOTAL_DECODED;
        cache.decoded("1", Ok(oversized));
        assert!(cache.entries["1"].failed);
        assert!(cache.entries["1"].clip.is_none());
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
