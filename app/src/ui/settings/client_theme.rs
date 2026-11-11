//! Colour themes (client themes): a gradient behind the app with the surfaces drawn as translucent black or white
//! layers over it, the Preview Theme mode a free account gets from the Appearance page, and applying and saving a
//! theme for Nitro.
//!
//! Read in the bundle: the presets (`385803`: id, base theme, angle, stops), the stylesheet rules of
//! `.custom-theme-background` (`419070`: the opacity of each surface level over the gradient) and the token
//! definitions (`646514`: `gradient: {light, dark}`). Measured on `appearance-theme-preview-dark-darker-cozy@1.png`:
//! the sidebar is the gradient's start colour at 0.75 black (`--custom-background-gradient-high-opacity`) and the title
//! bar at 0.70 (`higher`).

use super::model::Swatch;
use super::swatches::SWATCHES;
use super::{Root, color, mask_icon};
use crate::ui::{set_theme, theme};
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, MouseButton, ParentElement, Rgba,
    SharedString, StatefulInteractiveElement, Styled, div, img, prelude::*, px, rgba,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// The colour theme drawn now: an index into `SWATCHES` plus one, 0 for none.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// Preview Theme mode is on (a free account trying a colour theme; nothing is saved).
static PREVIEW: AtomicBool = AtomicBool::new(false);
/// The Custom Theme editor is open (a preview mode of its own: the panel on the right is the editor).
static EDITING: AtomicBool = AtomicBool::new(false);
/// Counts every change of the custom theme, so the pictures and mixes made from it are made again.
static GENERATION: AtomicUsize = AtomicUsize::new(0);
/// `ACTIVE` while the custom theme of the editor is the one on screen.
const CUSTOM_ACTIVE: usize = usize::MAX;
/// The name of the swatch the editor builds.
pub(in crate::ui) const CUSTOM_NAME: &str = "Custom Theme";
/// What Exit Preview restores: the palette and the colour theme from before the preview.
static SAVED_THEME: AtomicU8 = AtomicU8::new(2);
static SAVED_ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// `BackgroundGradientPresetId` (module 873298) of each colour swatch, by the name the page shows.
const PRESET_IDS: &[(&str, u64)] = &[
    ("Mint Apple", 0),
    ("Citrus Sherbert", 1),
    ("Retro Raincloud", 2),
    ("Hanami", 3),
    ("Sunrise", 4),
    ("Cotton Candy", 5),
    ("LoFi Vibes", 6),
    ("Desert Khaki", 7),
    ("Sunset", 8),
    ("Chroma Glow", 9),
    ("Forest", 10),
    ("Crimson Moon", 11),
    ("Midnight Blurple", 12),
    ("Mars", 13),
    ("Dusk", 14),
    ("Under the Sea", 15),
    ("Retro Storm", 17),
    ("Neon Nights", 18),
    ("Sepia", 19),
    ("Strawberry Lemonade", 20),
    ("Aurora", 21),
    ("Blurple Twilight", 22),
];

pub(in crate::ui) fn preset_id(name: &str) -> Option<u64> {
    PRESET_IDS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, id)| *id)
}

fn swatch_of_preset(id: u64) -> Option<usize> {
    let name = PRESET_IDS.iter().find(|(_, known)| *known == id)?.0;
    SWATCHES.iter().position(|swatch| swatch.name == name)
}

/// What the editor's colours, direction and intensity make (`customUserThemeSettings`: colours, gradientAngle, baseMix).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Custom {
    /// 0xRRGGBB.
    pub colors: Vec<u32>,
    pub angle: f32,
    /// `baseMix` / `chassisMixAmount`, 0 to 100 (74 by default).
    pub base_mix: f32,
    /// The editor's Appearance pair: light mode, or dark.
    pub light: bool,
}

thread_local! {
    static CUSTOM: std::cell::RefCell<Option<(&'static Swatch, Custom)>> =
        const { std::cell::RefCell::new(None) };
}

/// Puts the editor's theme on screen: a swatch built from its colours (`linear-gradient(<angle>deg, <colours>)`, one
/// colour doubled, the stops evenly spaced). The swatch is leaked: one per change, a few bytes.
pub(in crate::ui) fn show_custom(custom: &Custom) {
    let mut colors = custom.colors.clone();
    if colors.len() == 1 {
        colors.push(colors[0]);
    }
    let last = colors.len().saturating_sub(1).max(1) as f32;
    let stops: Vec<(u32, f32)> = colors
        .iter()
        .enumerate()
        .map(|(index, colour)| (colour << 8, index as f32 / last))
        .collect();
    let layers: Vec<super::model::Layer> = vec![super::model::Layer {
        angle: custom.angle,
        stops: Box::leak(stops.into_boxed_slice()),
    }];
    let swatch: &'static Swatch = Box::leak(Box::new(Swatch {
        name: CUSTOM_NAME,
        group: "custom",
        base: None,
        layers: Box::leak(layers.into_boxed_slice()),
    }));
    CUSTOM.with(|slot| *slot.borrow_mut() = Some((swatch, custom.clone())));
    ACTIVE.store(CUSTOM_ACTIVE, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    set_theme(base_of(swatch));
}

