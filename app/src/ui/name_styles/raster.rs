//! Draws a styled display name from its glyph outlines. gpui paints glyphs in one flat colour, so the effects that fill
//! text with a gradient, stroke it or put a shadow behind it are rasterised here into a small image, once per name.
//!
//! Every rule below is the stylesheet of module 658385 (`display name styles` effects, class names `container_or4Hj-`
//! and its children, rules in `assets/163466.*.css`). Lengths are CSS pixels; `Spec::scale` is the device pixel ratio.

use dc_state::display_name_styles::{Effect, Palette, Rgb};
use std::sync::Arc;
use tiny_skia::{
    Color, FillRule, GradientStop, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Point, Shader, SpreadMode, Stroke, Transform,
};

/// One glyph of the laid-out name: its id in the face, where its origin sits (CSS px from the start of the name), the
/// room it takes up to the next one, and which coloured letter it is (Gummy colours letters in turn; `None` for
/// spaces and emoji).
#[derive(Clone, Debug)]
pub struct GlyphAt {
    pub id: u16,
    pub x: f32,
    pub advance: f32,
    pub letter: Option<usize>,
}

#[derive(Clone)]
pub struct Spec {
    pub font: Arc<Vec<u8>>,
    pub size: f32,
    pub glyphs: Vec<GlyphAt>,
    /// The name's advance width.
    pub width: f32,
    pub line_height: f32,
    /// Ascent and descent already rounded the way text is placed on the line.
    pub ascent: f32,
    pub descent: f32,
    pub effect: Effect,
    pub palette: Palette,
    /// The contrast-adjusted colours of the style.
    pub colors: Vec<Rgb>,
    /// The name's length in UTF-16 units, which sizes the Prism cycle together with the width of `0`.
    pub name_len: usize,
    pub zero_width: f32,
    pub scale: f32,
}

/// Where the image sits, in CSS px from the top left of the name's line box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Premultiplied BGRA, as gpui's atlas takes it.
    pub bgra: Vec<u8>,
}

/// Length of one animation iteration in seconds: 4s for everything but the Prism's 2s scroll.
pub fn cycle(effect: Effect) -> f32 {
    if effect == Effect::Prism { 2. } else { 4. }
}

/// Whether the effect has anything to animate (Gradient and Glow hold still; Solid has no image).
pub fn animates(effect: Effect) -> bool {
    matches!(
        effect,
        Effect::Neon | Effect::Toon | Effect::Pop | Effect::Prism | Effect::Gummy
    )
}

/// Whether the effect is drawn here (Solid is plain coloured text).
pub fn needs_image(effect: Effect) -> bool {
    effect != Effect::Solid
}

fn neon_radius(size: f32) -> f32 {
    4. + 0.12 * size
}

fn toon_stroke(size: f32) -> f32 {
    1.6 + 0.04 * size
}

fn pop_stroke(size: f32) -> f32 {
    1.2 + 0.04 * size
}

fn pop_shift(size: f32) -> f32 {
    0.08 * size
}

/// The padding box of the name's inner span, which clips what the effect paints.
pub fn geometry(spec: &Spec) -> Geometry {
    let (w, lh, s) = (spec.width, spec.line_height, spec.size);
    match spec.effect {
        Effect::Neon => {
            let r = neon_radius(s);
            Geometry {
                left: -r,
                top: -r,
                width: w + 2. * r,
                height: lh + 2. * r,
            }
        }
        Effect::Toon => {
            let t = toon_stroke(s);
            Geometry {
                left: -t,
                top: 0.,
                width: w + 2. * t,
                height: lh + t,
            }
        }
        Effect::Pop => {
            let (t, shift) = (pop_stroke(s), pop_shift(s));
            Geometry {
                left: -t,
                top: 0.,
                width: w + t,
                height: lh + t + shift,
            }
        }
        Effect::Gummy => {
            let headroom = 0.12 * s;
            Geometry {
                left: -w * 0.3,
                top: -headroom,
                width: w * 1.6,
                height: lh + 2. * headroom,
            }
        }
        _ => Geometry {
            left: 0.,
            top: 0.,
            width: w,
            height: lh,
        },
    }
}

