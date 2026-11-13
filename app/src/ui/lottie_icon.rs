//! Animated Lottie icons of the call control tray. The official buttons draw these with the Lottie icon player of
//! module 744682 (lottie-web, SVG renderer, `loop: false`, `autoplay: false`), each through a hook that names the
//! animation file and its markers:
//!
//! | hook   | button (wrapper module)                 | animation module | markers (start, duration)                          |
//! |--------|-----------------------------------------|------------------|----------------------------------------------------|
//! | 416696 | screen share (958449, 528057)           | 478313           | enable 0+70, disable 90+70, hover_enabled 180+40, hover_disabled 240+40 |
//! | 446080 | camera (970636)                         | 55089            | disable 0+70, enable 100+70, hover_enabled 180+70, hover_disabled 280+70 |
//! | 523875 | microphone (577062, user panel 993382)  | 705844           | mute 0+70, unmute 100+70, hover_unmuted 180+40, hover_muted 240+40 |
//! | 348858 | deafen (user panel 993382)              | 894619           | deafen 0+70, undeafen 110+70, hover_undeafened 200+70, hover_deafened 300+70 |
//! | 621956 | soundboard (533978)                     | 206905           | all 0+66                                           |
//! | 697744 | activity (168754, via 543274)           | 695941           | hover 0+59, click 68+26                            |
//! | 377802 | disconnect (905216, 404355)             | 772203           | all 0+90                                           |
//!
//! The files are `app/assets/lottie/call/<name>.json`, extracted unchanged from those modules.
//!
//! Player rules (module 744682, over lottie-web's `AnimationItem` in module 883885):
//! * At rest after mounting, the frame shown is the start of the hook's `initialAnimation` (the hook argument at mount),
//!   or of the only marker when there is one, or the file's in point (`initialSegment` and `autoplay: false`).
//! * `play(marker)` remembers the marker and plays `[start, start + duration]` from its start at once, whatever was
//!   playing (`resetSegments(true)` then `playSegments(.., true)`); the hover markers do not loop (the
//!   `lottie_hover_multiple_loop` design toggle is off by default). A finished run rests on `start + duration - 1`
//!   (`totalFrames - 1` of the segment).
//! * `stopIfPlaying(marker)` jumps to that marker's start and stops, only when it is the last marker played (finished
//!   or not).
//! * Reduced motion (or a hidden main window): `play` jumps straight to `start + duration` and stops;
//!   `stopIfPlaying` does nothing. Here only reduced motion (`motion::reduced`) takes that branch; the hidden-window
//!   half (`isMainWindowVisible`) is not followed yet.
//! * Colour: with `lottieIconColors_69Onuk` every fill and stroke outside `<defs>` takes `--__lottieIconColor` (the
//!   tray passes `currentColor`), keeping its own `fill-opacity` / `stroke-opacity`; masks and mattes keep their own
//!   colours. Every drawn pixel therefore has the tint's colour, and only its coverage comes from the file: a frame is
//!   rendered once as an alpha mask and tinted per colour.
//!
//! lottie-web renders sub-frames on the rAF clock; here a frame is a whole frame of the file (60 fps for all seven),
//! `start + floor(0.001 + elapsed * fps / 1000)`, on the motion clock.
//!
//! The player follows its state while the element is built; the picture is made at paint time, at the scale of the
//! window it lands in (the call can be drawn in the main window or the popped-out one).

use super::{Root, motion};
use gpui::{AnyElement, Corners, IntoElement, RenderImage, Rgba, Styled, div, px};
use rasterlottie::{Animation, PreparedAnimation, RenderConfig, Renderer, Rgba8};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// One named segment of an animation, as the hook module declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Marker {
    pub name: &'static str,
    pub start: u32,
    pub duration: u32,
}

impl Marker {
    const fn new(name: &'static str, start: u32, duration: u32) -> Self {
        Self {
            name,
            start,
            duration,
        }
    }

    /// `start + duration`: the last frame of the segment handed to lottie-web.
    pub(crate) const fn end(self) -> u32 {
        self.start + self.duration
    }
}

/// The tray animations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Anim {
    ScreenShare,
    Camera,
    Microphone,
    Deafen,
    Soundboard,
    Activity,
    Disconnect,
}

const SCREEN_SHARE: [Marker; 4] = [
    Marker::new("enable", 0, 70),
    Marker::new("disable", 90, 70),
    Marker::new("hover_enabled", 180, 40),
    Marker::new("hover_disabled", 240, 40),
];
const CAMERA: [Marker; 4] = [
    Marker::new("disable", 0, 70),
    Marker::new("enable", 100, 70),
    Marker::new("hover_enabled", 180, 70),
    Marker::new("hover_disabled", 280, 70),
];
const MICROPHONE: [Marker; 4] = [
    Marker::new("mute", 0, 70),
    Marker::new("unmute", 100, 70),
    Marker::new("hover_unmuted", 180, 40),
    Marker::new("hover_muted", 240, 40),
];
const DEAFEN: [Marker; 4] = [
    Marker::new("deafen", 0, 70),
    Marker::new("undeafen", 110, 70),
    Marker::new("hover_undeafened", 200, 70),
    Marker::new("hover_deafened", 300, 70),
];
const SOUNDBOARD: [Marker; 1] = [Marker::new("all", 0, 66)];
const ACTIVITY: [Marker; 2] = [Marker::new("hover", 0, 59), Marker::new("click", 68, 26)];
const DISCONNECT: [Marker; 1] = [Marker::new("all", 0, 90)];

