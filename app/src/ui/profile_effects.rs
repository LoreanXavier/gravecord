//! Profile effects: the animated layers drawn over a profile (modules 971217, 855915, 940622 and 942426, and the
//! `profileEffects_ZjcCyW` rules of stylesheet 291103). The config of an effect comes from its collectibles product
//! (`dc_state::profile_effects`); each layer is an APNG that this module decodes off the UI thread, scaled to the
//! size it is drawn at, and plays on the effect clock of the bundle's component:
//!
//! * the clock starts when every layer image is ready, 500 ms below zero (`delayIntro`), and a layer is on screen
//!   between its `start` and `start + duration`, repeating after its `loopDelay` when it loops
//!   (`EffectLayer::local_time`); every run of a layer starts its image from the first frame;
//! * the box is `inset: 0` over the profile with `overflow: hidden` and `pointer-events: none`, at the middle layer
//!   of the profile (above the banner, body and avatar, below the buttons on the banner); each image is as wide as
//!   the box, at `top: y - bannerAdjustment` and `left: x`; the effect fades to 35% over 0.6 s while the pointer is
//!   over the profile;
//! * with reduced motion, an unfocused window (for persistent effects and Shop previews), or autoplay off, the
//!   static image shows instead (`reducedMotionSrc`, or `thumbnailPreviewSrc` for Shop cards); an intermittent
//!   effect is not drawn at all in an unfocused window;
//! * the layers of the profile in view are the only ones decoded: a frame sits in a texture only while it is on
//!   screen, decoded clips are bounded (`CLIP_BUDGET` each, `DECODED_BUDGET` together), and everything an effect
//!   held is released the first render after its profile is gone.
//!
//! The Shop draws its product previews with [`Root::profile_effect_layer`] and [`Surface::shop_preview`].
use super::Root;
use crate::net::Request;
use dc_state::{
    Id,
    profile_effects::{AnimationType, EffectLayer, Equipped, ProfileEffect, ProfileEffectStore},
};
use gpui::{
    AnyElement, Context, IntoElement, ObjectFit, RenderImage, Window, div, img, prelude::*, px,
};
use image::{AnimationDecoder, Rgba, RgbaImage, imageops};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// `Update::Fetched` key of a product read: `profile-effect:<sku>`.
pub(super) const PRODUCT_PREFIX: &str = "profile-effect:";
/// `Update::Image` key of a layer or still: `profile-effect-art:<url>`.
pub(super) const ART_PREFIX: &str = "profile-effect-art:";

/// The clock of a profile effect starts this far below zero (`delayIntro`: `useState(-500)`); without the intro it is
/// 0.1 ms.
const INTRO_MS: f64 = 500.;
/// `transition: opacity .6s ease-in-out` of `.profileEffects_ZjcCyW`.
const FADE_MS: f64 = 600.;
/// `.hovered_ZjcCyW`.
const HOVER_OPACITY: f32 = 0.35;
/// Most decoded bytes of one clip. A clip that would be larger is drawn from smaller frames.
const CLIP_BUDGET: usize = 40 * 1024 * 1024;
/// Most decoded bytes of all clips together.
const DECODED_BUDGET: usize = 96 * 1024 * 1024;
/// Most downloaded bytes kept for reuse.
const ENCODED_BUDGET: usize = 32 * 1024 * 1024;
/// A layer is decoded this long before it starts.
const LOOKAHEAD_MS: f64 = 1500.;
/// A frame that claims no time is held this long, as browsers do.
const MIN_DELAY_MS: f64 = 100.;

/// How a surface draws an effect: the options of the bundle's component (`M` of module 971217) that a call site sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Surface {
    /// The width of the effect box in logical pixels; each layer image is drawn this wide (`width: 100%`).
    pub width: f32,
    /// `bannerAdjustment`: pixels the images are moved up.
    pub banner_adjustment: f32,
    /// The pointer is over the profile: the effect dims (`useOpacityOnHover`) and, when autoplay is off, plays.
    pub hovering: bool,
    /// `autoPlay`, true everywhere except the Shop's cards.
    pub auto_play: bool,
    pub use_opacity_on_hover: bool,
    /// `shopPreview`: drawn from a Shop card or modal.
    pub shop_preview: bool,
    /// `useThumbnail`: the still is the thumbnail while the pointer is away.
    pub use_thumbnail: bool,
    /// `delayIntro`.
    pub delay_intro: bool,
}

impl Surface {
    /// A profile popout or full profile: `<ProfileEffects skuId isHovering/>` with the component's defaults.
    pub fn profile(width: f32, hovering: bool) -> Self {
        Self {
            width,
            banner_adjustment: 0.,
            hovering,
            auto_play: true,
            use_opacity_on_hover: true,
            shop_preview: false,
            use_thumbnail: false,
            delay_intro: true,
        }
    }

