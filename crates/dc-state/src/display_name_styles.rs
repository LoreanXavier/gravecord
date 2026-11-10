//! Display name styles (the Nitro name fonts, colours and effects): the data on user and member objects, the font and
//! effect tables, the rule for which style applies, and the colour arithmetic that feeds the effect CSS.
//!
//! Follows modules 559949 (font ids), 508425 (effect ids), 73392 and 516064 (font classes),
//! 898985 (per-effect minimum contrast), 501803 (`$0`, the derived colours), 998304 (`lZ`, the contrast adjustment),
//! 609425 (`useDisplayNameStyles`) and 16371 (`UserNameWithEffects`, which turns the colours into CSS variables).

use serde_json::Value;

/// `display_name_styles` of a user or guild member object (501803 `mT`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayNameStyles {
    pub font_id: u32,
    pub effect_id: u32,
    /// `0xRRGGBB` each; the number of colours an effect reads is in [`Effect::color_count`].
    pub colors: Vec<u32>,
}

impl DisplayNameStyles {
    /// A user or member object's `display_name_styles`; null or a malformed object is no style.
    pub fn from_value(value: &Value) -> Option<Self> {
        if !value.is_object() {
            return None;
        }
        Some(Self {
            font_id: u32::try_from(value["font_id"].as_u64()?).ok()?,
            effect_id: u32::try_from(value["effect_id"].as_u64()?).ok()?,
            colors: value["colors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|color| u32::try_from(color.as_u64()?).ok())
                .collect(),
        })
    }

    pub fn font(&self) -> Font {
        Font::from_id(self.font_id)
    }

    pub fn effect(&self) -> Effect {
        Effect::from_id(self.effect_id)
    }
}

/// 609425: the member's own style wins over the user's. A disabled setting hides both (the caller passes `None`).
pub fn effective<'a>(
    member: Option<&'a DisplayNameStyles>,
    user: Option<&'a DisplayNameStyles>,
) -> Option<&'a DisplayNameStyles> {
    member.or(user)
}

/// `DisplayNameFont` (559949). Ids the table of 73392 does not map (Bangers, Bio Rhyme, Compagnon, Ribes and anything
/// unknown) carry no class, so those names draw in the normal font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    Default,
    CherryBomb,
    Chicle,
    MuseoModerno,
    NeoCastel,
    Pixelify,
    Sinistre,
    ZillaSlab,
    PlaypenSans,
    Orbitron,
    NewRocker,
    Kalam,
    Hexagon,
}

/// One face of a name font: the CSS family and weight of its `@font-face` (327545 and 952007 stylesheets), the
/// letter-spacing of its class in em, and the hash of the file on the official asset host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontFace {
    pub family: &'static str,
    pub weight: u16,
    pub letter_spacing_em: f32,
    pub hash: &'static str,
}

impl Font {
    pub const ALL: [Self; 12] = [
        Self::CherryBomb,
        Self::Chicle,
        Self::MuseoModerno,
        Self::NeoCastel,
        Self::Pixelify,
        Self::Sinistre,
        Self::ZillaSlab,
        Self::PlaypenSans,
        Self::Orbitron,
        Self::NewRocker,
        Self::Kalam,
        Self::Hexagon,
    ];

    pub fn from_id(id: u32) -> Self {
        match id {
            3 => Self::CherryBomb,
            4 => Self::Chicle,
            6 => Self::MuseoModerno,
            7 => Self::NeoCastel,
            8 => Self::Pixelify,
            10 => Self::Sinistre,
            12 => Self::ZillaSlab,
            13 => Self::PlaypenSans,
            14 => Self::Orbitron,
            15 => Self::NewRocker,
            16 => Self::Kalam,
            17 => Self::Hexagon,
            _ => Self::Default,
        }
    }

    /// The font id as the wire carries it.
    pub fn id(self) -> u32 {
        match self {
            Self::Default => 11,
            Self::CherryBomb => 3,
            Self::Chicle => 4,
            Self::MuseoModerno => 6,
            Self::NeoCastel => 7,
            Self::Pixelify => 8,
            Self::Sinistre => 10,
            Self::ZillaSlab => 12,
            Self::PlaypenSans => 13,
            Self::Orbitron => 14,
            Self::NewRocker => 15,
            Self::Kalam => 16,
            Self::Hexagon => 17,
        }
    }