impl Anim {
    #[cfg(any(test, feature = "capture"))]
    pub(crate) const ALL: [Anim; 7] = [
        Anim::ScreenShare,
        Anim::Camera,
        Anim::Microphone,
        Anim::Deafen,
        Anim::Soundboard,
        Anim::Activity,
        Anim::Disconnect,
    ];

    /// The file stem under `app/assets/lottie/call`.
    #[cfg(any(test, feature = "capture"))]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Anim::ScreenShare => "screenshare",
            Anim::Camera => "camera",
            Anim::Microphone => "microphone",
            Anim::Deafen => "deafen",
            Anim::Soundboard => "soundboard",
            Anim::Activity => "activity",
            Anim::Disconnect => "disconnect",
        }
    }

    #[cfg(feature = "capture")]
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|anim| anim.name() == name)
    }

    fn json(self) -> &'static str {
        match self {
            Anim::ScreenShare => include_str!("../../assets/lottie/call/screenshare.json"),
            Anim::Camera => include_str!("../../assets/lottie/call/camera.json"),
            Anim::Microphone => include_str!("../../assets/lottie/call/microphone.json"),
            Anim::Deafen => include_str!("../../assets/lottie/call/deafen.json"),
            Anim::Soundboard => include_str!("../../assets/lottie/call/soundboard.json"),
            Anim::Activity => include_str!("../../assets/lottie/call/activity.json"),
            Anim::Disconnect => include_str!("../../assets/lottie/call/disconnect.json"),
        }
    }

    /// The markers the hook hands the player (they equal the file's own `markers`).
    pub(crate) fn markers(self) -> &'static [Marker] {
        match self {
            Anim::ScreenShare => &SCREEN_SHARE,
            Anim::Camera => &CAMERA,
            Anim::Microphone => &MICROPHONE,
            Anim::Deafen => &DEAFEN,
            Anim::Soundboard => &SOUNDBOARD,
            Anim::Activity => &ACTIVITY,
            Anim::Disconnect => &DISCONNECT,
        }
    }

    pub(crate) fn marker(self, name: &str) -> Option<Marker> {
        self.markers()
            .iter()
            .copied()
            .find(|marker| marker.name == name)
    }

    /// The marker the hook's `onMouseEnter` plays (and `onMouseLeave` stops) while the hook argument is `state`.
    pub(crate) fn hover_marker(self, state: Option<&str>) -> &'static str {
        match self {
            // 416696 and 446080: `"enable" === e ? "hover_disabled" : "hover_enabled"`.
            Anim::ScreenShare | Anim::Camera => {
                if state == Some("enable") {
                    "hover_disabled"
                } else {
                    "hover_enabled"
                }
            }
            // 523875: `"mute" === e ? "hover_unmuted" : "hover_muted"`.
            Anim::Microphone => {
                if state == Some("mute") {
                    "hover_unmuted"
                } else {
                    "hover_muted"
                }
            }
            // 348858: `"deafen" === e ? "hover_undeafened" : "hover_deafened"`.
            Anim::Deafen => {
                if state == Some("deafen") {
                    "hover_undeafened"
                } else {
                    "hover_deafened"
                }
            }
            Anim::Soundboard | Anim::Disconnect => "all",
            Anim::Activity => "hover",
        }
    }

    /// What a click on the tray button plays directly: the activity button plays `click` (168754), the soundboard
    /// button plays `all` when it opens its picker (533978 `ed`). The toggles play through their state change instead
    /// (the wrappers' `useEffect(() => () => play(), [state, play])`), and disconnect plays nothing on click.
    pub(crate) fn click_marker(self) -> Option<&'static str> {
        match self {
            Anim::Activity => Some("click"),
            Anim::Soundboard => Some("all"),
            _ => None,
        }
    }
}

/// What the player shows: a still frame, or a segment running since `started` (motion-clock ms).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Run {
    Rest(u32),
    Playing { from: u32, end: u32, started: f64 },
}

/// One mounted icon: module 744682's `j` (the last marker played) and its lottie-web item.
#[derive(Clone, Debug, PartialEq)]
struct Player {
    anim: Anim,
    /// `j.current`.
    current: Option<&'static str>,
    /// The hook argument at the last draw.
    state: Option<&'static str>,
    run: Run,
    fps: f64,
    /// Between the button's `onMouseEnter` and `onMouseLeave`.
    hovered: bool,
}

impl Player {
    /// The load effect of 744682: `u = j ?? initialAnimation ?? (the only marker)`, `initialSegment: [u.start, ..]`,
    /// `autoplay: false`; without a segment the file's in point shows.
    fn mount(anim: Anim, state: Option<&'static str>, in_point: u32, fps: f64) -> Self {
        let markers = anim.markers();
        let initial = state
            .and_then(|name| anim.marker(name))
            .or_else(|| (markers.len() == 1).then(|| markers[0]));
        Self {
            anim,
            current: None,
            state,
            run: Run::Rest(initial.map_or(in_point, |marker| marker.start)),
            fps,
            hovered: false,
        }
    }

