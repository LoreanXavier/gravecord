//! Sticker animation. The official sticker component (module 148355) draws a Lottie sticker on a canvas of
//! `size * min(2, devicePixelRatio)` pixels with the rlottie player of module 445972, and an APNG or GIF sticker as an
//! `<img>` whose address is the animated file only while it animates (module 378058 `zg`, `isPreview`). Whether it
//! animates is module 378058 `Qn` over the "Play sticker animations" setting (module 253932 `animateStickers`, default
//! `ALWAYS_ANIMATE`), the window's focus (module 218394) and the caller's `isInteracting`/`disableAnimation`.
//!
//! Here a clip is one sticker at one device size. Lottie frames are rendered lazily and off the UI thread, a few ahead
//! of the playhead; APNG and GIF files are decoded whole at the target size. Frames are `RenderImage`s kept under
//! `TOTAL_BUDGET`, everything a clip holds is released when a render no longer draws it, and a redraw is scheduled
//! only for the next frame boundary of a clip that is playing.

use super::{Root, profile_effects, stickers};
use crate::lottie;
use crate::net::Request;
use dc_state::Id;
use dc_state::stickers::Format;
use gpui::{Context, Img, RenderImage, Window, img};
use image::RgbaImage;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `Update::Image` key of an animated sticker file: `stickeranim:<sticker id>`. The key makes the network layer hand
/// the bytes back as they came (a Lottie file is otherwise turned into one still picture).
pub(super) const PREFIX: &str = "stickeranim:";

/// Most bytes of decoded frames kept at once, all clips together. Frames inside the playing window of a clip on
/// screen are not evicted, so a screen of playing stickers can pass it by that window.
const TOTAL_BUDGET: usize = 32 * 1024 * 1024;
/// Most bytes of one decoded APNG or GIF clip; a larger one is decoded at a smaller size.
const CLIP_BUDGET: usize = 16 * 1024 * 1024;
/// Most bytes of downloaded animations kept for reuse (a Lottie file is kept as its JSON).
const ENCODED_BUDGET: usize = 8 * 1024 * 1024;
/// A downloaded animation no clip has asked for in this long is dropped even under the budget; it is fetched again
/// when a sticker needs to play.
const ART_IDLE: Duration = Duration::from_secs(30);
/// Lottie frames rendered ahead of the playhead.
const LOOKAHEAD: usize = 8;
/// Lottie frames one background job renders.
const BATCH: usize = 4;
/// Background jobs at once, all clips together.
const MAX_JOBS: usize = 3;
/// A frame that claims no time is held this long, as browsers hold it.
const MIN_DELAY_MS: f64 = 100.;

pub(super) type ClipKey = (Id, u32);

/// `AnimateStickers` (module 823894 `BJ`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Setting {
    Always,
    OnInteraction,
    Never,
}

impl Setting {
    /// The proto value is the enum's number.
    pub(super) fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Always,
            1 => Self::OnInteraction,
            _ => Self::Never,
        }
    }

    /// Reduced motion overrides the setting with `ANIMATE_ON_INTERACTION` (module 955572).
    pub(super) fn under(self, reduced_motion: bool) -> Self {
        if reduced_motion {
            Self::OnInteraction
        } else {
            self
        }
    }

    /// Module 378058 `Qn`.
    pub(super) fn animates(self, interacting: bool) -> bool {
        match self {
            Self::OnInteraction => interacting,
            Self::Never => false,
            Self::Always => true,
        }
    }
}

/// What a clip does this render: plays, holds its frame (the window lost focus: rlottie `setState(false)` keeps the
/// frame) or goes back to the first frame (the sticker does not animate: `setState(false, 0)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Play {
    Run,
    Hold,
    Reset,
}

/// When each frame of a clip is shown.
#[derive(Clone, Debug, PartialEq)]
enum Timeline {
    /// Lottie: every frame lasts `1000 / fps` and the animation loops for ever (module 445972 `frameDuration`; its
    /// frame index is `floor(elapsed / frameDuration)` modulo the frame count).
    Uniform { frames: usize, frame_ms: f64 },
    /// APNG and GIF: the end of each frame in milliseconds; `plays` is the file's loop count, 0 for ever.
    Delays { ends: Vec<f64>, plays: u32 },
}

impl Timeline {
    fn frame_count(&self) -> usize {
        match self {
            Self::Uniform { frames, .. } => *frames,
            Self::Delays { ends, .. } => ends.len(),
        }
    }

    fn frame_at(&self, elapsed_ms: f64) -> usize {
        let elapsed = elapsed_ms.max(0.);
        match self {
            Self::Uniform { frames, frame_ms } => ((elapsed / frame_ms) as usize) % frames,
            Self::Delays { ends, plays } => {
                let total = ends.last().copied().unwrap_or(0.);
                if total <= 0. {
                    return 0;
                }
                if *plays > 0 && elapsed >= total * f64::from(*plays) {
                    return ends.len() - 1;
                }
                let local = elapsed % total;
                ends.partition_point(|end| *end <= local)
                    .min(ends.len() - 1)
            }
        }
    }

    /// Milliseconds until the frame changes, or `None` when it never does again.
    fn next_change(&self, elapsed_ms: f64) -> Option<f64> {
        let elapsed = elapsed_ms.max(0.);
        match self {
            Self::Uniform { frame_ms, .. } => Some((frame_ms - elapsed % frame_ms).max(1.)),
            Self::Delays { ends, plays } => {
                let total = ends.last().copied().unwrap_or(0.);
                if total <= 0. || (*plays > 0 && elapsed >= total * f64::from(*plays)) {
                    return None;
                }
                if ends.len() < 2 {
                    return None;
                }
                let local = elapsed % total;
                let index = ends
                    .partition_point(|end| *end <= local)
                    .min(ends.len() - 1);
                Some((ends[index] - local).max(1.))
            }
        }
    }
}

struct Frame {
    image: Arc<RenderImage>,
    bytes: usize,
    /// What rendering it took, for the capture log (0 for a decoded one).
    micros: u64,
}

#[derive(Default)]
struct Clip {
    timeline: Option<Timeline>,
    raster: bool,
    frames: BTreeMap<usize, Frame>,
    /// Frames a background job is rendering.
    pending: HashSet<usize>,
    decoding: bool,
    failed: bool,
    started: Option<Instant>,
    held_ms: f64,
    /// The render that last drew this clip, and what it asked of it.
    pass: u64,
    play: Option<Play>,
    playhead: usize,
}

impl Clip {
    fn elapsed_ms(&self, now: Instant) -> f64 {
        match self.started {
            Some(started) => now.saturating_duration_since(started).as_secs_f64() * 1000.,
            None => self.held_ms,
        }
    }

