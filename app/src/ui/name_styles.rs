//! Display name styles on screen: the Nitro name fonts, colours and effects (module 16371 `UserNameWithEffects`, 73392
//! `useDisplayNameStylesFont`, 824994 `useDisplayNameStylesEnabled`; the effect rules are the stylesheet of 658385).
//!
//! The font files are fetched from the official asset host the first time a name needs one and cached on disk; until a
//! font arrives its names show in the normal font, as the web page does while a `@font-face` loads. Names with an
//! effect that gpui's flat-colour text cannot draw (gradient fill, stroke, shadow) are rasterised off the UI thread
//! into a small image by [`raster`], one image per name, with frames for the animated effects.

mod kerning;
mod raster;
#[cfg(feature = "capture")]
mod sheet;
#[cfg(feature = "capture")]
pub(in crate::ui) use sheet::layer as sheet_layer;

use super::color;
use crate::parity::Parity;
use dc_state::display_name_styles::{
    DisplayNameStyles, Effect, Font as NameFont, FontFace, Palette, Rgb, adjusted_colors,
};
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Corners, Element, ElementId, Font, FontFeatures,
    FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, ParentElement as _, Pixels, RenderImage, ShapedLine, SharedString, Size,
    Style, Styled as _, TextAlign, TextOverflow, TextRun, Window, point, px, size,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The accessibility setting "Display Name Styles" (`AccessibilityStore.displayNameStylesEnabled`, default on).
static ENABLED: AtomicBool = AtomicBool::new(true);
/// `useReducedMotion`: effects show but do not animate.
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

/// Set each frame from the settings, like the clock format, for the code that has no `Root` at hand.
pub(in crate::ui) fn set_preferences(enabled: bool, reduced_motion: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    REDUCED_MOTION.store(reduced_motion, Ordering::Relaxed);
}

pub(in crate::ui) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// What a place shows of a style (the `effectDisplayType` of 922301, and when it switches on hover).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Display {
    /// `PLAIN`: the font only; the name keeps its normal colour.
    Font,
    /// `STATIC`: font and the effect at rest.
    Static,
    /// `ANIMATED` always (a profile's name).
    Animated,
    /// `PLAIN`, `ANIMATED` while the pointer is on the name.
    FontOnHover,
    /// `STATIC` at rest, `ANIMATED` while the pointer is on the name (the account panel, 993382).
    StaticOnHover,
}

// ---------------------------------------------------------------------------------------------------------------
// Fonts

struct Loaded {
    family: SharedString,
    data: Arc<Vec<u8>>,
    face: FontFace,
    kerning: kerning::Kerning,
}

/// The `kern` pairs of a font file's GPOS table.
fn kerning_of(data: &[u8]) -> kerning::Kerning {
    ttf_parser::Face::parse(data, 0)
        .ok()
        .and_then(|face| {
            face.raw_face()
                .table(ttf_parser::Tag::from_bytes(b"GPOS"))
                .map(kerning::Kerning::new)
        })
        .unwrap_or_else(|| kerning::Kerning::new(&[]))
}

enum FontState {
    Loading,
    Ready(Arc<Loaded>),
    Failed,
}

fn fonts() -> &'static Mutex<HashMap<NameFont, FontState>> {
    static FONTS: OnceLock<Mutex<HashMap<NameFont, FontState>>> = OnceLock::new();
    FONTS.get_or_init(Mutex::default)
}

fn loaded(font: NameFont) -> Option<Arc<Loaded>> {
    match fonts().lock().ok()?.get(&font)? {
        FontState::Ready(loaded) => Some(loaded.clone()),
        _ => None,
    }
}

fn cache_path(hash: &str) -> Option<std::path::PathBuf> {
    let dir = crate::storage::data_dir()?.join("fonts");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{hash}.ttf")))
}