    fn play(&mut self, name: &str, now: f64, reduced: bool) {
        let Some(marker) = self.anim.marker(name) else {
            return;
        };
        self.current = Some(marker.name);
        self.run = if reduced || marker.duration == 0 {
            // `setSegment(end, end)` and `stop()`.
            Run::Rest(marker.end())
        } else {
            Run::Playing {
                from: marker.start,
                end: marker.end(),
                started: now,
            }
        };
    }

    fn stop_if_playing(&mut self, name: &str, reduced: bool) {
        if reduced || self.current != Some(name) {
            return;
        }
        if let Some(marker) = self.anim.marker(name) {
            // `setSegment(start, start)` and `stop()`.
            self.run = Run::Rest(marker.start);
        }
    }

    /// The hook argument changed: the wrapper's effect cleanup plays the old argument's animation.
    fn follow(&mut self, state: Option<&'static str>, now: f64, reduced: bool) {
        if state == self.state {
            return;
        }
        if let Some(old) = self.state {
            self.play(old, now, reduced);
        }
        self.state = state;
    }

    /// The frame at `now`, and whether it is still playing. A run that reached `totalFrames - 1` settles there.
    fn frame(&mut self, now: f64) -> (u32, bool) {
        match self.run {
            Run::Rest(frame) => (frame, false),
            Run::Playing { from, end, started } => {
                let total = end - from;
                let raw = 0.001 + (now - started).max(0.) * self.fps / 1000.;
                if raw >= f64::from(total - 1) {
                    self.run = Run::Rest(end - 1);
                    (end - 1, false)
                } else {
                    (from + raw.floor() as u32, true)
                }
            }
        }
    }
}

/// A parsed file ready to render, with what the player needs of it.
struct Art {
    prepared: PreparedAnimation,
    width: u32,
    height: u32,
    in_point: u32,
    fps: f64,
}

/// Rewrites what rasterlottie reads differently from lottie-web, without changing what lottie-web would draw:
/// * Bodymovin writes a split position's flag as `"s": true`; rasterlottie reads it only as a number (a position it
///   cannot read as split it reads as one vector whose `x` axis looks like an expression). It becomes `1`; lottie-web
///   only tests it for truth.
/// * A mask of mode `"n"` is skipped by lottie-web's `MaskElement` (a layer whose masks are all `"n"` is drawn
///   unmasked); rasterlottie leaves such a layer with an empty coverage. Those masks are dropped (the headphones of
///   the deafen icon have one).
/// * A trim path whose start equals its end draws nothing in lottie-web (`TrimModifier`: `s === e` empties the
///   paths); rasterlottie draws the untrimmed path instead. See `hide_empty_trims`.
fn for_rasterlottie(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            if map.get("ty").and_then(Value::as_i64) == Some(4) {
                hide_empty_trims(map);
            }
            if let Some(Value::Object(position)) = map.get_mut("p")
                && let Some(Value::Bool(split)) = position.get("s").cloned()
                && position.contains_key("x")
            {
                position.insert("s".to_owned(), Value::from(u8::from(split)));
            }
            if let Some(Value::Array(masks)) = map.get_mut("masksProperties") {
                masks.retain(|mask| mask.get("mode").and_then(Value::as_str) != Some("n"));
                if masks.is_empty() {
                    map.remove("masksProperties");
                    map.insert("hasMask".to_owned(), Value::Bool(false));
                }
            }
            map.values_mut().for_each(for_rasterlottie);
        }
        Value::Array(items) => items.iter_mut().for_each(for_rasterlottie),
        _ => {}
    }
}

/// The value of a scalar property (static, or keyframed in the layer's own frames) at frame `at`, when it is not
/// moving there: before the first key, from the last key on, on a key, or between two keys that hold or are equal.
fn steady(property: &serde_json::Value, at: f64) -> Option<f64> {
    use serde_json::Value;
    let number = |value: &Value| {
        value
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_f64)
            .or_else(|| value.as_f64())
    };
    if property.get("a").and_then(Value::as_i64) != Some(1) {
        return number(property.get("k")?);
    }
    let keys = property.get("k")?.as_array()?;
    let time = |index: usize| keys.get(index)?.get("t")?.as_f64();
    let value = |index: usize| {
        let key = keys.get(index)?;
        key.get("s").and_then(number).or_else(|| {
            // An old-style last key carries no value: the one before ends on its `e`.
            let before = keys.get(index.checked_sub(1)?)?;
            before.get("e").and_then(number)
        })
    };
    if at <= time(0)? {
        return value(0);
    }
    let index = (0..keys.len())
        .rev()
        .find(|index| time(*index).is_some_and(|t| t <= at))?;
    if index + 1 == keys.len() || time(index)? == at {
        return value(index);
    }
    let (here, next) = (value(index)?, value(index + 1)?);
    let hold = keys[index].get("h").and_then(Value::as_i64) == Some(1);
    (hold || here == next).then_some(here)
}

/// Whether trim item `trim` leaves nothing at frame `at`: lottie-web's rule, start and end in percent clamped, plus
/// the offset in turns, rounded to 1/10000, equal.
fn trim_is_empty(trim: &serde_json::Value, at: f64) -> bool {
    let (Some(start), Some(end)) = (
        trim.get("s").and_then(|s| steady(s, at)),
        trim.get("e").and_then(|e| steady(e, at)),
    ) else {
        return false;
    };
    let offset = trim.get("o").and_then(|o| steady(o, at)).unwrap_or(0.) / 360.;
    let rounded = |percent: f64| ((percent.clamp(0., 100.) / 100. + offset) * 10_000.).round();
    rounded(start) == rounded(end)
}