    /// A Shop product preview (module 139136): thumbnail while idle, the animation from its start while the card is
    /// hovered or `auto_play` is set, no dimming. The Shop page calls this; nothing else does.
    #[allow(dead_code)]
    pub fn shop_preview(width: f32, hovering: bool, auto_play: bool) -> Self {
        Self {
            width,
            banner_adjustment: 0.,
            hovering,
            auto_play,
            use_opacity_on_hover: false,
            shop_preview: true,
            use_thumbnail: true,
            delay_intro: true,
        }
    }
}

/// What the component draws for one render (`I`, `p` and the null cases of `M`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Nothing: an intermittent effect in an unfocused window.
    Hidden,
    /// The still image.
    Still { thumbnail: bool },
    /// The layers on their clock.
    Live,
}

fn mode(effect: &ProfileEffect, surface: &Surface, focused: bool, reduced_motion: bool) -> Mode {
    if !focused && !surface.shop_preview && effect.animation_type == AnimationType::Intermittent {
        return Mode::Hidden;
    }
    let still = (!focused && surface.shop_preview)
        || (!focused && effect.animation_type == AnimationType::Persistent)
        || reduced_motion
        || (!surface.auto_play && !surface.hovering);
    if still {
        Mode::Still {
            thumbnail: surface.use_thumbnail && !surface.hovering,
        }
    } else {
        Mode::Live
    }
}

/// `ease-in-out` is `cubic-bezier(.42, 0, .58, 1)`.
fn ease_in_out(progress: f64) -> f64 {
    let (x1, y1, x2, y2) = (0.42, 0., 0.58, 1.);
    let at = |t: f64, a: f64, b: f64| {
        3. * a * (1. - t).powi(2) * t + 3. * b * (1. - t) * t * t + t.powi(3)
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..40 {
        let middle = (low + high) / 2.;
        if at(middle, x1, x2) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    at((low + high) / 2., y1, y2)
}

struct Fade {
    from: f32,
    to: f32,
    since: Instant,
}

impl Fade {
    fn progress(&self, now: Instant) -> f64 {
        (now.saturating_duration_since(self.since).as_secs_f64() * 1000. / FADE_MS).min(1.)
    }
    fn value(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease_in_out(self.progress(now)) as f32
    }
    fn target(&mut self, to: f32, now: Instant) {
        if (self.to - to).abs() > f32::EPSILON {
            *self = Fade {
                from: self.value(now),
                to,
                since: now,
            };
        }
    }
}

/// The frames of one image at the size it is drawn: an APNG layer (many frames and their timing) or a still (one).
pub(super) struct Clip {
    frames: Vec<Arc<RenderImage>>,
    /// End of each frame in milliseconds from the start of the run.
    ends_ms: Vec<f64>,
    total_ms: f64,
    /// `num_plays` of the APNG; 0 is forever.
    plays: u32,
    /// Size of the frames in pixels.
    size: (u32, u32),
    bytes: usize,
}

impl Clip {
    /// The frame at `local_ms` into a run of the layer, as a browser plays the APNG from the moment it is mounted: it
    /// loops `plays` times (forever for 0) and then holds its last frame.
    fn frame_at(&self, local_ms: f64) -> usize {
        if self.frames.len() <= 1 || self.total_ms <= 0. {
            return 0;
        }
        let held = self.plays != 0 && local_ms >= self.total_ms * f64::from(self.plays);
        if held {
            return self.frames.len() - 1;
        }
        let within = local_ms.rem_euclid(self.total_ms);
        self.ends_ms
            .partition_point(|end| *end <= within)
            .min(self.frames.len() - 1)
    }

    /// Milliseconds until the picture of `frame_at(local_ms)` changes, `None` when it never does.
    fn next_change(&self, local_ms: f64) -> Option<f64> {
        if self.frames.len() <= 1 || self.total_ms <= 0. {
            return None;
        }
        if self.plays != 0 && local_ms >= self.total_ms * f64::from(self.plays) {
            return None;
        }
        let within = local_ms.rem_euclid(self.total_ms);
        let index = self.frame_at(local_ms);
        Some((self.ends_ms[index] - within).max(1.))
    }
}

/// Image size, frame count and play count read from the PNG chunks without decoding any pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sniff {
    width: u32,
    height: u32,
    frames: u32,
    plays: u32,
}

/// `IHDR` and `acTL`, which both come before the first `IDAT`.
fn sniff(bytes: &[u8]) -> Option<Sniff> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let (mut width, mut height, mut frames, mut plays) = (0, 0, 1, 1);
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().ok()?) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + length)?;
        match kind {
            b"IHDR" if length >= 8 => {
                width = u32::from_be_bytes(body[0..4].try_into().ok()?);
                height = u32::from_be_bytes(body[4..8].try_into().ok()?);
            }
            b"acTL" if length >= 8 => {
                frames = u32::from_be_bytes(body[0..4].try_into().ok()?);
                plays = u32::from_be_bytes(body[4..8].try_into().ok()?);
            }
            b"IDAT" => break,
            _ => {}
        }
        at += 12 + length;
    }
    (width > 0 && height > 0 && frames > 0).then_some(Sniff {
        width,
        height,
        frames,
        plays,
    })
}