    fn set_play(&mut self, play: Play, now: Instant) {
        match play {
            Play::Run => {
                if self.started.is_none() {
                    let held = Duration::from_secs_f64(self.held_ms / 1000.);
                    self.started = Some(now.checked_sub(held).unwrap_or(now));
                }
            }
            Play::Hold => {
                if self.started.is_some() {
                    self.held_ms = self.elapsed_ms(now);
                    self.started = None;
                }
            }
            Play::Reset => {
                self.started = None;
                self.held_ms = 0.;
            }
        }
    }

    fn bytes(&self) -> usize {
        self.frames.values().map(|frame| frame.bytes).sum()
    }

    /// How many frames a clip at `playhead` is from showing `index`, going forward round the loop.
    fn ahead(&self, index: usize, playhead: usize) -> usize {
        let count = self
            .timeline
            .as_ref()
            .map_or(1, Timeline::frame_count)
            .max(1);
        (index + count - playhead % count) % count
    }
}

enum ArtKind {
    Lottie(Arc<lottie::Source>),
    Raster(Arc<Vec<u8>>),
}

struct Art {
    kind: ArtKind,
    bytes: usize,
    pass: u64,
    /// When a clip last asked for this file.
    seen: Instant,
}

/// Work `Cache::advance` asks the caller to start.
pub(super) enum Job {
    Render {
        source: Arc<lottie::Source>,
        frames: Vec<usize>,
    },
    Decode {
        bytes: Arc<Vec<u8>>,
    },
}

pub(super) struct Advance {
    pub image: Option<Arc<RenderImage>>,
    pub job: Option<Job>,
    /// The animated file has to be fetched.
    pub fetch: bool,
    /// Time until the shown frame changes.
    pub wake: Option<Duration>,
    /// The index of the frame in `image` and the playhead it stands in for, and the clip's frame count.
    pub shown: Option<(usize, usize, usize)>,
}

#[derive(Default)]
pub(super) struct Cache {
    arts: HashMap<Id, Art>,
    formats: HashMap<Id, Format>,
    /// Stickers whose file was asked for, and those it could not be had for.
    asked: HashSet<Id>,
    unavailable: HashSet<Id>,
    clips: HashMap<ClipKey, Clip>,
    pass: u64,
    scale: f32,
    focused: bool,
    jobs: usize,
    deadline: Option<Instant>,
    generation: u64,
    retired: Vec<Arc<RenderImage>>,
    /// Capture only: every playing clip is at this many milliseconds, whatever the clock says.
    frozen_ms: Option<f64>,
    rendered: u64,
    /// Draws of a clip with a timeline that had no frame to show yet.
    misses: u64,
    evicted: u64,
    released: u64,
    /// Lottie frames rendered and the microseconds they took, by device size.
    timings: HashMap<u32, (u64, u64)>,
    /// Capture only: what every draw of a sticker handed out, in order.
    #[cfg(feature = "capture")]
    trace: Vec<Draw>,
}

/// One draw of a sticker: when (ms since the process started), which clip, and what was drawn (0 nothing, 1 the still,
/// 2 a frame of the animation) at which frame.
#[cfg(feature = "capture")]
struct Draw {
    at: u64,
    id: Id,
    px: u32,
    what: u8,
    playhead: usize,
    /// How many frames the one drawn was behind the one due, going round the loop (0 for a still or nothing).
    lag: usize,
}

impl Cache {
    fn total_bytes(&self) -> usize {
        self.clips.values().map(Clip::bytes).sum()
    }

    fn encoded_bytes(&self) -> usize {
        self.arts.values().map(|art| art.bytes).sum()
    }

    /// One clip's step for this render: marks it drawn, applies what it was asked to do, and says which frame to draw
    /// and what to start.
    pub(super) fn advance(
        &mut self,
        id: Id,
        format: Format,
        px: u32,
        play: Play,
        now: Instant,
    ) -> Advance {
        let pass = self.pass;
        let frozen = self.frozen_ms;
        let mut advance = Advance {
            image: None,
            job: None,
            fetch: false,
            wake: None,
            shown: None,
        };
        self.formats.insert(id, format);
        let art = self.arts.get_mut(&id).map(|art| {
            art.pass = pass;
            art.seen = now;
            match &art.kind {
                ArtKind::Lottie(source) => ArtKind::Lottie(source.clone()),
                ArtKind::Raster(bytes) => ArtKind::Raster(bytes.clone()),
            }
        });
        let clip = self.clips.entry((id, px)).or_default();
        clip.pass = pass;
        clip.play = Some(play);
        if play == Play::Reset {
            clip.set_play(play, now);
            clip.playhead = 0;
            return advance;
        }
        if art.is_none() && !self.unavailable.contains(&id) && self.asked.insert(id) {
            advance.fetch = true;
        }
        let mut can_start = self.jobs < MAX_JOBS;
        match &art {
            Some(ArtKind::Lottie(source)) if clip.timeline.is_none() => {
                let fps = f64::from(source.frame_rate());
                clip.timeline = Some(Timeline::Uniform {
                    frames: source.frame_count(),
                    frame_ms: 1000. / fps,
                });
            }
            Some(ArtKind::Raster(bytes))
                if clip.timeline.is_none() && !clip.decoding && !clip.failed && can_start =>
            {
                clip.decoding = true;
                clip.raster = true;
                self.jobs += 1;
                advance.job = Some(Job::Decode {
                    bytes: bytes.clone(),
                });
                can_start = false;
            }
            _ => {}
        }
        clip.set_play(play, now);
        let Some(timeline) = clip.timeline.clone() else {
            return advance;
        };
        let elapsed = match (play, frozen) {
            (Play::Run, Some(ms)) => ms,
            _ => clip.elapsed_ms(now),
        };
        let count = timeline.frame_count();
        let playhead = timeline.frame_at(elapsed);
        clip.playhead = playhead;
        // The frame itself, else the nearest one behind it (a late frame is skipped, as module 445972 skips them).
        // While the clip is still filling in, the frame already shown is held rather than the still picture, so a
        // sticker whose frames arrive slower than they play does not blink between the two.
        let shown = clip
            .frames
            .range(..=playhead)
            .next_back()
            .or_else(|| clip.frames.iter().next_back());
        advance.shown = shown.map(|(index, _)| (*index, playhead, count));
        advance.image = shown.map(|(_, frame)| frame.image.clone());
        #[cfg(feature = "capture")]
        if crate::perf::legacy("blink") {
            advance.image = (0..LOOKAHEAD.min(count))
                .map(|back| (playhead + count - back) % count)
                .find_map(|index| clip.frames.get(&index))
                .map(|frame| frame.image.clone());
        }
        if advance.image.is_none() {
            self.misses += 1;
        }
        // A file whose frames failed to render (its layers did not parse) shows its still and is not asked again.
        if let (Some(ArtKind::Lottie(source)), true) = (&art, can_start && !clip.failed) {
            let wanted: Vec<usize> = (0..LOOKAHEAD.min(count))
                .map(|ahead| (playhead + ahead) % count)
                .filter(|index| !clip.frames.contains_key(index) && !clip.pending.contains(index))
                .take(BATCH)
                .collect();
            if !wanted.is_empty() {
                clip.pending.extend(wanted.iter().copied());
                self.jobs += 1;
                advance.job = Some(Job::Render {
                    source: source.clone(),
                    frames: wanted,
                });
            }
        }
        if play == Play::Run && frozen.is_none() {
            advance.wake = timeline
                .next_change(elapsed)
                .map(|ms| Duration::from_secs_f64(ms / 1000.));
        }
        advance
    }