fn custom_params() -> Option<Custom> {
    CUSTOM.with(|slot| slot.borrow().as_ref().map(|(_, custom)| custom.clone()))
}

pub(in crate::ui) fn editing() -> bool {
    EDITING.load(Ordering::Relaxed)
}

/// What the regions' cache has to key on besides the preset: changes with every edit of the custom theme.
pub(in crate::ui) fn generation() -> usize {
    GENERATION.load(Ordering::Relaxed)
}

/// Width of the panel on the right of the preview modes.
pub(in crate::ui) fn panel_width() -> f32 {
    if editing() {
        super::custom_theme::PANEL_WIDTH
    } else {
        PANEL_WIDTH
    }
}

/// The key the pictures and mixes of the active theme are kept under.
fn cache_id() -> usize {
    ACTIVE
        .load(Ordering::Relaxed)
        .wrapping_add(GENERATION.load(Ordering::Relaxed).wrapping_mul(1000))
}

/// `--custom-background-gradient-opacity-mix-amount` of a custom theme: (30 + 0.7 * (100 - baseMix)) / 100.
fn custom_mix_amount() -> Option<f32> {
    let custom = custom_params()?;
    Some((30. + 0.7 * (100. - custom.base_mix)) / 100.)
}

pub(in crate::ui) fn active() -> Option<&'static Swatch> {
    let index = ACTIVE.load(Ordering::Relaxed);
    if index == CUSTOM_ACTIVE {
        return CUSTOM.with(|slot| slot.borrow().as_ref().map(|(swatch, _)| *swatch));
    }
    SWATCHES.get(index.checked_sub(1)?)
}

/// Puts a colour theme on screen by its preset id (none: the plain palette).
pub(in crate::ui) fn show_preset(id: Option<u64>) {
    ACTIVE.store(
        id.and_then(swatch_of_preset).map_or(0, |index| index + 1),
        Ordering::Relaxed,
    );
}

pub(in crate::ui) fn previewing() -> bool {
    PREVIEW.load(Ordering::Relaxed)
}

/// The presets of `385803` with `theme: LIGHT` are the first eight (Mint Apple to Desert Khaki); their tile carries
/// a white layer over the gradient, the dark ones a black one.
pub(in crate::ui) fn is_light(swatch: &Swatch) -> bool {
    if swatch.name == CUSTOM_NAME {
        return custom_params().is_some_and(|custom| custom.light);
    }
    swatch
        .layers
        .first()
        .and_then(|layer| layer.stops.first())
        .is_some_and(|(value, _)| value >> 8 == 0xff_ffff)
}

/// The palette under a colour theme (`theme` of the preset: LIGHT, or DARK which is this client's `Darker`).
pub(in crate::ui) fn base_of(swatch: &Swatch) -> Theme {
    if is_light(swatch) {
        Theme::Light
    } else {
        Theme::Darker
    }
}

/// The opacity of the black (dark) or white (light) layer a surface token gets over the gradient while a colour
/// theme is on: `--custom-background-gradient-*-opacity` of `.custom-theme-background.custom-client-theme` with the
/// mix amount 1 (`419070`). The surfaces that read `--background-gradient-high` (sidebar), `chat` (chat, member list)
/// and `highest` (chat input, user panel) use the token each is built from here.
fn level(name: &str, light: bool) -> Option<f32> {
    // (dark, light)
    // (dark offset, dark factor, light offset, light factor): opacity = offset + mix amount * 0.4 * factor; a preset's
    // mix amount is 1, a custom theme's comes from its intensity.
    let (dark, light_opacity) = match name {
        // `lowest` is the frame behind everything and the sidebars (`high`: 0.45 + 0.4 * 0.75, light 0.42 + 0.4 * 0.7).
        "--background-base-lowest" => ((0.45, 0.75), (0.42, 0.70)),
        // `chat`: 0.48 + 0.4 * 0.8, light 0.85 * (0.6 + 0.4).
        "--background-base-lower" | "--channel-background-default" => ((0.48, 0.80), (0.51, 0.85)),
        // `highest`: 0.39 + 0.4 * 0.65, light 0.95.
        "--background-base-low" | "--chat-background-default" | "--chat-background" => {
            ((0.39, 0.65), (0.95, 0.))
        }
        _ => return None,
    };
    let amount = custom_mix_amount().unwrap_or(1.);
    let (offset, factor) = if light { light_opacity } else { dark };
    Some(offset + amount * 0.4 * factor)
}

/// The colour a surface token resolves to while a colour theme is on, or `None` to use the palette's own.
#[cfg(test)]
fn overlay_opacity(name: &str, light: bool) -> Option<f32> {
    level(name, light)
}