/// The decoded font file: from the disk cache, else from the official asset host.
fn fetch(hash: &str) -> Option<Vec<u8>> {
    let cached = cache_path(hash);
    if let Some(bytes) = cached.as_ref().and_then(|path| std::fs::read(path).ok()) {
        return Some(bytes);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let client = dc_http::Client::new(
        dc_proto::secrets::Token::new(""),
        dc_proto::identity::Identity::new(),
    )
    .ok()?;
    let url = format!("{}/assets/{hash}.woff2", dc_proto::pin::WEBAPP_ORIGIN);
    let compressed = runtime.block_on(client.fetch_public(&url)).ok()?;
    let bytes = wuff::decompress_woff2(&compressed).ok()?;
    if let Some(path) = cached {
        let partial = path.with_extension("part");
        if std::fs::write(&partial, &bytes).is_ok() {
            let _ = std::fs::rename(&partial, &path);
        }
    }
    Some(bytes)
}

/// The family name gpui will find the face under: the typographic family of the file, else the plain one.
fn family_names(data: &[u8]) -> Vec<String> {
    let Ok(face) = ttf_parser::Face::parse(data, 0) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for wanted in [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
    ] {
        for name in face.names() {
            if name.name_id == wanted
                && let Some(text) = name.to_string()
                && !names.contains(&text)
            {
                names.push(text);
            }
        }
    }
    names
}

/// Fonts wanted by code that draws text without an element of its own (a mention in a message); fetched on the next
/// frame by [`pump`].
fn wanted() -> &'static Mutex<Vec<NameFont>> {
    static WANTED: OnceLock<Mutex<Vec<NameFont>>> = OnceLock::new();
    WANTED.get_or_init(Mutex::default)
}

/// The face a style's font draws with, once it is loaded, for text laid out outside [`StyledName`]; asks for the
/// font otherwise.
pub(in crate::ui) fn run_font(styles: &DisplayNameStyles) -> Option<(SharedString, f32)> {
    if !enabled() {
        return None;
    }
    let font = styles.font();
    font.face()?;
    match loaded(font) {
        Some(loaded) => Some((loaded.family.clone(), f32::from(loaded.face.weight))),
        None => {
            if let Ok(mut wanted) = wanted().lock()
                && !wanted.contains(&font)
            {
                wanted.push(font);
            }
            None
        }
    }
}

/// Starts the fetches [`run_font`] asked for. Called every frame.
pub(in crate::ui) fn pump(cx: &mut App) {
    let fonts: Vec<NameFont> = wanted()
        .lock()
        .map(|mut wanted| std::mem::take(&mut *wanted))
        .unwrap_or_default();
    for font in fonts {
        request_font(font, cx);
    }
}