/// The width frames are decoded at: the width they are drawn at in device pixels, never above the source and small
/// enough that all frames fit `budget`.
fn decode_width(info: Sniff, wanted: u32, budget: usize) -> u32 {
    let aspect = f64::from(info.height) / f64::from(info.width);
    let fits = (budget as f64 / (f64::from(info.frames) * 4. * aspect)).sqrt();
    (fits.floor() as u32).min(wanted).min(info.width).max(1)
}

/// Scales straight-alpha pixels by averaging premultiplied ones, so transparent edges do not darken.
pub(super) fn scale(image: RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        return image;
    }
    let mut image = image;
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel.0[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut scaled = imageops::resize(&image, width, height, imageops::FilterType::CatmullRom);
    for Rgba(pixel) in scaled.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    scaled
}

pub(super) fn render_image(mut buffer: RgbaImage) -> Arc<RenderImage> {
    for pixel in buffer.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// Decodes one layer or still to `wanted` pixels wide, frame by frame, so only the scaled frames are ever held.
fn decode(bytes: &[u8], wanted: u32) -> Option<Clip> {
    let info = sniff(bytes)?;
    let width = decode_width(info, wanted, CLIP_BUDGET);
    let height =
        ((f64::from(width) * f64::from(info.height) / f64::from(info.width)).round() as u32).max(1);
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).ok()?;
    let mut clip = Clip {
        frames: Vec::new(),
        ends_ms: Vec::new(),
        total_ms: 0.,
        plays: info.plays,
        size: (width, height),
        bytes: 0,
    };
    if decoder.is_apng().ok()? {
        for frame in decoder.apng().ok()?.into_frames() {
            let frame = frame.ok()?;
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            let mut delay = f64::from(numerator) / f64::from(denominator.max(1));
            if delay <= 10. {
                delay = MIN_DELAY_MS;
            }
            clip.total_ms += delay;
            clip.ends_ms.push(clip.total_ms);
            clip.frames
                .push(render_image(scale(frame.into_buffer(), width, height)));
        }
    } else {
        let picture = image::DynamicImage::from_decoder(decoder).ok()?.to_rgba8();
        clip.frames
            .push(render_image(scale(picture, width, height)));
        clip.ends_ms.push(0.);
    }
    clip.bytes = clip.frames.len() * (width as usize) * (height as usize) * 4;
    (!clip.frames.is_empty()).then_some(clip)
}

/// Milliseconds until this layer next appears, disappears or restarts, from `time_ms` on the effect clock.
fn layer_toggle(layer: &EffectLayer, time_ms: f64) -> Option<f64> {
    if time_ms < layer.start_ms {
        return Some(layer.start_ms - time_ms);
    }
    let local = time_ms - layer.start_ms;
    if !layer.looping {
        return (local <= layer.duration_ms).then_some(layer.duration_ms - local + 1.);
    }
    let delay = layer.loop_delay_ms.filter(|delay| *delay > 0.)?;
    let cycle = layer.duration_ms + delay;
    let within = local - (local / cycle).floor() * cycle;
    Some(if within <= layer.duration_ms {
        layer.duration_ms - within + 1.
    } else {
        cycle - within
    })
}

/// Whether a layer's frames are needed at `time_ms`: from `LOOKAHEAD_MS` before it starts until it has run to its end,
/// and for good when it loops.
fn layer_needed(layer: &EffectLayer, time_ms: f64) -> bool {
    time_ms >= layer.start_ms - LOOKAHEAD_MS
        && (layer.looping || time_ms <= layer.start_ms + layer.duration_ms)
}

/// A clip is keyed by its source and the pixel width it was decoded at.
type ClipKey = (String, u32);

struct Slot {
    clip: Arc<Clip>,
    pass: u64,
}

#[derive(Default)]
struct Instance {
    pass: u64,
    sku: Id,
    /// Which of the randomised sources every layer uses (`b` of module 971217).
    pick: usize,
    mode: Option<Mode>,
    /// When the layers were all ready and the effect clock started.
    live_since: Option<Instant>,
    /// Harness: the effect clock stands at this many milliseconds.
    frozen_ms: Option<f64>,
    fade: Option<Fade>,
    shown: HashMap<usize, (usize, Arc<RenderImage>)>,
    wake: Option<Duration>,
}

struct Override {
    sku: Id,
    frozen_ms: Option<f64>,
}

#[derive(Default)]
pub(super) struct Cache {
    store: ProfileEffectStore,
    instances: HashMap<String, Instance>,
    hovered: HashSet<String>,
    encoded: HashMap<String, (Arc<Vec<u8>>, u64)>,
    requested: HashSet<String>,
    failed: HashSet<String>,
    clips: HashMap<ClipKey, Slot>,
    decoding: HashMap<ClipKey, usize>,
    pass: u64,
    scale: f32,
    deadline: Option<Instant>,
    generation: u64,
    harness: Option<Override>,
    /// Harness: reduced motion is on for effects whatever the setting says.
    force_reduced: bool,
    retired_textures: u64,
    /// Instances opened and dropped, for the capture log.
    opened: u64,
    released: u64,
    seed: u64,
}