pub(in crate::ui) fn overlay(name: &str) -> Option<Rgba> {
    let swatch = active()?;
    let light = is_light(swatch);
    if name == "--background-surface-high" {
        return Some(mid_surface(swatch, light));
    }
    let Some(opacity) = level(name, light) else {
        return mixed(name, swatch);
    };
    Some(layer(opacity, light))
}

/// "High Contrast Mode" (`contrastMode` "high" of the accessibility store): the colour resolver of module 646514 swaps
/// the listed semantic tokens for the raw colour of `high_contrast_tokens`.
static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

/// "Show on/off indicators" (`switchIconsEnabled`): the switch thumb carries a cross (off) or a tick (on), module 489718.
static SWITCH_ICONS: AtomicBool = AtomicBool::new(false);

pub(in crate::ui) fn switch_icons() -> bool {
    SWITCH_ICONS.load(Ordering::Relaxed)
}

pub(in crate::ui) fn high_contrast(name: &str, theme: dc_tokens::Theme) -> Option<Rgba> {
    if !HIGH_CONTRAST.load(Ordering::Relaxed) {
        return None;
    }
    let table = super::high_contrast_tokens::TOKENS;
    let index = table.binary_search_by(|(key, _)| (*key).cmp(name)).ok()?;
    let column = match theme {
        dc_tokens::Theme::Dark => 0,
        dc_tokens::Theme::Light => 1,
        dc_tokens::Theme::Darker => 2,
        dc_tokens::Theme::Midnight => 3,
    };
    Some(rgba(table[index].1[column]))
}

/// The cards of the member list and the like (`--background-surface-high`) while a colour theme is on: the official
/// card paints the preset's gradient over its own box under a 0.70 white (light) or 0.75 black (dark) layer
/// (`background-image` of `container_g7esM5` in both preview captures). A flat colour stands in for the gradient:
/// the layer over the middle of the preset's first and last colour.
fn mid_surface(swatch: &Swatch, light: bool) -> Rgba {
    let stops = swatch.layers.last().map_or(&[][..], |layer| layer.stops);
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return rgba(0);
    };
    let mid = |shift: u32| ((first.0 >> shift) & 0xff).midpoint((last.0 >> shift) & 0xff);
    let under = [mid(24), mid(16), mid(8)].map(|channel| channel as f32 / 255.);
    let (over, opacity) = if light { (1., 0.70) } else { (0., 0.75) };
    let mixed = under.map(|channel| channel * (1. - opacity) + over * opacity);
    Rgba {
        r: mixed[0],
        g: mixed[1],
        b: mixed[2],
        a: 1.,
    }
}

/// A surface that repaints the gradient instead of lying over the surface behind it: the chat input
/// (`--background-gradient-highest`: the window's gradient under 0.65 black, or 0.95 white). It fills its parent, so
/// the parent has to be positioned, and it is drawn first.
pub(in crate::ui) fn cover(radius: f32) -> Option<AnyElement> {
    cover_with(radius, 0.95, 0.65)
}

/// The cards of the member list (`--background-surface-high`): the window's gradient at the card's own place under a
/// 0.70 white (light) or 0.75 black (dark) layer; the Citrus capture's first card is `251,232,194` at the top, which
/// is the first stop under 0.70 white.
pub(in crate::ui) fn surface_cover(radius: f32) -> Option<AnyElement> {
    cover_with(radius, 0.70, 0.75)
}

