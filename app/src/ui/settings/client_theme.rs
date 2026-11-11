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
/// What Exit Preview restores: the palette and the colour theme from before the preview.
static SAVED_THEME: AtomicU8 = AtomicU8::new(2);
static SAVED_ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// The preset id of the account that was last applied, so a change made elsewhere is picked up once.
static LAST_ACCOUNT: AtomicUsize = AtomicUsize::new(usize::MAX);

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

pub(in crate::ui) fn active() -> Option<&'static Swatch> {
    SWATCHES.get(ACTIVE.load(Ordering::Relaxed).checked_sub(1)?)
}

pub(in crate::ui) fn previewing() -> bool {
    PREVIEW.load(Ordering::Relaxed)
}

/// The presets of `385803` with `theme: LIGHT` are the first eight (Mint Apple to Desert Khaki); their tile carries
/// a white layer over the gradient, the dark ones a black one.
pub(in crate::ui) fn is_light(swatch: &Swatch) -> bool {
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
    let (dark_opacity, light_opacity) = match name {
        // `lowest` is the frame behind everything and the sidebars (`high`: 0.45 + 0.4 * 0.75, light 0.42 + 0.4 * 0.7).
        "--background-base-lowest" => (0.75, 0.70),
        // `chat`: 0.48 + 0.4 * 0.8, light 0.85 * (0.6 + 0.4).
        "--background-base-lower" | "--channel-background-default" => (0.80, 0.85),
        // `highest`: 0.39 + 0.4 * 0.65, light 0.95.
        "--background-base-low" | "--chat-background-default" | "--chat-background" => (0.65, 0.95),
        _ => return None,
    };
    Some(if light { light_opacity } else { dark_opacity })
}

/// The colour a surface token resolves to while a colour theme is on, or `None` to use the palette's own.
fn overlay_opacity(name: &str, light: bool) -> Option<f32> {
    level(name, light)
}

pub(in crate::ui) fn overlay(name: &str) -> Option<Rgba> {
    let swatch = active()?;
    let light = is_light(swatch);
    let opacity = level(name, light)?;
    Some(layer(opacity, light))
}

/// A surface that repaints the gradient instead of lying over the surface behind it: the chat input
/// (`--background-gradient-highest`: the window's gradient under 0.65 black, or 0.95 white). It fills its parent, so
/// the parent has to be positioned, and it is drawn first.
pub(in crate::ui) fn cover(radius: f32) -> Option<AnyElement> {
    let swatch = active()?;
    let light = is_light(swatch);
    let opacity = if light { 0.95 } else { 0.65 };
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
        let key = (ACTIVE.load(Ordering::Relaxed), width as u32, height as u32);
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

/// Mixes the base colour of the theme into a panel colour (`--custom-theme-base-color-amount`, 50% for a client
/// theme): the preview panel of the dark capture is the palette's panel tinted toward the gradient's darkest hue.
fn tinted(name: &str) -> Rgba {
    let own = rgba(theme().color(name));
    let Some(swatch) = active() else {
        return own;
    };
    let Some(layer) = swatch.layers.last() else {
        return own;
    };
    // `--custom-theme-base-color` (module 345815): the darker (dark theme) or lighter (light theme) of the two
    // gradient colours at full saturation, lightness 0.05 or 0.9.
    let light = is_light(swatch);
    let brightness = |value: u32| (value >> 24) + ((value >> 16) & 0xff) + ((value >> 8) & 0xff);
    let stops = layer.stops.iter().map(|(value, _)| *value);
    let pick = if light {
        stops.max_by_key(|value| brightness(*value))
    } else {
        stops.min_by_key(|value| brightness(*value))
    };
    let hsl: Hsla = rgba(pick.unwrap_or(0)).into();
    let base: Rgba = Hsla {
        s: 1.,
        l: if light { 0.9 } else { 0.05 },
        a: 1.,
        ..hsl
    }
    .into();
    // The mix is 50% in the bundle; the panels of both captures sit about a fifth (dark) and a third (light) of the
    // way to it, measured on their pixels.
    let amount = if light { 0.33 } else { 0.2 };
    Rgba {
        r: own.r + (base.r - own.r) * amount,
        g: own.g + (base.g - own.g) * amount,
        b: own.b + (base.b - own.b) * amount,
        a: 1.,
    }
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

    /// "Exit Preview": the palette and colour theme from before.
    pub(in crate::ui) fn exit_theme_preview(&mut self, cx: &mut Context<Self>) {
        if !PREVIEW.swap(false, Ordering::Relaxed) {
            return;
        }
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
        ACTIVE.store(index + 1, Ordering::Relaxed);
        LAST_ACCOUNT.store(id as usize, Ordering::Relaxed);
        self.choose_theme(base_of(swatch));
        // `choose_theme` clears the colour theme; this is the one chosen.
        ACTIVE.store(index + 1, Ordering::Relaxed);
        let patch = self
            .stores
            .set_pref(&dc_proto::settings::CLIENT_THEME_PRESET, id);
        if !patch.is_empty() {
            let _ = self.requests.send(crate::net::Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(serde_json::json!({"settings": patch})),
            });
        }
        cx.refresh_windows();
        cx.notify();
    }

    /// A palette chosen on the Appearance page replaces a colour theme.
    pub(in crate::ui) fn clear_client_theme(&mut self) {
        ACTIVE.store(0, Ordering::Relaxed);
    }

    /// Follows the account's colour theme at READY and when another session changes it (Nitro only), once per value.
    pub(in crate::ui) fn sync_client_theme(&mut self) {
        if previewing() {
            return;
        }
        let account = if self.premium() == dc_state::premium::PremiumType::None {
            None
        } else {
            self.stores
                .prefs
                .get("appearance_client_theme_preset")
                .copied()
        };
        let key = account.map_or(usize::MAX - 1, |id| id as usize);
        if LAST_ACCOUNT.swap(key, Ordering::Relaxed) == key {
            return;
        }
        ACTIVE.store(
            account
                .and_then(swatch_of_preset)
                .map_or(0, |index| index + 1),
            Ordering::Relaxed,
        );
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
        if !previewing() {
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
            .on_click(cx.listener(|this, _, _, cx| {
                this.exit_theme_preview(cx);
                this.open_purchase(false, cx);
            }))
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
                                    "icons/modal_close.svg",
                                    24.,
                                    "--interactive-icon-default",
                                )),
                        ),
                )
                .child(
                    div()
                        .mx(px(8.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(tinted("--background-base-low"))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .px(px(8.))
                                .child(
                                    text(16., 600., "--text-strong").child("Create your own theme"),
                                )
                                .child(
                                    text(14., 400., "--text-default")
                                        .child("Style your space with endless color combinations."),
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
                                        this.exit_theme_preview(cx);
                                        this.open_purchase(false, cx);
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
                        .child(text(16., 600., "--text-strong").child("Color Themes"))
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
                                        .child(mask_icon(
                                            "icons/golive_heart.svg",
                                            20.,
                                            "--interactive-icon-default",
                                        )),
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