/// In a shape layer whose trim paths come after all of its other items (every trim of the seven files), a trim that
/// is empty empties the whole layer. Its static opacity becomes hold keys that are 0 on the frames (in the layer's
/// own time, which its keys use) where one of its trims is empty.
fn hide_empty_trims(layer: &mut serde_json::Map<String, serde_json::Value>) {
    use serde_json::{Value, json};
    let Some(Value::Array(items)) = layer.get("shapes") else {
        return;
    };
    let is_trim = |item: &Value| item.get("ty").and_then(Value::as_str) == Some("tm");
    let Some(first) = items.iter().position(is_trim) else {
        return;
    };
    if !items[first..].iter().all(is_trim) {
        return;
    }
    let trims: Vec<Value> = items[first..].to_vec();
    let number = |key: &str| layer.get(key).and_then(Value::as_f64);
    let (Some(in_point), Some(out_point)) = (number("ip"), number("op")) else {
        return;
    };
    let start_time = number("st").unwrap_or(0.);
    let Some(opacity) = layer.get("ks").and_then(|ks| ks.get("o")) else {
        return;
    };
    if opacity.get("a").and_then(Value::as_i64) == Some(1) {
        return;
    }
    let Some(shown) = steady(opacity, 0.) else {
        return;
    };
    let first_frame = (in_point - start_time).floor() as i64;
    let last_frame = (out_point - start_time).ceil() as i64;
    let mut keys = Vec::new();
    let mut previous = None;
    for frame in first_frame..=last_frame {
        let hidden = trims.iter().any(|trim| trim_is_empty(trim, frame as f64));
        let value = if hidden { 0. } else { shown };
        if previous != Some(value) {
            keys.push(json!({ "t": frame, "s": [value], "h": 1 }));
            previous = Some(value);
        }
    }
    if (keys.len() > 1 || previous == Some(0.))
        && let Some(ks) = layer.get_mut("ks")
    {
        ks["o"] = json!({ "a": 1, "k": keys });
    }
}

fn load(anim: Anim) -> Option<Art> {
    let mut json: serde_json::Value = serde_json::from_str(anim.json()).ok()?;
    for_rasterlottie(&mut json);
    // lottie-web draws any frame it is sent, the file's out point included (a reduced-motion `play` rests on
    // `start + duration`, which is the out point of the soundboard and disconnect files); rasterlottie draws nothing
    // from the out point on. Each layer still keeps its own in and out points.
    let last = anim.markers().iter().map(|marker| marker.end()).max()?;
    if let Some(out) = json.get("op").and_then(serde_json::Value::as_f64)
        && out <= f64::from(last)
    {
        json["op"] = serde_json::Value::from(last + 1);
    }
    let animation = Animation::from_json_str(&json.to_string()).ok()?;
    let prepared = Renderer::default().prepare(&animation).ok()?;
    Some(Art {
        prepared,
        width: animation.width,
        height: animation.height,
        in_point: animation.in_point.max(0.) as u32,
        fps: f64::from(animation.frame_rate),
    })
}

/// Frame `frame` of `art`, `side` pixels on the longer side, as its coverage (one byte a pixel).
fn render_mask(art: &Art, frame: u32, side: u32) -> Option<Mask> {
    let scale = side as f32 / art.width.max(art.height) as f32;
    let rendered = art
        .prepared
        .render_frame(frame as f32, RenderConfig::new(Rgba8::TRANSPARENT, scale))
        .ok()?;
    let alpha = rendered
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[3])
        .collect();
    Some(Mask {
        width: rendered.width,
        height: rendered.height,
        alpha,
        used: 0,
    })
}

struct Mask {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
    used: u64,
}

fn channel(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255. + 0.5) as u8
}