fn cover_with(radius: f32, light_opacity: f32, dark_opacity: f32) -> Option<AnyElement> {
    let swatch = active()?;
    let light = is_light(swatch);
    let opacity = if light { light_opacity } else { dark_opacity };
    Some(
        gpui::canvas(
            move |_, window, _| {
                let size = window.viewport_size();
                backdrop_for(swatch, f32::from(size.width), f32::from(size.height))
            },
            move |bounds, picture, window, _| {
                let size = window.viewport_size();
                let corners = gpui::Corners::all(px(radius));
                let _ = window.paint_image(
                    bounds,
                    gpui::Bounds::new(gpui::point(px(0.), px(0.)), size),
                    corners,
                    picture,
                    0,
                    false,
                );
                let mut layer = gpui::fill(bounds, layer(opacity, light));
                layer.corner_radii = corners;
                window.paint_quad(layer);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// The backdrop picture for the active theme and window size, made once per size.
fn backdrop_for(swatch: &'static Swatch, width: f32, height: f32) -> Arc<gpui::RenderImage> {
    BACKDROP.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (cache_id(), width as u32, height as u32);
        if cache.as_ref().map(|(known, _)| *known) != Some(key) {
            *cache = Some((key, backdrop_image(swatch, width, height)));
        }
        cache.as_ref().map(|(_, picture)| picture.clone()).unwrap()
    })
}

/// A black (dark) or white (light) layer of `opacity`.
fn layer(opacity: f32, light: bool) -> Rgba {
    let base = if light {
        0xffff_ff00_u32
    } else {
        0x0000_0000_u32
    };
    rgba(base | (opacity * 255.).round() as u32)
}

/// The gradient painted behind the whole window, `width` by `height` pixels: the preset's last layer, CSS angle and
/// stop positions (`--custom-theme-background`).
pub(in crate::ui) fn gradient_pixels(swatch: &Swatch, width: u32, height: u32) -> image::RgbaImage {
    let mut picture = image::RgbaImage::new(width.max(1), height.max(1));
    let Some(layer) = swatch.layers.last() else {
        return picture;
    };
    let radians = layer.angle.to_radians();
    let (dx, dy) = (radians.sin(), -radians.cos());
    let (w, h) = (width as f32, height as f32);
    let length = (w * dx).abs() + (h * dy).abs();
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        let along =
            ((x as f32 + 0.5 - w / 2.) * dx + (y as f32 + 0.5 - h / 2.) * dy) / length + 0.5;
        let [r, g, b] = colour_at(layer.stops, along);
        *pixel = image::Rgba([r, g, b, 255]);
    }
    picture
}

fn colour_at(stops: &[(u32, f32)], at: f32) -> [u8; 3] {
    let channels = |value: u32| [(value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8];
    let Some(first) = stops.first() else {
        return [0; 3];
    };
    if at <= first.1 {
        return channels(first.0);
    }
    for pair in stops.windows(2) {
        let ((from, start), (to, end)) = (pair[0], pair[1]);
        if at <= end {
            let t = if end > start {
                (at - start) / (end - start)
            } else {
                1.
            };
            let (a, b) = (channels(from), channels(to));
            return [0, 1, 2]
                .map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8);
        }
    }
    channels(stops.last().map_or(0, |last| last.0))
}

/// The window's backdrop: the active colour theme's gradient, one pixel per four of the window.
fn backdrop_image(swatch: &'static Swatch, width: f32, height: f32) -> Arc<gpui::RenderImage> {
    let picture = gradient_pixels(
        swatch,
        (width / 4.).ceil() as u32,
        (height / 4.).ceil() as u32,
    );
    // `RenderImage` frames are BGRA.
    let mut picture = picture;
    for pixel in picture.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(picture)]))
}

/// `--custom-theme-base-color` and `--custom-theme-text-color` of a preset (module 345815 `i`, fed by `162772` `m`
/// with the preset's first and last colour): the lighter and the darker of the two by luminance, the darker one
/// standing in for the lighter when it has no hue; the base is the darker (dark theme, lightness .05) or lighter (light
/// theme, .9) one at full saturation, the text colour the lighter (dark) or darker (light) one at full saturation and
/// a lightness of .8 or .95 (dark) or .25 or .15 (light), the first for hues from 200 to 300.
struct ThemeMix {
    base: Rgba,
    text: Rgba,
}

fn luminance(colour: Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
}

fn theme_mix(swatch: &Swatch) -> ThemeMix {
    let stops = swatch.layers.last().map_or(&[][..], |layer| layer.stops);
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return ThemeMix {
            base: rgba(0x000000ff),
            text: rgba(0xffffffff),
        };
    };
    let (primary, secondary) = (rgba(first.0 | 0xff), rgba(last.0 | 0xff));
    let (lighter, mut darker) = if luminance(primary) > luminance(secondary) {
        (primary, secondary)
    } else {
        (secondary, primary)
    };
    let dark = !is_light(swatch);
    let hsl = |colour: Rgba| -> Hsla { colour.into() };
    if hsl(darker).s == 0. {
        darker = lighter;
    }
    let with = |colour: Rgba, lightness: f32| -> Rgba {
        Hsla {
            s: 1.,
            l: lightness,
            a: 1.,
            ..hsl(colour)
        }
        .into()
    };
    let text_source = if dark { lighter } else { darker };
    let hue = hsl(text_source).h * 360.;
    let blue = (200. ..=300.).contains(&hue);
    let text_lightness = match (dark, blue) {
        (true, true) => 0.8,
        (true, false) => 0.95,
        (false, true) => 0.25,
        (false, false) => 0.15,
    };
    ThemeMix {
        base: if dark {
            with(darker, 0.05)
        } else {
            with(lighter, 0.9)
        },
        text: with(text_source, text_lightness),
    }
}