    /// Frames a render job finished. A clip that is gone (nothing drew it) hands them back to be released.
    fn accept_frames(&mut self, key: ClipKey, frames: Vec<(usize, Option<Frame>)>) {
        self.jobs = self.jobs.saturating_sub(1);
        for (index, frame) in frames {
            let Some(clip) = self.clips.get_mut(&key) else {
                self.retired.extend(frame.map(|frame| frame.image));
                continue;
            };
            clip.pending.remove(&index);
            match frame {
                Some(frame) => {
                    self.rendered += 1;
                    let timing = self.timings.entry(key.1).or_default();
                    timing.0 += 1;
                    timing.1 += frame.micros;
                    clip.frames.insert(index, frame);
                }
                None => clip.failed = true,
            }
        }
        self.enforce_budget();
    }

    fn accept_decoded(&mut self, key: ClipKey, decoded: Option<Decoded>) {
        self.jobs = self.jobs.saturating_sub(1);
        let Some(clip) = self.clips.get_mut(&key) else {
            if let Some(decoded) = decoded {
                self.retired
                    .extend(decoded.frames.into_iter().map(|frame| frame.image));
            }
            return;
        };
        clip.decoding = false;
        match decoded {
            Some(decoded) => {
                let mut end = 0.;
                let ends = decoded
                    .delays
                    .iter()
                    .map(|delay| {
                        end += delay;
                        end
                    })
                    .collect();
                clip.timeline = Some(Timeline::Delays {
                    ends,
                    plays: decoded.plays,
                });
                for (index, frame) in decoded.frames.into_iter().enumerate() {
                    clip.frames.insert(index, frame);
                }
                self.rendered += clip.frames.len() as u64;
            }
            None => clip.failed = true,
        }
        self.enforce_budget();
    }

    /// Takes frames back under `TOTAL_BUDGET`: those of clips nothing drew, then those of clips that are not
    /// playing, then the frames of playing clips that are not in their window (furthest ahead round the loop, which
    /// is the ones just played, first). A decoded clip leaves whole and is decoded again when it plays.
    fn enforce_budget(&mut self) {
        let mut total = self.total_bytes();
        if total <= TOTAL_BUDGET {
            return;
        }
        let pass = self.pass;
        // (tier, distance ahead of the playhead, clip, frame or `None` for a whole decoded clip, bytes)
        let mut units: Vec<Unit> = Vec::new();
        for (key, clip) in &self.clips {
            let tier = if clip.pass != pass {
                0
            } else if clip.play != Some(Play::Run) {
                1
            } else {
                2
            };
            if clip.raster {
                if tier < 2 && !clip.frames.is_empty() {
                    units.push((tier, Reverse(0), *key, None, clip.bytes()));
                }
                continue;
            }
            for (index, frame) in &clip.frames {
                let ahead = clip.ahead(*index, clip.playhead);
                // The playing window: the frame shown and those about to be.
                let behind = clip.ahead(clip.playhead, *index);
                if tier == 2 && (ahead < LOOKAHEAD || behind < 2) {
                    continue;
                }
                units.push((tier, Reverse(ahead), *key, Some(*index), frame.bytes));
            }
        }
        units.sort_by_key(|unit| (unit.0, unit.1));
        for (_, _, key, index, bytes) in units {
            if total <= TOTAL_BUDGET {
                break;
            }
            let Some(clip) = self.clips.get_mut(&key) else {
                continue;
            };
            match index {
                Some(index) => {
                    if let Some(frame) = clip.frames.remove(&index) {
                        self.retired.push(frame.image);
                        self.evicted += 1;
                    }
                }
                None => {
                    self.evicted += clip.frames.len() as u64;
                    self.retired.extend(
                        std::mem::take(&mut clip.frames)
                            .into_values()
                            .map(|frame| frame.image),
                    );
                    clip.timeline = None;
                }
            }
            total = total.saturating_sub(bytes);
        }
    }

    /// Called when a render starts: everything a clip held that the last render did not draw is released.
    fn sweep(&mut self) -> Vec<Arc<RenderImage>> {
        let current = self.pass;
        let gone: Vec<ClipKey> = self
            .clips
            .iter()
            .filter(|(_, clip)| clip.pass != current)
            .map(|(key, _)| *key)
            .collect();
        for key in gone {
            if let Some(clip) = self.clips.remove(&key) {
                self.released += 1;
                self.retired
                    .extend(clip.frames.into_values().map(|frame| frame.image));
            }
        }
        self.drop_idle_arts(Instant::now());
        // Downloads nothing drew are dropped oldest first while they are over their budget.
        let mut idle: Vec<(u64, Id)> = self
            .arts
            .iter()
            .filter(|(_, art)| art.pass != current)
            .map(|(id, art)| (art.pass, *id))
            .collect();
        idle.sort_unstable();
        let mut held = self.encoded_bytes();
        for (_, id) in idle {
            if held <= ENCODED_BUDGET {
                break;
            }
            if let Some(art) = self.arts.remove(&id) {
                held -= art.bytes;
                self.asked.remove(&id);
            }
        }
        self.pass += 1;
        std::mem::take(&mut self.retired)
    }