/// The mask in `tint`: straight-alpha BGRA, the layout the atlas takes.
fn tinted(mask: &Mask, tint: [u8; 4]) -> Arc<RenderImage> {
    let [r, g, b, a] = tint;
    let mut pixels = Vec::with_capacity(mask.alpha.len() * 4);
    for coverage in &mask.alpha {
        let alpha = (u16::from(*coverage) * u16::from(a) + 127) / 255;
        pixels.extend_from_slice(&[b, g, r, alpha as u8]);
    }
    let buffer = image::RgbaImage::from_raw(mask.width, mask.height, pixels)
        .unwrap_or_else(|| image::RgbaImage::new(mask.width.max(1), mask.height.max(1)));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// Most bytes of frame masks kept, all icons together (a 24 px icon at 2x is 2.3 KiB a frame).
const MASK_BUDGET: usize = 4 * 1024 * 1024;
/// Most tinted frames kept in the atlas at once.
const IMAGE_LIMIT: usize = 96;

type MaskKey = (Anim, u32, u32);
type ImageKey = (Anim, u32, u32, [u8; 4]);

#[derive(Default)]
struct Store {
    players: HashMap<u64, Player>,
    arts: HashMap<Anim, Option<Art>>,
    masks: HashMap<MaskKey, Mask>,
    images: HashMap<ImageKey, (Arc<RenderImage>, u64)>,
    /// Evicted images, released from the atlas at the next draw.
    retired: Vec<Arc<RenderImage>>,
    tick: u64,
    rendered: u64,
    render_us: u64,
    #[cfg(feature = "capture")]
    probe: Option<Probe>,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with(|store| f(&mut store.borrow_mut()))
}

impl Store {
    fn art(&mut self, anim: Anim) -> Option<&Art> {
        self.arts.entry(anim).or_insert_with(|| load(anim)).as_ref()
    }

    /// The player of `id`, mounted on first use.
    fn player(&mut self, id: u64, anim: Anim, state: Option<&'static str>) -> Option<&mut Player> {
        let (in_point, fps) = self.art(anim).map(|art| (art.in_point, art.fps))?;
        let player = self
            .players
            .entry(id)
            .or_insert_with(|| Player::mount(anim, state, in_point, fps));
        if player.anim != anim {
            *player = Player::mount(anim, state, in_point, fps);
        }
        Some(player)
    }

    fn mask_bytes(&self) -> usize {
        self.masks.values().map(|mask| mask.alpha.len()).sum()
    }

    fn image(
        &mut self,
        anim: Anim,
        frame: u32,
        side: u32,
        tint: [u8; 4],
    ) -> Option<Arc<RenderImage>> {
        self.tick += 1;
        let tick = self.tick;
        let key = (anim, side, frame, tint);
        if let Some((image, used)) = self.images.get_mut(&key) {
            *used = tick;
            return Some(image.clone());
        }
        let mask_key = (anim, side, frame);
        if !self.masks.contains_key(&mask_key) {
            let started = std::time::Instant::now();
            let mask = render_mask(self.art(anim)?, frame, side)?;
            self.render_us += started.elapsed().as_micros() as u64;
            self.rendered += 1;
            self.masks.insert(mask_key, mask);
            while self.mask_bytes() > MASK_BUDGET && self.masks.len() > 1 {
                let oldest = self
                    .masks
                    .iter()
                    .filter(|(key, _)| **key != mask_key)
                    .min_by_key(|(_, mask)| mask.used)
                    .map(|(key, _)| *key);
                match oldest {
                    Some(oldest) => {
                        self.masks.remove(&oldest);
                    }
                    None => break,
                }
            }
        }
        let mask = self.masks.get_mut(&mask_key)?;
        mask.used = tick;
        let image = tinted(mask, tint);
        self.images.insert(key, (image.clone(), tick));
        while self.images.len() > IMAGE_LIMIT {
            let oldest = self
                .images
                .iter()
                .filter(|(other, _)| **other != key)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| *key);
            match oldest.and_then(|oldest| self.images.remove(&oldest)) {
                Some((image, _)) => self.retired.push(image),
                None => break,
            }
        }
        Some(image)
    }
}

fn tint_bytes(tint: Rgba) -> [u8; 4] {
    [
        channel(tint.r),
        channel(tint.g),
        channel(tint.b),
        channel(tint.a),
    ]
}

/// Draws the icon `id` (any stable name of one button, e.g. `"tray-mic"`): `anim` at `size` logical pixels in
/// `tint` (the button's text colour, the official `currentColor`). `state` is the hook argument (the marker the next
/// toggle plays: `Some("mute")` while unmuted, `Some("unmute")` while muted, ...; `None` for soundboard, activity and
/// disconnect). The first draw mounts the player on that state's start frame; a later draw with a different state
/// plays the old state's marker, as the wrappers' effect cleanup does. Asks for the next frame only while playing.
pub(crate) fn icon(
    id: &str,
    anim: Anim,
    state: Option<&'static str>,
    size: f32,
    tint: Rgba,
) -> AnyElement {
    let now = motion::now();
    let reduced = motion::reduced();
    let shown = with(|store| {
        let player = store.player(motion::key(id), anim, state)?;
        player.follow(state, now, reduced);
        Some(player.frame(now))
    });
    let Some((frame, playing)) = shown else {
        return div().size(px(size)).flex_none().into_any_element();
    };
    if playing {
        motion::keep_animating();
    }
    let tint = tint_bytes(tint);
    gpui::canvas(
        |_, _, _| {},
        move |bounds, (), window, cx| {
            let side = ((size * window.scale_factor()).round() as u32).max(1);
            let (image, retired) = with(|store| {
                (
                    store.image(anim, frame, side, tint),
                    std::mem::take(&mut store.retired),
                )
            });
            // Out of every window's atlas, as the call may have been drawn in another.
            for old in retired {
                cx.drop_image(old, Some(window));
            }
            if let Some(image) = image {
                let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
            }
        },
    )
    .size(px(size))
    .flex_none()
    .into_any_element()
}

/// Whether the pointer is over `id`'s button, as its last `hover` call said.
pub(crate) fn hovered(id: &str) -> bool {
    with(|store| {
        store
            .players
            .get(&motion::key(id))
            .is_some_and(|player| player.hovered)
    })
}

/// `player.play(marker)` (module 744682). Does nothing before the icon's first draw, as the official ref is null
/// until the animation loaded.
pub(crate) fn play(id: &str, marker: &str) {
    let (now, reduced) = (motion::now(), motion::reduced());
    with(|store| {
        if let Some(player) = store.players.get_mut(&motion::key(id)) {
            player.play(marker, now, reduced);
        }
    });
}

/// `player.stopIfPlaying(marker)` (module 744682).
pub(crate) fn stop_if_playing(id: &str, marker: &str) {
    let reduced = motion::reduced();
    with(|store| {
        if let Some(player) = store.players.get_mut(&motion::key(id)) {
            player.stop_if_playing(marker, reduced);
        }
    });
}