fn to_oklab(colour: Rgba) -> [f32; 3] {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(colour.r), linear(colour.g), linear(colour.b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab(lab: [f32; 3], alpha: f32) -> Rgba {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    let channel = |linear: f32| {
        let linear = linear.clamp(0., 1.);
        if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1. / 2.4) - 0.055
        }
    };
    Rgba {
        r: channel(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s),
        g: channel(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
        b: channel(-0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s),
        a: alpha,
    }
}

/// `color-mix(in oklab, <own> 100%, <with> <amount>%)`: the percentages add to more than 100, so they are scaled to
/// share it, `with` getting `amount / (100 + amount)`.
fn mix_oklab(own: Rgba, with: Rgba, amount_percent: f32) -> Rgba {
    let weight = amount_percent / (100. + amount_percent);
    let (a, b) = (to_oklab(own), to_oklab(with));
    from_oklab([0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * weight), own.a)
}

/// The default amounts of a preset (`Zk`): 50% base, 38% text.
const BASE_AMOUNT: f32 = 50.;
const TEXT_AMOUNT: f32 = 38.;

/// The mix of the active theme, made once per theme.
fn active_mix(swatch: &'static Swatch) -> std::rc::Rc<ThemeMix> {
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(usize, std::rc::Rc<ThemeMix>)>> = const { std::cell::RefCell::new(None) };
    }
    let index = cache_id();
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().map(|(known, _)| *known) != Some(index) {
            *cache = Some((index, std::rc::Rc::new(theme_mix(swatch))));
        }
        cache.as_ref().map(|(_, mix)| mix.clone()).unwrap()
    })
}

/// A token the theme mixes with its base colour or its text colour (see `theme_mix_tokens`), mixed.
fn mixed(name: &str, swatch: &'static Swatch) -> Option<Rgba> {
    use super::theme_mix_tokens as tokens;
    let light = is_light(swatch);
    let (base, text) = if light {
        (tokens::BASE_LIGHT, tokens::TEXT_LIGHT)
    } else {
        (tokens::BASE_DARK, tokens::TEXT_DARK)
    };
    let is_base = base.binary_search(&name).is_ok();
    if !is_base && text.binary_search(&name).is_err() {
        return None;
    }
    let own = rgba(theme().color(name));
    let mix = active_mix(swatch);
    Some(if is_base {
        mix_oklab(own, mix.base, BASE_AMOUNT)
    } else {
        mix_oklab(own, mix.text, TEXT_AMOUNT)
    })
}

/// A panel colour of the palette with the theme's base colour mixed in, as the preview panel of the captures.
pub(super) fn tinted(name: &str) -> Rgba {
    let own = rgba(theme().color(name));
    let Some(swatch) = active() else {
        return own;
    };
    mix_oklab(own, active_mix(swatch).base, BASE_AMOUNT)
}

fn theme_value(theme: Theme) -> u8 {
    match theme {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::Darker => 2,
        Theme::Midnight => 3,
    }
}

fn theme_from(value: u8) -> Theme {
    match value {
        0 => Theme::Dark,
        1 => Theme::Light,
        3 => Theme::Midnight,
        _ => Theme::Darker,
    }
}

const PANEL_WIDTH: f32 = 298.;

impl Root {
    /// A colour tile or "Preview Theme": Nitro applies and saves the theme, any other account previews it.
    pub(super) fn pick_client_theme(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some(index) = SWATCHES.iter().position(|swatch| swatch.name == name) else {
            return;
        };
        if self.premium() == dc_state::premium::PremiumType::None {
            self.enter_theme_preview(index, cx);
            return;
        }
        self.apply_client_theme(index, cx);
    }

    /// Settings close and the whole app takes the colour theme, with the panel on the right (`Preview Theme`).
    fn enter_theme_preview(&mut self, index: usize, cx: &mut Context<Self>) {
        if !previewing() {
            SAVED_THEME.store(theme_value(theme()), Ordering::Relaxed);
            SAVED_ACTIVE.store(ACTIVE.load(Ordering::Relaxed), Ordering::Relaxed);
            PREVIEW.store(true, Ordering::Relaxed);
        }
        ACTIVE.store(index + 1, Ordering::Relaxed);
        set_theme(base_of(&SWATCHES[index]));
        self.close_settings(cx);
        cx.refresh_windows();
    }

    /// Settings close and the whole app takes the editor's theme, with the editor panel on the right.
    pub(super) fn enter_custom_theme_editor(&mut self, custom: &Custom, cx: &mut Context<Self>) {
        if !previewing() {
            SAVED_THEME.store(theme_value(theme()), Ordering::Relaxed);
            SAVED_ACTIVE.store(ACTIVE.load(Ordering::Relaxed), Ordering::Relaxed);
            PREVIEW.store(true, Ordering::Relaxed);
        }
        EDITING.store(true, Ordering::Relaxed);
        show_custom(custom);
        self.close_settings(cx);
        cx.refresh_windows();
    }

    /// "Exit Preview": the palette and colour theme from before.
    pub(in crate::ui) fn exit_theme_preview(&mut self, cx: &mut Context<Self>) {
        if !PREVIEW.swap(false, Ordering::Relaxed) {
            return;
        }
        EDITING.store(false, Ordering::Relaxed);
        self.custom_theme_closed();
        ACTIVE.store(SAVED_ACTIVE.load(Ordering::Relaxed), Ordering::Relaxed);
        set_theme(theme_from(SAVED_THEME.load(Ordering::Relaxed)));
        cx.refresh_windows();
        cx.notify();
    }