impl Cache {
    fn decoded_bytes(&self) -> usize {
        self.clips
            .values()
            .map(|slot| slot.clip.bytes)
            .sum::<usize>()
            + self.decoding.values().sum::<usize>()
    }

    fn encoded_bytes(&self) -> usize {
        self.encoded.values().map(|(bytes, _)| bytes.len()).sum()
    }

    /// A pseudo-random pick for a new instance. Effects only need the choice to vary between opens.
    fn random(&mut self, range: usize) -> usize {
        if range == 0 {
            return 0;
        }
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            ^ SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos() as u64);
        ((self.seed >> 33) as usize) % range
    }

    fn evict_encoded(&mut self, keep: &HashSet<String>) {
        while self.encoded_bytes() > ENCODED_BUDGET {
            let oldest = self
                .encoded
                .iter()
                .filter(|(url, _)| !keep.contains(*url))
                .min_by_key(|(_, (_, pass))| *pass)
                .map(|(url, _)| url.clone());
            let Some(url) = oldest else { break };
            self.encoded.remove(&url);
            self.requested.remove(&url);
        }
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |time| time.as_secs_f64() * 1000.)
}

fn art_key(url: &str) -> String {
    format!("{ART_PREFIX}{url}")
}

/// The pieces an element needs, copied out of the cache so painting does not hold its borrow.
struct Painted {
    layers: Vec<(f32, f32, f32, f32, Arc<RenderImage>)>,
    opacity: f32,
}

impl Root {
    /// The sku of the effect `user`'s profile shows, from the profile that was read for it (the harness can stand one
    /// in): see `Equipped::from_profile`.
    pub(super) fn profile_effect_sku(&self, user: Id) -> Option<Id> {
        if let Some(over) = &self.profile_effects.borrow().harness {
            return Some(over.sku);
        }
        let body = self.fetched.get(&format!("profile:{user}"))?;
        Equipped::from_profile(body, self.guild, now_ms()).map(|effect| effect.sku_id)
    }

    pub(super) fn profile_effect_hovered(&self, key: &str) -> bool {
        self.profile_effects.borrow().hovered.contains(key)
    }

    /// The pointer entered or left the profile that `key` names.
    pub(super) fn profile_effect_hover(&mut self, key: &str, inside: bool, cx: &mut Context<Self>) {
        let mut cache = self.profile_effects.borrow_mut();
        let changed = if inside {
            cache.hovered.insert(key.to_owned())
        } else {
            cache.hovered.remove(key)
        };
        drop(cache);
        if changed {
            cx.notify();
        }
    }

    /// The effect layer of `sku` for one surface, ready to be the last child of a `relative` profile element: an
    /// absolute box over the whole element that does not take the pointer. `key` names the instance (one per
    /// open profile or preview: `"popout"`, `"full-profile"`, `"shop-<sku>"`) and keeps its clock and fade between
    /// renders; an instance that is not asked for in one render is released at the next.
    ///
    /// Returns `None` while the product, the layers or the still are still loading (this asks for them and the view
    /// is drawn again when they arrive), when the effect does not apply in the current mode, and when its art failed.
    pub(super) fn profile_effect_layer(
        &self,
        key: &str,
        sku: Id,
        surface: Surface,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let focused = window.is_window_active() || self.profile_effects.borrow().harness.is_some();
        let reduced =
            self.pref_on("reduced_motion", false) || self.profile_effects.borrow().force_reduced;
        let now = Instant::now();
        let mut retired = Vec::new();
        let mut decode_jobs: Vec<(ClipKey, Arc<Vec<u8>>)> = Vec::new();
        let mut needs_art: Vec<String> = Vec::new();
        let painted = {
            let mut guard = self.profile_effects.borrow_mut();
            let cache = &mut *guard;
            if cache.store.begin_fetch(sku, now_ms()) {
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{PRODUCT_PREFIX}{sku}"),
                    route: dc_state::profile_effects::product_route(sku),
                });
            }
            let effect = cache.store.get(sku).cloned()?;
            let pixel_width = ((surface.width * cache.scale.max(1.)).ceil() as u32).max(1);
            let pass = cache.pass;
            let frozen = cache.harness.as_ref().and_then(|over| over.frozen_ms);
            let mut instance = match cache.instances.remove(key) {
                Some(known) if known.sku == sku => known,
                _ => {
                    cache.opened += 1;
                    Instance {
                        sku,
                        pick: cache.random(effect.pick_range()),
                        ..Instance::default()
                    }
                }
            };
            instance.pass = pass;
            instance.frozen_ms = frozen;
            let wanted_mode = mode(&effect, &surface, focused, reduced);
            if instance.mode != Some(wanted_mode) {
                // The component remounts when it switches between the still and the layers: its clock starts over.
                if instance.mode == Some(Mode::Live) || wanted_mode == Mode::Live {
                    instance.live_since = None;
                }
                for (_, (_, frame)) in instance.shown.drain() {
                    retired.push(frame);
                }
                instance.mode = Some(wanted_mode);
            }
            let target = if surface.hovering && surface.use_opacity_on_hover {
                HOVER_OPACITY
            } else {
                1.
            };
            instance
                .fade
                .get_or_insert(Fade {
                    from: target,
                    to: target,
                    since: now,
                })
                .target(target, now);
            let opacity = instance.fade.as_ref().map_or(1., |fade| fade.value(now));
            let fading = instance
                .fade
                .as_ref()
                .is_some_and(|fade| fade.progress(now) < 1.);
            instance.wake = fading.then_some(Duration::from_millis(16));