fn color(rgb: Rgb, alpha: f32) -> Color {
    Color::from_rgba(
        f32::from(rgb.0) / 255.,
        f32::from(rgb.1) / 255.,
        f32::from(rgb.2) / 255.,
        alpha,
    )
    .unwrap_or(Color::BLACK)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let channel = |a: u8, b: u8| lerp(f32::from(a), f32::from(b), t).round().clamp(0., 255.) as u8;
    Rgb(channel(a.0, b.0), channel(a.1, b.1), channel(a.2, b.2))
}

/// The value of a CSS `cubic-bezier(x1, y1, x2, y2)` timing function at progress `x`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    let axis = |a: f32, b: f32, t: f32| {
        let u = 1. - t;
        3. * u * u * t * a + 3. * u * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0f32, 1f32);
    let mut t = x;
    for _ in 0..24 {
        let at = axis(x1, x2, t);
        if (at - x).abs() < 1e-5 {
            break;
        }
        if at < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.;
    }
    axis(y1, y2, t)
}

/// `cubic-bezier(.44,.29,.48,1)`, the easing of the Toon, Pop and Gummy animations.
fn ease_main(x: f32) -> f32 {
    bezier(0.44, 0.29, 0.48, 1., x)
}

/// Value of a keyframe track at progress `p` (0..1 of the iteration): each segment between two keyframes is eased.
fn track(frames: &[(f32, f32)], p: f32, ease: impl Fn(f32) -> f32) -> f32 {
    let Some(&(first_at, first)) = frames.first() else {
        return 0.;
    };
    if p <= first_at {
        return first;
    }
    for pair in frames.windows(2) {
        let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
        if p <= to_at {
            let span = (to_at - from_at).max(1e-6);
            return lerp(from, to, ease((p - from_at) / span));
        }
    }
    frames.last().map_or(first, |frame| frame.1)
}

/// `toon-animation`: the background rests at the top for 5%, slides to the bottom by 55%, and rests there.
fn toon_position(t: f32) -> f32 {
    track(
        &[(0., 0.), (0.05, 0.), (0.55, 1.), (1., 1.)],
        t / 4.,
        ease_main,
    )
}

/// `neon-flicker-animation`: 0 is the white text, 1 the tinted flash.
fn neon_flash(t: f32) -> f32 {
    let frames = [
        (0., 0.),
        (0.15, 0.),
        (0.16, 1.),
        (0.18, 0.),
        (0.20, 0.),
        (0.22, 1.),
        (0.23, 0.),
        (0.25, 0.),
        (0.28, 1.),
        (0.50, 0.),
        (1., 0.),
    ];
    track(&frames, t / 4., |x| bezier(0.24, 0.31, 0.36, 0.93, x))
}

/// `pop-animation-main`: how far the white text lifts (negative) or sinks, in em.
fn pop_main_shift(t: f32) -> f32 {
    track(
        &[(0., 0.), (0.18, -0.05), (0.35, 0.08), (0.50, 0.), (1., 0.)],
        t / 4.,
        ease_main,
    )
}

/// `pop-animation-shadow`: the shadow's drop in em, and how far its gradient has slid (0..1).
fn pop_shadow(t: f32) -> (f32, f32) {
    let drop = track(
        &[
            (0., 0.08),
            (0.18, 0.13),
            (0.35, 0.),
            (0.50, 0.08),
            (1., 0.08),
        ],
        t / 4.,
        ease_main,
    );
    let slide = track(&[(0., 0.), (0.50, 1.), (1., 1.)], t / 4., ease_main);
    (drop, slide)
}

