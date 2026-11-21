//! Discord classic `MediaPlayer` (`153440.js`, `317714.js`, `122641.js`, `20504.js`, `423562.js`, `96782.js`)
//! backed by the progressive H.264 decoder and A/V clock in `crate::video_vod`.

use super::audio_attachment::{BUFFER_MIN_SECONDS, clock, clock_pair, control_icon, control_tint};
use super::audio_menu::{slider_to_volume, volume_to_slider};
use super::tooltip::{Side, Tip};
use super::voice_message::{
    fraction_of, position_of, store_volume, stored_volume, tnum, unmute_restore_volume,
};
use super::{Root, color, icon};
use crate::parity::Parity;
use crate::video_vod::{KEY_PREFIX, LoadStatus, TickResult, VodPlayer};
use dc_state::{Attachment, Message};
use dc_tokens::Theme;
use gpui::{
    AnyElement, Bounds, Context, Corners, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, Pixels, RenderImage, Window, canvas, deferred, div, hsla, img,
    point, prelude::*, px, rgba, size, svg,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MIN_ATT_WIDTH: f32 = 150.0;
const MIN_ATT_HEIGHT: f32 = 110.0;
const CLOCK_MIN_WIDTH: f32 = 250.0;
const CONTROL_BAR_HEIGHT: f32 = 32.0;
const VOLUME_POPOUT_WIDTH: f32 = 28.0;
const VOLUME_POPOUT_HEIGHT: f32 = 96.0;
const VOLUME_TRACK_HEIGHT: f32 = 76.0;
const VOLUME_HIDE_GRACE_MS: u64 = 150;
const INLINE_HIDE_SECS: f64 = 3.0;
const FULLSCREEN_HIDE_SECS: f64 = 1.0;
const CENTER_POP_MS: f64 = 400.0;
const MAX_CACHED_PLAYERS: usize = 16;
/// Module 953584: the store forgets the oldest source past ten.
const STATS_LIMIT: usize = 10;
/// Module 410694: "Copied!" shows for two seconds.
pub(super) const STATS_COPIED: Duration = Duration::from_secs(2);

fn midnight(name: &str) -> gpui::Rgba {
    rgba(Theme::Midnight.color(name))
}

#[derive(Clone)]
pub(super) struct AttachmentActions {
    pub message: Message,
    pub attachment: Attachment,
    pub can_edit: bool,
    pub can_remove: bool,
}

pub(super) struct PlayerSpec {
    pub key: String,
    pub src: String,
    pub width: f32,
    pub height: f32,
    pub aspect_size: (u32, u32),
    pub radius: f32,
    pub poster_key: Option<String>,
    pub auto_play: bool,
    pub allow_fullscreen: bool,
    pub is_fullscreen_view: bool,
    pub actions: Option<AttachmentActions>,
}

#[derive(Clone)]
pub(super) struct FullscreenVideo {
    pub key: String,
    pub src: String,
    pub aspect_size: (u32, u32),
    pub poster_key: Option<String>,
}

struct ScrubDrag {
    key: String,
    fraction: f32,
    resume_after: bool,
}

struct Entry {
    src: String,
    aspect_size: (u32, u32),
    player: VodPlayer,
    tick: TickResult,
    has_clicked_play: bool,
    mouse_inside: bool,
    last_interaction: Instant,
    center_flash: Option<(&'static str, Instant)>,
    scrub_bounds: Bounds<Pixels>,
    scrub_hover: Option<f32>,
    volume_hover_btn: bool,
    volume_hover_pop: bool,
    volume_hide_at: Option<Instant>,
    volume_track_bounds: Bounds<Pixels>,
    epoch: u64,
}

impl Entry {
    fn new(
        key: &str,
        src: &str,
        aspect_size: (u32, u32),
        preloaded: Option<Arc<Vec<u8>>>,
        auto_play: bool,
        volume: f32,
        muted: bool,
    ) -> Self {
        let mut player = VodPlayer::new(format!("{KEY_PREFIX}{key}"), src, preloaded);
        if auto_play {
            player.play(volume, muted);
        }
        let tick = player.tick();
        Self {
            src: src.to_owned(),
            aspect_size,
            player,
            tick,
            has_clicked_play: auto_play,
            mouse_inside: false,
            last_interaction: Instant::now(),
            center_flash: None,
            scrub_bounds: Bounds::default(),
            scrub_hover: None,
            volume_hover_btn: false,
            volume_hover_pop: false,
            volume_hide_at: None,
            volume_track_bounds: Bounds::default(),
            epoch: 0,
        }
    }

    fn effective_duration(&self) -> Option<f64> {
        self.tick.duration.filter(|d| *d > 0.0)
    }

    fn is_buffered(&self) -> bool {
        self.tick.buffered
            && self
                .effective_duration()
                .is_some_and(|d| d >= BUFFER_MIN_SECONDS)
    }

    fn display_dimensions(&self) -> (u32, u32) {
        self.tick
            .dimensions
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((self.aspect_size.0.max(1), self.aspect_size.1.max(1)))
    }
}

pub(super) struct VideoHub {
    entries: HashMap<String, Entry>,
    preloaded: HashMap<String, Arc<Vec<u8>>>,
    pending_retired: Vec<Arc<RenderImage>>,
    pub(super) volume: f32,
    pub(super) muted: bool,
    scrub_drag: Option<ScrubDrag>,
    volume_drag: Option<String>,
    pub(super) fullscreen: Option<FullscreenVideo>,
    epoch: u64,
    timer_generation: u64,
    timer_deadline: Option<Instant>,
    /// `VideoStatsStore` (module 953584): the sources whose "Stats for Nerds" is on, oldest first, at most ten.
    stats: Vec<String>,
    /// The source whose stats were just copied, and when: "Copied!" for two seconds.
    stats_copied: Option<(String, Instant)>,
    #[cfg(any(test, feature = "capture"))]
    forced_volume_open: Option<String>,
}

impl VideoHub {
    pub(super) fn new() -> Self {
        let (volume, muted) = stored_volume();
        Self {
            entries: HashMap::new(),
            preloaded: HashMap::new(),
            pending_retired: Vec::new(),
            volume,
            muted,
            scrub_drag: None,
            volume_drag: None,
            fullscreen: None,
            epoch: 0,
            timer_generation: 0,
            timer_deadline: None,
            stats: Vec::new(),
            stats_copied: None,
            #[cfg(any(test, feature = "capture"))]
            forced_volume_open: None,
        }
    }

    /// `isVideoStatsEnabled`.
    pub(super) fn stats_on(&self, src: &str) -> bool {
        self.stats.iter().any(|known| known == src)
    }

    pub(super) fn set_stats_copied(&mut self, src: &str) {
        self.stats_copied = Some((src.to_owned(), Instant::now()));
    }

    fn stats_copied(&self, src: &str, now: Instant) -> bool {
        self.stats_copied
            .as_ref()
            .is_some_and(|(known, at)| known == src && now.duration_since(*at) < STATS_COPIED)
    }

    /// Module 338659 `getStats` of the player under `key`, when its stats are on.
    fn stats_of(
        &self,
        key: &str,
        src: &str,
        viewport: (f32, f32),
        file_size: Option<u64>,
    ) -> Option<super::video_stats::VideoStats> {
        if !self.stats_on(src) {
            return None;
        }
        let entry = self.entries.get(key)?;
        let media = entry.player.media_info();
        let codec = media.as_ref().map(|media| media.codec.clone());
        let (video_width, video_height, resolution) = match codec
            .as_ref()
            .and_then(|codec| Some((codec.video_width?, codec.video_height?)))
        {
            Some((width, height)) => (width, height, format!("{width}x{height}")),
            None => match entry.tick.dimensions {
                Some((width, height)) if width > 0 && height > 0 => {
                    (width, height, format!("{width}x{height}"))
                }
                _ => (0, 0, "Unknown".to_owned()),
            },
        };
        let duration = entry.effective_duration();
        // The whole file is fetched before it plays, so everything after the playhead is buffered once it is read.
        let buffered_seconds = match (&entry.tick.status, duration) {
            (LoadStatus::Ready, Some(duration)) => (duration - entry.tick.position).max(0.0),
            _ => 0.0,
        };
        // `MediaError` codes: 2 network, 3 decode, 4 source not supported.
        let error = match &entry.tick.status {
            LoadStatus::Failed(reason) => {
                let code = if reason.starts_with("decoder") {
                    3
                } else if reason.starts_with("MP4") || reason.starts_with("timing") {
                    4
                } else {
                    2
                };
                Some((code, reason.clone()))
            }
            _ => None,
        };
        Some(super::video_stats::VideoStats {
            resolution,
            video_width,
            video_height,
            viewport: (viewport.0.round() as u32, viewport.1.round() as u32),
            current_time: entry.tick.position,
            duration,
            buffered_seconds,
            dropped_frames: 0,
            total_frames: entry.player.decoded_frames(),
            frame_rate: codec.as_ref().and_then(|codec| codec.frame_rate),
            file_size: file_size.or(media.map(|media| media.file_size as u64)),
            codec,
            error,
        })
    }

    /// `toggleVideoStats`: on moves the source to the newest place and keeps the ten newest.
    pub(super) fn toggle_stats(&mut self, src: &str) {
        let was = self.stats_on(src);
        self.stats.retain(|known| known != src);
        if !was {
            self.stats.push(src.to_owned());
            let extra = self.stats.len().saturating_sub(STATS_LIMIT);
            self.stats.drain(..extra);
        }
    }

    /// Registers offline encoded MP4 bytes under `src` (used by capture demos and tests).
    #[cfg(any(test, feature = "capture"))]
    pub(super) fn seed_bytes(&mut self, src: impl Into<String>, bytes: Arc<Vec<u8>>) {
        let src = src.into();
        for entry in self.entries.values_mut() {
            if entry.src == src {
                entry.player.preload_bytes(bytes.clone());
            }
        }
        self.preloaded.insert(src, bytes);
    }

    fn sync_stored_volume(&mut self) {
        if self.volume_drag.is_some() {
            return;
        }
        let (volume, muted) = stored_volume();
        if (self.volume - volume).abs() > 1e-6 || self.muted != muted {
            self.volume = volume;
            self.muted = muted;
            for entry in self.entries.values_mut() {
                entry.player.set_audio_output(volume, muted);
            }
        }
    }

    fn ensure_entry(
        &mut self,
        key: &str,
        src: &str,
        aspect_size: (u32, u32),
        auto_play: bool,
    ) -> &mut Entry {
        self.sync_stored_volume();
        let epoch = self.epoch;
        let preloaded = self.preloaded.get(src).cloned();
        let (volume, muted) = (self.volume, self.muted);
        if !self.entries.contains_key(key) {
            if auto_play {
                self.pause_others(key);
            }
            let entry = Entry::new(
                key,
                src,
                aspect_size,
                preloaded.clone(),
                auto_play,
                volume,
                muted,
            );
            self.entries.insert(key.to_owned(), entry);
        }
        let need_swap = self.entries.get(key).is_some_and(|entry| entry.src != src);
        if need_swap {
            if auto_play {
                self.pause_others(key);
            }
            let mut old_images = Vec::new();
            if let Some(entry) = self.entries.get_mut(key) {
                old_images = entry.player.take_all_images();
                entry.src = src.to_owned();
                entry.aspect_size = aspect_size;
                entry.has_clicked_play = auto_play;
                entry.player = VodPlayer::new(format!("{KEY_PREFIX}{key}"), src, preloaded);
                if auto_play {
                    entry.player.play(volume, muted);
                }
                entry.tick = entry.player.tick();
            }
            self.pending_retired.extend(old_images);
        }
        let entry = self
            .entries
            .get_mut(key)
            .expect("inserted video entry is present");
        entry.epoch = epoch;
        entry
    }

    fn pause_others(&mut self, active_key: &str) {
        for (key, entry) in &mut self.entries {
            if key != active_key && entry.player.playing() {
                entry.player.pause();
            }
        }
    }

    fn play_or_toggle(&mut self, key: &str, now: Instant) {
        self.sync_stored_volume();
        let was_playing = self
            .entries
            .get(key)
            .is_some_and(|entry| entry.player.playing());
        if !was_playing {
            self.pause_others(key);
        }
        let (volume, muted) = (self.volume, self.muted);
        if let Some(entry) = self.entries.get_mut(key) {
            let ended = entry.player.ended();
            entry.has_clicked_play = true;
            entry.last_interaction = now;
            if was_playing {
                entry.player.pause();
                entry.center_flash = Some(("clips_pause", now));
            } else {
                entry.player.play(volume, muted);
                entry.center_flash = Some((if ended { "media_replay" } else { "clips_play" }, now));
            }
            entry.tick = entry.player.tick();
        }
    }

    fn toggle_mute(&mut self) {
        self.sync_stored_volume();
        self.muted = !self.muted;
        if !self.muted && self.volume <= 0.0 {
            self.volume = unmute_restore_volume();
        }
        store_volume(self.volume, self.muted);
        for entry in self.entries.values_mut() {
            entry.player.set_audio_output(self.volume, self.muted);
        }
    }

    fn set_slider_volume(&mut self, slider: f32) {
        self.volume = (slider_to_volume(slider.clamp(0.0, 100.0)) / 100.0).clamp(0.0, 1.0);
        self.muted = self.volume <= 0.0;
        store_volume(self.volume, self.muted);
        for entry in self.entries.values_mut() {
            entry.player.set_audio_output(self.volume, self.muted);
        }
    }

    fn is_volume_open(&self, key: &str, entry: &Entry, now: Instant) -> bool {
        if self.volume_drag.as_deref() == Some(key)
            || entry.volume_hover_btn
            || entry.volume_hover_pop
        {
            return true;
        }
        #[cfg(any(test, feature = "capture"))]
        if self.forced_volume_open.as_deref() == Some(key) {
            return true;
        }
        entry.volume_hide_at.is_some_and(|until| now < until)
    }

    fn controls_visible(
        &self,
        key: &str,
        entry: &Entry,
        is_fullscreen: bool,
        now: Instant,
    ) -> bool {
        if !entry.has_clicked_play {
            return false;
        }
        let volume_open = self.is_volume_open(key, entry, now);
        let scrubbing = self.scrub_drag.as_ref().is_some_and(|d| d.key == key);
        if !entry.tick.playing || volume_open || scrubbing {
            return true;
        }
        if !entry.mouse_inside {
            return false;
        }
        let timeout = if is_fullscreen {
            FULLSCREEN_HIDE_SECS
        } else {
            INLINE_HIDE_SECS
        };
        now.saturating_duration_since(entry.last_interaction)
            .as_secs_f64()
            < timeout
    }

    fn schedule_volume_hide(entry: &mut Entry, now: Instant) {
        if !entry.volume_hover_btn && !entry.volume_hover_pop {
            entry.volume_hide_at = Some(now + Duration::from_millis(VOLUME_HIDE_GRACE_MS));
        } else {
            entry.volume_hide_at = None;
        }
    }

    fn prepare(&mut self, now: Instant) -> (Vec<Arc<RenderImage>>, Option<Instant>) {
        self.epoch = self.epoch.wrapping_add(1);
        let mut retired = std::mem::take(&mut self.pending_retired);
        let mut next_deadline: Option<Instant> = None;

        for (key, entry) in &mut self.entries {
            if let Some(until) = entry.volume_hide_at
                && now >= until
                && !entry.volume_hover_btn
                && !entry.volume_hover_pop
            {
                entry.volume_hide_at = None;
            }
            if let Some((_, at)) = entry.center_flash
                && now.saturating_duration_since(at).as_secs_f64() * 1000.0 >= CENTER_POP_MS
            {
                entry.center_flash = None;
            }
            let tick = entry.player.tick();
            retired.extend(tick.retired.iter().cloned());
            entry.tick = tick;

            if entry.tick.needs_redraw || entry.center_flash.is_some() {
                let candidate = now + Duration::from_millis(16);
                next_deadline = Some(match next_deadline {
                    Some(prev) => prev.min(candidate),
                    None => candidate,
                });
            }
            if let Some(until) = entry.volume_hide_at
                && until > now
            {
                next_deadline = Some(match next_deadline {
                    Some(prev) => prev.min(until),
                    None => until,
                });
            }
            if entry.tick.playing && entry.mouse_inside {
                let timeout = if self.fullscreen.as_ref().is_some_and(|fs| fs.key == *key) {
                    FULLSCREEN_HIDE_SECS
                } else {
                    INLINE_HIDE_SECS
                };
                let hide_at = entry.last_interaction + Duration::from_secs_f64(timeout);
                if hide_at > now {
                    next_deadline = Some(match next_deadline {
                        Some(prev) => prev.min(hide_at),
                        None => hide_at,
                    });
                }
            }
        }

        // Evict stale inactive players once cache exceeds MAX_CACHED_PLAYERS.
        if self.entries.len() > MAX_CACHED_PLAYERS {
            let mut candidates: Vec<(String, u64)> = self
                .entries
                .iter()
                .filter(|(key, entry)| {
                    !entry.player.playing()
                        && self.fullscreen.as_ref().is_none_or(|fs| fs.key != **key)
                })
                .map(|(key, entry)| (key.clone(), entry.epoch))
                .collect();
            candidates.sort_by_key(|(_, epoch)| *epoch);
            let excess = self.entries.len().saturating_sub(MAX_CACHED_PLAYERS);
            for (key, _) in candidates.into_iter().take(excess) {
                if let Some(mut old) = self.entries.remove(&key) {
                    retired.extend(old.player.take_all_images());
                }
            }
        }

        (retired, next_deadline)
    }

    /// Drops the players of messages in the chat, as the official list unmounting its `<video>` elements does: a
    /// video started in a channel that was left starts over from the poster when the channel opens again. The
    /// lightbox's and the full-screen one stay. Their pictures are freed on the next frame.
    pub(super) fn release_inline(&mut self) {
        let fullscreen = self.fullscreen.as_ref().map(|fs| fs.key.clone());
        let keys: Vec<String> = self
            .entries
            .keys()
            .filter(|key| !key.starts_with("lb:") && fullscreen.as_ref() != Some(*key))
            .cloned()
            .collect();
        for key in keys {
            if let Some(mut entry) = self.entries.remove(&key) {
                if entry.player.playing() {
                    entry.player.pause();
                }
                let images = entry.player.take_all_images();
                self.pending_retired.extend(images);
            }
        }
    }

    /// Whether a lightbox or full-screen video plays (its sound is the shared engine's).
    pub(super) fn fullscreen_or_lightbox_playing(&self) -> bool {
        let fullscreen = self.fullscreen.as_ref().map(|fs| fs.key.as_str());
        self.entries.iter().any(|(key, entry)| {
            (key.starts_with("lb:") || fullscreen == Some(key.as_str())) && entry.player.playing()
        })
    }

    pub(super) fn pause_all(&mut self) {
        for entry in self.entries.values_mut() {
            if entry.player.playing() {
                entry.player.pause();
            }
        }
    }

    pub(super) fn release_prefix(&mut self, prefix: &str) -> Vec<Arc<RenderImage>> {
        let keys: Vec<String> = self
            .entries
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let mut out = std::mem::take(&mut self.pending_retired);
        for key in keys {
            if let Some(mut entry) = self.entries.remove(&key) {
                out.extend(entry.player.take_all_images());
            }
        }
        out
    }
}

fn fit_contain(bounds: Bounds<Pixels>, aspect: (u32, u32)) -> Bounds<Pixels> {
    let bw = f32::from(bounds.size.width).max(1.0);
    let bh = f32::from(bounds.size.height).max(1.0);
    let vw = (aspect.0.max(1)) as f32;
    let vh = (aspect.1.max(1)) as f32;
    let scale = (bw / vw).min(bh / vh);
    let dw = (vw * scale).min(bw);
    let dh = (vh * scale).min(bh);
    let ox = f32::from(bounds.origin.x) + (bw - dw) * 0.5;
    let oy = f32::from(bounds.origin.y) + (bh - dh) * 0.5;
    Bounds::new(point(px(ox), px(oy)), size(px(dw), px(dh)))
}

fn control_bar_bg() -> gpui::Hsla {
    hsla(0.0, 0.0, 0.0, 0.6)
}

fn play_circle_bg(hover: bool) -> gpui::Hsla {
    hsla(0.0, 0.0, 0.0, if hover { 0.8 } else { 0.6 })
}

impl Root {
    pub(super) fn prepare_videos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let (retired, deadline) = {
            let mut hub = self.video_hub.borrow_mut();
            hub.prepare(now)
        };
        for image in retired {
            cx.drop_image(image, Some(window));
        }

        let schedule = deadline.and_then(|at| {
            let mut hub = self.video_hub.borrow_mut();
            let reschedule = match hub.timer_deadline {
                Some(prev) => at + Duration::from_millis(2) < prev,
                None => true,
            };
            if reschedule {
                hub.timer_generation = hub.timer_generation.wrapping_add(1);
                hub.timer_deadline = Some(at);
                Some((hub.timer_generation, at))
            } else {
                None
            }
        });
        if deadline.is_none() {
            self.video_hub.borrow_mut().timer_deadline = None;
        }

        if let Some((generation, at)) = schedule {
            let hub = self.video_hub.clone();
            cx.spawn(async move |this, cx| {
                let wait = at.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    cx.background_executor().timer(wait).await;
                }
                let _ = this.update(cx, |_, cx| {
                    let mut hub = hub.borrow_mut();
                    if hub.timer_generation == generation {
                        hub.timer_deadline = None;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Another channel opened: its message list is no longer drawn, so its media stop as an unmounted `<video>` or
    /// `<audio>` element does in the official client (modules 153440, 527388 and 989161 pause nothing themselves; the
    /// element leaving the page does). Inline videos are dropped, so they start over when the channel opens again; the
    /// shared sound engine pauses, and a Play still waiting for its download is dropped rather than starting later in
    /// a channel no longer shown.
    pub(super) fn media_left_channel(&mut self) {
        self.video_hub.borrow_mut().release_inline();
        self.voice_hub.borrow_mut().drop_play_intents();
        // Sent whatever the status says: it lags, and a play clicked just before leaving may not show in it yet. The
        // clip editor's preview is not a message's media and keeps playing.
        let engine = dc_voice::voice_player::engine();
        let lightbox = self.video_hub.borrow().fullscreen_or_lightbox_playing();
        if !lightbox
            && !engine
                .status()
                .key
                .is_some_and(|key| key.starts_with("clip-editor:"))
        {
            engine.pause();
        }
    }

    pub(super) fn release_videos(&mut self, cx: &mut gpui::App) {
        let mut hub = self.video_hub.borrow_mut();
        hub.fullscreen = None;
        let mut images = std::mem::take(&mut hub.pending_retired);
        images.extend(
            hub.entries
                .drain()
                .flat_map(|(_, mut entry)| entry.player.take_all_images()),
        );
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }

    pub(super) fn release_lightbox_videos(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut gpui::App,
    ) {
        let images = self.video_hub.borrow_mut().release_prefix("lb:");
        if let Some(window) = window {
            for image in images {
                cx.drop_image(image, Some(window));
            }
        } else {
            cx.defer(move |cx| {
                for image in images {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    #[cfg(feature = "capture")]
    pub(super) fn seed_video_bytes(&mut self, url: impl Into<String>, bytes: Arc<Vec<u8>>) {
        self.video_hub.borrow_mut().seed_bytes(url, bytes);
    }

    pub(super) fn toggle_video_fullscreen(
        &mut self,
        spec: FullscreenVideo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut hub = self.video_hub.borrow_mut();
        let same = hub
            .fullscreen
            .as_ref()
            .is_some_and(|open| open.key == spec.key);
        if same {
            hub.fullscreen = None;
            if let Some(entry) = hub.entries.get_mut(&spec.key) {
                entry.last_interaction = Instant::now();
            }
            if !crate::storage::is_capture_run() && window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        } else {
            hub.sync_stored_volume();
            hub.fullscreen = Some(spec.clone());
            let (vol, muted) = (hub.volume, hub.muted);
            let entry = hub.ensure_entry(&spec.key, &spec.src, spec.aspect_size, false);
            entry.last_interaction = Instant::now();
            if !entry.has_clicked_play {
                entry.has_clicked_play = true;
                entry.player.play(vol, muted);
                entry.tick = entry.player.tick();
            }
            if !crate::storage::is_capture_run() && !window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
        cx.notify();
    }

    pub(super) fn exit_video_fullscreen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let had = {
            let mut hub = self.video_hub.borrow_mut();
            let had = hub.fullscreen.take().is_some();
            if had && !crate::storage::is_capture_run() && window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            had
        };
        if had {
            cx.notify();
        }
        had
    }

    pub(super) fn video_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let mut changed = false;
        {
            let mut hub = self.video_hub.borrow_mut();
            if let Some(mut drag) = hub.scrub_drag.take() {
                if let Some(entry) = hub.entries.get_mut(&drag.key) {
                    let frac = fraction_of(event.position.x, entry.scrub_bounds) as f32;
                    if (frac - drag.fraction).abs() > 0.002 {
                        drag.fraction = frac;
                        if let Some(dur) = entry.effective_duration() {
                            entry.player.seek(f64::from(frac) * dur);
                        }
                        entry.last_interaction = Instant::now();
                        changed = true;
                    }
                }
                hub.scrub_drag = Some(drag);
            }
            if let Some(key) = hub.volume_drag.clone()
                && let Some(bounds) = hub.entries.get(&key).map(|e| e.volume_track_bounds)
            {
                let slider = position_of(event.position.y, bounds) * 100.0;
                hub.set_slider_volume(slider);
                if let Some(entry) = hub.entries.get_mut(&key) {
                    entry.last_interaction = Instant::now();
                }
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn video_mouse_up(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        {
            let mut hub = self.video_hub.borrow_mut();
            if let Some(drag) = hub.scrub_drag.take() {
                let (vol, muted) = (hub.volume, hub.muted);
                if let Some(entry) = hub.entries.get_mut(&drag.key) {
                    if let Some(dur) = entry.effective_duration() {
                        entry.player.seek(f64::from(drag.fraction) * dur);
                    }
                    if drag.resume_after {
                        entry.player.play(vol, muted);
                    }
                    entry.last_interaction = Instant::now();
                }
                changed = true;
            }
            if hub.volume_drag.take().is_some() {
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn video_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
            return false;
        }
        let fs_key = {
            let hub = self.video_hub.borrow();
            hub.fullscreen.as_ref().map(|fs| fs.key.clone())
        };
        let Some(fs_key) = fs_key else {
            return false;
        };
        match keystroke.key.as_str() {
            "escape" => {
                self.exit_video_fullscreen(window, cx);
                true
            }
            "space" | "k" => {
                self.video_hub
                    .borrow_mut()
                    .play_or_toggle(&fs_key, Instant::now());
                cx.notify();
                true
            }
            "m" => {
                self.video_hub.borrow_mut().toggle_mute();
                cx.notify();
                true
            }
            "left" | "right" => {
                let delta = if keystroke.key == "left" { -5.0 } else { 5.0 };
                let mut hub = self.video_hub.borrow_mut();
                if let Some(entry) = hub.entries.get_mut(&fs_key) {
                    let target = (entry.tick.position + delta).max(0.0);
                    entry.player.seek(target);
                    entry.last_interaction = Instant::now();
                }
                cx.notify();
                true
            }
            _ => false,
        }
    }

    pub(super) fn attachment_video_player(
        &self,
        message: &Message,
        attachment: &Attachment,
        size: (Pixels, Pixels),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let w = f32::from(size.0).max(MIN_ATT_WIDTH);
        let h = f32::from(size.1).max(MIN_ATT_HEIGHT);
        let key = format!("attachment:{}", attachment.id);
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        let can_remove = own && (message.attachments.len() > 1 || !message.content.is_empty());
        let src = if attachment.proxy_url.is_empty() {
            attachment.url.clone()
        } else {
            attachment.proxy_url.clone()
        };
        self.render_media_player(
            PlayerSpec {
                key: key.clone(),
                src,
                width: w,
                height: h,
                aspect_size: (
                    attachment.width.unwrap_or(16).max(1),
                    attachment.height.unwrap_or(9).max(1),
                ),
                radius: 8.0,
                poster_key: Some(key),
                auto_play: false,
                allow_fullscreen: true,
                is_fullscreen_view: false,
                actions: Some(AttachmentActions {
                    message: message.clone(),
                    attachment: attachment.clone(),
                    can_edit: own,
                    can_remove,
                }),
            },
            cx,
        )
    }

    pub(super) fn video_fullscreen_layer(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fs = self.video_hub.borrow().fullscreen.clone()?;
        let vw = self.viewport.0.max(320.0);
        let vh = self.viewport.1.max(240.0);
        let player = self.render_media_player(
            PlayerSpec {
                key: fs.key,
                src: fs.src,
                width: vw,
                height: vh,
                aspect_size: fs.aspect_size,
                radius: 0.0,
                poster_key: fs.poster_key,
                auto_play: false,
                allow_fullscreen: true,
                is_fullscreen_view: true,
                actions: None,
            },
            cx,
        );
        Some(
            deferred(
                div()
                    .id("video-fullscreen-layer")
                    .parity("video-fullscreen")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::black())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(player),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    pub(super) fn render_media_player(
        &self,
        spec: PlayerSpec,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Instant::now();
        let hub = self.video_hub.clone();
        let key = spec.key.clone();
        let key_hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            key.hash(&mut h);
            h.finish()
        };

        let (
            has_clicked_play,
            tick_playing,
            tick_ended,
            tick_pos,
            tick_dur,
            tick_buffered,
            tick_dims,
            tick_frame,
            tick_status,
            controls_visible,
            volume_open,
            volume,
            muted,
            scrub_fraction_override,
            scrub_hover,
            center_flash,
            is_fullscreen_open,
        ) = {
            let mut hub_mut = hub.borrow_mut();
            let entry = hub_mut.ensure_entry(&key, &spec.src, spec.aspect_size, spec.auto_play);
            let dims = entry.display_dimensions();
            let dur = entry.effective_duration();
            let buffered = entry.is_buffered();
            let frame = entry.tick.frame.clone();
            let status = entry.tick.status.clone();
            let pos = entry.tick.position;
            let playing = entry.tick.playing;
            let ended = entry.tick.ended;
            let has_clicked = entry.has_clicked_play;
            let flash = entry.center_flash;
            let shover = entry.scrub_hover;
            let fs_active = hub_mut.fullscreen.as_ref().is_some_and(|fs| fs.key == key);
            let drag_frac = hub_mut
                .scrub_drag
                .as_ref()
                .filter(|d| d.key == key)
                .map(|d| d.fraction);
            let entry_ref = hub_mut.entries.get(&key).expect("entry present");
            let vol_open = hub_mut.is_volume_open(&key, entry_ref, now);
            let ctrl_vis = hub_mut.controls_visible(
                &key,
                entry_ref,
                spec.is_fullscreen_view || fs_active,
                now,
            );
            (
                has_clicked,
                playing,
                ended,
                pos,
                dur,
                buffered,
                dims,
                frame,
                status,
                ctrl_vis,
                vol_open,
                hub_mut.volume,
                hub_mut.muted,
                drag_frac,
                shover,
                flash,
                fs_active,
            )
        };

        let stats_overlay = {
            let hub_ref = hub.borrow();
            let size = spec.actions.as_ref().map(|actions| actions.attachment.size);
            hub_ref
                .stats_of(&key, &spec.src, (spec.width, spec.height), size)
                .map(|stats| (stats, hub_ref.stats_copied(&spec.src, now)))
        }
        .map(|(stats, copied)| self.video_stats_overlay(&spec.src, &stats, copied, cx));

        let poster = spec
            .poster_key
            .as_ref()
            .and_then(|k| self.images.get(k).cloned());

        let show_frame = has_clicked_play && !(is_fullscreen_open && !spec.is_fullscreen_view);
        let stage_visual: AnyElement = if show_frame && let Some(frame_img) = tick_frame {
            let aspect = tick_dims;
            let r = spec.radius;
            canvas(
                |bounds, _, _| bounds,
                move |bounds, _, window, _| {
                    let fit = fit_contain(bounds, aspect);
                    let corners = if (f32::from(fit.size.width) - f32::from(bounds.size.width))
                        .abs()
                        < 1.5
                        && (f32::from(fit.size.height) - f32::from(bounds.size.height)).abs() < 1.5
                    {
                        Corners::all(px(r))
                    } else {
                        Corners::all(px(0.0))
                    };
                    let _ = window.paint_image(fit, fit, corners, frame_img.clone(), 0, false);
                },
            )
            .size_full()
            .into_any_element()
        } else if let Some(poster_img) = poster {
            img(poster_img)
                .size_full()
                .rounded(px(spec.radius))
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div().size_full().bg(gpui::black()).into_any_element()
        };

        let center_overlay: Option<AnyElement> = if !has_clicked_play {
            let play_hub = hub.clone();
            let play_key = key.clone();
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id(("vp-init-play", key_hash))
                            .size(px(48.0))
                            .rounded_full()
                            .bg(play_circle_bg(false))
                            .hover(|style| style.bg(play_circle_bg(true)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(
                                svg()
                                    .path("icons/clips_play.svg")
                                    .size(px(24.0))
                                    .ml(px(1.0))
                                    .mr(px(-1.0))
                                    .text_color(gpui::white()),
                            )
                            .on_click(cx.listener(move |_, _, _, cx| {
                                play_hub
                                    .borrow_mut()
                                    .play_or_toggle(&play_key, Instant::now());
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            )
        } else if let Some((flash_icon, flash_at)) = center_flash {
            let elapsed_ms = now.saturating_duration_since(flash_at).as_secs_f64() * 1000.0;
            let progress = (elapsed_ms / CENTER_POP_MS).clamp(0.0, 1.0) as f32;
            let size_px = 48.0 + progress * 14.0;
            let alpha = (1.0 - progress).clamp(0.0, 1.0);
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(size_px))
                            .rounded_full()
                            .bg(hsla(0.0, 0.0, 0.0, 0.6 * alpha))
                            .opacity(alpha)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path(format!("icons/{flash_icon}.svg"))
                                    .size(px(24.0))
                                    .text_color(gpui::white()),
                            ),
                    )
                    .into_any_element(),
            )
        } else {
            // While a started video loads, module 153440 draws nothing over it (`onWaiting` only reports analytics).
            None
        };

        // Top-right hover bar (`96782.js`): only on attachments when hovered, hidden in fullscreen.
        let hovered_key = format!("vp-att:{}", key);
        let actions_bar: Option<AnyElement> = if !spec.is_fullscreen_view
            && let Some(actions) = spec.actions.as_ref()
            && (self.hovered_media.as_deref() == Some(hovered_key.as_str())
                || self.hovered_media.as_deref() == Some(key.as_str()))
        {
            let max_buttons = ((spec.width - 8.0) / 32.0).floor().max(0.0) as usize;
            let mut items: Vec<AnyElement> = Vec::new();
            if actions.can_edit && items.len() < max_buttons {
                let (msg, att) = (actions.message.clone(), actions.attachment.clone());
                items.push(
                    self.with_tip(
                        format!("vp-edit-{}", att.id),
                        Tip::text(dc_intl::tr("Modify Attachment"), Side::Top),
                        div()
                            .id(("vp-edit", att.id))
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(color("--interactive-text-default"))
                            .hover(|s| {
                                s.bg(color("--interactive-background-hover"))
                                    .text_color(color("--interactive-text-hover"))
                            })
                            .child(icon("clips_pencil", 20.0, "--interactive-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_modify_sent(&msg, &att, cx);
                                cx.stop_propagation();
                            })),
                        cx,
                    )
                    .into_any_element(),
                );
            }
            if actions.can_remove && items.len() < max_buttons {
                let (msg, att) = (actions.message.clone(), actions.attachment.clone());
                items.push(
                    self.with_tip(
                        format!("vp-rm-{}", att.id),
                        Tip::text(dc_intl::tr("Remove Message Attachment"), Side::Top),
                        div()
                            .id(("vp-rm", att.id))
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(color("--interactive-text-default"))
                            .hover(|s| {
                                s.bg(color("--interactive-background-hover"))
                                    .text_color(color("--text-feedback-critical"))
                            })
                            .child(icon("clips_trash", 20.0, "--interactive-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_remove_sent(&msg, &att, cx);
                                cx.stop_propagation();
                            })),
                        cx,
                    )
                    .into_any_element(),
                );
            }
            if items.len() < max_buttons {
                let att = actions.attachment.clone();
                items.push(
                    self.with_tip(
                        format!("vp-dl-{}", att.id),
                        Tip::text(dc_intl::tr("Download"), Side::Top),
                        div()
                            .id(("vp-dl", att.id))
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(color("--interactive-text-default"))
                            .hover(|s| {
                                s.bg(color("--interactive-background-hover"))
                                    .text_color(color("--interactive-text-hover"))
                            })
                            .child(icon("lb_download", 20.0, "--interactive-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.download_attachment(&att, cx);
                                cx.stop_propagation();
                            })),
                        cx,
                    )
                    .into_any_element(),
                );
            }
            (!items.is_empty()).then(|| {
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .shadow_sm()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items)
                    .into_any_element()
            })
        } else {
            None
        };

        // Bottom 32px control bar + volume popout (`153440.js`, `122641.js`, `20504.js`, `423562.js`).
        let (bottom_bar, volume_popout): (Option<AnyElement>, Option<AnyElement>) =
            if controls_visible {
                let play_icon = if tick_playing {
                    "clips_pause"
                } else if tick_ended {
                    "media_replay"
                } else {
                    "clips_play"
                };
                let play_tip = dc_intl::tr(if tick_playing {
                    "Pause"
                } else if tick_ended {
                    "Play again"
                } else {
                    "Play"
                });
                let (play_hub, play_key) = (hub.clone(), key.clone());
                let play_btn = self.with_tip(
                    format!("vp-play-tip-{key_hash}"),
                    Tip::text(play_tip, Side::Top),
                    div()
                        .id(("vp-play", key_hash))
                        .size(px(CONTROL_BAR_HEIGHT))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_color(control_tint())
                        .hover(|s| s.text_color(gpui::white()))
                        .child(control_icon(play_icon))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            play_hub
                                .borrow_mut()
                                .play_or_toggle(&play_key, Instant::now());
                            cx.stop_propagation();
                            cx.notify();
                        })),
                    cx,
                );

                let show_clock = spec.width > CLOCK_MIN_WIDTH || spec.is_fullscreen_view;
                let display_pos = scrub_fraction_override
                    .zip(tick_dur)
                    .map(|(f, d)| f64::from(f) * d)
                    .unwrap_or(tick_pos);
                let clock_el: Option<AnyElement> = show_clock.then(|| {
                    let (elapsed_str, total_str) = clock_pair(
                        Some(display_pos),
                        tick_dur.or(if matches!(tick_status, LoadStatus::Ready) {
                            Some(0.0)
                        } else {
                            None
                        }),
                    );
                    div()
                        .flex_none()
                        .px(px(4.0))
                        .whitespace_nowrap()
                        .text_size(px(12.0))
                        .line_height(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .font_features(tnum())
                        .text_color(midnight("--text-default"))
                        .child(format!("{elapsed_str} / {total_str}"))
                        .into_any_element()
                });

                let played_pct = scrub_fraction_override.unwrap_or_else(|| match tick_dur {
                    Some(dur) if dur > 0.0 => (tick_pos / dur).clamp(0.0, 1.0) as f32,
                    _ => 0.0,
                });
                let scrub_active = scrub_fraction_override.is_some() || scrub_hover.is_some();
                let preview_pct = scrub_fraction_override.or(scrub_hover);
                let preview_bubble: Option<AnyElement> =
                    preview_pct.zip(tick_dur).map(|(frac, dur)| {
                        let label = clock(f64::from(frac) * dur);
                        div()
                            .absolute()
                            .bottom(px(22.0))
                            .left(gpui::relative(frac.clamp(0.06, 0.94)))
                            .ml(px(-18.0))
                            .px(px(5.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(hsla(0.0, 0.0, 0.0, 0.85))
                            .text_size(px(11.0))
                            .line_height(px(14.0))
                            .font_features(tnum())
                            .text_color(gpui::white())
                            .child(label)
                            .into_any_element()
                    });

                let (bounds_hub, bounds_key) = (hub.clone(), key.clone());
                let (down_hub, down_key) = (hub.clone(), key.clone());
                let (move_hub, move_key) = (hub.clone(), key.clone());
                let (hover_hub, hover_key) = (hub.clone(), key.clone());

                let scrubber = div()
                    .id(("vp-scrub", key_hash))
                    .relative()
                    .flex_1()
                    .min_w(px(24.0))
                    .h_full()
                    .mx(px(6.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                if let Some(entry) =
                                    bounds_hub.borrow_mut().entries.get_mut(&bounds_key)
                                {
                                    entry.scrub_bounds = bounds;
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(6.0))
                            .rounded(px(3.0))
                            .bg(hsla(0.0, 0.0, 1.0, 0.2))
                            .when(tick_buffered, |bar| {
                                bar.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .h_full()
                                        .w_full()
                                        .rounded(px(3.0))
                                        .bg(hsla(0.0, 0.0, 1.0, 0.3)),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .w(gpui::relative(played_pct.clamp(0.0, 1.0)))
                                    .rounded(px(3.0))
                                    .bg(midnight("--brand-500")),
                            )
                            .when(scrub_active, |bar| {
                                bar.child(
                                    div()
                                        .absolute()
                                        .top(px(-2.0))
                                        .left(gpui::relative(played_pct.clamp(0.0, 1.0)))
                                        .ml(px(-5.0))
                                        .size(px(10.0))
                                        .rounded_full()
                                        .bg(gpui::white())
                                        .shadow_xs(),
                                )
                            }),
                    )
                    .children(preview_bubble)
                    .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                        if !*inside {
                            if let Some(entry) = hover_hub.borrow_mut().entries.get_mut(&hover_key)
                            {
                                entry.scrub_hover = None;
                            }
                            cx.notify();
                        }
                    }))
                    .on_mouse_move(cx.listener(move |_, event: &MouseMoveEvent, _, cx| {
                        let mut hub = move_hub.borrow_mut();
                        if let Some(entry) = hub.entries.get_mut(&move_key) {
                            let frac = fraction_of(event.position.x, entry.scrub_bounds) as f32;
                            entry.scrub_hover = Some(frac);
                            entry.last_interaction = Instant::now();
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                            let mut hub = down_hub.borrow_mut();
                            if let Some(entry) = hub.entries.get_mut(&down_key) {
                                let frac = fraction_of(event.position.x, entry.scrub_bounds) as f32;
                                let resume_after = entry.player.playing();
                                if resume_after {
                                    entry.player.pause();
                                }
                                if let Some(dur) = entry.effective_duration() {
                                    entry.player.seek(f64::from(frac) * dur);
                                }
                                entry.last_interaction = Instant::now();
                                hub.scrub_drag = Some(ScrubDrag {
                                    key: down_key.clone(),
                                    fraction: frac,
                                    resume_after,
                                });
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    );

                // Mute + hover popout (`20504.js`).
                let vol_icon = if muted || volume <= 0.0 {
                    "clips_voice_off"
                } else {
                    "clips_voice_on"
                };
                let (vol_click_hub, vol_click_key) = (hub.clone(), key.clone());
                let (vol_hover_hub, vol_hover_key) = (hub.clone(), key.clone());
                let vol_btn = div()
                    .id(("vp-vol", key_hash))
                    .size(px(CONTROL_BAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(control_tint())
                    .hover(|s| s.text_color(gpui::white()))
                    .child(icon(vol_icon, 20.0, "--interactive-text-active"))
                    .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                        let mut hub = vol_hover_hub.borrow_mut();
                        if let Some(entry) = hub.entries.get_mut(&vol_hover_key) {
                            entry.volume_hover_btn = *inside;
                            VideoHub::schedule_volume_hide(entry, Instant::now());
                            entry.last_interaction = Instant::now();
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let mut hub = vol_click_hub.borrow_mut();
                        hub.toggle_mute();
                        if let Some(entry) = hub.entries.get_mut(&vol_click_key) {
                            entry.last_interaction = Instant::now();
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }));

                let fs_btn: Option<AnyElement> = spec.allow_fullscreen.then(|| {
                    let fs_spec = FullscreenVideo {
                        key: key.clone(),
                        src: spec.src.clone(),
                        aspect_size: spec.aspect_size,
                        poster_key: spec.poster_key.clone(),
                    };
                    let fs_icon = if spec.is_fullscreen_view || is_fullscreen_open {
                        "fullscreen_exit"
                    } else {
                        "voice_view_fullscreen"
                    };
                    let fs_tip = dc_intl::tr(if spec.is_fullscreen_view || is_fullscreen_open {
                        "Exit Full Screen"
                    } else {
                        "Full Screen"
                    });
                    self.with_tip(
                        format!("vp-fs-tip-{key_hash}"),
                        Tip::text(fs_tip, Side::Top),
                        div()
                            .id(("vp-fs", key_hash))
                            .size(px(CONTROL_BAR_HEIGHT))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(control_tint())
                            .hover(|s| s.text_color(gpui::white()))
                            .child(icon(fs_icon, 18.0, "--interactive-text-active"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_video_fullscreen(fs_spec.clone(), window, cx);
                                cx.stop_propagation();
                            })),
                        cx,
                    )
                    .into_any_element()
                });

                let right_offset = if spec.allow_fullscreen { 34.0 } else { 2.0 };
                let popout: Option<AnyElement> = volume_open.then(|| {
                    let slider_pct = if muted || volume <= 0.0 {
                        0.0
                    } else {
                        (volume_to_slider(volume * 100.0) / 100.0).clamp(0.0, 1.0)
                    };
                    let fill_h = (VOLUME_TRACK_HEIGHT * slider_pct).clamp(0.0, VOLUME_TRACK_HEIGHT);
                    let (pop_hover_hub, pop_hover_key) = (hub.clone(), key.clone());
                    let (track_bounds_hub, track_bounds_key) = (hub.clone(), key.clone());
                    let (track_down_hub, track_down_key) = (hub.clone(), key.clone());
                    let (track_move_hub, track_move_key) = (hub.clone(), key.clone());

                    div()
                        .id(("vp-vol-pop", key_hash))
                        .parity("video-volume-popout")
                        .absolute()
                        .bottom(px(CONTROL_BAR_HEIGHT + 4.0))
                        .right(px(right_offset))
                        .w(px(VOLUME_POPOUT_WIDTH))
                        .h(px(VOLUME_POPOUT_HEIGHT))
                        .py(px(10.0))
                        .rounded(px(8.0))
                        .bg(midnight("--background-surface-higher"))
                        .border_1()
                        .border_color(midnight("--border-subtle"))
                        .shadow_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                            let mut hub = pop_hover_hub.borrow_mut();
                            if let Some(entry) = hub.entries.get_mut(&pop_hover_key) {
                                entry.volume_hover_pop = *inside;
                                VideoHub::schedule_volume_hide(entry, Instant::now());
                                entry.last_interaction = Instant::now();
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .id(("vp-vol-track-hit", key_hash))
                                .relative()
                                .w(px(16.0))
                                .h(px(VOLUME_TRACK_HEIGHT))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(
                                    canvas(
                                        move |bounds, _, _| {
                                            if let Some(entry) = track_bounds_hub
                                                .borrow_mut()
                                                .entries
                                                .get_mut(&track_bounds_key)
                                            {
                                                entry.volume_track_bounds = bounds;
                                            }
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .w(px(6.0))
                                        .h(px(VOLUME_TRACK_HEIGHT))
                                        .rounded_full()
                                        .bg({
                                            // `mediaBarWrapperVolume_s1cr3c`: `rgba(201,202,206,.3)` (`--primary-300` at 30%).
                                            let mut rail = midnight("--primary-300");
                                            rail.a *= 0.3;
                                            rail
                                        })
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_0()
                                                .w_full()
                                                .h(px(fill_h))
                                                .rounded_full()
                                                .bg(midnight("--brand-500")),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom(px((fill_h - 6.0)
                                                    .clamp(0.0, VOLUME_TRACK_HEIGHT - 12.0)))
                                                .left(px(-3.0))
                                                .size(px(12.0))
                                                .rounded_full()
                                                .bg(gpui::white())
                                                .shadow_xs(),
                                        ),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                                        let mut hub = track_down_hub.borrow_mut();
                                        if let Some(bounds) = hub
                                            .entries
                                            .get(&track_down_key)
                                            .map(|e| e.volume_track_bounds)
                                        {
                                            let pos = position_of(event.position.y, bounds);
                                            hub.set_slider_volume(pos * 100.0);
                                            hub.volume_drag = Some(track_down_key.clone());
                                        }
                                        if let Some(entry) = hub.entries.get_mut(&track_down_key) {
                                            entry.last_interaction = Instant::now();
                                        }
                                        cx.stop_propagation();
                                        cx.notify();
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    move |_, event: &MouseMoveEvent, _, cx| {
                                        let mut hub = track_move_hub.borrow_mut();
                                        if hub.volume_drag.as_deref()
                                            == Some(track_move_key.as_str())
                                            && let Some(bounds) = hub
                                                .entries
                                                .get(&track_move_key)
                                                .map(|e| e.volume_track_bounds)
                                        {
                                            let pos = position_of(event.position.y, bounds);
                                            hub.set_slider_volume(pos * 100.0);
                                            if let Some(entry) =
                                                hub.entries.get_mut(&track_move_key)
                                            {
                                                entry.last_interaction = Instant::now();
                                            }
                                            cx.notify();
                                        }
                                    },
                                )),
                        )
                        .into_any_element()
                });

                let bar = div()
                    .id(("vp-bar", key_hash))
                    .parity("video-controls")
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(CONTROL_BAR_HEIGHT))
                    .px(px(4.0))
                    .rounded_b(px(spec.radius))
                    .bg(control_bar_bg())
                    .flex()
                    .items_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(play_btn)
                    .children(clock_el)
                    .child(scrubber)
                    .child(vol_btn)
                    .children(fs_btn)
                    .into_any_element();

                (Some(bar), popout)
            } else {
                (None, None)
            };

        let (hover_hub, hover_key) = (hub.clone(), key.clone());
        let (move_hub, move_key) = (hub.clone(), key.clone());
        let (click_hub, click_key) = (hub.clone(), key.clone());
        let show_actions = spec.actions.is_some();
        // Module 761304: the media mosaic passes the attachment (`mediaItem`), so the message menu's link rows use its
        // address (module 777933: a video by its extension reads Copy Media Link / Open Media Link).
        let menu_url = spec
            .actions
            .as_ref()
            .map(|actions| (actions.attachment.url.clone(), spec.src.clone()));

        div()
            .id(("video-player", key_hash))
            .parity("media-player")
            .relative()
            .w(px(spec.width))
            .h(px(spec.height))
            .flex_none()
            .when_some(menu_url, |this, (url, src)| {
                this.on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                        this.menu_hit = Some((
                            event.position,
                            super::menu::Hit::Video {
                                url: url.clone(),
                                src: src.clone(),
                            },
                        ));
                    }),
                )
            })
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if let Some(entry) = hover_hub.borrow_mut().entries.get_mut(&hover_key) {
                    entry.mouse_inside = *inside;
                    if *inside {
                        entry.last_interaction = Instant::now();
                    }
                }
                if show_actions {
                    this.media_hover(&hovered_key, *inside, cx);
                } else {
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(move |_, _, _, cx| {
                if let Some(entry) = move_hub.borrow_mut().entries.get_mut(&move_key) {
                    entry.last_interaction = Instant::now();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id(("vp-stage", key_hash))
                    .relative()
                    .size_full()
                    .rounded(px(spec.radius))
                    .overflow_hidden()
                    .bg(gpui::black())
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        click_hub
                            .borrow_mut()
                            .play_or_toggle(&click_key, Instant::now());
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(stage_visual)
                    .children(center_overlay)
                    .children(bottom_bar),
            )
            .children(stats_overlay)
            .children(actions_bar)
            .children(volume_popout)
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture script helper routed via `embeds video ...`:
    /// `state | play <k> | pause <k> | clock <s> <k> | seek <s> <k> | volume-open <k|off>`.
    pub(super) fn script_video_cx(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let mut hub = self.video_hub.borrow_mut();
        match words {
            ["state"] => {
                let mut parts: Vec<String> = hub
                    .entries
                    .iter()
                    .map(|(k, e)| {
                        format!(
                            "{k}(status={:?},pos={:.3},dur={:?},playing={},clicked={},frame={},muted={},vol={:.1},fs={})",
                            e.tick.status,
                            e.tick.position,
                            e.tick.duration,
                            e.tick.playing,
                            e.has_clicked_play,
                            e.tick.frame.is_some(),
                            hub.muted,
                            hub.volume,
                            hub.fullscreen.as_ref().is_some_and(|fs| fs.key == *k)
                        )
                    })
                    .collect();
                parts.sort();
                format!("video: {}", parts.join("; "))
            }
            ["play", key] => {
                let (vol, muted) = (hub.volume, hub.muted);
                if let Some(entry) = hub.entries.get_mut(*key) {
                    entry.has_clicked_play = true;
                    entry.mouse_inside = true;
                    entry.last_interaction = Instant::now();
                    entry.player.play(vol, muted);
                    entry.tick = entry.player.tick();
                    cx.notify();
                    format!("video: played {key}")
                } else {
                    format!("video: missing entry {key}")
                }
            }
            ["pause", key] => {
                if let Some(entry) = hub.entries.get_mut(*key) {
                    entry.mouse_inside = true;
                    entry.last_interaction = Instant::now();
                    entry.player.pause();
                    entry.tick = entry.player.tick();
                    cx.notify();
                    format!("video: paused {key}")
                } else {
                    format!("video: missing entry {key}")
                }
            }
            ["clock", secs, key] => match secs.parse::<f64>() {
                Ok(secs) => {
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        let ready = {
                            if let Some(entry) = hub.entries.get_mut(*key) {
                                entry.player.force_clock_for_test(Some(secs));
                                entry.tick = entry.player.tick();
                                matches!(entry.tick.status, LoadStatus::Ready)
                                    && entry.tick.frame.is_some()
                            } else {
                                break format!("video: missing entry {key}");
                            }
                        };
                        if ready || Instant::now() >= deadline {
                            cx.notify();
                            break format!("video: clock {key} -> {secs:.2}s");
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Err(_) => "video: bad clock seconds".to_owned(),
            },
            ["seek", secs, key] => match secs.parse::<f64>() {
                Ok(secs) => {
                    if let Some(entry) = hub.entries.get_mut(*key) {
                        entry.has_clicked_play = true;
                        entry.mouse_inside = true;
                        entry.last_interaction = Instant::now();
                        entry.player.seek(secs);
                        entry.player.force_clock_for_test(Some(secs));
                        let deadline = Instant::now() + Duration::from_secs(2);
                        while Instant::now() < deadline {
                            entry.tick = entry.player.tick();
                            if entry.tick.frame.is_some() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        cx.notify();
                        format!("video: seeked {key} -> {secs:.2}s")
                    } else {
                        format!("video: missing entry {key}")
                    }
                }
                Err(_) => "video: bad seek seconds".to_owned(),
            },
            ["volume-open", "off"] => {
                hub.forced_volume_open = None;
                cx.notify();
                "video: volume popout cleared".to_owned()
            }
            ["volume-open", key] => {
                hub.forced_volume_open = Some((*key).to_owned());
                cx.notify();
                format!("video: volume popout opened on {key}")
            }
            _ => "video: usage `embeds video state | play <k> | pause <k> | clock <s> <k> | seek <s> <k> | volume-open <k|off>`".to_owned(),
        }
    }
}

/// Module 40705 (`discord-web-video-player`) playback speed snap stops (`playbackSpeedPopout_hFq1lM`).
pub(super) const DVP_SPEED_STOPS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// Size presets for module 40705 (`530567.X4`: SM, MD, LG).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum DvpSize {
    Sm,
    Md,
    Lg,
}

impl DvpSize {
    fn bar_height(self) -> f32 {
        match self {
            Self::Sm => 40.0,
            Self::Md => 64.0,
            Self::Lg => 72.0,
        }
    }

    fn pad_x(self) -> f32 {
        match self {
            Self::Sm => 12.0,
            Self::Md | Self::Lg => 20.0,
        }
    }

    fn gap(self) -> f32 {
        match self {
            Self::Sm => 12.0,
            Self::Md | Self::Lg => 16.0,
        }
    }

    fn volume_slider_width(self) -> f32 {
        match self {
            Self::Sm => 48.0,
            Self::Md | Self::Lg => 100.0,
        }
    }
}

/// Interactive control bar state for module 40705 (`discord-web-video-player`, callers `91982` / `955368`).
#[derive(Clone, Debug)]
pub(crate) struct DvpControlState {
    pub playing: bool,
    pub ended: bool,
    pub current_secs: u32,
    pub max_watched_secs: u32,
    pub duration_secs: u32,
    pub volume: f32,
    pub muted: bool,
    pub show_remaining: bool,
    pub transcript_open: bool,
    pub captions_enabled: bool,
    pub speed_open: bool,
    pub playback_rate: f32,
    pub fullscreen: bool,
}

impl Default for DvpControlState {
    fn default() -> Self {
        Self {
            playing: true,
            ended: false,
            current_secs: 0,
            max_watched_secs: 0,
            duration_secs: 0,
            volume: 1.0,
            muted: false,
            show_remaining: false,
            transcript_open: false,
            captions_enabled: false,
            speed_open: false,
            playback_rate: 1.0,
            fullscreen: false,
        }
    }
}

impl DvpControlState {
    pub(crate) fn for_video(watch_secs: u32, duration_secs: u32, ended: bool) -> Self {
        Self {
            playing: !ended,
            ended,
            current_secs: watch_secs.min(duration_secs),
            max_watched_secs: watch_secs.min(duration_secs),
            duration_secs,
            ..Self::default()
        }
    }

    /// Module 40705 `seekForwardEnabled`: cannot skip past already-watched sections (`current + 1 < max_watched`).
    pub(crate) fn seek_forward_enabled(&self) -> bool {
        !self.ended && self.current_secs.saturating_add(1) < self.max_watched_secs
    }

    pub(crate) fn seek_back_10(&mut self) {
        self.current_secs = self.current_secs.saturating_sub(10);
        if self.ended && self.current_secs < self.duration_secs {
            self.ended = false;
        }
    }

    pub(crate) fn seek_forward_10(&mut self) {
        if !self.seek_forward_enabled() {
            return;
        }
        let ceiling = self.max_watched_secs.min(self.duration_secs);
        self.current_secs = self.current_secs.saturating_add(10).min(ceiling);
    }

    pub(crate) fn toggle_play(&mut self) {
        if self.ended {
            self.ended = false;
            self.current_secs = 0;
            self.playing = true;
        } else {
            self.playing = !self.playing;
        }
    }

    pub(crate) fn adjust_volume(&mut self, delta: f32) {
        self.volume = (self.volume + delta).clamp(0.0, 1.0);
        if self.volume > 0.0 {
            self.muted = false;
        }
    }

    /// Module 40705 `O` (`discord-web-video-player-duration`): elapsed `m:ss` or remaining `-m:ss`, optionally
    /// followed by ` / m:ss` unless `compact` is true; placeholder `"-:--"` when duration is unknown.
    pub(crate) fn duration_text(&self, compact: bool) -> String {
        if self.duration_secs == 0 {
            return "-:--".to_owned();
        }
        let fmt = |secs: u32| format!("{}:{:02}", secs / 60, secs % 60);
        let cur = self.current_secs.min(self.duration_secs);
        let lead = if self.show_remaining {
            format!("-{}", fmt(self.duration_secs.saturating_sub(cur)))
        } else {
            fmt(cur)
        };
        if compact {
            lead
        } else {
            format!("{lead} / {}", fmt(self.duration_secs))
        }
    }

    /// Keyboard shortcuts of module 876230 (`TJ`) / module 40705: `k`/`Space`, `j`/`left`, `l`/`right`, `c`, `f`,
    /// `m`, `up`/`down` (±10% volume).
    pub(crate) fn handle_shortcut(&mut self, key: &str) -> bool {
        match key {
            "k" | "space" | " " => self.toggle_play(),
            "j" | "left" => self.seek_back_10(),
            "l" | "right" => self.seek_forward_10(),
            "c" => self.captions_enabled = !self.captions_enabled,
            "f" => self.fullscreen = !self.fullscreen,
            "m" => self.muted = !self.muted,
            "up" => self.adjust_volume(0.10),
            "down" => self.adjust_volume(-0.10),
            _ => return false,
        }
        true
    }
}

/// Callback updating the host's `DvpControlState` (e.g. `VideoQuestModal.dvp`).
pub(super) type DvpMutator =
    std::rc::Rc<dyn Fn(&mut Root, Box<dyn FnOnce(&mut DvpControlState)>, &mut Context<Root>)>;

impl Root {
    /// Newer `discord-web-video-player` 3-group control bar (module 40705 `er`, size presets `530567.X4`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_dvp_control_bar(
        &self,
        id_prefix: &'static str,
        state: &DvpControlState,
        size: DvpSize,
        hide_transcript: bool,
        hide_captions: bool,
        hide_speed: bool,
        download_url: Option<String>,
        on_mutate: DvpMutator,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let seek_fwd = state.seek_forward_enabled();
        let play_icon = if state.ended {
            "media_replay"
        } else if state.playing {
            "clips_pause"
        } else {
            "clips_play"
        };
        let play_tip = dc_intl::t(if state.ended {
            "hsvh0i"
        } else if state.playing {
            "ZcgDJX"
        } else {
            "RscU7I"
        });
        let m_play = on_mutate.clone();
        let play_btn = self.with_tip(
            format!("{id_prefix}-play-tip"),
            Tip::text(play_tip, Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-play")))
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .child(icon(play_icon, 18., "--white"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    m_play(this, Box::new(DvpControlState::toggle_play), cx);
                })),
            cx,
        );
        let m_back = on_mutate.clone();
        let rewind_btn = self.with_tip(
            format!("{id_prefix}-rewind-tip"),
            Tip::text(dc_intl::t("dRVF+Z"), Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-rewind")))
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .child(icon("clips_skip_back", 18., "--white"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    m_back(this, Box::new(DvpControlState::seek_back_10), cx);
                })),
            cx,
        );
        let m_fwd = on_mutate.clone();
        let fwd_tip = dc_intl::t(if seek_fwd { "yV2FLL" } else { "YWbiPw" });
        let skip_btn = self.with_tip(
            format!("{id_prefix}-fwd-tip"),
            Tip::text(fwd_tip, Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-fwd")))
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .when(seek_fwd, |s| {
                    s.cursor_pointer().hover(|h| h.bg(rgba(0xffffff1a)))
                })
                .when(!seek_fwd, |s| s.opacity(0.4))
                .child(icon("clips_skip_forward", 18., "--white"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if seek_fwd {
                        m_fwd(this, Box::new(DvpControlState::seek_forward_10), cx);
                    }
                })),
            cx,
        );

        let vol_effective = if state.muted { 0.0 } else { state.volume };
        let vol_icon = if vol_effective <= 0.0 {
            "clips_voice_off"
        } else {
            "clips_voice_on"
        };
        let vol_tip = dc_intl::t(if vol_effective <= 0.0 {
            "YqAjXy"
        } else {
            "w4m945"
        });
        let m_mute = on_mutate.clone();
        let mute_btn = self.with_tip(
            format!("{id_prefix}-mute-tip"),
            Tip::text(vol_tip, Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-mute")))
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .child(icon(vol_icon, 18., "--white"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    m_mute(
                        this,
                        Box::new(|s| {
                            s.muted = !s.muted;
                            if !s.muted && s.volume <= 0.0 {
                                s.volume = 0.5;
                            }
                        }),
                        cx,
                    );
                })),
            cx,
        );

        let slider_w = size.volume_slider_width();
        let slider_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::default()));
        let sb_write = slider_bounds.clone();
        let sb_read = slider_bounds.clone();
        let m_vol = on_mutate.clone();
        let vol_slider = self.with_tip(
            format!("{id_prefix}-vol-tip"),
            Tip::text(dc_intl::t("XiLvuG"), Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-vol-slider")))
                .relative()
                .w(px(slider_w))
                .h(px(20.))
                .flex()
                .items_center()
                .cursor_pointer()
                .child(
                    canvas(move |b, _, _| sb_write.set(b), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(rgba(0xffffff33))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(vol_effective.clamp(0.0, 1.0)))
                                .bg(color("--white")),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let area = sb_read.get();
                        if area.size.width > px(0.) {
                            let frac = ((event.position.x - area.origin.x) / area.size.width)
                                .clamp(0.0, 1.0);
                            m_vol(
                                this,
                                Box::new(move |s| {
                                    s.volume = frac;
                                    s.muted = frac <= 0.001;
                                }),
                                cx,
                            );
                        }
                    }),
                ),
            cx,
        );

        let m_time = on_mutate.clone();
        let duration_btn = div()
            .id(gpui::SharedString::from(format!("{id_prefix}-duration")))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(0xffffff14)))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(color("--white"))
            .child(state.duration_text(false))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                m_time(this, Box::new(|s| s.show_remaining = !s.show_remaining), cx);
            }));

        let transcript_active = state.transcript_open && !state.ended;
        let transcript_disabled = state.ended;
        let m_tr = on_mutate.clone();
        let transcript_btn = (!hide_transcript).then(|| {
            self.with_tip(
                format!("{id_prefix}-transcript-tip"),
                Tip::text(dc_intl::t("6EjGUv"), Side::Top),
                div()
                    .id(gpui::SharedString::from(format!("{id_prefix}-transcript")))
                    .size(px(28.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(transcript_active, |s| s.bg(color("--brand-500")))
                    .when(!transcript_disabled, |s| {
                        s.cursor_pointer().hover(|h| h.bg(rgba(0xffffff1a)))
                    })
                    .when(transcript_disabled, |s| s.opacity(0.4))
                    .child(icon("discover_doc", 16., "--white"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !transcript_disabled {
                            m_tr(
                                this,
                                Box::new(|s| s.transcript_open = !s.transcript_open),
                                cx,
                            );
                        }
                    })),
                cx,
            )
            .into_any_element()
        });

        let cc_on = state.captions_enabled;
        let m_cc = on_mutate.clone();
        let captions_btn = (!hide_captions).then(|| {
            self.with_tip(
                format!("{id_prefix}-cc-tip"),
                Tip::text(dc_intl::t("0DbPcL"), Side::Top),
                div()
                    .id(gpui::SharedString::from(format!("{id_prefix}-cc")))
                    .h(px(28.))
                    .px(px(6.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(cc_on, |s| s.bg(color("--brand-500")))
                    .when(!cc_on, |s| s.hover(|h| h.bg(rgba(0xffffff1a))))
                    .child(
                        div()
                            .px(px(3.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(color("--white"))
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child("CC"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        m_cc(
                            this,
                            Box::new(|s| s.captions_enabled = !s.captions_enabled),
                            cx,
                        );
                    })),
                cx,
            )
            .into_any_element()
        });

        let download_btn = download_url.map(|url| {
            self.with_tip(
                format!("{id_prefix}-dl-tip"),
                Tip::text(dc_intl::t("1WjMbC"), Side::Top),
                div()
                    .id(gpui::SharedString::from(format!("{id_prefix}-dl")))
                    .size(px(28.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xffffff1a)))
                    .child(icon("lb_download", 18., "--white"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.open_url(&url);
                    })),
                cx,
            )
            .into_any_element()
        });

        let rate = state.playback_rate;
        let speed_open = state.speed_open;
        let m_speed_toggle = on_mutate.clone();
        let speed_popout = speed_open.then(|| {
            let mut pills = Vec::new();
            for stop in DVP_SPEED_STOPS {
                let selected = (stop - rate).abs() < 0.01;
                let m_stop = on_mutate.clone();
                let label = if (stop - 1.0).abs() < 0.01 {
                    "1x".to_owned()
                } else {
                    format!("{stop}x")
                };
                pills.push(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "{id_prefix}-spd-{}",
                            (stop * 100.0).round() as u32
                        )))
                        .flex_1()
                        .h(px(26.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight(600.))
                        .bg(if selected {
                            color("--brand-500")
                        } else {
                            color("--background-surface-higher")
                        })
                        .text_color(color("--white"))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            m_stop(
                                this,
                                Box::new(move |s| {
                                    s.playback_rate = stop;
                                    s.speed_open = false;
                                }),
                                cx,
                            );
                        })),
                );
            }
            div()
                .id(gpui::SharedString::from(format!(
                    "{id_prefix}-speed-popout"
                )))
                .occlude()
                .absolute()
                .right(px(0.))
                .bottom(px(58.))
                .w(px(260.))
                .px(px(16.))
                .pt(px(14.))
                .pb(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lowest"))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("ZwPhbB")),
                )
                .child(div().flex().gap(px(4.)).children(pills))
        });
        let speed_btn = (!hide_speed).then(|| {
            let badge = ((rate - 1.0).abs() > 0.01).then(|| {
                div()
                    .absolute()
                    .right(px(-2.))
                    .bottom(px(-2.))
                    .px(px(3.))
                    .rounded(px(4.))
                    .bg(color("--brand-500"))
                    .text_size(px(9.))
                    .line_height(px(11.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--white"))
                    .child(format!("{rate}x"))
            });
            div()
                .relative()
                .child(
                    self.with_tip(
                        format!("{id_prefix}-speed-tip"),
                        Tip::text(dc_intl::t("ZwPhbB"), Side::Top),
                        div()
                            .id(gpui::SharedString::from(format!("{id_prefix}-speed")))
                            .relative()
                            .size(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(speed_open, |s| s.bg(rgba(0xffffff22)))
                            .hover(|s| s.bg(rgba(0xffffff1a)))
                            .child(icon("emoji_error_speedometer", 18., "--white"))
                            .children(badge)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                m_speed_toggle(
                                    this,
                                    Box::new(|s| s.speed_open = !s.speed_open),
                                    cx,
                                );
                            })),
                        cx,
                    ),
                )
                .children(speed_popout)
                .into_any_element()
        });

        let fs_icon = if state.fullscreen {
            "fullscreen_exit"
        } else {
            "voice_view_fullscreen"
        };
        let m_fs = on_mutate;
        let fs_btn = self.with_tip(
            format!("{id_prefix}-fs-tip"),
            Tip::text(dc_intl::t("z9Cnzv"), Side::Top),
            div()
                .id(gpui::SharedString::from(format!("{id_prefix}-fs")))
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff1a)))
                .child(icon(fs_icon, 18., "--white"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    m_fs(this, Box::new(|s| s.fullscreen = !s.fullscreen), cx);
                })),
            cx,
        );

        let gap = size.gap();
        div()
            .id(gpui::SharedString::from(format!("{id_prefix}-bar")))
            .relative()
            .w_full()
            .h(px(size.bar_height()))
            .px(px(size.pad_x()))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(gap))
            .child(
                // Start group: Play/Pause/Replay + Rewind 10s + Skip 10s
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(play_btn)
                    .child(rewind_btn)
                    .child(skip_btn),
            )
            .child(
                // Mid group: Volume + Duration
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(mute_btn)
                    .child(vol_slider)
                    .child(duration_btn),
            )
            .child(
                // End group: Transcript, Captions, Download, Speed, Fullscreen
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(transcript_btn)
                    .children(captions_btn)
                    .children(download_btn)
                    .children(speed_btn)
                    .child(fs_btn),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod dvp_tests {
    use super::*;

    #[test]
    fn dvp_control_bar_duration_seek_guard_and_shortcuts_match_40705() {
        let mut st = DvpControlState::for_video(15, 60, false);
        assert_eq!(st.duration_text(false), "0:15 / 1:00");
        assert_eq!(st.duration_text(true), "0:15");
        st.show_remaining = true;
        assert_eq!(st.duration_text(false), "-0:45 / 1:00");
        assert_eq!(DvpControlState::default().duration_text(false), "-:--");

        // At max watched: cannot seek forward (`YWbiPw`).
        assert!(!st.seek_forward_enabled());
        assert!(st.handle_shortcut("j"));
        assert_eq!(st.current_secs, 5);
        assert!(st.seek_forward_enabled());
        assert!(st.handle_shortcut("l"));
        assert_eq!(st.current_secs, 15);
        assert!(!st.seek_forward_enabled());

        assert!(st.handle_shortcut("k"));
        assert!(!st.playing);
        assert!(st.handle_shortcut("c"));
        assert!(st.captions_enabled);
        assert!(st.handle_shortcut("f"));
        assert!(st.fullscreen);
        assert!(st.handle_shortcut("m"));
        assert!(st.muted);
        assert!(st.handle_shortcut("up"));
        assert!(!st.muted);
        assert!(!st.handle_shortcut("x"));

        // Verify all 40705 i18n keys resolve cleanly.
        assert_eq!(dc_intl::t("dRVF+Z"), "Rewind 10 seconds");
        assert_eq!(dc_intl::t("yV2FLL"), "Skip ahead 10 seconds");
        assert_eq!(dc_intl::t("YWbiPw"), "Can't skip unwatched sections yet");
        assert_eq!(dc_intl::t("ZwPhbB"), "Playback speed");
        assert_eq!(dc_intl::t("6EjGUv"), "Toggle transcript");
        assert_eq!(dc_intl::t("0DbPcL"), "Toggle captions");
        assert_eq!(dc_intl::t("z9Cnzv"), "Toggle full screen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_vod::synthetic_mp4;

    #[test]
    fn video_hub_toggles_play_controls_and_auto_hides_after_timeouts() {
        let mp4 = Arc::new(
            synthetic_mp4(64, 64, 10, &[[220, 40, 40], [40, 220, 40]], 1).expect("synthetic mp4"),
        );
        let mut hub = VideoHub::new();
        hub.seed_bytes("https://example.test/v.mp4", mp4);
        let t0 = Instant::now();
        {
            hub.ensure_entry("k1", "https://example.test/v.mp4", (320, 180), false);
            let entry = hub.entries.get("k1").unwrap();
            assert!(!entry.has_clicked_play);
            assert!(!hub.controls_visible("k1", entry, false, t0));
        }

        hub.play_or_toggle("k1", t0);
        {
            hub.entries.get_mut("k1").unwrap().mouse_inside = true;
            let entry = hub.entries.get("k1").unwrap();
            // Visible right after start while inside.
            assert!(hub.controls_visible("k1", entry, false, t0 + Duration::from_secs(1)));
            // Auto-hides after 3000 ms inline.
            assert!(!hub.controls_visible("k1", entry, false, t0 + Duration::from_secs(4)));
            // Fullscreen timeout is 1000 ms.
            assert!(!hub.controls_visible("k1", entry, true, t0 + Duration::from_millis(1200)));
            // Immediate hide on mouse leave while playing.
            hub.entries.get_mut("k1").unwrap().mouse_inside = false;
            let entry = hub.entries.get("k1").unwrap();
            assert!(!hub.controls_visible("k1", entry, false, t0 + Duration::from_millis(200)));
        }

        // Paused state keeps controls visible even when mouse is outside.
        hub.play_or_toggle("k1", t0 + Duration::from_secs(5));
        let entry = hub.entries.get("k1").unwrap();
        assert!(!entry.tick.playing);
        assert!(hub.controls_visible("k1", entry, false, t0 + Duration::from_secs(20)));
    }

    static VOLUME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn volume_popout_honors_150ms_leave_grace_and_persists_slider() {
        let _guard = VOLUME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut hub = VideoHub::new();
        hub.ensure_entry("k2", "https://example.test/v2.mp4", (320, 180), false);
        let t0 = Instant::now();
        {
            let entry = hub.entries.get_mut("k2").unwrap();
            entry.volume_hover_btn = true;
            VideoHub::schedule_volume_hide(entry, t0);
        }
        assert!(hub.is_volume_open("k2", hub.entries.get("k2").unwrap(), t0));

        {
            let entry = hub.entries.get_mut("k2").unwrap();
            entry.volume_hover_btn = false;
            VideoHub::schedule_volume_hide(entry, t0);
        }
        let entry = hub.entries.get("k2").unwrap();
        assert!(hub.is_volume_open("k2", entry, t0 + Duration::from_millis(100)));
        assert!(!hub.is_volume_open("k2", entry, t0 + Duration::from_millis(200)));

        hub.set_slider_volume(50.0);
        assert!(!hub.muted);
        assert!(hub.volume > 0.0 && hub.volume < 1.0);
        hub.set_slider_volume(100.0);
        assert!((hub.volume - 1.0).abs() < 1e-4);
        assert!((volume_to_slider(hub.volume * 100.0) - 100.0).abs() < 1e-3);
        hub.toggle_mute();
        assert!(hub.muted);
    }

    #[test]
    fn volume_and_mute_sync_between_video_and_voice_hubs() {
        let _guard = VOLUME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut video = VideoHub::new();
        let mut voice = super::super::voice_message::Hub::new();
        video.set_slider_volume(40.0);
        let expected = video.volume;
        assert!(!video.muted && expected > 0.0);

        voice.sync_stored_volume();
        assert!((voice.volume - expected).abs() < 1e-6);
        assert!(!voice.muted);

        voice.toggle_mute();
        assert!(voice.muted);
        video.ensure_entry(
            "sync-test",
            "https://example.test/sync.mp4",
            (320, 180),
            false,
        );
        assert!(video.muted);

        video.set_slider_volume(0.0);
        assert!(video.muted);
        voice.toggle_mute();
        assert!(!voice.muted);
        assert!((voice.volume - expected).abs() < 1e-5);
        video.ensure_entry(
            "sync-test",
            "https://example.test/sync.mp4",
            (320, 180),
            false,
        );
        assert!(!video.muted);
        assert!((video.volume - expected).abs() < 1e-5);
    }

    #[test]
    fn video_hub_scrub_seek_and_release_drains_gpu_textures() {
        let mp4 = Arc::new(
            synthetic_mp4(
                64,
                64,
                3,
                &[
                    [200, 40, 40],
                    [200, 40, 40],
                    [200, 40, 40],
                    [40, 200, 40],
                    [40, 200, 40],
                    [40, 200, 40],
                ],
                3,
            )
            .expect("synthetic mp4"),
        );
        let mut hub = VideoHub::new();
        hub.seed_bytes("https://example.test/scrub.mp4", mp4.clone());
        hub.seed_bytes("https://example.test/scrub2.mp4", mp4);
        hub.ensure_entry("lb:att:1", "https://example.test/scrub.mp4", (64, 64), true);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut ready = false;
        while Instant::now() < deadline {
            if let Some(entry) = hub.entries.get_mut("lb:att:1") {
                entry.tick = entry.player.tick();
                if matches!(entry.tick.status, LoadStatus::Ready) && entry.tick.frame.is_some() {
                    ready = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "player should decode initial frame");

        if let Some(entry) = hub.entries.get_mut("lb:att:1") {
            entry.player.pause();
            entry.player.seek(1.0);
        }
        let seek_deadline = Instant::now() + Duration::from_secs(3);
        let mut seek_frame = false;
        while Instant::now() < seek_deadline {
            if let Some(entry) = hub.entries.get_mut("lb:att:1") {
                entry.tick = entry.player.tick();
                if entry.tick.frame.is_some() {
                    seek_frame = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seek_frame, "seek should decode keyframe");

        // Replacing `src` on an active key must queue the prior player's GPU frames for drop.
        hub.ensure_entry(
            "lb:att:1",
            "https://example.test/scrub2.mp4",
            (64, 64),
            true,
        );
        let (retired_swap, _) = hub.prepare(Instant::now());
        assert!(
            !retired_swap.is_empty(),
            "replacing src on an entry must drain its prior GPU textures"
        );

        let swap_deadline = Instant::now() + Duration::from_secs(3);
        let mut swap_ready = false;
        while Instant::now() < swap_deadline {
            if let Some(entry) = hub.entries.get_mut("lb:att:1") {
                entry.tick = entry.player.tick();
                if matches!(entry.tick.status, LoadStatus::Ready) && entry.tick.frame.is_some() {
                    swap_ready = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(swap_ready, "replacement player should decode initial frame");

        let retired = hub.release_prefix("lb:");
        assert!(
            !retired.is_empty(),
            "releasing player must yield its GPU frames"
        );
        assert!(hub.entries.is_empty());
    }

    #[test]
    fn lightbox_items_include_video_attachments_and_proxied_embeds_only() {
        let message = dc_state::Message::from_value(&serde_json::json!({
            "id": "1400000000000005001",
            "channel_id": "1",
            "type": 0,
            "content": "",
            "timestamp": "2026-11-13T08:28:00+00:00",
            "author": {"id": "1", "username": "ada"},
            "attachments": [{
                "id": "10",
                "filename": "clip.mp4",
                "size": 1234,
                "url": "https://cdn.example/clip.mp4",
                "proxy_url": "https://media.example/clip.mp4",
                "width": 640,
                "height": 360
            }],
            "embeds": [
                {
                    "type": "video",
                    "url": "https://fxtwitter.com/example/status/1",
                    "video": {
                        "url": "https://video.example/fx.mp4",
                        "proxy_url": "https://media.example/fx.mp4",
                        "width": 640,
                        "height": 360
                    }
                },
                {
                    "type": "video",
                    "url": "https://www.youtube.com/watch?v=1",
                    "video": {
                        "url": "https://www.youtube.com/embed/1",
                        "width": 640,
                        "height": 360
                    }
                }
            ]
        }))
        .unwrap();
        let items = super::super::lightbox::items_of(&message);
        assert_eq!(items.len(), 2);
        assert!(items[0].is_video);
        assert_eq!(
            items[0].video_src.as_deref(),
            Some("https://media.example/clip.mp4")
        );
        assert!(items[1].is_video);
        assert_eq!(
            items[1].video_src.as_deref(),
            Some("https://media.example/fx.mp4")
        );
    }

    #[test]
    fn player_design_tokens_resolve_in_midnight_and_dark() {
        for token in [
            "--white",
            "--interactive-text-default",
            "--interactive-background-hover",
            "--interactive-text-hover",
            "--text-feedback-critical",
            "--background-surface-high",
            "--border-subtle",
            "--text-default",
            "--brand-500",
            "--interactive-text-active",
            "--background-surface-higher",
            "--primary-300",
        ] {
            assert!(Theme::Midnight.has(token), "Midnight missing {token}");
            assert!(Theme::Dark.has(token), "Dark missing {token}");
        }
    }
}