/// Starts fetching a font the first time a name needs it.
fn request_font(font: NameFont, cx: &mut App) {
    let Some(face) = font.face() else {
        return;
    };
    {
        let Ok(mut known) = fonts().lock() else {
            return;
        };
        if known.contains_key(&font) {
            return;
        }
        known.insert(font, FontState::Loading);
    }
    let slot: Arc<Mutex<Option<Option<Vec<u8>>>>> = Arc::default();
    let worker = slot.clone();
    std::thread::spawn(move || {
        let bytes = fetch(face.hash);
        if let Ok(mut slot) = worker.lock() {
            *slot = Some(bytes);
        }
    });
    cx.spawn(async move |cx| {
        let bytes = loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            if let Some(done) = slot.lock().ok().and_then(|mut slot| slot.take()) {
                break done;
            }
        };
        cx.update(|cx| {
            let state = bytes
                .and_then(|bytes| register(&face, bytes, cx))
                .map_or(FontState::Failed, FontState::Ready);
            if let Ok(mut known) = fonts().lock() {
                known.insert(font, state);
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

fn register(face: &FontFace, bytes: Vec<u8>, cx: &mut App) -> Option<Arc<Loaded>> {
    let names = family_names(&bytes);
    let data = Arc::new(bytes);
    cx.text_system()
        .add_fonts(vec![Cow::Owned(data.as_ref().clone())])
        .ok()?;
    let known = cx.text_system().all_font_names();
    let family = names.into_iter().find(|name| known.contains(name))?;
    tracing::debug!(family = face.family, registered = %family, "display name font ready");
    Some(Arc::new(Loaded {
        kerning: kerning_of(&data),
        family: family.into(),
        data,
        face: *face,
    }))
}

/// The normal font (gg sans) carrying an effect: the rasteriser needs its outlines, which the text system holds but does
/// not hand out, so the file is read from the same disk cache the app's fonts come from, one weight at a time.
fn default_hash(weight: f32) -> (&'static str, u16) {
    match weight as u16 {
        0..=449 => ("66d715454104d24e", 400),
        450..=549 => ("b272b33815319bae", 500),
        550..=649 => ("2df2c3ff74408972", 600),
        650..=749 => ("189422196a4f8b53", 700),
        _ => ("b2fdbe507d6ce9ef", 800),
    }
}

fn default_faces() -> &'static Mutex<HashMap<u16, Option<Arc<Loaded>>>> {
    static FACES: OnceLock<Mutex<HashMap<u16, Option<Arc<Loaded>>>>> = OnceLock::new();
    FACES.get_or_init(Mutex::default)
}

/// gg sans at `weight` for an effect on the normal font; starts the read the first time and answers once it is done.
fn default_face(weight: FontWeight, cx: &mut App) -> Option<Arc<Loaded>> {
    let (hash, weight) = default_hash(weight.0);
    {
        let mut known = default_faces().lock().ok()?;
        if let Some(state) = known.get(&weight) {
            return state.clone();
        }
        known.insert(weight, None);
    }
    let slot: Arc<Mutex<Option<Option<Vec<u8>>>>> = Arc::default();
    let worker = slot.clone();
    std::thread::spawn(move || {
        let bytes = fetch(hash);
        if let Ok(mut slot) = worker.lock() {
            *slot = Some(bytes);
        }
    });
    cx.spawn(async move |cx| {
        let bytes = loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            if let Some(done) = slot.lock().ok().and_then(|mut slot| slot.take()) {
                break done;
            }
        };
        cx.update(|cx| {
            if let (Some(bytes), Ok(mut known)) = (bytes, default_faces().lock()) {
                known.insert(
                    weight,
                    Some(Arc::new(Loaded {
                        family: "gg sans".into(),
                        kerning: kerning_of(&bytes),
                        data: Arc::new(bytes),
                        face: FontFace {
                            family: "gg sans",
                            weight,
                            letter_spacing_em: 0.,
                            hash,
                        },
                    })),
                );
            }
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

// ---------------------------------------------------------------------------------------------------------------
// Layout

/// Characters that continue the cluster before them: marks, joiners, variation selectors and skin tones.
fn continues_cluster(c: char, previous: Option<char>) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x200D | 0xFE00..=0xFE0F | 0x1F3FB..=0x1F3FF | 0x20D0..=0x20FF)
        || previous == Some('\u{200D}')
}

/// The text split into the units letter-spacing separates.
fn clusters(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut previous = None;
    for (at, c) in text.char_indices() {
        if at > 0 && !continues_cluster(c, previous) {
            out.push(&text[start..at]);
            start = at;
        }
        previous = Some(c);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

struct Cluster {
    text: SharedString,
    x: Pixels,
    advance: Pixels,
}

struct Plan {
    clusters: Vec<Cluster>,
    width: Pixels,
    ascent: Pixels,
    descent: Pixels,
    /// The line box's ascent and descent as the browser takes them from the face: each rounded from the `hhea` table.
    /// `None` for the normal font, which the text system measures the same way.
    metrics: Option<(Pixels, Pixels)>,
    font: Font,
    font_size: Pixels,
    spacing: Pixels,
    /// The id of the face in gpui's text system, to tell its glyphs from fallback ones.
    font_id: gpui::FontId,
}

fn shape(text: &str, font: &Font, font_size: Pixels, color: Hsla, window: &Window) -> ShapedLine {
    window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        font_size,
        &[TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

/// The face's ascent and descent for a line, rounded one by one the way the browser rounds them.
fn face_metrics(face: &Loaded, font_size: Pixels) -> Option<(Pixels, Pixels)> {
    let parsed = ttf_parser::Face::parse(&face.data, 0).ok()?;
    let scale = f32::from(font_size) / f32::from(parsed.units_per_em());
    Some((
        px((f32::from(parsed.ascender()) * scale).round()),
        px((-f32::from(parsed.descender()) * scale).round()),
    ))
}

/// A name whose letters all have glyphs in the face, laid out from the font file: each glyph's advance, the pair
/// kerning, then the letter-spacing after every glyph. `None` when some letter is missing or joins its neighbour.
fn direct_layout(
    text: &str,
    face: &Loaded,
    font_size: Pixels,
    spacing: Pixels,
) -> Option<Vec<Cluster>> {
    let parsed = ttf_parser::Face::parse(&face.data, 0).ok()?;
    let scale = f32::from(font_size) / f32::from(parsed.units_per_em());
    let mut glyphs = Vec::new();
    let mut previous = None;
    for c in text.chars() {
        if continues_cluster(c, previous) || c.is_control() {
            return None;
        }
        let glyph = parsed.glyph_index(c)?;
        glyphs.push((
            c,
            glyph.0,
            f32::from(parsed.glyph_hor_advance(glyph)?) * scale,
        ));
        previous = Some(c);
    }
    let mut x = 0.;
    let mut out = Vec::with_capacity(glyphs.len());
    for (at, (c, glyph, advance)) in glyphs.iter().enumerate() {
        let kern = glyphs
            .get(at + 1)
            .map_or(0., |next| face.kerning.pair(*glyph, next.1) as f32 * scale);
        let total = advance + kern + f32::from(spacing);
        out.push(Cluster {
            text: SharedString::from(c.to_string()),
            x: px(x),
            advance: px(total),
        });
        x += total;
    }
    Some(out)
}

fn plan(
    text: &str,
    font: Font,
    font_size: Pixels,
    spacing: Pixels,
    face: Option<&Loaded>,
    window: &Window,
) -> Plan {
    let metrics = face.and_then(|face| face_metrics(face, font_size));
    let direct = face.and_then(|face| direct_layout(text, face, font_size, spacing));
    let parts: Vec<&str> = if direct.is_some() {
        clusters(text)
    } else if spacing == px(0.) {
        vec![text]
    } else {
        clusters(text)
    };
    let (mut ascent, mut descent) = (px(0.), px(0.));
    for part in &parts {
        let line = shape(part, &font, font_size, Hsla::black(), window);
        ascent = ascent.max(line.ascent);
        descent = descent.max(line.descent);
    }
    let out = match direct {
        Some(laid) => laid,
        None => {
            let mut x = px(0.);
            let mut out = Vec::with_capacity(parts.len());
            for part in parts {
                let line = shape(part, &font, font_size, Hsla::black(), window);
                let advance = line.width + spacing;
                out.push(Cluster {
                    text: SharedString::from(part.to_owned()),
                    x,
                    advance,
                });
                x += advance;
            }
            out
        }
    };
    let width = out.last().map_or(px(0.), |last| last.x + last.advance);
    Plan {
        clusters: out,
        width,
        ascent,
        descent,
        metrics,
        font_id: window.text_system().resolve_font(&font),
        font,
        font_size,
        spacing,
    }
}

/// How far the text system's own baseline for `line` sits below the browser's for a line box of `line_height`.
fn baseline_shift(plan: &Plan, line: &ShapedLine, line_height: Pixels) -> Pixels {
    let Some((ascent, descent)) = plan.metrics else {
        return px(0.);
    };
    let browser = px(f32::from((line_height - ascent - descent) / 2.).floor()) + ascent;
    let ours = (line_height - line.ascent - line.descent) / 2. + line.ascent;
    // Half a pixel further down: the text system rounds the baseline a whole pixel up where the browser rounds half
    // up, measured against the style editor's message line (16px in a 22px line).
    browser - ours + px(0.5)
}

// ---------------------------------------------------------------------------------------------------------------
// Images

const FRAMES_PER_SECOND: f32 = 30.;
const IDLE_AFTER: Duration = Duration::from_secs(30);

struct Entry {
    image: Option<Arc<RenderImage>>,
    pending: bool,
    touched: Instant,
}

fn image_cache() -> &'static Mutex<HashMap<(u64, bool), Entry>> {
    static IMAGES: OnceLock<Mutex<HashMap<(u64, bool), Entry>>> = OnceLock::new();
    IMAGES.get_or_init(Mutex::default)
}

/// When each hovered name was entered, so its animation starts from its first frame.
fn hovers() -> &'static Mutex<HashMap<SharedString, Instant>> {
    static HOVERS: OnceLock<Mutex<HashMap<SharedString, Instant>>> = OnceLock::new();
    HOVERS.get_or_init(Mutex::default)
}

/// Capture only: the animation clock held at this many seconds (the comparison sheet's `at`); NaN bits when running.
#[cfg(feature = "capture")]
static FROZEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

#[cfg(feature = "capture")]
fn set_frozen(seconds: Option<f32>) {
    FROZEN.store(seconds.map_or(u32::MAX, f32::to_bits), Ordering::Relaxed);
}

fn frozen_time() -> Option<f32> {
    #[cfg(feature = "capture")]
    {
        let bits = FROZEN.load(Ordering::Relaxed);
        (bits != u32::MAX).then(|| f32::from_bits(bits))
    }
    #[cfg(not(feature = "capture"))]
    {
        None
    }
}

fn clock() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn fingerprint(spec: &raster::Spec, text: &str, family: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (text, family).hash(&mut hasher);
    for value in [
        spec.size,
        spec.line_height,
        spec.scale,
        spec.width,
        spec.shift_x,
    ] {
        value.to_bits().hash(&mut hasher);
    }
    spec.effect.hash(&mut hasher);
    for rgb in &spec.colors {
        (rgb.0, rgb.1, rgb.2).hash(&mut hasher);
    }
    hasher.finish()
}

/// Drops the images nobody painted for a while.
fn evict_idle(window: &mut Window, cx: &mut App) {
    static LAST: OnceLock<Mutex<Instant>> = OnceLock::new();
    let last = LAST.get_or_init(|| Mutex::new(Instant::now()));
    let Ok(mut last) = last.lock() else {
        return;
    };
    if last.elapsed() < Duration::from_secs(1) {
        return;
    }
    *last = Instant::now();
    let Ok(mut images) = image_cache().lock() else {
        return;
    };
    let idle: Vec<_> = images
        .iter()
        .filter(|(_, entry)| !entry.pending && entry.touched.elapsed() >= IDLE_AFTER)
        .map(|(key, _)| *key)
        .collect();
    for key in idle {
        if let Some(Entry {
            image: Some(image), ..
        }) = images.remove(&key)
        {
            cx.drop_image(image, Some(window));
        }
    }
}

/// The image of a name's effect, if it is ready; asks for it when it is not. `animated` asks for the whole loop.
fn image(
    key: u64,
    animated: bool,
    held: Option<f32>,
    spec: &raster::Spec,
    cx: &mut App,
) -> Option<Arc<RenderImage>> {
    // A clock held at one moment (capture) renders only that frame, under its own key.
    let key = held.map_or(key, |at| key ^ u64::from(at.to_bits()).rotate_left(32));
    let mut images = image_cache().lock().ok()?;
    let entry = images.entry((key, animated)).or_insert(Entry {
        image: None,
        pending: false,
        touched: Instant::now(),
    });
    entry.touched = Instant::now();
    if entry.image.is_none() && !entry.pending {
        entry.pending = true;
        let spec = spec.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(at) = held {
                raster::render(&spec, Some(at)).into_iter().collect()
            } else if animated {
                let cycle = raster::cycle(spec.effect);
                let count = (cycle * FRAMES_PER_SECOND).round() as usize;
                (0..count)
                    .filter_map(|at| raster::render(&spec, Some(at as f32 / FRAMES_PER_SECOND)))
                    .collect()
            } else {
                raster::render(&spec, None).into_iter().collect()
            }
        });
        cx.spawn(async move |cx| {
            let frames: Vec<raster::Frame> = task.await;
            cx.update(|cx| {
                let frames: Vec<image::Frame> = frames
                    .into_iter()
                    .filter_map(|frame| {
                        image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
                    })
                    .map(image::Frame::new)
                    .collect();
                if let Ok(mut images) = image_cache().lock()
                    && let Some(entry) = images.get_mut(&(key, animated))
                {
                    entry.pending = false;
                    entry.image = (!frames.is_empty()).then(|| Arc::new(RenderImage::new(frames)));
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
    entry.image.clone()
}

// ---------------------------------------------------------------------------------------------------------------
// The element

/// A display name with its style. Without a style, or with the setting off, it is the plain text.
pub(in crate::ui) struct StyledName {
    key: SharedString,
    text: SharedString,
    styles: Option<DisplayNameStyles>,
    display: Display,
    opacity: f32,
}

/// `text` drawn with `styles`. `key` tells names apart for the hover animation.
pub(in crate::ui) fn name(
    key: impl Into<SharedString>,
    text: impl Into<SharedString>,
    styles: Option<&DisplayNameStyles>,
    display: Display,
) -> StyledName {
    StyledName {
        key: key.into(),
        text: text.into(),
        styles: styles.cloned(),
        display,
        opacity: 1.,
    }
}

/// `name` in a box as wide as the text, carrying the parity probe `id` when the name is styled.
pub(in crate::ui) fn probed(name: StyledName, id: &'static str) -> gpui::Div {
    let styled = name.styles.is_some() && enabled();
    let holder = gpui::div().flex().flex_none().self_start();
    if styled {
        holder.parity(id).child(name)
    } else {
        holder.child(name)
    }
}

impl StyledName {
    /// `boldFontOpacity`: how opaque the Cherry Bomb and Chicle fonts draw when no effect shows.
    pub(in crate::ui) fn bold_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }
}

impl IntoElement for StyledName {
    type Element = AnyElement;

    fn into_element(self) -> AnyElement {
        match self.styles {
            Some(styles) if enabled() => NameText {
                key: self.key,
                text: self.text,
                styles,
                display: self.display,
                opacity: self.opacity,
                plan: Rc::default(),
            }
            .into_any_element(),
            _ => self.text.into_any_element(),
        }
    }
}

struct NameText {
    key: SharedString,
    text: SharedString,
    styles: DisplayNameStyles,
    display: Display,
    opacity: f32,
    plan: Rc<RefCell<Option<Plan>>>,
}

impl IntoElement for NameText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// How the name is drawn this frame.
#[derive(PartialEq)]
enum Shown {
    Plain,
    Resting,
    Animated,
}

impl NameText {
    fn shown(&self, hovered: bool) -> Shown {
        let reduced = REDUCED_MOTION.load(Ordering::Relaxed);
        let animated = if reduced {
            Shown::Resting
        } else {
            Shown::Animated
        };
        match self.display {
            Display::Font => Shown::Plain,
            Display::Static => Shown::Resting,
            Display::Animated => animated,
            Display::FontOnHover if hovered => animated,
            Display::FontOnHover => Shown::Plain,
            Display::StaticOnHover if hovered => animated,
            Display::StaticOnHover => Shown::Resting,
        }
    }
}

fn background() -> Rgb {
    let token = color("--background-base-low");
    let channel = |value: f32| (value * 255.).round().clamp(0., 255.) as u8;
    Rgb(channel(token.r), channel(token.g), channel(token.b))
}

fn hsla_of(rgb: Rgb, alpha: f32) -> Hsla {
    gpui::Rgba {
        r: f32::from(rgb.0) / 255.,
        g: f32::from(rgb.1) / 255.,
        b: f32::from(rgb.2) / 255.,
        a: alpha,
    }
    .into()
}

impl Element for NameText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let font = self.styles.font();
        if font.face().is_some() {
            request_font(font, cx);
        }
        let inherited = window.text_style();
        let rem = window.rem_size();
        let font_size = inherited.font_size.to_pixels(rem);
        let line_height = inherited.line_height_in_pixels(rem);
        let face_loaded = loaded(font);
        let (face_font, spacing) = match &face_loaded {
            Some(loaded) => (
                Font {
                    family: loaded.family.clone(),
                    features: FontFeatures::default(),
                    fallbacks: None,
                    weight: FontWeight(f32::from(loaded.face.weight)),
                    style: FontStyle::Normal,
                },
                font_size * loaded.face.letter_spacing_em,
            ),
            None => (inherited.font(), px(0.)),
        };
        let text = self.text.clone();
        let shared = self.plan.clone();
        let id =
            window.request_measured_layout(Style::default(), move |known, available, window, _| {
                let planned = plan(
                    &text,
                    face_font.clone(),
                    font_size,
                    spacing,
                    face_loaded.as_deref(),
                    window,
                );
                let limit = known.width.or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let width = limit.map_or(planned.width, |limit| planned.width.min(limit));
                let measured: Size<Pixels> = size(known.width.unwrap_or(width), line_height);
                *shared.borrow_mut() = Some(planned);
                measured
            });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Hitbox {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let plan_ref = self.plan.borrow();
        let Some(plan) = plan_ref.as_ref() else {
            return;
        };
        let inherited = window.text_style();
        let rem = window.rem_size();
        let line_height = inherited.line_height_in_pixels(rem);
        let hovered = hitbox.is_hovered(window);
        let shown = self.shown(hovered);
        if hovered && shown == Shown::Animated {
            if let Ok(mut hovers) = hovers().lock() {
                hovers.entry(self.key.clone()).or_insert_with(Instant::now);
            }
        } else if let Ok(mut hovers) = hovers().lock() {
            hovers.remove(&self.key);
        }

        let effect = self.styles.effect();
        let colors = adjusted_colors(&self.styles, background(), 1.);
        let main = colors.first().copied();
        let palette = main.map(Palette::of);
        let font = self.styles.font();
        let wants_effect = shown != Shown::Plain && raster::needs_image(effect);
        let face = loaded(font).or_else(|| {
            (font.face().is_none() && wants_effect)
                .then(|| default_face(inherited.font_weight, cx))
                .flatten()
        });
        let effect_shown = shown != Shown::Plain && face.is_some();

        // The ellipsis the parent asked for (`text-overflow: ellipsis`), when the name does not fit.
        let overflow = matches!(inherited.text_overflow, Some(TextOverflow::Truncate(_)))
            && plan.width > bounds.size.width + px(0.01);
        let limit = bounds.size.width;
        let ellipsis_width = if overflow {
            shape(
                "\u{2026}",
                &plan.font,
                plan.font_size,
                Hsla::black(),
                window,
            )
            .width
        } else {
            px(0.)
        };
        let mut visible: Vec<&Cluster> = Vec::new();
        for cluster in &plan.clusters {
            if overflow && cluster.x + cluster.advance - plan.spacing + ellipsis_width > limit {
                break;
            }
            visible.push(cluster);
        }
        let truncated = overflow && visible.len() < plan.clusters.len();

        // Plain text colour: the parent's, as opaque as `boldFontOpacity` says for the two bold fonts.
        let mut text_color = inherited.color;
        if shown == Shown::Plain && font.takes_bold_opacity() {
            text_color.a *= self.opacity;
        }
        if let (true, Some(main)) = (effect_shown && effect == Effect::Solid, main) {
            text_color = hsla_of(main, 1.);
        }

        let raster_effect = effect_shown && raster::needs_image(effect) && !truncated;
        let mut drawn = false;
        if raster_effect
            && let (Some(face), Some(palette)) = (face.as_ref(), palette)
            && let Some(mut spec) = self.spec(plan, face, &colors, palette, line_height, window)
        {
            // Chromium puts a line's baseline on a whole device pixel but positions glyphs along it to a fraction;
            // the picture is placed on whole pixels and carries the fraction in its glyph positions.
            let scale = window.scale_factor();
            let geometry = raster::geometry(&spec);
            let want_x = f32::from(bounds.origin.x) + geometry.left;
            let want_y = f32::from(bounds.origin.y) + geometry.top;
            let snapped_x = (want_x * scale).floor() / scale;
            let snapped_y = (want_y * scale).round() / scale;
            spec.shift_x = want_x - snapped_x;
            let key = fingerprint(&spec, &self.text, &face.family);
            let animating = shown == Shown::Animated && raster::animates(effect);
            let held = frozen_time().filter(|_| animating);
            let resting = image(key, false, held, &spec, cx);
            let frames = if animating && held.is_none() {
                image(key, true, None, &spec, cx)
            } else {
                None
            };
            let (pixels_w, pixels_h) = raster::image_size(&spec);
            let target = Bounds::new(
                point(px(snapped_x), px(snapped_y)),
                size(px(pixels_w as f32 / scale), px(pixels_h as f32 / scale)),
            );
            let picked = match (&frames, &resting) {
                (Some(frames), _) => {
                    let start = hovers()
                        .lock()
                        .ok()
                        .and_then(|hovers| hovers.get(&self.key).copied())
                        .unwrap_or_else(clock);
                    let t = start.elapsed().as_secs_f32() % raster::cycle(effect);
                    let at = ((t * FRAMES_PER_SECOND) as usize).min(frames.frame_count() - 1);
                    Some((frames.clone(), at))
                }
                (None, Some(resting)) => Some((resting.clone(), 0)),
                _ => None,
            };
            if let Some((picked, at)) = picked {
                let _ = window.paint_image(target, target, Corners::default(), picked, at, false);
                drawn = true;
                if animating && held.is_none() {
                    window.request_animation_frame();
                }
            }
        }
        if drawn {
            evict_idle(window, cx);
            return;
        }
        evict_idle(window, cx);

        // Plain text: the font's face, one cluster at a time so the letter-spacing applies.
        for cluster in &visible {
            let line = shape(
                &cluster.text,
                &plan.font,
                plan.font_size,
                text_color,
                window,
            );
            let lift = baseline_shift(plan, &line, line_height);
            let _ = line.paint(
                point(bounds.origin.x + cluster.x, bounds.origin.y + lift),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        if truncated {
            let x = visible.last().map_or(px(0.), |last| last.x + last.advance);
            let line = shape("\u{2026}", &plan.font, plan.font_size, text_color, window);
            let lift = baseline_shift(plan, &line, line_height);
            let _ = line.paint(
                point(bounds.origin.x + x, bounds.origin.y + lift),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
}

impl NameText {
    /// The glyphs of the laid-out name and the style, for the rasteriser.
    fn spec(
        &self,
        plan: &Plan,
        face: &Loaded,
        colors: &[Rgb],
        palette: Palette,
        line_height: Pixels,
        window: &Window,
    ) -> Option<raster::Spec> {
        let mut glyphs = Vec::new();
        let mut letter = 0usize;
        for cluster in &plan.clusters {
            let line = shape(
                &cluster.text,
                &plan.font,
                plan.font_size,
                Hsla::black(),
                window,
            );
            for run in line.runs.iter().filter(|run| run.font_id == plan.font_id) {
                for (at, glyph) in run.glyphs.iter().enumerate() {
                    let next = run
                        .glyphs
                        .get(at + 1)
                        .map_or(line.width, |next| next.position.x);
                    let character = cluster.text[glyph.index..].chars().next();
                    let colored = character.is_some_and(|c| !c.is_whitespace()) && !glyph.is_emoji;
                    glyphs.push(raster::GlyphAt {
                        id: u16::try_from(glyph.id.0).ok()?,
                        x: f32::from(cluster.x + glyph.position.x),
                        advance: f32::from(next - glyph.position.x),
                        letter: colored.then(|| {
                            letter += 1;
                            letter - 1
                        }),
                    });
                }
            }
        }
        let zero = shape("0", &plan.font, plan.font_size, Hsla::black(), window).width;
        Some(raster::Spec {
            font: face.data.clone(),
            size: f32::from(plan.font_size),
            glyphs,
            width: f32::from(plan.width),
            line_height: f32::from(line_height),
            ascent: f32::from(
                plan.metrics
                    .map_or(plan.ascent, |metrics| metrics.0)
                    .round(),
            ),
            descent: f32::from(
                plan.metrics
                    .map_or(plan.descent, |metrics| metrics.1)
                    .round(),
            ),
            effect: self.styles.effect(),
            palette,
            colors: colors.to_vec(),
            name_len: self.text.encode_utf16().count(),
            zero_width: f32::from(zero),
            scale: window.scale_factor(),
            shift_x: 0.,
        })
    }
}

impl super::Root {
    /// 609425: the style a user's name draws with in a server (`None` outside one): the member's own first, then
    /// the user's. The accessibility setting off hides both.
    pub(in crate::ui) fn name_styles(
        &self,
        user: &dc_state::User,
        guild: Option<dc_state::Id>,
    ) -> Option<DisplayNameStyles> {
        if !enabled() {
            return None;
        }
        let member = guild
            .and_then(|guild| self.stores.members.get(guild, user.id))
            .and_then(|member| member.display_name_styles.as_ref());
        dc_state::display_name_styles::effective(member, user.display_name_styles.as_deref())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_keep_marks_and_joiners_together() {
        assert_eq!(clusters("ab"), vec!["a", "b"]);
        assert_eq!(clusters("e\u{301}x"), vec!["e\u{301}", "x"]);
        assert_eq!(
            clusters("\u{1F468}\u{200D}\u{1F469}!"),
            vec!["\u{1F468}\u{200D}\u{1F469}", "!"]
        );
        assert!(clusters("").is_empty());
    }

    #[test]
    fn modes_follow_the_places() {
        let name = |display| NameText {
            key: SharedString::default(),
            text: SharedString::default(),
            styles: DisplayNameStyles::default(),
            display,
            opacity: 1.,
            plan: Rc::default(),
        };
        assert!(name(Display::Font).shown(true) == Shown::Plain);
        assert!(name(Display::Static).shown(true) == Shown::Resting);
        assert!(name(Display::FontOnHover).shown(false) == Shown::Plain);
        assert!(name(Display::Animated).shown(false) == Shown::Animated);
    }
}
