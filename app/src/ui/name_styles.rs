//! Display name styles on screen: the Nitro name fonts, colours and effects (module 16371 `UserNameWithEffects`, 73392
//! `useDisplayNameStylesFont`, 824994 `useDisplayNameStylesEnabled`; the effect rules are the stylesheet of 658385).
//!
//! The font files are fetched from the official asset host the first time a name needs one and cached on disk; until a
//! font arrives its names show in the normal font, as the web page does while a `@font-face` loads. Names with an
//! effect that gpui's flat-colour text cannot draw (gradient fill, stroke, shadow) are rasterised off the UI thread
//! into a small image by [`raster`], one image per name, with frames for the animated effects.

mod raster;

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
}

// ---------------------------------------------------------------------------------------------------------------
// Fonts

struct Loaded {
    family: SharedString,
    data: Arc<Vec<u8>>,
    face: FontFace,
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
        family: family.into(),
        data,
        face: *face,
    }))
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

fn plan(text: &str, font: Font, font_size: Pixels, spacing: Pixels, window: &Window) -> Plan {
    let parts: Vec<&str> = if spacing == px(0.) {
        vec![text]
    } else {
        clusters(text)
    };
    let mut x = px(0.);
    let mut out = Vec::with_capacity(parts.len());
    let (mut ascent, mut descent) = (px(0.), px(0.));
    for part in parts {
        let line = shape(part, &font, font_size, Hsla::black(), window);
        ascent = ascent.max(line.ascent);
        descent = descent.max(line.descent);
        let advance = line.width + spacing;
        out.push(Cluster {
            text: SharedString::from(part.to_owned()),
            x,
            advance,
        });
        x += advance;
    }
    Plan {
        clusters: out,
        width: x,
        ascent,
        descent,
        font_id: window.text_system().resolve_font(&font),
        font,
        font_size,
        spacing,
    }
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

fn clock() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn fingerprint(spec: &raster::Spec, text: &str, family: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (text, family).hash(&mut hasher);
    for value in [spec.size, spec.line_height, spec.scale, spec.width] {
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
fn image(key: u64, animated: bool, spec: &raster::Spec, cx: &mut App) -> Option<Arc<RenderImage>> {
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
            if animated {
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
        let (face_font, spacing) = match loaded(font) {
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
                let planned = plan(&text, face_font.clone(), font_size, spacing, window);
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
        let face = loaded(font);
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
            && let Some(spec) = self.spec(plan, face, &colors, palette, line_height, window)
        {
            let key = fingerprint(&spec, &self.text, &face.family);
            let animating = shown == Shown::Animated && raster::animates(effect);
            let resting = image(key, false, &spec, cx);
            let frames = if animating {
                image(key, true, &spec, cx)
            } else {
                None
            };
            let geometry = raster::geometry(&spec);
            let origin = point(
                bounds.origin.x + px(geometry.left),
                bounds.origin.y + px(geometry.top),
            );
            let target = Bounds::new(origin, size(px(geometry.width), px(geometry.height)));
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
                if animating {
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
            let _ = line.paint(
                point(bounds.origin.x + cluster.x, bounds.origin.y),
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
            let _ = line.paint(
                point(bounds.origin.x + x, bounds.origin.y),
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
            ascent: f32::from(plan.ascent.round()),
            descent: f32::from(plan.descent.round()),
            effect: self.styles.effect(),
            palette,
            colors: colors.to_vec(),
            name_len: self.text.encode_utf16().count(),
            zero_width: f32::from(zero),
            scale: window.scale_factor(),
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
