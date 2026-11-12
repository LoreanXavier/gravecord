//! A name in a role's gradient colours (the "enhanced role styles"). Module 36075 `Wq` puts the role's colours in
//! `--custom-gradient-color-1..3` and picks `twoColorGradient`/`threeColorGradient` (stylesheet 447505):
//! `linear-gradient(to right, c1, c2, c1)` or `(c1, c2, c3, c1)`, drawn on the name with `background-size: 100px auto`
//! (repeating) and `background-clip: text` (`usernameGradient`). While the pointer is on the name (or its message) the
//! background slides 100px in 1.5s, linear, forever (`gradientUsernameAnimation`).
//!
//! gpui paints one flat colour per text run, so each character takes the gradient's colour at its own centre (a
//! character is under 10px of the 100px period). The layout is the plain text's.

use dc_state::RoleColors;
use gpui::{
    App, IntoElement, RenderImage, Rgba, SharedString, StyledText, TextRun, Window, prelude::*, rgb,
};
use image::Frame;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// `background-size: 100px auto`.
const PERIOD: f32 = 100.;
/// `gradientUsernameAnimation 1.5s linear infinite`.
const ANIMATION_SECONDS: f32 = 1.5;

/// The gradient's stops, evenly spaced, ending on the first colour again.
fn stops(colors: &RoleColors) -> Vec<Rgba> {
    let mut list = vec![rgb(colors.primary)];
    list.push(rgb(colors.secondary.unwrap_or(colors.primary)));
    if let Some(third) = colors.tertiary {
        list.push(rgb(third));
    }
    list.push(rgb(colors.primary));
    list
}

/// The colour `x` px into the name (plus the animation's offset): the repeating gradient sampled there.
pub(super) fn color_at(colors: &RoleColors, x: f32) -> Rgba {
    let list = stops(colors);
    let position = x.rem_euclid(PERIOD) / PERIOD * (list.len() - 1) as f32;
    let index = (position.floor() as usize).min(list.len() - 2);
    let t = position - index as f32;
    let (from, to) = (list[index], list[index + 1]);
    Rgba {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: 1.,
    }
}

/// The offset of the sliding background `seconds` after the animation began.
pub(super) fn slide(seconds: f32) -> f32 {
    seconds.rem_euclid(ANIMATION_SECONDS) / ANIMATION_SECONDS * PERIOD
}

/// `text` in the gradient. It takes the font, size and weight of the place it sits in.
#[derive(IntoElement)]
pub(super) struct GradientName {
    text: SharedString,
    colors: RoleColors,
    animate: bool,
}

pub(super) fn name(text: impl Into<SharedString>, colors: RoleColors) -> GradientName {
    GradientName {
        text: text.into(),
        colors,
        animate: false,
    }
}

impl GradientName {
    /// The background slides (the pointer is on the name or its message); not with reduced motion.
    pub(super) fn animate(mut self, animate: bool) -> Self {
        self.animate = animate && !super::motion::reduced();
        self
    }
}

fn started() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// The sliding background's offset now, for text drawn outside `GradientName` (the inline engine's mentions).
pub(crate) fn offset_now() -> f32 {
    slide(started().elapsed().as_secs_f32())
}

/// One run per character, each in the gradient's colour at the character's centre (`offset` px of slide).
/// `plain` is the text shaped in one colour: colour does not change the layout, so its positions are the result's.
pub(crate) fn runs_for(
    text: &str,
    plain: &gpui::ShapedLine,
    base: &TextRun,
    colors: &RoleColors,
    offset: f32,
) -> Vec<TextRun> {
    text.char_indices()
        .map(|(at, ch)| {
            let end = at + ch.len_utf8();
            let middle =
                (f32::from(plain.x_for_index(at)) + f32::from(plain.x_for_index(end))) / 2.;
            TextRun {
                len: ch.len_utf8(),
                color: color_at(colors, middle - offset).into(),
                ..base.clone()
            }
        })
        .collect()
}

impl RenderOnce for GradientName {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let whole = style.to_run(self.text.len());
        let line = window.text_system().shape_line(
            self.text.clone(),
            size,
            std::slice::from_ref(&whole),
            None,
        );
        let offset = if self.animate {
            window.request_animation_frame();
            slide(started().elapsed().as_secs_f32())
        } else {
            0.
        };
        let runs: Vec<TextRun> = self
            .text
            .char_indices()
            .map(|(at, ch)| {
                let end = at + ch.len_utf8();
                let middle =
                    (f32::from(line.x_for_index(at)) + f32::from(line.x_for_index(end))) / 2.;
                TextRun {
                    len: ch.len_utf8(),
                    // The background slides to the right, so the colours move that way.
                    color: color_at(&self.colors, middle - offset).into(),
                    ..whole.clone()
                }
            })
            .collect();
        StyledText::new(self.text).with_runs(runs)
    }
}