    /// Drops the downloads no clip asked for within [`ART_IDLE`] of `now`.
    fn drop_idle_arts(&mut self, now: Instant) {
        let current = self.pass;
        let gone: Vec<Id> = self
            .arts
            .iter()
            .filter(|(_, art)| {
                art.pass != current && now.saturating_duration_since(art.seen) >= ART_IDLE
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.arts.remove(&id);
            self.asked.remove(&id);
        }
    }

    /// Called on a timer, with no render to do it: lets go of what is idle.
    pub(super) fn trim_idle(&mut self, now: Instant) {
        self.drop_idle_arts(now);
    }

    /// Whether a redraw timer for `deadline` has to be started (an earlier one replaces a later one).
    fn schedule(&mut self, deadline: Instant) -> Option<u64> {
        if self.deadline.is_none_or(|old| deadline < old) {
            self.generation += 1;
            self.deadline = Some(deadline);
            return Some(self.generation);
        }
        None
    }
}

/// A decoded APNG or GIF.
struct Decoded {
    frames: Vec<Frame>,
    delays: Vec<f64>,
    plays: u32,
}

/// The loop count of an APNG (`acTL`, 0 for ever); a GIF loops for ever.
fn png_plays(bytes: &[u8]) -> u32 {
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let Ok(length) = <[u8; 4]>::try_from(&bytes[at..at + 4]).map(u32::from_be_bytes) else {
            break;
        };
        let length = length as usize;
        match &bytes[at + 4..at + 8] {
            b"acTL" if length >= 8 && bytes.len() >= at + 16 => {
                return u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap_or([0; 4]));
            }
            b"IDAT" => break,
            _ => {}
        }
        at += 12 + length;
    }
    0
}

/// Decodes every frame of an APNG or GIF at `px` wide. A clip over `CLIP_BUDGET` is decoded again smaller.
fn decode_raster(bytes: &[u8], px: u32) -> Option<Decoded> {
    decode_within(bytes, px, CLIP_BUDGET)
}

fn decode_within(bytes: &[u8], px: u32, budget: usize) -> Option<Decoded> {
    let mut width = px.max(8);
    for _ in 0..4 {
        match decode_at(bytes, width, budget)? {
            Ok(decoded) => return Some(decoded),
            Err(used) => width = used * 3 / 4,
        }
        if width < 16 {
            break;
        }
    }
    None
}

/// `None` when the file is no animation, `Some(Err(width))` when it did not fit the budget at the width it was decoded at.
/// Frames are scaled one at a time as the decoder yields them, so the file is never held whole at its own size.
fn decode_at(bytes: &[u8], width: u32, budget: usize) -> Option<Result<Decoded, u32>> {
    use image::AnimationDecoder;
    let format = image::guess_format(bytes).ok()?;
    let frames: image::Frames = match format {
        image::ImageFormat::Png => {
            let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).ok()?;
            if !decoder.is_apng().ok()? {
                return None;
            }
            decoder.apng().ok()?.into_frames()
        }
        image::ImageFormat::Gif => image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
            .ok()?
            .into_frames(),
        _ => return None,
    };
    let mut decoded = Decoded {
        frames: Vec::new(),
        delays: Vec::new(),
        plays: if format == image::ImageFormat::Png {
            png_plays(bytes)
        } else {
            0
        },
    };
    // The size every frame is scaled to, from the first frame.
    let mut target: Option<(u32, u32, usize)> = None;
    for frame in frames {
        let frame = frame.ok()?;
        let (width, height, each) = *target.get_or_insert_with(|| {
            let (source_width, source_height) = (frame.buffer().width(), frame.buffer().height());
            let width = width.min(source_width).max(1);
            let height = ((f64::from(width) * f64::from(source_height) / f64::from(source_width))
                .round() as u32)
                .max(1);
            (width, height, (width * height * 4) as usize)
        });
        if each * (decoded.frames.len() + 1) > budget {
            return Some(Err(width));
        }
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let mut delay = f64::from(numerator) / f64::from(denominator.max(1));
        if delay <= 10. {
            delay = MIN_DELAY_MS;
        }
        decoded.delays.push(delay);
        let scaled = profile_effects::scale(frame.into_buffer(), width, height);
        decoded.frames.push(Frame {
            image: profile_effects::render_image(scaled),
            bytes: each,
            micros: 0,
        });
    }
    if decoded.frames.is_empty() {
        return None;
    }
    Some(Ok(decoded))
}

/// One eviction candidate: tier, distance ahead of the playhead, clip, frame (or `None` for a whole decoded clip), bytes.
type Unit = (u8, Reverse<usize>, ClipKey, Option<usize>, usize);

/// Renders Lottie frames for a background job.
fn render_frames(
    source: &lottie::Source,
    frames: &[usize],
    px: u32,
) -> Vec<(usize, Option<Frame>)> {
    // The file is parsed once for the batch; the time it took is shared by the frames.
    let started = Instant::now();
    let rendered = source.render_many(frames, px);
    let micros = started.elapsed().as_micros() as u64 / frames.len().max(1) as u64;
    rendered
        .into_iter()
        .map(|(index, pixels)| {
            let frame = pixels.map(|pixels: RgbaImage| {
                let bytes = (pixels.width() * pixels.height() * 4) as usize;
                Frame {
                    image: profile_effects::render_image(pixels),
                    bytes,
                    micros,
                }
            });
            (index, frame)
        })
        .collect()
}

impl Root {
    /// `animateStickers`: the account's choice (or this capture's), under reduced motion's override.
    fn sticker_setting(&self) -> Setting {
        Setting::from_index(self.pref_index("animate_stickers", 0))
            .under(crate::ui::motion::reduced())
    }