/// `gummy-squish` for a letter, `t` seconds after the animation started (the letter's own delay already removed).
fn gummy_scale(t: f32) -> (f32, f32) {
    if t < 0. {
        return (1., 1.);
    }
    let p = (t % 4.) / 4.;
    let x = track(
        &[
            (0., 1.),
            (0.07, 1.28),
            (0.14, 0.82),
            (0.21, 1.1),
            (0.28, 0.96),
            (0.35, 1.),
            (1., 1.),
        ],
        p,
        ease_main,
    );
    let y = track(
        &[
            (0., 1.),
            (0.07, 0.72),
            (0.14, 1.18),
            (0.21, 0.9),
            (0.28, 1.04),
            (0.35, 1.),
            (1., 1.),
        ],
        p,
        ease_main,
    );
    (x, y)
}

/// The glyph outlines of the whole name as one path in CSS px, origin at the line box's top left.
fn outlines(spec: &Spec, shift_y: f32) -> Option<Path> {
    let face = ttf_parser::Face::parse(&spec.font, 0).ok()?;
    let scale = spec.size / f32::from(face.units_per_em());
    let baseline =
        ((spec.line_height - spec.ascent - spec.descent) / 2.).floor() + spec.ascent + shift_y;
    let mut builder = PathBuilder::new();
    for glyph in &spec.glyphs {
        let mut pen = Pen {
            builder: &mut builder,
            scale,
            x: glyph.x,
            baseline,
        };
        face.outline_glyph(ttf_parser::GlyphId(glyph.id), &mut pen);
    }
    builder.finish()
}

/// One glyph's outline in CSS px at its own origin (for Gummy, which scales each letter on its own).
fn glyph_outline(spec: &Spec, glyph: &GlyphAt, shift_y: f32) -> Option<Path> {
    let one = Spec {
        glyphs: vec![glyph.clone()],
        ..spec.clone()
    };
    outlines(&one, shift_y)
}

struct Pen<'a> {
    builder: &'a mut PathBuilder,
    scale: f32,
    x: f32,
    baseline: f32,
}

impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x * self.scale, self.baseline - y * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.builder.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

fn solid(rgb: Rgb) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(color(rgb, 1.)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// A CSS `linear-gradient(to bottom <left|right>)` over a box: the line through the centre that puts the 50% colour on
/// the other two corners.
fn corner_line(x: f32, y: f32, w: f32, h: f32, to_right: bool) -> (Point, Point) {
    let norm = (w * w + h * h).sqrt().max(1e-3);
    let (dx, dy) = ((if to_right { h } else { -h }) / norm, w / norm);
    let length = 2. * w * h / norm;
    let (cx, cy) = (x + w / 2., y + h / 2.);
    (
        Point::from_xy(cx - dx * length / 2., cy - dy * length / 2.),
        Point::from_xy(cx + dx * length / 2., cy + dy * length / 2.),
    )
}

fn stops(list: &[(Rgb, f32)]) -> Vec<GradientStop> {
    list.iter()
        .map(|(rgb, at)| GradientStop::new(*at, color(*rgb, 1.)))
        .collect()
}

fn gradient(start: Point, end: Point, list: &[(Rgb, f32)], mode: SpreadMode) -> Paint<'static> {
    Paint {
        shader: LinearGradient::new(start, end, stops(list), mode, Transform::identity())
            .unwrap_or(Shader::SolidColor(Color::TRANSPARENT)),
        anti_alias: true,
        ..Paint::default()
    }
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_join: LineJoin::Miter,
        ..Stroke::default()
    }
}

/// Blurs a pixmap in place the way a CSS `text-shadow` blur of `radius` does (a Gaussian of sigma `radius / 2`),
/// as three box passes.
fn blur(pixmap: &mut Pixmap, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let ideal = (12. * sigma * sigma / 3. + 1.).sqrt();
    let mut low = ideal.floor() as i32;
    if low % 2 == 0 {
        low -= 1;
    }
    let low = low.max(1);
    let high = low + 2;
    let below = (12. * sigma * sigma - 3. * (low * low) as f32 - 12. * low as f32 - 9.)
        / (-4. * low as f32 - 4.);
    let below = below.round() as i32;
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    for pass in 0..3 {
        let size = if pass < below { low } else { high };
        let radius = ((size - 1) / 2) as usize;
        let data = pixmap.data_mut();
        box_blur(data, width, height, radius, true);
        box_blur(data, width, height, radius, false);
    }
}