/// A filled circle of `diameter` px whose fill is the gradient across its width (the role circle of module 545442
/// `R`, `roleCircle_7RsUnm`: 12px, `linear-gradient(to right, c1, c2, c1)` on the element's own box, and the dot
/// svg's circles, whose gradient is `objectBoundingBox`), at `opacity`. Anti-aliased by 4x4 coverage.
pub(super) fn circle(colors: &RoleColors, diameter: u32, opacity: f32) -> Arc<RenderImage> {
    type Key = (u32, Option<u32>, Option<u32>, u32, u32);
    thread_local! {
        static CACHE: RefCell<HashMap<Key, Arc<RenderImage>>> = RefCell::new(HashMap::new());
    }
    let key = (
        colors.primary,
        colors.secondary,
        colors.tertiary,
        diameter,
        opacity.to_bits(),
    );
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                Arc::new(RenderImage::new(vec![Frame::new(circle_pixels(
                    colors, diameter, opacity,
                ))]))
            })
            .clone()
    })
}

fn circle_pixels(colors: &RoleColors, diameter: u32, opacity: f32) -> image::RgbaImage {
    const SAMPLES: u32 = 4;
    let radius = diameter as f32 / 2.;
    let mut pixels = image::RgbaImage::new(diameter, diameter);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let mut inside = 0;
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32 - radius;
                let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32 - radius;
                if px * px + py * py <= radius * radius {
                    inside += 1;
                }
            }
        }
        let coverage = inside as f32 / (SAMPLES * SAMPLES) as f32;
        let fill = color_at(colors, (x as f32 + 0.5) / diameter as f32 * PERIOD);
        let channel = |value: f32| (value * 255. + 0.5) as u8;
        // BGRA, straight alpha: the layout the atlas takes (as `channel_status::gradient_pixels`).
        *pixel = image::Rgba([
            channel(fill.b),
            channel(fill.g),
            channel(fill.r),
            channel(coverage * opacity),
        ]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_is_clear_in_its_corners_and_runs_from_the_first_colour_to_the_second() {
        let pixels = circle_pixels(&two(), 12, 1.);
        assert_eq!(pixels.get_pixel(0, 0).0[3], 0);
        let left = pixels.get_pixel(1, 6).0;
        let middle = pixels.get_pixel(6, 6).0;
        // BGRA: red on the left, blue in the middle (the pair reflects at 50%).
        assert!(left[2] > 170 && left[0] < 90, "{left:?}");
        assert!(middle[0] > 200 && middle[2] < 80, "{middle:?}");
        assert_eq!(middle[3], 255);
    }

    fn two() -> RoleColors {
        RoleColors {
            primary: 0xff0000,
            secondary: Some(0x0000ff),
            tertiary: None,
        }
    }

    #[test]
    fn two_colours_run_there_and_back_over_one_hundred_pixels() {
        let colors = two();
        assert_eq!(color_at(&colors, 0.), rgb(0xff0000));
        let middle = color_at(&colors, 50.);
        assert!((middle.b - 1.).abs() < 1e-4 && middle.r.abs() < 1e-4);
        let quarter = color_at(&colors, 25.);
        assert!((quarter.r - 0.5).abs() < 1e-4 && (quarter.b - 0.5).abs() < 1e-4);
        // The background repeats.
        assert_eq!(color_at(&colors, 100.), color_at(&colors, 0.));
        assert_eq!(color_at(&colors, 125.), color_at(&colors, 25.));
    }

    #[test]
    fn three_colours_take_a_third_each() {
        let colors = RoleColors {
            tertiary: Some(0x00ff00),
            ..two()
        };
        let a = color_at(&colors, 100. / 3.);
        assert!((a.b - 1.).abs() < 1e-4);
        let b = color_at(&colors, 200. / 3.);
        assert!((b.g - 1.).abs() < 1e-4);
        assert_eq!(color_at(&colors, 99.99).r.round(), 1.);
    }

    #[test]
    fn the_slide_is_one_period_per_animation_cycle() {
        assert_eq!(slide(0.), 0.);
        assert!((slide(0.75) - 50.).abs() < 1e-3);
        assert!(slide(1.5) < 1e-3);
    }
}