            // The sources this render needs: one still, or the layers that are on or about to be (a layer that has
            // run to its end, or is more than `LOOKAHEAD_MS` from its start, is not held in memory).
            let intro = if surface.delay_intro { INTRO_MS } else { 0.1 };
            let clock_ms = instance.frozen_ms.unwrap_or_else(|| {
                instance.live_since.map_or(-intro, |start| {
                    now.saturating_duration_since(start).as_secs_f64() * 1000. - intro
                })
            });
            let sources: Vec<(usize, String, u32)> = match wanted_mode {
                Mode::Hidden => Vec::new(),
                Mode::Still { thumbnail } => {
                    let url = if thumbnail && !effect.thumbnail_preview_src.is_empty() {
                        effect.thumbnail_preview_src.clone()
                    } else {
                        effect.reduced_motion_src.clone()
                    };
                    if url.is_empty() {
                        Vec::new()
                    } else {
                        vec![(0, url, pixel_width)]
                    }
                }
                Mode::Live => effect
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(_, layer)| layer_needed(layer, clock_ms))
                    .map(|(index, layer)| {
                        (index, layer.source(instance.pick).to_owned(), pixel_width)
                    })
                    .collect(),
            };
            let mut ready = wanted_mode != Mode::Hidden;
            for (_, url, width) in &sources {
                let clip_key = (url.clone(), *width);
                if let Some(slot) = cache.clips.get_mut(&clip_key) {
                    slot.pass = pass;
                    continue;
                }
                ready = false;
                if cache.failed.contains(url) {
                    continue;
                }
                let known = cache.encoded.get_mut(url).map(|(bytes, used)| {
                    *used = pass;
                    bytes.clone()
                });
                if let Some(bytes) = known {
                    if !cache.decoding.contains_key(&clip_key) {
                        let estimate = sniff(&bytes)
                            .map(|info| {
                                let used = decode_width(info, *width, CLIP_BUDGET);
                                let height = (f64::from(used) * f64::from(info.height)
                                    / f64::from(info.width))
                                    as usize;
                                info.frames as usize * used as usize * height * 4
                            })
                            .unwrap_or(0);
                        if cache.decoded_bytes() + estimate <= DECODED_BUDGET {
                            cache.decoding.insert(clip_key.clone(), estimate);
                            decode_jobs.push((clip_key, bytes));
                        }
                    }
                } else if cache.requested.insert(url.clone()) {
                    needs_art.push(url.clone());
                }
            }
            // Layers that start later are downloaded now so their frames can be decoded in time.
            if wanted_mode == Mode::Live {
                for layer in &effect.layers {
                    let url = layer.source(instance.pick);
                    if !cache.encoded.contains_key(url)
                        && !cache.failed.contains(url)
                        && cache.requested.insert(url.to_owned())
                    {
                        needs_art.push(url.to_owned());
                    }
                }
            }

            let mut painted = None;
            if ready {
                let clips: HashMap<usize, Arc<Clip>> = sources
                    .iter()
                    .map(|(index, url, width)| {
                        (*index, cache.clips[&(url.clone(), *width)].clip.clone())
                    })
                    .collect();
                match wanted_mode {
                    Mode::Hidden => {}
                    Mode::Still { .. } => {
                        let clip = &clips[&0];
                        let height = surface.width * clip.size.1 as f32 / clip.size.0 as f32;
                        painted = Some(Painted {
                            layers: vec![(
                                0.,
                                -surface.banner_adjustment,
                                surface.width,
                                height,
                                clip.frames[0].clone(),
                            )],
                            opacity,
                        });
                    }
                    Mode::Live => {
                        let start = *instance.live_since.get_or_insert(now);
                        let time_ms = instance.frozen_ms.unwrap_or_else(|| {
                            now.saturating_duration_since(start).as_secs_f64() * 1000. - intro
                        });
                        let mut layers = Vec::new();
                        let mut wake: Option<f64> = None;
                        let mut soonest =
                            |ms: f64| wake = Some(wake.map_or(ms, |old: f64| old.min(ms)));
                        let mut alive = HashSet::new();
                        for (index, layer) in effect.layers.iter().enumerate() {
                            if let Some(ms) = layer_toggle(layer, time_ms) {
                                soonest(ms);
                            }
                            if time_ms < layer.start_ms - LOOKAHEAD_MS {
                                soonest(layer.start_ms - LOOKAHEAD_MS - time_ms);
                            }
                            let (Some(clip), Some(local)) =
                                (clips.get(&index), layer.local_time(time_ms))
                            else {
                                continue;
                            };
                            let frame_index = clip.frame_at(local);
                            if let Some(ms) = clip.next_change(local) {
                                soonest(ms);
                            }
                            alive.insert(index);
                            let frame = clip.frames[frame_index].clone();
                            if instance
                                .shown
                                .get(&index)
                                .is_none_or(|(known, _)| *known != frame_index)
                                && let Some((_, old)) =
                                    instance.shown.insert(index, (frame_index, frame.clone()))
                            {
                                retired.push(old);
                            }
                            let width = surface.width;
                            let height = width * clip.size.1 as f32 / clip.size.0 as f32;
                            layers.push((
                                layer.position.0 as f32,
                                layer.position.1 as f32 - surface.banner_adjustment,
                                width,
                                height,
                                frame,
                            ));
                        }
                        let gone: Vec<usize> = instance
                            .shown
                            .keys()
                            .copied()
                            .filter(|index| !alive.contains(index))
                            .collect();
                        for index in gone {
                            if let Some((_, old)) = instance.shown.remove(&index) {
                                retired.push(old);
                            }
                        }
                        if instance.frozen_ms.is_none() {
                            let layer_wake =
                                wake.map(|ms| Duration::from_secs_f64(ms.max(1.) / 1000.));
                            instance.wake = match (instance.wake, layer_wake) {
                                (Some(a), Some(b)) => Some(a.min(b)),
                                (a, b) => a.or(b),
                            };
                        }
                        painted = Some(Painted { layers, opacity });
                    }
                }
            }
            let wake = instance.wake;
            cache.instances.insert(key.to_owned(), instance);
            if let Some(wake) = wake {
                let deadline = now + wake;
                if cache.deadline.is_none_or(|old| deadline < old) {
                    cache.generation += 1;
                    cache.deadline = Some(deadline);
                    let generation = cache.generation;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(deadline.saturating_duration_since(Instant::now()))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            let mut cache = this.profile_effects.borrow_mut();
                            if cache.generation == generation {
                                cache.deadline = None;
                                drop(cache);
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            }
            painted
        };
        self.profile_effect_work(needs_art, decode_jobs, retired, cx);
        let painted = painted?;
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .overflow_hidden()
                .opacity(painted.opacity)
                .children(
                    painted
                        .layers
                        .into_iter()
                        .map(|(x, y, width, height, frame)| {
                            img(frame)
                                .absolute()
                                .left(px(x))
                                .top(px(y))
                                .w(px(width))
                                .h(px(height))
                                .object_fit(ObjectFit::Fill)
                        }),
                )
                .into_any_element(),
        )
    }

    /// Starts the downloads and decodes a render asked for and releases the textures it moved off.
    fn profile_effect_work(
        &self,
        art: Vec<String>,
        decodes: Vec<(ClipKey, Arc<Vec<u8>>)>,
        retired: Vec<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        for url in art {
            let _ = self.requests.send(Request::Image {
                key: art_key(&url),
                url,
            });
        }
        for (key, bytes) in decodes {
            let wanted = key.1;
            cx.spawn(async move |this, cx| {
                let clip = cx
                    .background_executor()
                    .spawn(async move { decode(&bytes, wanted) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let mut cache = this.profile_effects.borrow_mut();
                    cache.decoding.remove(&key);
                    match clip {
                        Some(clip) => {
                            let pass = cache.pass;
                            cache.clips.insert(
                                key,
                                Slot {
                                    clip: Arc::new(clip),
                                    pass,
                                },
                            );
                        }
                        None => {
                            cache.failed.insert(key.0);
                        }
                    }
                    drop(cache);
                    cx.notify();
                });
            })
            .detach();
        }
        if !retired.is_empty() {
            self.profile_effects.borrow_mut().retired_textures += retired.len() as u64;
            // The window being drawn is back in the app's window map when deferred effects run.
            cx.defer(move |cx| {
                for image in retired {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    /// The body of a product read for an effect (`PRODUCT_PREFIX`). True when the key was an effect's.
    pub(super) fn profile_effect_fetched(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(sku) = key
            .strip_prefix(PRODUCT_PREFIX)
            .and_then(|sku| sku.parse::<Id>().ok())
        else {
            return false;
        };
        self.profile_effects
            .borrow_mut()
            .store
            .fetched(sku, body, now_ms());
        cx.notify();
        true
    }

    /// A downloaded layer or still (`ART_PREFIX`).
    pub(super) fn profile_effect_art_fetched(
        &mut self,
        key: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = key.strip_prefix(ART_PREFIX) {
            let mut cache = self.profile_effects.borrow_mut();
            let pass = cache.pass;
            cache
                .encoded
                .insert(url.to_owned(), (Arc::new(bytes), pass));
            let keep: HashSet<String> = std::iter::once(url.to_owned()).collect();
            cache.evict_encoded(&keep);
            drop(cache);
            cx.notify();
        }
    }

    pub(super) fn profile_effect_art_failed(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(url) = key.strip_prefix(ART_PREFIX) {
            self.profile_effects
                .borrow_mut()
                .failed
                .insert(url.to_owned());
            cx.notify();
        }
    }

    /// Called at the start of every render: releases what instances that were not drawn since the last render held
    /// (the profile closed), and remembers the window scale the layers are decoded for.
    pub(super) fn profile_effects_sweep(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut cache = self.profile_effects.borrow_mut();
        cache.scale = window.scale_factor();
        if cache.instances.is_empty() && cache.clips.is_empty() {
            cache.hovered.clear();
            cache.pass += 1;
            return;
        }
        let current = cache.pass;
        let gone: Vec<String> = cache
            .instances
            .iter()
            .filter(|(_, instance)| instance.pass != current)
            .map(|(key, _)| key.clone())
            .collect();
        let mut images = Vec::new();
        for key in gone {
            if let Some(instance) = cache.instances.remove(&key) {
                cache.released += 1;
                images.extend(instance.shown.into_values().map(|(_, frame)| frame));
            }
        }
        // A profile that closes under the pointer never sees the pointer leave: only profiles that show an effect keep
        // their hover state.
        let live: HashSet<String> = cache.instances.keys().cloned().collect();
        cache.hovered.retain(|key| live.contains(key));
        let unused: Vec<ClipKey> = cache
            .clips
            .iter()
            .filter(|(_, slot)| slot.pass != current)
            .map(|(key, _)| key.clone())
            .collect();
        for key in unused {
            if let Some(slot) = cache.clips.remove(&key) {
                images.extend(slot.clip.frames.iter().cloned());
            }
        }
        cache.retired_textures += images.len() as u64;
        cache.pass += 1;
        drop(cache);
        for image in images {
            cx.drop_image(image, Some(window));
        }
    }

    /// One line for the capture log: what the effects hold right now.
    #[cfg(feature = "capture")]
    pub(super) fn profile_effect_status(&self) -> String {
        let cache = self.profile_effects.borrow();
        let shown: usize = cache
            .instances
            .values()
            .map(|instance| instance.shown.len())
            .sum();
        format!(
            "effects={} instances={} clips={} decoded_bytes={} encoded_bytes={} textures_shown={shown} retired_textures={} opened={} released={} failed={}",
            cache.store.len(),
            cache.instances.len(),
            cache.clips.len(),
            cache.decoded_bytes(),
            cache.encoded_bytes(),
            cache.retired_textures,
            cache.opened,
            cache.released,
            cache.failed.len(),
        )
    }

    /// Capture harness step `profile-effect <sku> [at <ms>]` / `profile-effect off` / `profile-effect state`: shows the
    /// effect of `sku`, read from the collectibles product (a GET), over whichever profile is open, in this process
    /// only. `at <ms>` stops the effect clock at that many milliseconds so a frame can be read again.
    #[cfg(feature = "capture")]
    pub(crate) fn script_profile_effect(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        match words {
            ["off"] => {
                self.profile_effects.borrow_mut().harness = None;
                cx.notify();
                "profile effect: off".to_owned()
            }
            ["state"] => self.profile_effect_status(),
            ["reduced", on] => {
                self.profile_effects.borrow_mut().force_reduced = *on == "on";
                cx.notify();
                format!("profile effect: reduced motion {on}")
            }
            [sku, rest @ ..] => {
                let Ok(sku) = sku.parse::<Id>() else {
                    return format!("profile effect: bad sku {sku}");
                };
                let frozen_ms = match rest {
                    ["at", ms] => ms.parse::<f64>().ok(),
                    _ => None,
                };
                self.profile_effects.borrow_mut().harness = Some(Override { sku, frozen_ms });
                cx.notify();
                format!("profile effect: {sku} frozen_ms={frozen_ms:?}")
            }
            [] => "profile effect: usage".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(ends: &[f64], plays: u32) -> Clip {
        let frame = Arc::new(RenderImage::new(vec![image::Frame::new(RgbaImage::new(
            1, 1,
        ))]));
        Clip {
            frames: ends.iter().map(|_| frame.clone()).collect(),
            ends_ms: ends.to_vec(),
            total_ms: *ends.last().unwrap(),
            plays,
            size: (1, 1),
            bytes: 0,
        }
    }

    #[test]
    fn frames_follow_their_own_delays_and_loop_or_hold_by_the_play_count() {
        let once = clip(&[100., 300., 600.], 1);
        assert_eq!(once.frame_at(0.), 0);
        assert_eq!(once.frame_at(99.), 0);
        assert_eq!(once.frame_at(100.), 1);
        assert_eq!(once.frame_at(599.), 2);
        assert_eq!(once.frame_at(600.), 2);
        assert_eq!(once.frame_at(5000.), 2);
        assert_eq!(once.next_change(150.), Some(150.));
        assert_eq!(once.next_change(700.), None);
        let forever = clip(&[100., 300., 600.], 0);
        assert_eq!(forever.frame_at(650.), 0);
        assert_eq!(forever.frame_at(700.), 1);
        assert_eq!(forever.next_change(650.), Some(50.));
        let twice = clip(&[100., 200.], 2);
        assert_eq!(twice.frame_at(250.), 0);
        assert_eq!(twice.frame_at(400.), 1);
        assert_eq!(twice.frame_at(399.), 1);
    }

    #[test]
    fn the_mode_follows_motion_focus_and_autoplay() {
        let effect = |animation_type| ProfileEffect {
            animation_type,
            ..ProfileEffect::default()
        };
        let live = Surface::profile(300., false);
        let intermittent = effect(AnimationType::Intermittent);
        let persistent = effect(AnimationType::Persistent);
        assert_eq!(mode(&intermittent, &live, true, false), Mode::Live);
        assert_eq!(
            mode(&intermittent, &live, true, true),
            Mode::Still { thumbnail: false }
        );
        assert_eq!(mode(&intermittent, &live, false, false), Mode::Hidden);
        assert_eq!(
            mode(&persistent, &live, false, false),
            Mode::Still { thumbnail: false }
        );
        let shop = Surface::shop_preview(300., false, false);
        assert_eq!(
            mode(&intermittent, &shop, true, false),
            Mode::Still { thumbnail: true }
        );
        assert_eq!(
            mode(&intermittent, &shop, false, false),
            Mode::Still { thumbnail: true }
        );
        let hovered = Surface::shop_preview(300., true, false);
        assert_eq!(mode(&intermittent, &hovered, true, false), Mode::Live);
    }

    #[test]
    fn a_layer_is_held_from_shortly_before_its_start_until_it_ends() {
        let once = EffectLayer {
            start_ms: 5500.,
            duration_ms: 1000.,
            ..EffectLayer::default()
        };
        assert!(!layer_needed(&once, 3999.));
        assert!(layer_needed(&once, 4000.));
        assert!(layer_needed(&once, 6500.));
        assert!(!layer_needed(&once, 6501.));
        let looped = EffectLayer {
            looping: true,
            ..once
        };
        assert!(layer_needed(&looped, 1e9));
    }

    #[test]
    fn a_layer_names_when_it_next_changes() {
        let layer = EffectLayer {
            looping: true,
            start_ms: 5500.,
            duration_ms: 5042.,
            loop_delay_ms: Some(3500.),
            ..EffectLayer::default()
        };
        assert_eq!(layer_toggle(&layer, 0.), Some(5500.));
        assert_eq!(layer_toggle(&layer, 5500.), Some(5043.));
        assert_eq!(layer_toggle(&layer, 11000.), Some(3042. - 0.));
        let once = EffectLayer {
            duration_ms: 5000.,
            ..EffectLayer::default()
        };
        assert_eq!(layer_toggle(&once, 100.), Some(4901.));
        assert_eq!(layer_toggle(&once, 6000.), None);
    }

    #[test]
    fn the_fade_follows_ease_in_out_over_six_tenths_of_a_second() {
        assert!((ease_in_out(0.) - 0.).abs() < 1e-6);
        assert!((ease_in_out(1.) - 1.).abs() < 1e-6);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-3);
        assert!(ease_in_out(0.25) < 0.25 && ease_in_out(0.75) > 0.75);
        let start = Instant::now();
        let mut fade = Fade {
            from: 1.,
            to: 1.,
            since: start,
        };
        fade.target(HOVER_OPACITY, start);
        assert_eq!(fade.value(start), 1.);
        let end = start + Duration::from_millis(600);
        assert!((fade.value(end) - HOVER_OPACITY).abs() < 1e-6);
        assert!(fade.progress(start + Duration::from_millis(100)) < 1.);
    }

    #[test]
    fn png_headers_give_size_frames_and_plays_and_frames_are_decoded_at_a_bounded_size() {
        let animated = include_bytes!("../../../tests/fixtures/profile_effect_layer.png");
        let info = sniff(animated).unwrap();
        assert_eq!(
            (info.width, info.height, info.frames, info.plays),
            (8, 16, 3, 2)
        );
        assert!(sniff(b"not a png").is_none());
        let clip = decode(animated, 8).unwrap();
        assert_eq!(clip.frames.len(), 3);
        assert_eq!(clip.ends_ms, vec![100., 300., 600.]);
        assert_eq!(clip.plays, 2);
        assert_eq!(clip.size, (8, 16));
        assert_eq!(clip.bytes, 3 * 8 * 16 * 4);
        let small = decode(animated, 4).unwrap();
        assert_eq!(small.size, (4, 8));
        // Frames are never made larger than the source.
        assert_eq!(decode(animated, 64).unwrap().size, (8, 16));
        assert_eq!(decode_width(info, 1000, 3 * 4 * 4 * 8), 4);
    }

    #[test]
    fn scaling_keeps_transparent_edges_from_darkening() {
        let mut image = RgbaImage::new(2, 1);
        image.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
        let scaled = scale(image, 1, 1);
        let pixel = scaled.get_pixel(0, 0).0;
        assert!(pixel[3] > 100 && pixel[3] < 160);
        assert!(
            pixel[0] > 240,
            "premultiplied averaging keeps the colour: {pixel:?}"
        );
    }
}