fn box_blur(data: &mut [u8], width: usize, height: usize, radius: usize, horizontal: bool) {
    if radius == 0 {
        return;
    }
    let (lines, length) = if horizontal {
        (height, width)
    } else {
        (width, height)
    };
    let at = |line: usize, i: usize| {
        if horizontal {
            (line * width + i) * 4
        } else {
            (i * width + line) * 4
        }
    };
    let window = (2 * radius + 1) as u32;
    // Prefix sums per channel; what lies outside the image is transparent.
    let mut sums = vec![[0u32; 4]; length + 1];
    for line in 0..lines {
        for i in 0..length {
            for c in 0..4 {
                sums[i + 1][c] = sums[i][c] + u32::from(data[at(line, i) + c]);
            }
        }
        for i in 0..length {
            let low = i.saturating_sub(radius);
            let high = (i + radius + 1).min(length);
            for c in 0..4 {
                data[at(line, i) + c] = ((sums[high][c] - sums[low][c]) / window) as u8;
            }
        }
    }
}

fn pixmap(geometry: &Geometry, scale: f32) -> Option<Pixmap> {
    Pixmap::new(
        (geometry.width * scale).ceil().max(1.) as u32,
        (geometry.height * scale).ceil().max(1.) as u32,
    )
}

/// The transform from the line box's CSS px to the image's device pixels.
fn view(geometry: &Geometry, scale: f32) -> Transform {
    Transform::from_scale(scale, scale).pre_translate(-geometry.left, -geometry.top)
}

fn into_frame(pixmap: Pixmap) -> Frame {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut bgra = pixmap.take();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Frame {
        width,
        height,
        bgra,
    }
}