    /// Nitro: the colour theme is drawn now and saved as `appearance.theme` and the gradient preset id.
    fn apply_client_theme(&mut self, index: usize, cx: &mut Context<Self>) {
        let swatch = &SWATCHES[index];
        let Some(id) = preset_id(swatch.name) else {
            return;
        };
        let theme = super::theme_state::proto_of(base_of(swatch));
        self.theme_choose_preset(super::theme_state::Pick {
            theme,
            preset: Some(id),
        });
        cx.refresh_windows();
        cx.notify();
    }

    /// Every frame: the theme rules of `theme_state` put the palette and colour theme on screen.
    pub(in crate::ui) fn sync_client_theme(&mut self) {
        HIGH_CONTRAST.store(self.pref_on("high_contrast_mode", false), Ordering::Relaxed);
        SWITCH_ICONS.store(
            self.pref_on("enable_switch_icons", false),
            Ordering::Relaxed,
        );
        self.theme_frame();
    }

    /// The gradient behind the app, painted first in the window.
    pub(in crate::ui) fn theme_backdrop(&self, width: f32, height: f32) -> Option<AnyElement> {
        let swatch = active()?;
        let picture = backdrop_for(swatch, width, height);
        // The frame around the chat: the title bar and the guild list (`--custom-background-gradient-app-frame`:
        // 0.70 black, 0.65 white) and the sidebar (`high`: 0.75, 0.70), measured on the capture at their pixels.
        // The user panel at the bottom keeps its own token layer.
        let light = is_light(swatch);
        let (frame, side) = if light { (0.65, 0.70) } else { (0.70, 0.75) };
        let shade = |x: f32, y: f32, w: f32, h: f32, opacity: f32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .bg(layer(opacity, light))
        };
        let panel = 64.;
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(img(picture).size_full().object_fit(gpui::ObjectFit::Fill))
                .child(shade(0., 0., width, 32., frame))
                .child(shade(0., 32., 72., height - 32. - panel, frame))
                .child(shade(72., 32., 303., height - 32. - panel, side))
                // The margins around the user panel (8px, the panel at x 8 to 366 of the capture).
                .child(shade(0., height - panel, 375., 8., frame))
                .child(shade(0., height - 8., 375., 8., frame))
                .child(shade(0., height - panel + 8., 8., panel - 16., frame))
                .child(shade(366., height - panel + 8., 9., panel - 16., side))
                .into_any_element(),
        )
    }

    /// The right panel of Preview Theme mode (`design/golden/windows/settings/appearance-theme-preview-*`): 298 wide.
    pub(in crate::ui) fn theme_preview_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !previewing() || editing() {
            return None;
        }
        let chosen = ACTIVE.load(Ordering::Relaxed);
        let tiles: Vec<AnyElement> = SWATCHES
            .iter()
            .enumerate()
            .filter(|(_, swatch)| swatch.group == "client" && !swatch.layers.is_empty())
            .map(|(index, swatch)| {
                let picture = super::appearance::swatch_image(swatch);
                let selected = chosen == index + 1;
                div()
                    .id(SharedString::from(format!("preview-tile-{}", swatch.name)))
                    .relative()
                    .flex_none()
                    .size(px(60.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .child(img(picture).size(px(60.)).rounded(px(8.)))
                    .when(selected, |tile| {
                        // The page's selected tile: a brand ring 4px out and the tick disc over the top edge.
                        tile.child(
                            div()
                                .absolute()
                                .top(px(-4.))
                                .left(px(-4.))
                                .size(px(68.))
                                .rounded(px(10.))
                                .border_4()
                                .border_color(color("--background-brand")),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(px(-10.))
                                .right(px(-10.))
                                .size(px(24.))
                                .child(div().absolute().child(mask_icon(
                                    "theme_check_disc",
                                    24.,
                                    "--white",
                                )))
                                .child(div().absolute().child(mask_icon(
                                    "theme_check",
                                    24.,
                                    "--background-brand",
                                ))),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        ACTIVE.store(index + 1, Ordering::Relaxed);
                        set_theme(base_of(&SWATCHES[index]));
                        cx.refresh_windows();
                        cx.notify();
                        let _ = this;
                    }))
                    .into_any_element()
            })
            .collect();
        // The first tile is "Custom Theme" with its NEW badge (`badgeContainer_lUEm2A`).
        let custom = div()
            .id("preview-tile-custom")
            .relative()
            .flex_none()
            .size(px(60.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-strong"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.open_custom_theme_editor(cx)))
            .child(mask_icon("theme_palette", 24., "--text-default"))
            .child(div().absolute().top(px(-5.)).left(px(-5.)).child(mask_icon(
                "theme_new_ring",
                70.,
                "--background-brand",
            )))
            .child(
                div()
                    .absolute()
                    .top(px(-9.))
                    .left(px(8.))
                    .child(super::new_badge()),
            )
            .into_any_element();
        let tiles: Vec<AnyElement> = std::iter::once(custom).chain(tiles).collect();
        let on_unlock = if matches!(theme(), Theme::Light) {
            "--control-primary-text-default"
        } else {
            "--black"
        };
        let text = |size: f32, weight: f32, tint: &'static str| {
            div()
                .text_size(px(size))
                .line_height(px(size + 4.))
                .font_weight(FontWeight(weight))
                .text_color(color(tint))
        };
        Some(
            div()
                .id("theme-preview-panel")
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(PANEL_WIDTH))
                .flex()
                .flex_col()
                .bg(tinted("--background-base-lower"))
                .occlude()
                .child(
                    div()
                        .h(px(56.))
                        .flex_none()
                        .pl(px(34.))
                        .pr(px(18.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(text(20., 600., "--text-strong").child("Preview Theme"))
                        .child(
                            div()
                                .id("theme-preview-close")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| this.exit_theme_preview(cx)))
                                .child(mask_icon(
                                    "circle_x_small",
                                    20.,
                                    "--interactive-text-default",
                                )),
                        ),
                )
                .child(
                    div()
                        .ml(px(22.))
                        .mr(px(8.))
                        .mt(px(9.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(tinted("--background-base-low"))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .gap(px(12.))
                                .child(
                                    // `bannerImage_tE9t7v`: 57.2 by 33.8, centred in the 58px row.
                                    div()
                                        .flex_none()
                                        .w(px(57.2))
                                        .flex()
                                        .items_center()
                                        .children(
                                            self.images.get("theme_upsell_banner").cloned().map(
                                                |picture| img(picture).w(px(57.2)).h(px(33.8)),
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .child(
                                            text(16., 600., "--text-strong")
                                                .child("Create your own theme"),
                                        )
                                        // `body-sm/medium`, 2px under the title.
                                        .child(text(14., 500., "--text-default").mt(px(2.)).child(
                                            "Style your space with endless color combinations.",
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .id("theme-preview-try")
                                .h(px(32.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.open_custom_theme_editor(cx);
                                    }),
                                )
                                .child(
                                    text(14., 500., "--control-secondary-text-default")
                                        .child("Try It Out"),
                                ),
                        ),
                )
                .child(
                    div()
                        .pl(px(34.))
                        .pr(px(18.))
                        .pt(px(16.))
                        .child(
                            text(16., 600., "--text-strong")
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child("Color Themes")
                                .child(mask_icon("nitro_wheel", 16., "--text-strong")),
                        )
                        .child(
                            text(14., 400., "--text-default")
                                .pt(px(8.))
                                .child("Get Nitro and make Discord yours with unique colors."),
                        ),
                )
                .child(
                    div()
                        .id("theme-preview-tiles")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .pl(px(34.))
                        .pr(px(18.))
                        .pt(px(16.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .content_start()
                        .children(tiles),
                )
                .child(
                    div()
                        .flex_none()
                        .pl(px(34.))
                        .pr(px(28.))
                        .pb(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .size(px(40.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(mask_icon("golive_heart", 20., "--icon-default")),
                                )
                                .child(
                                    div()
                                        .id("theme-preview-unlock")
                                        .flex_1()
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .bg(if matches!(theme(), Theme::Light) {
                                            color("--control-primary-background-default")
                                        } else {
                                            color("--white")
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(8.))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.exit_theme_preview(cx);
                                                this.open_purchase(false, cx);
                                            }),
                                        )
                                        .child(mask_icon("nitro_wheel", 20., on_unlock))
                                        .child(
                                            text(16., 600., on_unlock).child("Unlock with Nitro"),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("theme-preview-exit")
                                .h(px(40.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| this.exit_theme_preview(cx)))
                                .child(
                                    text(16., 600., "--control-secondary-text-default")
                                        .child("Exit Preview"),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// The gradient picture kept for the theme and window size it was made for.
type Backdrop = ((usize, u32, u32), Arc<gpui::RenderImage>);

thread_local! {
    static BACKDROP: std::cell::RefCell<Option<Backdrop>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swatch(name: &str) -> &'static Swatch {
        SWATCHES.iter().find(|swatch| swatch.name == name).unwrap()
    }

    #[test]
    fn oklab_round_trips_and_mixes_by_the_shared_percentage() {
        let colour = rgba(0x3a7bd5ff);
        let back = from_oklab(to_oklab(colour), 1.);
        assert!((back.r - colour.r).abs() < 0.003 && (back.b - colour.b).abs() < 0.003);
        // 50% on top of 100% is a third of the way: black mixed with white lands at a third of the lightness.
        let third = mix_oklab(rgba(0x000000ff), rgba(0xffffffff), 50.);
        assert!(
            (to_oklab(third)[0] - 1. / 3.).abs() < 0.01,
            "{:?}",
            to_oklab(third)
        );
        // Nothing mixed in leaves the colour as it was, and its alpha stays.
        let same = mix_oklab(rgba(0x12345680), rgba(0xffffffff), 0.);
        assert!((same.a - 128. / 255.).abs() < 0.01);
    }

    #[test]
    fn a_dark_theme_mixes_in_its_darker_colour_and_a_light_one_its_lighter() {
        let dark = theme_mix(swatch("Sunset"));
        let dark_base: Hsla = dark.base.into();
        assert!((dark_base.l - 0.05).abs() < 0.01 && (dark_base.s - 1.).abs() < 0.01);
        let light = theme_mix(swatch("Mint Apple"));
        let light_base: Hsla = light.base.into();
        assert!((light_base.l - 0.9).abs() < 0.01);
        // Dark text is light, light text is dark.
        let (dark_text, light_text): (Hsla, Hsla) = (dark.text.into(), light.text.into());
        assert!(dark_text.l >= 0.8 && light_text.l <= 0.25);
    }

    #[test]
    fn the_token_lists_are_sorted_for_the_search() {
        use super::super::theme_mix_tokens::*;
        for list in [BASE_DARK, TEXT_DARK, BASE_LIGHT, TEXT_LIGHT] {
            assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert!(BASE_DARK.contains(&"--modal-background"));
        assert!(TEXT_DARK.contains(&"--icon-default"));
    }

    #[test]
    fn presets_know_their_base_theme() {
        assert!(is_light(swatch("Citrus Sherbert")));
        assert!(!is_light(swatch("Sunset")));
        assert_eq!(base_of(swatch("Sunset")), Theme::Darker);
        assert_eq!(base_of(swatch("Mint Apple")), Theme::Light);
    }

    #[test]
    fn every_colour_tile_has_its_preset_id() {
        for swatch in SWATCHES.iter().filter(|swatch| swatch.group == "client") {
            if !swatch.layers.is_empty() {
                assert!(preset_id(swatch.name).is_some(), "{}", swatch.name);
            }
        }
        assert_eq!(preset_id("Sunset"), Some(8));
        assert_eq!(
            swatch_of_preset(8),
            SWATCHES.iter().position(|s| s.name == "Sunset")
        );
    }

    #[test]
    fn the_sidebar_over_sunset_is_the_capture_colour() {
        // Sidebar (200, 300) of the capture is (18, 10, 35): the gradient's start colour #48288c under 0.75 black.
        let start = colour_at(swatch("Sunset").layers.last().unwrap().stops, 0.0);
        assert_eq!(start, [0x48, 0x28, 0x8c]);
        let covered = start.map(|channel| (channel as f32 * (1. - 0.75)).round() as u8);
        assert_eq!(covered, [18, 10, 35]);
        // The title bar is 0.70 (`higher`): (22, 12, 42).
        let frame = start.map(|channel| (channel as f32 * (1. - 0.70)).round() as u8);
        assert_eq!(frame, [22, 12, 42]);
    }

    #[test]
    fn the_gradient_runs_along_its_angle() {
        // Sunset is 141.68 degrees: the top left is before the first stop, the bottom right after the last.
        let picture = gradient_pixels(swatch("Sunset"), 40, 20);
        assert_eq!(picture.get_pixel(0, 0).0[..3], [0x48, 0x28, 0x8c]);
        assert_eq!(picture.get_pixel(39, 19).0[..3], [0xdb, 0x7f, 0x4b]);
    }

    #[test]
    fn citrus_sherbert_surfaces_match_the_capture() {
        // `appearance-theme-preview-citrus-darker-cozy@1.png`: 180deg, #f3b336 31.1%, #ee8558 67.09%.
        let stops = swatch("Citrus Sherbert").layers.last().unwrap().stops;
        let at = |y: f32| colour_at(stops, y / 720.);
        let over = |colour: [u8; 3], opacity: f32| {
            colour.map(|channel| (channel as f32 * (1. - opacity) + 255. * opacity).round() as i32)
        };
        let close =
            |got: [i32; 3], want: [i32; 3]| got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 2);
        // Rail (36, 300): the app frame, 0.65 white.
        assert!(
            close(over(at(300.), 0.65), [249, 223, 187]),
            "{:?}",
            over(at(300.), 0.65)
        );
        // Sidebar (200, 600): 0.70 white.
        assert!(
            close(over(at(600.), 0.70), [250, 218, 205]),
            "{:?}",
            over(at(600.), 0.70)
        );
        // Chat (500, 600) is an image; the member list (900, 420) is the chat level, 0.85 white.
        assert!(
            close(over(at(420.), 0.85), [252, 237, 228]),
            "{:?}",
            over(at(420.), 0.85)
        );
        // Title bar (300, 10): 0.65 white.
        assert!(
            close(over(at(10.), 0.65), [250, 227, 184]),
            "{:?}",
            over(at(10.), 0.65)
        );
        // The chat input (500, 690) repaints the gradient under 0.95 white.
        assert!(
            close(over(at(690.), 0.95), [254, 249, 246]),
            "{:?}",
            over(at(690.), 0.95)
        );
        // The levels the token layer uses for a light preset.
        assert_eq!(overlay_opacity("--background-base-lower", true), Some(0.85));
    }
}