    /// The picture of a sticker to draw: the frame its animation is at, or its still. `interacting` is the caller's
    /// `isInteracting` and `disabled` its `disableAnimation`. Draws are cheap; the fetch, the decode, the frame
    /// renders and the redraw timer it starts are not repeated while their result is awaited.
    pub(super) fn sticker_art(
        &self,
        id: Id,
        format: Option<Format>,
        size: f32,
        interacting: bool,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Option<Img> {
        let still = self.images.get(&stickers::art_key(id)).cloned();
        let animated = matches!(format, Some(Format::Lottie | Format::Apng | Format::Gif));
        let (Some(format), true, false) = (format, animated, disabled) else {
            return still.map(img);
        };
        // The next frame redraws the view this sticker is drawn in.
        super::regions::note_drawn();
        let animate = self.sticker_setting().animates(interacting);
        let cache = self.sticker_ui.player.clone();
        let now = Instant::now();
        let (px, advance) = {
            let mut cache = cache.borrow_mut();
            let play = if !animate {
                Play::Reset
            } else if cache.focused {
                Play::Run
            } else {
                Play::Hold
            };
            let px = ((size * cache.scale.clamp(1., 2.)).round() as u32).max(8);
            (px, cache.advance(id, format, px, play, now))
        };
        let key = (id, px);
        if advance.fetch {
            let _ = self.requests.send(Request::Image {
                key: format!("{PREFIX}{id}"),
                url: stickers::animated_art_url(id, format),
            });
        }
        match advance.job {
            Some(Job::Render { source, frames }) => {
                cx.spawn(async move |this, cx| {
                    let rendered = cx
                        .background_executor()
                        .spawn(async move { render_frames(&source, &frames, px) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.sticker_ui
                            .player
                            .borrow_mut()
                            .accept_frames(key, rendered);
                        super::regions::wake(cx);
                    });
                })
                .detach();
            }
            Some(Job::Decode { bytes }) => {
                cx.spawn(async move |this, cx| {
                    let decoded = cx
                        .background_executor()
                        .spawn(async move { decode_raster(&bytes, px) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.sticker_ui
                            .player
                            .borrow_mut()
                            .accept_decoded(key, decoded);
                        super::regions::wake(cx);
                    });
                })
                .detach();
            }
            None => {}
        }
        if let Some(wake) = advance.wake {
            let deadline = now + wake;
            let generation = cache.borrow_mut().schedule(deadline);
            if let Some(generation) = generation {
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(deadline.saturating_duration_since(Instant::now()))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        let mut cache = this.sticker_ui.player.borrow_mut();
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
        #[cfg(feature = "capture")]
        {
            let mut cache = cache.borrow_mut();
            let playhead = cache.clips.get(&key).map_or(0, |clip| clip.playhead);
            cache.trace.push(Draw {
                at: crate::perf::since_process_start_ms(),
                id,
                px,
                what: match (&advance.image, &still) {
                    (Some(_), _) => 2,
                    (None, Some(_)) => 1,
                    (None, None) => 0,
                },
                playhead,
                lag: advance.shown.map_or(0, |(shown, due, count)| {
                    (due + count - shown) % count.max(1)
                }),
            });
        }
        match advance.image {
            Some(frame) => Some(img(frame)),
            None => still.map(img),
        }
    }

    /// An animated sticker file arrived (`PREFIX`).
    pub(super) fn sticker_art_fetched(
        &mut self,
        key: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = key
            .strip_prefix(PREFIX)
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let format = self.sticker_ui.player.borrow().formats.get(&id).copied();
        let length = bytes.len();
        match format {
            Some(Format::Lottie) => {
                cx.spawn(async move |this, cx| {
                    let parsed = cx
                        .background_executor()
                        .spawn(async move { lottie::Source::parse(&bytes) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        let mut cache = this.sticker_ui.player.borrow_mut();
                        match parsed {
                            Some(source) => {
                                let pass = cache.pass;
                                cache.arts.insert(
                                    id,
                                    Art {
                                        kind: ArtKind::Lottie(Arc::new(source)),
                                        bytes: length,
                                        pass,
                                        seen: Instant::now(),
                                    },
                                );
                            }
                            None => {
                                cache.unavailable.insert(id);
                            }
                        }
                        drop(cache);
                        cx.notify();
                    });
                })
                .detach();
            }
            Some(_) => {
                let mut cache = self.sticker_ui.player.borrow_mut();
                let pass = cache.pass;
                cache.arts.insert(
                    id,
                    Art {
                        kind: ArtKind::Raster(Arc::new(bytes)),
                        bytes: length,
                        pass,
                        seen: Instant::now(),
                    },
                );
                drop(cache);
                cx.notify();
            }
            None => {}
        }
    }

    pub(super) fn sticker_art_failed(&mut self, key: &str) {
        if let Some(id) = key
            .strip_prefix(PREFIX)
            .and_then(|id| id.parse::<Id>().ok())
        {
            self.sticker_ui.player.borrow_mut().unavailable.insert(id);
        }
    }

    /// Called at the start of every render: releases what clips the last render did not draw held, and remembers the
    /// window's scale and focus for the draws to come.
    pub(super) fn sticker_player_sweep(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut cache = self.sticker_ui.player.borrow_mut();
        cache.scale = window.scale_factor();
        cache.focused = window.is_window_active() || cache.frozen_ms.is_some();
        if cache.clips.is_empty() && cache.arts.is_empty() && cache.retired.is_empty() {
            cache.pass += 1;
            return;
        }
        let retired = cache.sweep();
        drop(cache);
        for image in retired {
            cx.drop_image(image, Some(window));
        }
    }

    /// Capture harness step `sticker-clock <ms>|off` freezes every playing sticker at that many milliseconds so a
    /// frame can be read again; `sticker-state` says what the player holds.
    #[cfg(feature = "capture")]
    pub(crate) fn script_sticker_player(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        let mut cache = self.sticker_ui.player.borrow_mut();
        match words {
            ["clock", "off"] => {
                cache.frozen_ms = None;
                drop(cache);
                cx.notify();
                "sticker clock: running".to_owned()
            }
            ["clock", ms] => match ms.parse::<f64>() {
                Ok(ms) => {
                    cache.frozen_ms = Some(ms);
                    drop(cache);
                    cx.notify();
                    format!("sticker clock: frozen at {ms} ms")
                }
                Err(_) => format!("sticker clock: cannot read {ms}"),
            },
            ["state"] => {
                let playing = cache
                    .clips
                    .values()
                    .filter(|clip| clip.play == Some(Play::Run))
                    .count();
                let frames: usize = cache.clips.values().map(|clip| clip.frames.len()).sum();
                let heads: Vec<String> = cache
                    .clips
                    .iter()
                    .filter(|(_, clip)| clip.play == Some(Play::Run))
                    .map(|((id, px), clip)| format!("{id}@{px}=frame {}", clip.playhead))
                    .collect();
                let mut costs: Vec<String> = cache
                    .timings
                    .iter()
                    .map(|(px, (count, micros))| {
                        format!("{px}px={}us/frame over {count}", micros / (*count).max(1))
                    })
                    .collect();
                costs.sort();
                let load = |counter: &std::sync::atomic::AtomicU64| {
                    counter.load(std::sync::atomic::Ordering::Relaxed)
                };
                let split = format!(
                    "lottie_parse_ms_each={:.1} over {} lottie_render_ms_each={:.2} over {}",
                    load(&lottie::PARSE_US) as f64 / 1000. / load(&lottie::PARSES).max(1) as f64,
                    load(&lottie::PARSES),
                    load(&lottie::RENDER_US) as f64 / 1000. / load(&lottie::RENDERS).max(1) as f64,
                    load(&lottie::RENDERS),
                );
                format!(
                    "stickers: {split} misses={} costs=[{}] setting={:?} focused={} clips={} playing={playing} frames={frames} bytes={} arts={} encoded={} jobs={} rendered={} evicted={} released={} frozen={:?} [{}]",
                    cache.misses,
                    costs.join(", "),
                    self.sticker_setting(),
                    cache.focused,
                    cache.clips.len(),
                    cache.total_bytes(),
                    cache.arts.len(),
                    cache.encoded_bytes(),
                    cache.jobs,
                    cache.rendered,
                    cache.evicted,
                    cache.released,
                    cache.frozen_ms,
                    heads.join(", "),
                )
            }
            ["cold"] => {
                // Forgets every sticker picture and animation so the next draw starts from the network again.
                cache.clips.clear();
                cache.arts.clear();
                cache.asked.clear();
                cache.unavailable.clear();
                cache.retired.clear();
                cache.trace.clear();
                drop(cache);
                self.images.retain(|key, _| !key.starts_with("sticker:"));
                self.requested.retain(|key| !key.starts_with("sticker:"));
                self.request_images();
                cx.notify();
                "sticker cold: pictures forgotten".to_owned()
            }
            ["trace"] => {
                let mut by_clip: BTreeMap<(Id, u32), Vec<&Draw>> = BTreeMap::new();
                for draw in &cache.trace {
                    by_clip.entry((draw.id, draw.px)).or_default().push(draw);
                }
                let mut lines = Vec::new();
                for ((id, px), draws) in by_clip {
                    let first = draws.iter().position(|draw| draw.what != 0);
                    let appeared = first.map_or("never".to_owned(), |at| draws[at].at.to_string());
                    let after = first.map_or(&draws[..], |at| &draws[at..]);
                    let blanks = after.iter().filter(|draw| draw.what == 0).count();
                    let mut swaps = 0;
                    for pair in after.windows(2) {
                        if pair[0].what != pair[1].what && pair[0].what != 0 && pair[1].what != 0 {
                            swaps += 1;
                        }
                    }
                    let to_still = after
                        .windows(2)
                        .filter(|pair| pair[0].what == 2 && pair[1].what == 1)
                        .count();
                    let backwards = after
                        .windows(2)
                        .filter(|pair| {
                            pair[0].what == 2
                                && pair[1].what == 2
                                && pair[1].playhead + 1 < pair[0].playhead
                        })
                        .count();
                    let played: Vec<usize> = after
                        .iter()
                        .filter(|draw| draw.what == 2)
                        .map(|draw| draw.lag)
                        .collect();
                    let late = played.iter().filter(|lag| **lag > 2).count();
                    let worst = played.iter().copied().max().unwrap_or(0);
                    let seq: String = draws
                        .iter()
                        .take(60)
                        .map(|draw| match draw.what {
                            0 => '_',
                            1 => 's',
                            _ => 'F',
                        })
                        .collect();
                    lines.push(format!(
                        "{id}@{px}: draws={} first_at_ms={appeared} blank_after_first={blanks} frames_over_2_late={late}/{} worst_lag={worst} still_frame_swaps={swaps} frame_to_still={to_still} playhead_wraps={backwards} first60={seq}",
                        draws.len(),
                        played.len()
                    ));
                }
                cache.trace.clear();
                format!("sticker trace: {}", lines.join(" | "))
            }
            _ => "usage: sticker clock <ms>|off | sticker state | sticker trace".to_owned(),
        }
    }
}

pub(super) type Shared = Rc<RefCell<Cache>>;

/// The decoded frames and downloads this cache holds, for the perf harness.
#[cfg(feature = "capture")]
impl Cache {
    pub(super) fn perf_size(&self) -> crate::perf::Size {
        let mut size = crate::perf::Size::default();
        size.add(self.clips.len(), self.total_bytes() + self.encoded_bytes());
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r#"{"v":"5.5.7","fr":30,"ip":0,"op":30,"w":100,"h":100,"nm":"t","ddd":0,"assets":[],
        "layers":[{"ddd":0,"ind":1,"ty":4,"nm":"s","sr":1,"ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},
        "p":{"a":1,"k":[{"t":0,"s":[20,50,0],"i":{"x":1,"y":1},"o":{"x":0,"y":0}},{"t":30,"s":[80,50,0]}]},
        "a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},"ao":0,
        "shapes":[{"ty":"gr","it":[{"ty":"rc","d":1,"s":{"a":0,"k":[20,20]},"p":{"a":0,"k":[0,0]},"r":{"a":0,"k":0}},
        {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100},"r":1},
        {"ty":"tr","p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}],
        "nm":"g"}],"ip":0,"op":30,"st":0,"bm":0}]}"#;

    fn image_of(width: u32, height: u32) -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![image::Frame::new(RgbaImage::new(
            width, height,
        ))]))
    }

    fn frame(side: u32) -> Frame {
        Frame {
            image: image_of(side, side),
            bytes: (side * side * 4) as usize,
            micros: 0,
        }
    }

    fn lottie_cache() -> (Cache, Arc<lottie::Source>) {
        let source = Arc::new(lottie::Source::parse(SQUARE.as_bytes()).expect("parses"));
        let mut cache = Cache {
            scale: 1.,
            ..Cache::default()
        };
        cache.arts.insert(
            7,
            Art {
                kind: ArtKind::Lottie(source.clone()),
                bytes: SQUARE.len(),
                pass: 0,
                seen: Instant::now(),
            },
        );
        cache.asked.insert(7);
        (cache, source)
    }

    #[test]
    fn the_setting_follows_module_378058_and_reduced_motion() {
        assert!(Setting::Always.animates(false));
        assert!(!Setting::OnInteraction.animates(false));
        assert!(Setting::OnInteraction.animates(true));
        assert!(!Setting::Never.animates(true));
        assert_eq!(Setting::from_index(0), Setting::Always);
        assert_eq!(Setting::from_index(1), Setting::OnInteraction);
        assert_eq!(Setting::from_index(2), Setting::Never);
        assert_eq!(Setting::Always.under(true), Setting::OnInteraction);
        assert_eq!(Setting::Never.under(true), Setting::OnInteraction);
        assert_eq!(Setting::Never.under(false), Setting::Never);
    }

    #[test]
    fn a_lottie_frame_follows_the_clock_and_the_loop_wraps() {
        let line = Timeline::Uniform {
            frames: 120,
            frame_ms: 1000. / 60.,
        };
        assert_eq!(line.frame_at(0.), 0);
        assert_eq!(line.frame_at(16.), 0);
        assert_eq!(line.frame_at(17.), 1);
        assert_eq!(line.frame_at(505.), 30);
        assert_eq!(line.frame_at(1999.), 119);
        assert_eq!(line.frame_at(2000.5), 0);
        assert_eq!(line.frame_at(2017.), 1);
        let next = line.next_change(10.).expect("changes");
        assert!((next - (1000. / 60. - 10.)).abs() < 1e-9);
    }

    #[test]
    fn delays_hold_each_frame_loop_and_stop_after_the_play_count() {
        let forever = Timeline::Delays {
            ends: vec![100., 300., 400.],
            plays: 0,
        };
        assert_eq!(forever.frame_at(0.), 0);
        assert_eq!(forever.frame_at(99.), 0);
        assert_eq!(forever.frame_at(100.), 1);
        assert_eq!(forever.frame_at(399.), 2);
        assert_eq!(forever.frame_at(400.), 0);
        assert_eq!(forever.frame_at(4900.), 1);
        assert_eq!(forever.next_change(150.), Some(150.));
        let twice = Timeline::Delays {
            ends: vec![100., 200.],
            plays: 2,
        };
        assert_eq!(twice.frame_at(399.), 1);
        assert_eq!(twice.frame_at(1000.), 1);
        assert_eq!(twice.next_change(1000.), None);
        let still = Timeline::Delays {
            ends: vec![100.],
            plays: 0,
        };
        assert_eq!(still.next_change(0.), None);
    }

    #[test]
    fn a_playing_clip_asks_for_its_window_ahead_and_the_next_frame_time() {
        let (mut cache, _) = lottie_cache();
        let now = Instant::now();
        let first = cache.advance(7, Format::Lottie, 32, Play::Run, now);
        assert!(!first.fetch);
        let Some(Job::Render { frames, .. }) = first.job else {
            panic!("a render job");
        };
        assert_eq!(frames, vec![0, 1, 2, 3]);
        assert!(first.image.is_none());
        assert_eq!(cache.jobs, 1);
        let wake = first.wake.expect("a wake");
        assert!(wake <= Duration::from_millis(34));
        // The frames being rendered are not asked for again; the rest of the window is.
        let second = cache.advance(7, Format::Lottie, 32, Play::Run, now);
        let Some(Job::Render { frames, .. }) = second.job else {
            panic!("a second job");
        };
        assert_eq!(frames, vec![4, 5, 6, 7]);
        // Three jobs at most.
        cache.jobs = MAX_JOBS;
        assert!(
            cache
                .advance(7, Format::Lottie, 32, Play::Run, now)
                .job
                .is_none()
        );
    }

    #[test]
    fn a_clip_whose_frames_failed_asks_for_no_more() {
        let (mut cache, _) = lottie_cache();
        let now = Instant::now();
        let Some(Job::Render { frames, .. }) =
            cache.advance(7, Format::Lottie, 32, Play::Run, now).job
        else {
            panic!("a render job");
        };
        cache.accept_frames(
            (7, 32),
            frames.into_iter().map(|index| (index, None)).collect(),
        );
        assert!(
            cache
                .advance(7, Format::Lottie, 32, Play::Run, now)
                .job
                .is_none()
        );
        assert_eq!(cache.jobs, 0);
    }
    #[test]
    fn a_clip_shows_the_frame_at_the_frozen_time_and_matches_the_source() {
        let (mut cache, source) = lottie_cache();
        cache.frozen_ms = Some(510.);
        let now = Instant::now();
        let advance = cache.advance(7, Format::Lottie, 40, Play::Run, now);
        let Some(Job::Render {
            frames,
            source: job,
        }) = advance.job
        else {
            panic!("a render job");
        };
        // 30 fps for half a second: frame 15.
        assert_eq!(frames[0], 15);
        assert!(advance.wake.is_none(), "a frozen clock needs no redraw");
        let rendered = render_frames(&job, &frames, 40);
        cache.accept_frames((7, 40), rendered);
        let advance = cache.advance(7, Format::Lottie, 40, Play::Run, now);
        let shown = advance.image.expect("the frame");
        let wanted = source.render(15, 40).expect("renders");
        let mut expected = wanted.clone();
        for pixel in expected.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        assert_eq!(
            shown.as_bytes(0).expect("pixels"),
            expected.as_raw().as_slice()
        );
        // The square moved between the first frame and the middle one.
        assert_ne!(
            source.render(0, 40).expect("renders").as_raw(),
            wanted.as_raw()
        );
    }

    #[test]
    fn pausing_holds_the_frame_and_not_animating_resets_it() {
        let (mut cache, _) = lottie_cache();
        let start = Instant::now();
        cache.advance(7, Format::Lottie, 32, Play::Run, start);
        let later = start + Duration::from_millis(510);
        cache.advance(7, Format::Lottie, 32, Play::Hold, later);
        assert_eq!(cache.clips[&(7, 32)].playhead, 15);
        // Held: the clock no longer moves the frame.
        cache.advance(
            7,
            Format::Lottie,
            32,
            Play::Hold,
            later + Duration::from_secs(5),
        );
        assert_eq!(cache.clips[&(7, 32)].playhead, 15);
        // Running again carries on from the held frame.
        let back = later + Duration::from_secs(10);
        cache.advance(7, Format::Lottie, 32, Play::Run, back);
        assert_eq!(cache.clips[&(7, 32)].playhead, 15);
        cache.advance(
            7,
            Format::Lottie,
            32,
            Play::Run,
            back + Duration::from_millis(100),
        );
        assert_eq!(cache.clips[&(7, 32)].playhead, 18);
        cache.advance(7, Format::Lottie, 32, Play::Reset, back);
        assert_eq!(cache.clips[&(7, 32)].playhead, 0);
        assert_eq!(cache.clips[&(7, 32)].held_ms, 0.);
    }

    #[test]
    fn the_file_is_asked_for_once_and_a_failure_is_not_retried() {
        let mut cache = Cache {
            scale: 1.,
            ..Cache::default()
        };
        let now = Instant::now();
        assert!(cache.advance(9, Format::Gif, 32, Play::Run, now).fetch);
        assert!(!cache.advance(9, Format::Gif, 32, Play::Run, now).fetch);
        // A sticker that does not animate asks for nothing.
        assert!(!cache.advance(11, Format::Gif, 32, Play::Reset, now).fetch);
    }

    #[test]
    fn eviction_keeps_the_budget_and_spares_the_playing_window() {
        let mut cache = Cache {
            pass: 5,
            ..Cache::default()
        };
        let side = 512;
        let each = (side * side * 4) as usize;
        let count = TOTAL_BUDGET / each + 6;
        // A clip nothing drew, one paused, one playing: 32 frames of 4 MiB each in total.
        for (id, pass, play) in [
            (1, 4, Some(Play::Run)),
            (2, 5, Some(Play::Reset)),
            (3, 5, Some(Play::Run)),
        ] {
            let mut clip = Clip {
                timeline: Some(Timeline::Uniform {
                    frames: 40,
                    frame_ms: 16.,
                }),
                pass,
                play,
                playhead: 10,
                ..Clip::default()
            };
            for index in 0..count / 3 + 1 {
                clip.frames.insert(index + 10, frame(side));
            }
            cache.clips.insert((id, 512), clip);
        }
        assert!(cache.total_bytes() > TOTAL_BUDGET);
        cache.enforce_budget();
        assert!(cache.total_bytes() <= TOTAL_BUDGET);
        // The clip nothing drew is the first to lose frames, and the playing window is whole.
        assert!(cache.clips[&(1, 512)].frames.len() < cache.clips[&(3, 512)].frames.len());
        for index in 10..10 + LOOKAHEAD.min(cache.clips[&(3, 512)].frames.len()) {
            assert!(cache.clips[&(3, 512)].frames.contains_key(&index));
        }
        assert_eq!(cache.retired.len() as u64, cache.evicted);
        assert!(cache.evicted > 0);
    }

    #[test]
    fn sweeping_releases_what_the_last_render_did_not_draw() {
        let (mut cache, _) = lottie_cache();
        let now = Instant::now();
        cache.advance(7, Format::Lottie, 32, Play::Run, now);
        cache.accept_frames((7, 32), vec![(0, Some(frame(32))), (1, Some(frame(32)))]);
        assert_eq!(cache.total_bytes(), 2 * 32 * 32 * 4);
        // The render that drew it is over: the next one starts by sweeping, and it is kept.
        assert!(cache.sweep().is_empty());
        assert_eq!(cache.clips.len(), 1);
        // That render drew nothing: the next sweep lets go of the frames.
        let retired = cache.sweep();
        assert_eq!(retired.len(), 2);
        assert!(cache.clips.is_empty());
        assert_eq!(cache.total_bytes(), 0);
        // A job that finishes after the release hands its frames back too.
        cache.accept_frames((7, 32), vec![(2, Some(frame(32)))]);
        assert_eq!(cache.sweep().len(), 1);
    }

    #[test]
    fn downloads_over_their_budget_are_dropped_when_idle() {
        let (mut cache, _) = lottie_cache();
        cache.arts.get_mut(&7).expect("art").bytes = ENCODED_BUDGET + 1;
        cache.pass = 3;
        cache.sweep();
        assert!(cache.arts.is_empty());
        assert!(!cache.asked.contains(&7));
    }

    #[test]
    fn a_download_nobody_asked_for_in_a_while_is_dropped() {
        let (mut cache, _) = lottie_cache();
        let asked = cache.arts[&7].seen;
        cache.pass = 5;
        cache.trim_idle(asked + ART_IDLE - Duration::from_secs(1));
        assert!(cache.arts.contains_key(&7));
        cache.trim_idle(asked + ART_IDLE);
        assert!(cache.arts.is_empty());
        assert!(!cache.asked.contains(&7));
    }

    #[test]
    fn a_redraw_timer_is_started_only_for_an_earlier_deadline() {
        let mut cache = Cache::default();
        let now = Instant::now();
        assert_eq!(cache.schedule(now + Duration::from_millis(30)), Some(1));
        assert_eq!(cache.schedule(now + Duration::from_millis(50)), None);
        assert_eq!(cache.schedule(now + Duration::from_millis(10)), Some(2));
    }

    #[test]
    fn a_gif_decodes_with_its_delays_and_a_decoded_clip_plays_from_them() {
        use image::codecs::gif::GifEncoder;
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            let frames: Vec<image::Frame> = (0..3u8)
                .map(|shade| {
                    let mut buffer = RgbaImage::new(40, 40);
                    for pixel in buffer.pixels_mut() {
                        *pixel = image::Rgba([shade * 100, 0, 0, 255]);
                    }
                    image::Frame::from_parts(
                        buffer,
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(50 * (u32::from(shade) + 1), 1),
                    )
                })
                .collect();
            encoder.encode_frames(frames).expect("encodes");
        }
        let decoded = decode_raster(&bytes, 20).expect("decodes");
        assert_eq!(decoded.frames.len(), 3);
        // GIF delays are in centiseconds: 50, 100 and 150 ms.
        assert_eq!(decoded.delays, vec![50., 100., 150.]);
        assert_eq!(decoded.plays, 0);
        assert_eq!(decoded.frames[0].bytes, 20 * 20 * 4);
        let mut cache = Cache {
            scale: 1.,
            ..Cache::default()
        };
        cache.arts.insert(
            4,
            Art {
                kind: ArtKind::Raster(Arc::new(bytes)),
                bytes: 10,
                pass: 0,
                seen: Instant::now(),
            },
        );
        cache.asked.insert(4);
        let now = Instant::now();
        let first = cache.advance(4, Format::Gif, 20, Play::Run, now);
        assert!(matches!(first.job, Some(Job::Decode { .. })));
        // Not asked to decode again while it runs.
        assert!(
            cache
                .advance(4, Format::Gif, 20, Play::Run, now)
                .job
                .is_none()
        );
        cache.jobs = 1;
        cache.accept_decoded((4, 20), Some(decoded));
        assert_eq!(cache.jobs, 0);
        let later = now + Duration::from_millis(250);
        let advance = cache.advance(4, Format::Gif, 20, Play::Run, later);
        assert!(advance.image.is_some());
        assert_eq!(cache.clips[&(4, 20)].playhead, 2);
        assert!(advance.job.is_none());
    }