    /// `None` for the normal font. The CSS is `.dnsFont .{class}.{class}{font-family: X, gg sans, ...; letter-spacing}`.
    pub fn face(self) -> Option<FontFace> {
        let (family, weight, spacing, hash) = match self {
            Self::Default => return None,
            Self::CherryBomb => ("Sakura", 400, 0.04, "33d4f12a85e1f736"),
            Self::Chicle => ("Jellybean", 400, 0., "e6f5f44abb520735"),
            Self::MuseoModerno => ("Modern", 500, 0.01, "c560709c3470bb66"),
            Self::NeoCastel => ("Medieval", 400, 0.02, "52b541f86401a5b6"),
            Self::Pixelify => ("8Bit", 400, 0.02, "69c735ca5c604de7"),
            Self::Sinistre => ("Vampyre", 400, 0.01, "8f20cb550d739cea"),
            Self::ZillaSlab => ("Tempo", 600, 0.03, "27b34a77ba5d693e"),
            Self::PlaypenSans => ("Monkey Bars", 700, 0., "45ef50dce38a931a"),
            Self::Orbitron => ("Mainframe", 700, 0.03, "4841dace333e9054"),
            Self::NewRocker => ("Headbang", 400, 0., "c00df4f25667809b"),
            Self::Kalam => ("Journal", 700, 0., "aa59ae54ce7e540a"),
            Self::Hexagon => ("Pricedown", 700, 0., "a7bafac436428e6c"),
        };
        Some(FontFace {
            family,
            weight,
            letter_spacing_em: spacing,
            hash,
        })
    }

    /// 16371: Cherry Bomb and Chicle names take `boldFontOpacity` (0.9 in message headers and DM titles) as the
    /// container's opacity when no effect is shown.
    pub fn takes_bold_opacity(self) -> bool {
        matches!(self, Self::CherryBomb | Self::Chicle)
    }
}

/// `DisplayNameEffect` (508425). The test ids and anything unknown draw as Solid, the class table's default (16371).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Solid,
    Gradient,
    Neon,
    Toon,
    Pop,
    Glow,
    Prism,
    Gummy,
}

impl Effect {
    pub fn from_id(id: u32) -> Self {
        match id {
            2 => Self::Gradient,
            3 => Self::Neon,
            4 => Self::Toon,
            5 => Self::Pop,
            6 => Self::Glow,
            7 => Self::Prism,
            8 => Self::Gummy,
            _ => Self::Solid,
        }
    }

    /// 898985 `minContrastRatio` against the name's background.
    pub fn min_contrast_ratio(self) -> f64 {
        match self {
            Self::Gradient | Self::Glow | Self::Prism => 2.5,
            _ => 3.,
        }
    }

    /// 501803 `as`: how many colours the editor asks for (and so the most the effect reads).
    pub fn color_count(self) -> usize {
        match self {
            Self::Gradient => 2,
            Self::Gummy => 4,
            Self::Prism => 5,
            _ => 1,
        }
    }
}