/// The hook's `onMouseEnter` (`entered`) or `onMouseLeave`: plays or stops the hover marker for the state drawn last.
/// The soundboard button calls the leave half only while its picker is closed (533978).
pub(crate) fn hover(id: &str, entered: bool) {
    let found = with(|store| {
        store.players.get_mut(&motion::key(id)).map(|player| {
            player.hovered = entered;
            player.anim.hover_marker(player.state)
        })
    });
    if let Some(marker) = found {
        if entered {
            play(id, marker);
        } else {
            stop_if_playing(id, marker);
        }
    }
}

/// A click on the tray button: plays `Anim::click_marker` where the wrapper does (activity, soundboard).
pub(crate) fn click(id: &str) {
    let found = with(|store| {
        store
            .players
            .get(&motion::key(id))
            .and_then(|player| player.anim.click_marker())
    });
    if let Some(marker) = found {
        play(id, marker);
    }
}

/// Unmounts `id`: its next draw starts again from its initial frame, as a remounted official button does.
pub(crate) fn forget(id: &str) {
    with(|store| {
        store.players.remove(&motion::key(id));
    });
}

/// The frame `id` shows at the motion clock's time (`getCurrentFrame`); the capture probe reports it.
#[cfg(feature = "capture")]
pub(crate) fn current_frame(id: &str) -> Option<u32> {
    let now = motion::now();
    with(|store| {
        store
            .players
            .get_mut(&motion::key(id))
            .map(|player| player.frame(now).0)
    })
}

/// Capture only: one icon drawn over the window by the `lottie` script steps.
#[cfg(feature = "capture")]
struct Probe {
    anim: Anim,
    size: f32,
    state: Option<&'static str>,
}

#[cfg(feature = "capture")]
const PROBE_ID: &str = "lottie-probe";

#[cfg(feature = "capture")]
impl Root {
    /// The `lottie` steps of the harness: `show <anim> [size] [state]`, `state <marker>|none` (a toggle's hook
    /// argument; a change plays the old one), `play <marker>`, `stop <marker>`, `hover on|off`, `click`, `hide`,
    /// `info`. The probe is drawn white on black at the window's top left.
    pub(crate) fn script_lottie_icon(
        &mut self,
        words: &[&str],
        cx: &mut gpui::Context<Self>,
    ) -> String {
        let marker_of = |anim: Anim, name: &str| anim.marker(name).map(|marker| marker.name);
        let probe_anim = with(|store| store.probe.as_ref().map(|probe| probe.anim));
        let line = match (words, probe_anim) {
            (["show", name, rest @ ..], _) => {
                let Some(anim) = Anim::from_name(name) else {
                    return format!("no animation {name}");
                };
                let size = rest
                    .first()
                    .and_then(|size| size.parse::<f32>().ok())
                    .unwrap_or(96.);
                let state = rest.get(1).and_then(|state| marker_of(anim, state));
                forget(PROBE_ID);
                with(|store| {
                    store.probe = Some(Probe { anim, size, state });
                });
                format!("showing {name} at {size} px, state {state:?}")
            }
            (["state", name], Some(anim)) => {
                let state = marker_of(anim, name);
                with(|store| {
                    if let Some(probe) = store.probe.as_mut() {
                        probe.state = state;
                    }
                });
                format!("state {state:?}")
            }
            (["play", name], Some(_)) => {
                play(PROBE_ID, name);
                format!("play {name}")
            }
            (["stop", name], Some(_)) => {
                stop_if_playing(PROBE_ID, name);
                format!("stop {name}")
            }
            (["hover", on], Some(_)) => {
                hover(PROBE_ID, *on == "on");
                format!("hover {on}")
            }
            (["click"], Some(_)) => {
                click(PROBE_ID);
                "click".to_owned()
            }
            (["hide"], _) => {
                forget(PROBE_ID);
                with(|store| store.probe = None);
                "hidden".to_owned()
            }
            (["info"], _) => {
                let frame = current_frame(PROBE_ID);
                with(|store| {
                    format!(
                        "frame {frame:?} at {} ms, last {:?}, rendered {} in {} us, masks {} ({} bytes), images {}",
                        motion::now(),
                        store
                            .players
                            .get(&motion::key(PROBE_ID))
                            .and_then(|player| player.current),
                        store.rendered,
                        store.render_us,
                        store.masks.len(),
                        store.mask_bytes(),
                        store.images.len(),
                    )
                })
            }
            (_, None) => "no probe shown (lottie show <anim>)".to_owned(),
            _ => format!("unknown lottie step {words:?}"),
        };
        cx.notify();
        line
    }

    /// The probe layer, drawn last in the root.
    pub(crate) fn lottie_icon_probe(&self) -> Option<AnyElement> {
        use gpui::ParentElement as _;
        let (anim, size, state) = with(|store| {
            store
                .probe
                .as_ref()
                .map(|probe| (probe.anim, probe.size, probe.state))
        })?;
        let white = Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: 1.,
        };
        let black = Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 1.,
        };
        let probe = div()
            .absolute()
            .left(px(16.))
            .top(px(48.))
            .p(px(8.))
            .bg(black)
            .child(icon(PROBE_ID, anim, state, size, white));
        // Above the regions, which paint after the root's own layers.
        Some(gpui::deferred(probe).with_priority(1).into_any_element())
    }
}