/// Paints the name with its effect, `time` seconds into the animation (`None`: the resting, not animated state).
pub fn render(spec: &Spec, time: Option<f32>) -> Option<Frame> {
    let geometry = geometry(spec);
    let mut target = pixmap(&geometry, spec.scale)?;
    let transform = view(&geometry, spec.scale);
    let t = time.unwrap_or(0.);
    let animated = time.is_some();
    let size = spec.size;
    match spec.effect {
        Effect::Solid => return None,
        Effect::Gradient | Effect::Glow => {
            let path = outlines(spec, 0.)?;
            let (start, end) = corner_line(0., 0., spec.width, spec.line_height, true);
            let list = dc_state::display_name_styles::gradient_stops(&spec.colors);
            target.fill_path(
                &path,
                &gradient(start, end, &list, SpreadMode::Pad),
                FillRule::Winding,
                transform,
                None,
            );
        }
        Effect::Prism => {
            let path = outlines(spec, 0.)?;
            let cycle_width = (spec.name_len as f32 * spec.zero_width).max(38.);
            let offset = if animated {
                -cycle_width * ((t / 2.) % 1.)
            } else {
                0.
            };
            let list = dc_state::display_name_styles::prism_stops(&spec.colors);
            let start = Point::from_xy(offset, 0.);
            let end = Point::from_xy(offset + cycle_width, 0.);
            target.fill_path(
                &path,
                &gradient(start, end, &list, SpreadMode::Repeat),
                FillRule::Winding,
                transform,
                None,
            );
        }
        Effect::Toon => {
            let path = outlines(spec, 0.)?;
            let width = toon_stroke(size);
            let palette = spec.palette;
            // The span's own text: the stroke colour, stroked in the same colour, stroke under fill.
            let paint = solid(palette.toon_stroke);
            target.stroke_path(&path, &paint, &stroke(width), transform, None);
            target.fill_path(&path, &paint, FillRule::Winding, transform, None);
            // The ::before copy: a 400% tall vertical gradient clipped to the text.
            let box_height = geometry.height;
            let position = if animated { toon_position(t) } else { 0. };
            let top = geometry.top - 3. * box_height * position;
            let list = [
                (Rgb(255, 255, 255), 0.),
                (palette.light2, 0.08),
                (palette.light1, 0.15),
                (palette.main, 0.25),
                (palette.light2, 0.45),
                (palette.main, 0.55),
                (Rgb(255, 255, 255), 0.75),
                (palette.light2, 0.83),
                (palette.light1, 0.90),
                (palette.main, 1.),
            ];
            let start = Point::from_xy(0., top);
            let end = Point::from_xy(0., top + 4. * box_height);
            target.fill_path(
                &path,
                &gradient(start, end, &list, SpreadMode::Pad),
                FillRule::Winding,
                transform,
                None,
            );
        }
        Effect::Neon => {
            let path = outlines(spec, 0.)?;
            let radius = neon_radius(size);
            let width = 1. + 0.04 * size;
            let mut shadow = pixmap(&geometry, spec.scale)?;
            let paint = solid(spec.palette.main);
            shadow.stroke_path(&path, &paint, &stroke(width), transform, None);
            shadow.fill_path(&path, &paint, FillRule::Winding, transform, None);
            blur(&mut shadow, radius / 2. * spec.scale);
            target.draw_pixmap(
                0,
                0,
                shadow.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            let flash = if animated { neon_flash(t) } else { 0. };
            let text = mix(Rgb(255, 255, 255), spec.palette.neon_flash(), flash);
            target.stroke_path(
                &path,
                &solid(spec.palette.neon_stroke),
                &stroke(width),
                transform,
                None,
            );
            target.fill_path(&path, &solid(text), FillRule::Winding, transform, None);
        }
        Effect::Pop => {
            let path = outlines(spec, 0.)?;
            let width = pop_stroke(size);
            let palette = spec.palette;
            let (drop, slide) = if animated { pop_shadow(t) } else { (0.08, 0.) };
            let lift = if animated { pop_main_shift(t) } else { 0. };
            // The ::before shadow: the gradient clipped to the text and its stroke, dropped below the white text.
            let shadow_path = outlines(spec, drop * size)?;
            let box_width = spec.width;
            let box_height = spec.line_height + 2. * (width + pop_shift(size));
            let (x0, y0) = (-slide * box_width, -slide * box_height);
            let (start, end) = corner_line(x0, y0, 2. * box_width, 2. * box_height, false);
            let list = [
                (palette.light1, 0.),
                (palette.light1, 0.06),
                (palette.main, 0.20),
                (palette.main, 0.50),
                (palette.light1, 0.56),
                (palette.main, 0.70),
                (palette.main, 1.),
            ];
            let paint = gradient(start, end, &list, SpreadMode::Pad);
            target.stroke_path(&shadow_path, &paint, &stroke(width), transform, None);
            target.fill_path(&shadow_path, &paint, FillRule::Winding, transform, None);
            let main = if lift == 0. {
                path
            } else {
                outlines(spec, lift * size)?
            };
            target.stroke_path(
                &main,
                &solid(palette.dark2),
                &stroke(width),
                transform,
                None,
            );
            target.fill_path(
                &main,
                &solid(Rgb(255, 255, 255)),
                FillRule::Winding,
                transform,
                None,
            );
        }
        Effect::Gummy => {
            if spec.colors.is_empty() {
                return None;
            }
            let headroom = 0.12 * size;
            for glyph in &spec.glyphs {
                let Some(path) = glyph_outline(spec, glyph, 0.) else {
                    continue;
                };
                let rgb = glyph
                    .letter
                    .map_or(spec.palette.main, |at| spec.colors[at % spec.colors.len()]);
                let local = match (animated, glyph.letter) {
                    (true, Some(index)) => gummy_scale(t - index as f32 * 0.05),
                    _ => (1., 1.),
                };
                let (cx, cy) = (
                    glyph.x + glyph.advance / 2.,
                    spec.line_height / 2. + headroom * 0.,
                );
                let letter = transform
                    .pre_translate(cx, cy)
                    .pre_scale(local.0, local.1)
                    .pre_translate(-cx, -cy);
                target.fill_path(&path, &solid(rgb), FillRule::Winding, letter, None);
            }
        }
    }
    Some(into_frame(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_reaches_both_ends() {
        assert!(ease_main(0.).abs() < 1e-3);
        assert!((ease_main(1.) - 1.).abs() < 1e-3);
        let mid = ease_main(0.5);
        assert!(mid > 0.5 && mid < 1.);
    }

    #[test]
    fn toon_rests_then_slides() {
        assert_eq!(toon_position(0.), 0.);
        assert_eq!(toon_position(0.1), 0.);
        assert!((toon_position(2.4) - 1.).abs() < 1e-3);
        assert!((toon_position(3.9) - 1.).abs() < 1e-3);
    }

    #[test]
    fn neon_flashes_only_at_three_moments() {
        assert_eq!(neon_flash(0.), 0.);
        assert!(neon_flash(0.64) > 0.99);
        assert!(neon_flash(0.88) > 0.99);
        assert!(neon_flash(1.12) > 0.99);
        assert_eq!(neon_flash(3.), 0.);
    }

    #[test]
    fn gummy_squishes_after_its_delay() {
        assert_eq!(gummy_scale(-0.1), (1., 1.));
        let (x, y) = gummy_scale(0.28);
        assert!((x - 1.28).abs() < 0.01 && (y - 0.72).abs() < 0.01);
        assert_eq!(gummy_scale(2.), (1., 1.));
    }

    #[test]
    fn the_bottom_right_gradient_line_crosses_the_centre() {
        let (start, end) = corner_line(0., 0., 60., 24., true);
        assert!(((start.x + end.x) / 2. - 30.).abs() < 1e-3);
        assert!(((start.y + end.y) / 2. - 12.).abs() < 1e-3);
        assert!(end.x > start.x && end.y > start.y);
        let (start, end) = corner_line(0., 0., 60., 24., false);
        assert!(end.x < start.x && end.y > start.y);
    }

    #[test]
    fn blur_spreads_and_keeps_the_total() {
        let mut map = Pixmap::new(21, 21).unwrap();
        map.fill_path(
            &PathBuilder::from_rect(tiny_skia::Rect::from_xywh(9., 9., 3., 3.).unwrap()),
            &solid(Rgb(255, 255, 255)),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let before: u32 = map.data().iter().map(|v| u32::from(*v)).sum();
        blur(&mut map, 2.);
        let after: u32 = map.data().iter().map(|v| u32::from(*v)).sum();
        assert!(map.data()[(5 * 21 + 10) * 4 + 3] > 0);
        assert!((i64::from(before) - i64::from(after)).abs() < i64::from(before) / 5);
    }

    /// Lays a name out from the font file alone (advances plus letter-spacing, no kerning).
    fn lay_out(
        data: &[u8],
        text: &str,
        size: f32,
        spacing_em: f32,
    ) -> (Vec<GlyphAt>, f32, f32, f32) {
        let face = ttf_parser::Face::parse(data, 0).unwrap();
        let scale = size / f32::from(face.units_per_em());
        let mut x = 0.;
        let mut glyphs = Vec::new();
        for (at, c) in text.chars().enumerate() {
            let id = face.glyph_index(c).unwrap_or_default();
            let advance =
                f32::from(face.glyph_hor_advance(id).unwrap_or(0)) * scale + spacing_em * size;
            glyphs.push(GlyphAt {
                id: id.0,
                x,
                advance,
                letter: Some(at),
            });
            x += advance;
        }
        (
            glyphs,
            x,
            (f32::from(face.ascender()) * scale).round(),
            (-f32::from(face.descender()) * scale).round(),
        )
    }

    /// Draws the two names captured in the real client's profile popout (lawleis: Toon in Headbang, Adam: Neon in
    /// Mainframe) when the fonts are in the capture cache, and writes the pictures to `GRAVECORD_NAME_STYLE_OUT`.
    #[test]
    fn the_captured_profile_names_draw() {
        let Some(cache) = std::env::var_os("LOCALAPPDATA").map(|dir| {
            std::path::PathBuf::from(dir)
                .join("gravecord-capture")
                .join("fonts")
        }) else {
            return;
        };
        let out = std::env::var_os("GRAVECORD_NAME_STYLE_OUT").map(std::path::PathBuf::from);
        let cases: [(&str, &str, Effect, Rgb, f32); 2] = [
            (
                "c00df4f25667809b",
                "lawleis",
                Effect::Toon,
                Rgb(0xf4, 0xf4, 0xf4),
                0.,
            ),
            (
                "4841dace333e9054",
                "Adam",
                Effect::Neon,
                Rgb(0xf3, 0xf3, 0xf4),
                0.03,
            ),
        ];
        for (hash, text, effect, main, spacing) in cases {
            let Ok(data) = std::fs::read(cache.join(format!("{hash}.ttf"))) else {
                continue;
            };
            let (glyphs, width, ascent, descent) = lay_out(&data, text, 20., spacing);
            let spec = Spec {
                font: Arc::new(data),
                size: 20.,
                glyphs,
                width,
                line_height: 24.,
                ascent,
                descent,
                effect,
                palette: Palette::of(main),
                colors: vec![main],
                name_len: text.len(),
                zero_width: 12.,
                scale: 4.,
            };
            let frame = render(&spec, None).expect("the name draws");
            assert!(frame.bgra.chunks(4).any(|pixel| pixel[3] > 200));
            // The resting picture is the animation's first frame, and the animation moves.
            let first = render(&spec, Some(0.)).expect("the first frame draws");
            assert_eq!(first.bgra, frame.bgra);
            let later = render(&spec, Some(if effect == Effect::Toon { 1.2 } else { 0.64 }));
            assert_ne!(later.expect("a later frame draws").bgra, frame.bgra);
            if let Some(out) = &out {
                let mut rgba = frame.bgra.clone();
                for pixel in rgba.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                let mut picture =
                    image::RgbaImage::from_raw(frame.width, frame.height, rgba).unwrap();
                // The popout's dark background, so the glow and strokes read the way they do on screen.
                let mut flat = image::RgbaImage::from_pixel(
                    frame.width,
                    frame.height,
                    image::Rgba([0, 0, 0, 255]),
                );
                image::imageops::overlay(&mut flat, &picture, 0, 0);
                picture = flat;
                picture.save(out.join(format!("{text}.png"))).unwrap();
            }
        }
    }

    #[test]
    fn effect_geometry_follows_the_stylesheet() {
        let spec = Spec {
            font: Arc::new(Vec::new()),
            size: 20.,
            glyphs: Vec::new(),
            width: 54.03,
            line_height: 24.,
            ascent: 20.,
            descent: 6.,
            effect: Effect::Neon,
            palette: Palette::of(Rgb(0xf3, 0xf3, 0xf4)),
            colors: Vec::new(),
            name_len: 4,
            zero_width: 12.,
            scale: 1.,
        };
        // pt-profile-art: the neon span measured 66.81 x 36.78 around a 54.03 x 24 name.
        let neon = geometry(&spec);
        assert!((neon.width - 66.83).abs() < 0.05 && (neon.height - 36.8).abs() < 0.05);
        let toon = geometry(&Spec {
            effect: Effect::Toon,
            ..spec
        });
        // pt-member-game: margins of -2.4px at 20px.
        assert!((toon.left + 2.4).abs() < 1e-3);
    }
}