/// An sRGB colour with 0-255 channels, the working type of the contrast arithmetic (chroma-js in the source).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn from_u32(value: u32) -> Self {
        Self((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// `rgb2hsl`: hue in degrees (None for greys), saturation and lightness 0..1.
    fn hsl(self) -> (Option<f64>, f64, f64) {
        let (r, g, b) = (
            f64::from(self.0) / 255.,
            f64::from(self.1) / 255.,
            f64::from(self.2) / 255.,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.;
        if max == min {
            return (None, 0., l);
        }
        let s = if l < 0.5 {
            (max - min) / (max + min)
        } else {
            (max - min) / (2. - max - min)
        };
        let mut h = if max == r {
            (g - b) / (max - min)
        } else if max == g {
            2. + (b - r) / (max - min)
        } else {
            4. + (r - g) / (max - min)
        };
        h *= 60.;
        if h < 0. {
            h += 360.;
        }
        (Some(h), s, l)
    }

    /// `hsl2rgb`, rounded to whole channels as chroma-js does.
    fn from_hsl(h: Option<f64>, s: f64, l: f64) -> Self {
        let channel = |value: f64| (value * 255.).round().clamp(0., 255.) as u8;
        let Some(h) = h.filter(|_| s != 0.) else {
            let grey = channel(l);
            return Self(grey, grey, grey);
        };
        let t2 = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
        let t1 = 2. * l - t2;
        let h = h / 360.;
        let one = |offset: f64| {
            let mut t = h + offset;
            if t < 0. {
                t += 1.;
            }
            if t > 1. {
                t -= 1.;
            }
            if 6. * t < 1. {
                t1 + (t2 - t1) * 6. * t
            } else if 2. * t < 1. {
                t2
            } else if 3. * t < 2. {
                t1 + (t2 - t1) * (2. / 3. - t) * 6.
            } else {
                t1
            }
        };
        Self(
            channel(one(1. / 3.)),
            channel(one(0.)),
            channel(one(-1. / 3.)),
        )
    }

    fn with_lightness(self, lightness: f64) -> Self {
        let (h, s, _) = self.hsl();
        Self::from_hsl(h, s, lightness)
    }

    fn with_saturation(self, saturation: f64) -> Self {
        let (h, _, l) = self.hsl();
        Self::from_hsl(h, saturation, l)
    }

    fn lightness(self) -> f64 {
        self.hsl().2
    }

    /// chroma-js `luminance()`: WCAG relative luminance.
    pub fn luminance(self) -> f64 {
        let linear = |value: u8| {
            let v = f64::from(value) / 255.;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.0) + 0.7152 * linear(self.1) + 0.0722 * linear(self.2)
    }

    /// chroma-js `contrast`.
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

/// 998304 `lZ`: step the lightness of `foreground` (by 0.05, at most ten times) until it reaches `ratio` against
/// `background`. A dark background lightens, a light one darkens. `saturation_factor` below 1 is the account's
/// "desaturate colors" setting, applied to both colours first.
pub fn adjust_contrast(
    foreground: Rgb,
    background: Rgb,
    ratio: f64,
    saturation_factor: f64,
) -> Rgb {
    let (mut foreground, background) = if saturation_factor < 1. {
        (
            foreground.with_saturation(foreground.hsl().1 * saturation_factor),
            background.with_saturation(background.hsl().1 * saturation_factor),
        )
    } else {
        (foreground, background)
    };
    let dark_background = 0.5 >= background.luminance();
    let mut contrast = foreground.contrast(background);
    for _ in 0..10 {
        if contrast >= ratio {
            break;
        }
        let lightness = foreground.lightness();
        if dark_background {
            if lightness < 0.95 {
                foreground = foreground.with_lightness(lightness + 0.05);
            } else {
                break;
            }
        } else if lightness > 0.05 {
            foreground = foreground.with_lightness(lightness - 0.05);
        } else {
            break;
        }
        contrast = foreground.contrast(background);
    }
    foreground
}

/// The colours the effect CSS reads (the `--custom-display-name-styles-*` variables of 16371 and 501803 `$0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub main: Rgb,
    pub light1: Rgb,
    pub light2: Rgb,
    pub dark1: Rgb,
    pub dark2: Rgb,
    pub toon_stroke: Rgb,
    pub neon_stroke: Rgb,
}

impl Palette {
    pub fn of(main: Rgb) -> Self {
        let (_, saturation, lightness) = main.hsl();
        let stroke_saturation = (1.2 * saturation).min(1.);
        let neon_lightness = (lightness + 0.1).min(0.6);
        Self {
            main,
            light1: main.with_lightness((1.2 * lightness).min(1.)),
            light2: main.with_lightness((1.6 * lightness).min(1.)),
            dark1: main.with_lightness((0.6 * lightness).max(0.)),
            dark2: main.with_lightness((0.2 * lightness).max(0.)),
            toon_stroke: main.with_lightness((0.4 * lightness).max(0.12)),
            neon_stroke: main
                .with_saturation(stroke_saturation)
                .with_lightness(neon_lightness),
        }
    }

    /// The colour the Neon flicker keyframe tints to: `hsl(from main h calc(min(1, s) * (s * 1.1 + 10)) 85)` with `s`
    /// in percent (508425's stylesheet, `neon-flicker-animation`).
    pub fn neon_flash(self) -> Rgb {
        let (h, s, _) = self.main.hsl();
        let percent = s * 100.;
        let flashed = (percent.min(1.) * (percent * 1.1 + 10.)).clamp(0., 100.) / 100.;
        Rgb::from_hsl(h, flashed, 0.85)
    }
}

/// The contrast-adjusted colours of a style against a background (16371's `$` memo).
pub fn adjusted_colors(
    styles: &DisplayNameStyles,
    background: Rgb,
    saturation_factor: f64,
) -> Vec<Rgb> {
    let ratio = styles.effect().min_contrast_ratio();
    let background = if styles.effect() == Effect::Toon {
        Rgb(0x33, 0x33, 0x33)
    } else {
        background
    };
    styles
        .colors
        .iter()
        .map(|color| adjust_contrast(Rgb::from_u32(*color), background, ratio, saturation_factor))
        .collect()
}

/// 16371: the stops of the Gradient and Glow `linear-gradient(to bottom right, ...)`: one colour twice, else spread
/// from 10% to 90%.
pub fn gradient_stops(colors: &[Rgb]) -> Vec<(Rgb, f32)> {
    match colors {
        [] => Vec::new(),
        [only] => vec![(*only, 0.), (*only, 1.)],
        _ => colors
            .iter()
            .enumerate()
            .map(|(at, color)| (*color, 0.1 + 0.8 * at as f32 / (colors.len() - 1) as f32))
            .collect(),
    }
}

/// 16371: the stops of the Prism's repeating 90deg gradient: the colours, then the first again at 100%.
pub fn prism_stops(colors: &[Rgb]) -> Vec<(Rgb, f32)> {
    (0..=colors.len())
        .filter(|_| !colors.is_empty())
        .map(|at| (colors[at % colors.len()], at as f32 / colors.len() as f32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_wire_object() {
        let styles = DisplayNameStyles::from_value(
            &json!({"font_id": 15, "effect_id": 4, "colors": [16119285]}),
        )
        .unwrap();
        assert_eq!(styles.font(), Font::NewRocker);
        assert_eq!(styles.effect(), Effect::Toon);
        assert_eq!(styles.colors, vec![0xf5f5f5]);
        assert!(DisplayNameStyles::from_value(&Value::Null).is_none());
        assert!(DisplayNameStyles::from_value(&json!({"effect_id": 1})).is_none());
    }

    #[test]
    fn the_member_style_wins() {
        let user = DisplayNameStyles {
            font_id: 14,
            ..Default::default()
        };
        let member = DisplayNameStyles {
            font_id: 15,
            ..Default::default()
        };
        assert_eq!(effective(Some(&member), Some(&user)), Some(&member));
        assert_eq!(effective(None, Some(&user)), Some(&user));
        assert_eq!(effective(None, None), None);
    }

    #[test]
    fn fonts_follow_the_class_table() {
        assert!(Font::from_id(11).face().is_none());
        assert!(Font::from_id(1).face().is_none());
        assert!(Font::from_id(99).face().is_none());
        let face = Font::from_id(15).face().unwrap();
        assert_eq!((face.family, face.hash), ("Headbang", "c00df4f25667809b"));
        let face = Font::from_id(7).face().unwrap();
        assert_eq!((face.family, face.letter_spacing_em), ("Medieval", 0.02));
        for font in Font::ALL {
            assert_eq!(Font::from_id(font.id()), font);
            assert!(font.face().is_some());
        }
    }

    #[test]
    fn derived_colours_match_the_captured_profile() {
        // pt-member-game: main #f4f4f4 gave these variables in the real client.
        let palette = Palette::of(Rgb(0xf4, 0xf4, 0xf4));
        assert_eq!(palette.light1.hex(), "#ffffff");
        assert_eq!(palette.light2.hex(), "#ffffff");
        assert_eq!(palette.dark1.hex(), "#929292");
        assert_eq!(palette.dark2.hex(), "#313131");
        assert_eq!(palette.toon_stroke.hex(), "#626262");
        assert_eq!(palette.neon_stroke.hex(), "#999999");
        // pt-profile-art: main #f3f3f4.
        let palette = Palette::of(Rgb(0xf3, 0xf3, 0xf4));
        assert_eq!(palette.dark1.hex(), "#8d8d97");
        assert_eq!(palette.dark2.hex(), "#2f2f33");
        assert_eq!(palette.toon_stroke.hex(), "#5d5d66");
        assert_eq!(palette.neon_stroke.hex(), "#95959d");
    }

    #[test]
    fn contrast_lightens_on_dark_and_darkens_on_light() {
        let dark = Rgb(0x1e, 0x1f, 0x22);
        let adjusted = adjust_contrast(Rgb(0x20, 0x20, 0x80), dark, 3., 1.);
        assert!(adjusted.contrast(dark) >= 3.);
        let light = Rgb(0xff, 0xff, 0xff);
        let adjusted = adjust_contrast(Rgb(0xff, 0xff, 0x80), light, 3., 1.);
        assert!(adjusted.contrast(light) >= 3.);
        // Already readable: unchanged.
        assert_eq!(
            adjust_contrast(Rgb(255, 255, 255), dark, 3., 1.),
            Rgb(255, 255, 255)
        );
    }

    #[test]
    fn gradient_and_prism_stops() {
        let a = Rgb(1, 2, 3);
        let b = Rgb(4, 5, 6);
        assert_eq!(gradient_stops(&[a]), vec![(a, 0.), (a, 1.)]);
        let stops = gradient_stops(&[a, b]);
        assert!((stops[0].1 - 0.1).abs() < 1e-6 && (stops[1].1 - 0.9).abs() < 1e-6);
        let stops = prism_stops(&[a, b]);
        assert_eq!(
            stops.iter().map(|(color, _)| *color).collect::<Vec<_>>(),
            vec![a, b, a]
        );
        assert!((stops[2].1 - 1.).abs() < 1e-6);
        assert!(prism_stops(&[]).is_empty());
    }

    #[test]
    fn effects_and_colour_counts() {
        assert_eq!(Effect::from_id(1001), Effect::Solid);
        assert_eq!(Effect::from_id(6), Effect::Glow);
        assert_eq!(Effect::Prism.color_count(), 5);
        assert_eq!(Effect::Gradient.min_contrast_ratio(), 2.5);
        assert_eq!(Effect::Neon.min_contrast_ratio(), 3.);
    }
}