#[cfg(not(feature = "capture"))]
impl Root {
    pub(crate) fn lottie_icon_probe(&self) -> Option<AnyElement> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(anim: Anim, state: Option<&'static str>) -> Player {
        Player::mount(anim, state, 0, 60.)
    }

    #[test]
    fn the_hook_markers_equal_the_files_markers() {
        for anim in Anim::ALL {
            let json: serde_json::Value = serde_json::from_str(anim.json()).expect("valid JSON");
            let file: Vec<(String, u64, u64)> = json["markers"]
                .as_array()
                .expect("markers")
                .iter()
                .map(|marker| {
                    (
                        marker["cm"].as_str().expect("name").to_owned(),
                        marker["tm"].as_u64().expect("start"),
                        marker["dr"].as_u64().expect("duration"),
                    )
                })
                .collect();
            let hook: Vec<(String, u64, u64)> = anim
                .markers()
                .iter()
                .map(|marker| {
                    (
                        marker.name.to_owned(),
                        u64::from(marker.start),
                        u64::from(marker.duration),
                    )
                })
                .collect();
            assert_eq!(file, hook, "{}", anim.name());
            assert_eq!(json["fr"].as_f64(), Some(60.), "{}", anim.name());
        }
    }

    #[test]
    fn at_rest_after_mount_the_initial_animation_start_shows() {
        assert_eq!(player(Anim::Microphone, Some("mute")).run, Run::Rest(0));
        assert_eq!(player(Anim::Microphone, Some("unmute")).run, Run::Rest(100));
        assert_eq!(player(Anim::Camera, Some("enable")).run, Run::Rest(100));
        assert_eq!(player(Anim::Deafen, Some("undeafen")).run, Run::Rest(110));
        // One marker: that marker; several and no initial animation: the in point.
        assert_eq!(player(Anim::Disconnect, None).run, Run::Rest(0));
        assert_eq!(player(Anim::Activity, None).run, Run::Rest(0));
        assert_eq!(
            Player::mount(Anim::Activity, None, 3, 60.).run,
            Run::Rest(3)
        );
    }

    #[test]
    fn a_play_runs_at_the_file_rate_and_rests_on_the_last_segment_frame() {
        let mut icon = player(Anim::ScreenShare, Some("enable"));
        icon.play("disable", 1000., false);
        assert_eq!(icon.frame(1000.), (90, true));
        // 0.001 + 500 ms * 60 / 1000 = 30.001 frames in.
        assert_eq!(icon.frame(1500.), (120, true));
        // totalFrames - 1 = 69 is reached at 1150 ms: rest on 90 + 69.
        assert_eq!(icon.frame(2149.), (158, true));
        assert_eq!(icon.frame(2150.), (159, false));
        assert_eq!(icon.frame(9000.), (159, false));
        assert_eq!(icon.current, Some("disable"));
    }

    #[test]
    fn a_new_play_interrupts_from_its_own_start() {
        let mut icon = player(Anim::Microphone, Some("mute"));
        icon.play("hover_unmuted", 0., false);
        assert_eq!(icon.frame(200.), (192, true));
        icon.play("mute", 200., false);
        assert_eq!(icon.frame(200.), (0, true));
        assert_eq!(icon.frame(300.), (6, true));
    }

    #[test]
    fn stop_if_playing_only_stops_the_last_marker_played() {
        let mut icon = player(Anim::Camera, Some("enable"));
        icon.play("hover_disabled", 0., false);
        icon.stop_if_playing("hover_enabled", false);
        assert_eq!(icon.frame(100.), (286, true));
        icon.stop_if_playing("hover_disabled", false);
        assert_eq!(icon.frame(100.), (280, false));
        // A finished marker still counts as the last one played.
        icon.play("enable", 0., false);
        assert_eq!(icon.frame(5000.), (169, false));
        icon.stop_if_playing("enable", false);
        assert_eq!(icon.frame(5000.), (100, false));
    }

    #[test]
    fn reduced_motion_jumps_to_the_segment_end_and_ignores_stop() {
        let mut icon = player(Anim::Disconnect, None);
        icon.play("all", 0., true);
        assert_eq!(icon.frame(0.), (90, false));
        icon.stop_if_playing("all", true);
        assert_eq!(icon.frame(10.), (90, false));
        assert_eq!(icon.current, Some("all"));
        let mut mic = player(Anim::Microphone, Some("mute"));
        mic.play("hover_unmuted", 0., true);
        assert_eq!(mic.frame(0.), (220, false));
    }

    #[test]
    fn a_state_change_plays_the_old_state_like_the_effect_cleanup() {
        let mut mic = player(Anim::Microphone, Some("mute"));
        mic.follow(Some("mute"), 0., false);
        assert_eq!(mic.run, Run::Rest(0));
        // Muting: the argument turns to "unmute" and the old "mute" plays.
        mic.follow(Some("unmute"), 10., false);
        assert_eq!(mic.current, Some("mute"));
        assert_eq!(mic.frame(10.), (0, true));
        assert_eq!(mic.frame(5000.), (69, false));
        mic.follow(Some("mute"), 6000., false);
        assert_eq!(mic.frame(6000.), (100, true));
    }