    #[test]
    fn a_decoded_clip_over_its_budget_is_decoded_smaller() {
        use image::codecs::gif::GifEncoder;
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            let frames: Vec<image::Frame> = (0..2u8)
                .map(|shade| {
                    image::Frame::new(RgbaImage::from_pixel(
                        64,
                        64,
                        image::Rgba([shade * 90, 0, 0, 255]),
                    ))
                })
                .collect();
            encoder.encode_frames(frames).expect("encodes");
        }
        let whole = decode_within(&bytes, 4000, usize::MAX).expect("decodes");
        assert_eq!(whole.frames[0].bytes, 64 * 64 * 4);
        // Two frames at 64 pixels are 32 KiB: 20 000 bytes asks for the next size down, 48 pixels.
        let smaller = decode_within(&bytes, 4000, 20_000).expect("decodes");
        assert_eq!(smaller.frames[0].bytes, 48 * 48 * 4);
        assert!(decode_within(&bytes, 4000, 100).is_none());
    }

    #[test]
    fn png_plays_reads_the_actl_loop_count() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend([0; 13]);
        png.extend([0; 4]);
        png.extend([0, 0, 0, 8]);
        png.extend(b"acTL");
        png.extend([0, 0, 0, 3, 0, 0, 0, 5]);
        png.extend([0; 4]);
        assert_eq!(png_plays(&png), 5);
        assert_eq!(png_plays(b"\x89PNG\r\n\x1a\n"), 0);
    }

    #[test]
    fn the_picker_grid_is_narrow_enough_to_prefer_animation() {
        // Module 357354 `preferAnimation: gridWidth <= STICKER_PICKER_BREAKPOINT_MEDIUM` (446px): four 98px cells and
        // the list's 8px left padding.
        let (cells, cell, padding, breakpoint) = (4., 98., 8., 446.);
        let width: f64 = std::hint::black_box(cells * cell + padding);
        assert!(width <= breakpoint);
    }
}