    #[test]
    fn hover_markers_follow_each_hook() {
        assert_eq!(
            Anim::ScreenShare.hover_marker(Some("enable")),
            "hover_disabled"
        );
        assert_eq!(
            Anim::ScreenShare.hover_marker(Some("disable")),
            "hover_enabled"
        );
        assert_eq!(Anim::Camera.hover_marker(Some("enable")), "hover_disabled");
        assert_eq!(Anim::Camera.hover_marker(Some("disable")), "hover_enabled");
        assert_eq!(Anim::Microphone.hover_marker(Some("mute")), "hover_unmuted");
        assert_eq!(Anim::Microphone.hover_marker(Some("unmute")), "hover_muted");
        assert_eq!(
            Anim::Deafen.hover_marker(Some("deafen")),
            "hover_undeafened"
        );
        assert_eq!(
            Anim::Deafen.hover_marker(Some("undeafen")),
            "hover_deafened"
        );
        assert_eq!(Anim::Soundboard.hover_marker(None), "all");
        assert_eq!(Anim::Disconnect.hover_marker(None), "all");
        assert_eq!(Anim::Activity.hover_marker(None), "hover");
        for anim in Anim::ALL {
            assert!(anim.marker(anim.hover_marker(None)).is_some());
            if let Some(click) = anim.click_marker() {
                assert!(anim.marker(click).is_some());
            }
        }
    }

    fn coverage(mask: &Mask) -> u64 {
        mask.alpha.iter().map(|alpha| u64::from(*alpha)).sum()
    }

    #[test]
    fn every_file_renders_and_every_rest_frame_draws_something() {
        for anim in Anim::ALL {
            let art = load(anim).unwrap_or_else(|| panic!("{} loads", anim.name()));
            assert_eq!(art.fps, 60.);
            let mut frames = vec![art.in_point];
            for marker in anim.markers() {
                frames.extend([marker.start, marker.end() - 1, marker.end()]);
            }
            for frame in frames {
                let mask = render_mask(&art, frame, 48).expect("renders");
                assert_eq!((mask.width, mask.height), (48, 48));
                assert!(
                    coverage(&mask) > 0,
                    "{} frame {frame} is empty",
                    anim.name()
                );
            }
        }
    }

    #[test]
    fn a_trim_is_empty_only_where_start_and_end_rest_equal() {
        // The screen share's "X" stroke (symbol morph): start 0 until key 119 then 100 from 132, end 0 until key 47
        // then 100 from 60.
        let trim = serde_json::json!({
            "ty": "tm",
            "s": { "a": 1, "k": [{ "t": 119, "s": [0] }, { "t": 132, "s": [100] }] },
            "e": { "a": 1, "k": [{ "t": 47, "s": [0] }, { "t": 60, "s": [100] }] },
            "o": { "a": 0, "k": 0 },
        });
        assert!(trim_is_empty(&trim, 0.));
        assert!(trim_is_empty(&trim, 47.));
        assert!(!trim_is_empty(&trim, 50.));
        assert!(!trim_is_empty(&trim, 90.));
        assert!(!trim_is_empty(&trim, 125.));
        assert!(trim_is_empty(&trim, 132.));
        assert!(trim_is_empty(&trim, 170.));
        let mut layer = serde_json::json!({
            "ty": 4, "ip": 0, "op": 180, "st": 0,
            "ks": { "o": { "a": 0, "k": 100 } },
            "shapes": [{ "ty": "gr", "it": [] }, trim],
        });
        hide_empty_trims(layer.as_object_mut().expect("object"));
        let keys: Vec<(i64, f64)> = layer["ks"]["o"]["k"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| (key["t"].as_i64().unwrap(), key["s"][0].as_f64().unwrap()))
            .collect();
        assert_eq!(keys, [(0, 0.), (48, 100.), (132, 0.)]);
    }

    #[test]
    fn a_running_segment_changes_the_picture() {
        let art = load(Anim::ScreenShare).expect("loads");
        let start = render_mask(&art, 0, 48).expect("renders");
        let middle = render_mask(&art, 30, 48).expect("renders");
        assert_ne!(start.alpha, middle.alpha);
    }

    #[test]
    fn a_tinted_frame_has_the_tint_and_the_mask_coverage() {
        let mask = Mask {
            width: 2,
            height: 1,
            alpha: vec![255, 128],
            used: 0,
        };
        let image = tinted(&mask, [10, 20, 30, 255]);
        let bytes = image.as_bytes(0).expect("one frame");
        assert_eq!(bytes, &[30, 20, 10, 255, 30, 20, 10, 128]);
        let half = tinted(&mask, [10, 20, 30, 128]);
        assert_eq!(half.as_bytes(0).expect("one frame")[3], 128);
        assert_eq!(half.as_bytes(0).expect("one frame")[7], 64);
    }

    #[test]
    fn the_store_caches_frames_and_retires_images_past_the_limit() {
        let mut store = Store::default();
        let first = store
            .image(Anim::Disconnect, 0, 24, [255; 4])
            .expect("image");
        let again = store
            .image(Anim::Disconnect, 0, 24, [255; 4])
            .expect("image");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(store.rendered, 1);
        // Another tint reuses the mask.
        store
            .image(Anim::Disconnect, 0, 24, [0, 0, 0, 255])
            .expect("image");
        assert_eq!(store.rendered, 1);
        for shade in 0..(IMAGE_LIMIT as u8 + 4) {
            store.image(Anim::Disconnect, 0, 24, [shade, 2, 3, 255]);
        }
        assert!(store.images.len() <= IMAGE_LIMIT);
        assert!(!store.retired.is_empty());
    }
}
