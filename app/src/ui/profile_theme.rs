//! Profile themes, ported from the hooks that colour every profile surface:
//!
//! - module 101928 picks the two colours: none unless the profile may edit themes (`canEditThemes`, Nitro tier 2,
//!   module 527212), then `theme_colors` (guild profile first), each missing one taken from the avatar's palette
//!   (module 654107 `rh`, the first two colours; `PRIMARY_530` while it loads), and the theme the card is drawn in:
//!   the viewer's when "sync profile themes" is on, else `tM` of module 543699 (`darker` when the darkness `OK` of
//!   module 317097 is above one half, `light` otherwise).
//! - module 946356 lets a surface override that theme: the DM sidebar passes the viewer's own (`themeOverride`).
//! - module 991988 writes the variables: both colours, the overlay (`--profile-gradient-overlay`, `.6` black in a dark
//!   theme and `.6` white in the light one, `.8` when synced), the modal background (`v2` of module 543699), and the
//!   custom theme colours of module 345815 `Zk` (base and text colour per theme, mixed in at 50% and 25%).
//! - the stylesheet (`952007`) then mixes every themed token with those colours (`color-mix(in oklab, token 100%,
//!   colour amount)`) and overrides a few profile tokens under `.custom-user-profile-theme.theme-dark|light`.

use dc_tokens::Theme;
use gpui::{Rgba, rgba};
use serde_json::Value;

/// `OK` of module 317097: one less the weighted brightness of the 24-bit colour.
pub(super) fn darkness(rgb: u32) -> f32 {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32;
    1. - (0.299 * channel(16) + 0.587 * channel(8) + 0.114 * channel(0)) / 255.
}

/// `tM` of module 543699: is a profile in these colours drawn dark?
pub(super) fn dark_colours(primary: u32) -> bool {
    darkness(primary) > 0.5
}

/// `canUsePremiumProfileCustomization`: `isPremiumAtLeast(premiumType, TIER_2)`; tier 2 is the only tier above it.
pub(super) fn can_edit_themes(body: &Value) -> bool {
    body["premium_type"].as_u64() == Some(2)
}

/// The two colours of a profile (module 101928), or `None` when it is drawn unthemed. `palette` is the avatar's
/// palette (first colours), `None` while it loads, when `PRIMARY_530` stands in for both.
pub(super) fn profile_colours(
    body: &Value,
    palette: Option<[u32; 2]>,
    guild: Option<dc_state::Id>,
) -> Option<(u32, u32)> {
    if !can_edit_themes(body) {
        return None;
    }
    // `themeColors`: the guild member profile's when it has them for the active guild, else the user profile's.
    let listed = |profile: &Value| -> Option<(Option<u32>, Option<u32>)> {
        let list = profile["theme_colors"].as_array()?;
        let at = |index: usize| list.get(index).and_then(Value::as_u64).map(|v| v as u32);
        Some((at(0), at(1)))
    };
    let for_guild = guild.is_some_and(|g| {
        body["guild_member_profile"]["guild_id"].as_str() == Some(g.to_string().as_str())
    });
    let (first, second) = for_guild
        .then(|| listed(&body["guild_member_profile"]))
        .flatten()
        .filter(|(a, b)| a.is_some() || b.is_some())
        .or_else(|| listed(&body["user_profile"]))
        .unwrap_or((None, None));
    let fallback = palette.unwrap_or([PRIMARY_530; 2]);
    Some((first.unwrap_or(fallback[0]), second.unwrap_or(fallback[1])))
}

/// `PRIMARY_530` of the raw palette (dc-tokens `--primary-530`).
const PRIMARY_530: u32 = 0x41424a;

/// A profile's resolved theme: the token table it is drawn in and its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ProfileTheme {
    pub tokens: Theme,
    pub primary: Rgba,
    pub secondary: Rgba,
    /// `--profile-gradient-overlay-color`.
    pub overlay: Rgba,
    /// `--custom-theme-base-color` and `--custom-theme-text-color` for `tokens`.
    base: Rgba,
    text: Rgba,
    /// `--profile-gradient-modal-background-color`.
    pub modal_background: Rgba,
}

fn opaque(rgb: u32) -> Rgba {
    rgba((rgb << 8) | 0xff)
}

impl ProfileTheme {
    /// `override_theme` is the surface's `themeOverride` (the viewer's theme on the DM sidebar), `viewer` the client's
    /// theme, `sync` the "Sync profile themes" preference.
    pub(super) fn new(
        (primary, secondary): (u32, u32),
        override_theme: Option<Theme>,
        viewer: Theme,
        sync: bool,
    ) -> Self {
        let own = if sync {
            viewer
        } else if dark_colours(primary) {
            Theme::Darker
        } else {
            Theme::Light
        };
        let tokens = override_theme.unwrap_or(own);
        let dark = tokens != Theme::Light;
        // `PROFILE_GRADIENT_OVERLAY(_SYNCED_WITH_USER_THEME)` resolved in the theme: black or white at .6 or .8.
        let synced = sync;
        let alpha = if synced { 0.8 } else { 0.6 };
        let overlay = if dark {
            Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: alpha,
            }
        } else {
            Rgba {
                r: 1.,
                g: 1.,
                b: 1.,
                a: alpha,
            }
        };
        let (base, text) = custom_theme_colours(primary, secondary, dark);
        Self {
            tokens,
            primary: opaque(primary),
            secondary: opaque(secondary),
            overlay,
            base,
            text,
            modal_background: opaque(modal_background(
                primary,
                secondary,
                if sync { Some(dark) } else { None },
            )),
        }
    }

    pub(super) fn light(&self) -> bool {
        self.tokens == Theme::Light
    }

    /// A token as the themed card resolves it: the profile overrides of the stylesheet first, then the theme's own
    /// value mixed with the custom theme colour (base tokens at 50%, text tokens at 25%) when the theme mixes it.
    pub(super) fn token(&self, name: &str) -> Rgba {
        if let Some(value) = self.profile_override(name) {
            return value;
        }
        let own = if self.tokens.has(name) {
            rgba(self.tokens.color(name))
        } else {
            rgba(Theme::Darker.color(name))
        };
        use super::settings::theme_mix_tokens as lists;
        let (base, text) = if self.light() {
            (lists::BASE_LIGHT, lists::TEXT_LIGHT)
        } else {
            (lists::BASE_DARK, lists::TEXT_DARK)
        };
        if base.binary_search(&name).is_ok() {
            mix_oklab(own, self.base, BASE_AMOUNT)
        } else if text.binary_search(&name).is_ok() {
            mix_oklab(own, self.text, TEXT_AMOUNT)
        } else {
            own
        }
    }

    /// `.custom-user-profile-theme.theme-dark|light` and `:root .custom-user-profile-theme.theme-dark|light`.
    fn profile_override(&self, name: &str) -> Option<Rgba> {
        let light = self.light();
        let tinted = |token: &str, alpha: f32| {
            let mut value = rgba(self.tokens.color(token));
            value.a = alpha;
            value
        };
        Some(match (name, light) {
            ("--text-default", false) => tinted("--opacity-white-96", 0.960_784_3),
            ("--text-default", true) => tinted("--opacity-black-96", 0.960_784_3),
            ("--text-muted", false) => tinted("--opacity-white-76", 0.760_784_3),
            ("--text-muted", true) => tinted("--opacity-black-76", 0.760_784_3),
            ("--text-subtle", false) => tinted("--opacity-white-88", 0.878_431_4),
            ("--text-subtle", true) => tinted("--opacity-black-88", 0.878_431_4),
            ("--user-profile-overlay-background", false) => tinted("--opacity-16", 0.160_784_3),
            ("--user-profile-overlay-background", true) => {
                tinted("--opacity-white-24", 0.239_215_7)
            }
            ("--user-profile-overlay-background-hover", false) => tinted("--opacity-20", 0.2),
            ("--user-profile-overlay-background-hover", true) => {
                tinted("--opacity-white-36", 0.360_784_3)
            }
            ("--user-profile-border", false) => tinted("--opacity-white-8", 0.078_431_4),
            ("--user-profile-border", true) => tinted("--opacity-black-8", 0.078_431_4),
            ("--user-profile-background-hover", false) => tinted("--opacity-white-4", 0.039_215_7),
            ("--user-profile-background-hover", true) => tinted("--opacity-black-8", 0.078_431_4),
            ("--user-profile-note-background-focus", false) => tinted("--black", 0.3),
            ("--user-profile-note-background-focus", true) => tinted("--white", 0.3),
            ("--control-secondary-background-default", false) => {
                rgba(self.tokens.color("--opacity-white-8"))
            }
            ("--control-secondary-background-hover", false) => {
                rgba(self.tokens.color("--opacity-white-16"))
            }
            ("--control-secondary-background-active", false) => {
                rgba(self.tokens.color("--opacity-white-12"))
            }
            ("--control-secondary-background-default", true) => {
                rgba(self.tokens.color("--opacity-white-72"))
            }
            ("--control-secondary-background-hover", true) => {
                rgba(self.tokens.color("--opacity-white-92"))
            }
            ("--control-secondary-background-active", true) => {
                rgba(self.tokens.color("--opacity-white-84"))
            }
            _ => return None,
        })
    }
}

/// `--custom-theme-base-color-amount` and `--custom-theme-text-color-amount` that module 991988 passes to `Zk`.
const BASE_AMOUNT: f32 = 50.;
const TEXT_AMOUNT: f32 = 25.;

/// chroma.js's view of a colour in HSL: hue `None` for a grey (chroma's `NaN`).
#[derive(Clone, Copy, Debug)]
struct Hsl {
    h: Option<f32>,
    s: f32,
    l: f32,
}

fn to_hsl(colour: Rgba) -> Hsl {
    let (r, g, b) = (colour.r, colour.g, colour.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    if max == min {
        return Hsl { h: None, s: 0., l };
    }
    let delta = max - min;
    let s = if l < 0.5 {
        delta / (max + min)
    } else {
        delta / (2. - max - min)
    };
    let h = if r == max {
        (g - b) / delta
    } else if g == max {
        2. + (b - r) / delta
    } else {
        4. + (r - g) / delta
    };
    let h = (h * 60.).rem_euclid(360.);
    Hsl { h: Some(h), s, l }
}

fn from_hsl(hsl: Hsl) -> Rgba {
    let Some(h) = hsl.h.filter(|_| hsl.s != 0.) else {
        return Rgba {
            r: hsl.l,
            g: hsl.l,
            b: hsl.l,
            a: 1.,
        };
    };
    let t2 = if hsl.l < 0.5 {
        hsl.l * (1. + hsl.s)
    } else {
        hsl.l + hsl.s - hsl.l * hsl.s
    };
    let t1 = 2. * hsl.l - t2;
    let h = h / 360.;
    let channel = |offset: f32| {
        let t3 = (h + offset).rem_euclid(1.);
        if 6. * t3 < 1. {
            t1 + (t2 - t1) * 6. * t3
        } else if 2. * t3 < 1. {
            t2
        } else if 3. * t3 < 2. {
            t1 + (t2 - t1) * (2. / 3. - t3) * 6.
        } else {
            t1
        }
    };
    // chroma rounds each channel to a byte on the way out.
    let byte = |value: f32| (value * 255.).round().clamp(0., 255.) / 255.;
    Rgba {
        r: byte(channel(1. / 3.)),
        g: byte(channel(0.)),
        b: byte(channel(-1. / 3.)),
        a: 1.,
    }
}

/// chroma.js `luminance()`: WCAG relative luminance.
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

/// Module 345815 `i`: the base and text colour of a theme from its two colours, for a dark or a light theme.
fn custom_theme_colours(primary: u32, secondary: u32, dark: bool) -> (Rgba, Rgba) {
    let (first, second) = (opaque(primary), opaque(secondary));
    let (lighter, mut darker) = if luminance(first) > luminance(second) {
        (first, second)
    } else {
        (second, first)
    };
    // A darker colour without a hue (0 or NaN) gives way to the lighter one.
    if to_hsl(darker).h.is_none_or(|h| h == 0.) {
        darker = lighter;
    }
    let with = |colour: Rgba, l: f32| {
        from_hsl(Hsl {
            s: 1.,
            l,
            ..to_hsl(colour)
        })
    };
    let source = if dark { lighter } else { darker };
    let saturated = Hsl {
        s: 1.,
        ..to_hsl(source)
    };
    let blue = saturated.h.is_some_and(|h| (200. ..=300.).contains(&h));
    let text_lightness = match (blue, dark) {
        (true, true) => 0.8,
        (true, false) => 0.25,
        (false, true) => 0.95,
        (false, false) => 0.15,
    };
    let text = from_hsl(Hsl {
        l: text_lightness,
        ..saturated
    });
    let base = if dark {
        with(darker, 0.05)
    } else {
        with(lighter, 0.9)
    };
    (base, text)
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

/// `color-mix(in oklab, own 100%, with amount%)`: the percentages are scaled to share 100, `with` getting
/// `amount / (100 + amount)`; the alpha is interpolated the same way.
pub(super) fn mix_oklab(own: Rgba, with: Rgba, amount_percent: f32) -> Rgba {
    let weight = amount_percent / (100. + amount_percent);
    let (a, b) = (to_oklab(own), to_oklab(with));
    from_oklab(
        [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * weight),
        own.a + (with.a - own.a) * weight,
    )
}

/// chroma.js Lab (D65, the classic constants).
fn to_lab(colour: Rgba) -> [f64; 3] {
    let linear = |c: f32| {
        let c = f64::from(c);
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(colour.r), linear(colour.g), linear(colour.b));
    let t0 = 4. / 29.;
    let t1 = 6. / 29.;
    let t2 = 3. * t1 * t1;
    let t3 = t1 * t1 * t1;
    let f = |t: f64| if t > t3 { t.cbrt() } else { t / t2 + t0 };
    let x = f((0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47);
    let y = f(0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b);
    let z = f((0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83);
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}

fn from_lab(lab: [f64; 3]) -> Rgba {
    let t0 = 4. / 29.;
    let t1 = 6. / 29.;
    let t2 = 3. * t1 * t1;
    let finv = |t: f64| if t > t1 { t * t * t } else { t2 * (t - t0) };
    let y = (lab[0] + 16.) / 116.;
    let x = y + lab[1] / 500.;
    let z = y - lab[2] / 200.;
    let (x, y, z) = (0.950_47 * finv(x), finv(y), 1.088_83 * finv(z));
    let gamma = |c: f64| {
        let c = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        };
        ((c * 255.).round().clamp(0., 255.) / 255.) as f32
    };
    Rgba {
        r: gamma(3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z),
        g: gamma(-0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z),
        b: gamma(0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z),
        a: 1.,
    }
}

fn rgb_u32(colour: Rgba) -> u32 {
    let byte = |c: f32| (c * 255.).round().clamp(0., 255.) as u32;
    (byte(colour.r) << 16) | (byte(colour.g) << 8) | byte(colour.b)
}

/// `v2` of module 543699: the two colours mixed half and half in Lab, the HSL lightness (rounded to hundredths)
/// clamped to 0..0.1 for a dark theme and 0.8..1 for a light one. `dark` is the theme passed in (synced), else the
/// colours' own `tM`.
pub(super) fn modal_background(primary: u32, secondary: u32, dark: Option<bool>) -> u32 {
    let dark = dark.unwrap_or_else(|| dark_colours(primary));
    let (a, b) = (to_lab(opaque(primary)), to_lab(opaque(secondary)));
    let mixed = from_lab([0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * 0.5));
    let hsl = to_hsl(mixed);
    let rounded = (100. * hsl.l).round() / 100.;
    let l = if dark {
        rounded.clamp(0., 0.1)
    } else {
        rounded.clamp(0.8, 1.)
    };
    rgb_u32(from_hsl(Hsl { l, ..hsl }))
}

/// The first two colours of an avatar's palette, as module 654107 keeps them (one colour stands in for both).
pub(super) fn palette_pair(palette: &[[u8; 3]]) -> Option<[u32; 2]> {
    let pack =
        |rgb: &[u8; 3]| (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
    let first = palette.first()?;
    Some([pack(first), pack(palette.get(1).unwrap_or(first))])
}

/// Avatar palettes by image key and byte count.
type PaletteCache = std::collections::HashMap<(String, usize), Vec<[u8; 3]>>;

thread_local! {
    /// Avatar palettes by image key and byte count: decoding and quantizing an avatar every frame is too slow.
    static PALETTES: std::cell::RefCell<PaletteCache> = std::cell::RefCell::default();
}

/// The palette of a decoded avatar image (`crate::palette::avatar_palette`), cached.
pub(super) fn cached_palette(key: &str, image: &gpui::Image) -> Vec<[u8; 3]> {
    let id = (key.to_owned(), image.bytes().len());
    if let Some(found) = PALETTES.with(|cache| cache.borrow().get(&id).cloned()) {
        return found;
    }
    let palette = image::load_from_memory(image.bytes())
        .map(|decoded| crate::palette::avatar_palette(decoded.to_rgba8().as_raw()))
        .unwrap_or_else(|_| vec![[0, 0, 0]]);
    PALETTES.with(|cache| cache.borrow_mut().insert(id, palette.clone()));
    palette
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(colour: Rgba) -> u32 {
        rgb_u32(colour)
    }

    #[test]
    fn darkness_decides_dark_or_light_as_ok_does() {
        assert!(dark_colours(0x000000));
        assert!(dark_colours(0x252539));
        assert!(!dark_colours(0xffffff));
        // 1 - (.299*200 + .587*200 + .114*200)/255 = 0.2157: light.
        assert!(!dark_colours(0xc8c8c8));
    }

    #[test]
    fn colours_need_tier_two_and_fall_back_to_the_avatar_palette() {
        let plain = json!({"premium_type": 0, "user_profile": {"theme_colors": [1, 2]}});
        assert_eq!(profile_colours(&plain, Some([3, 4]), None), None);
        let basic = json!({"premium_type": 3, "user_profile": {"theme_colors": [1, 2]}});
        assert_eq!(profile_colours(&basic, Some([3, 4]), None), None);
        let themed = json!({"premium_type": 2, "user_profile": {"theme_colors": [1, 2]}});
        assert_eq!(profile_colours(&themed, Some([3, 4]), None), Some((1, 2)));
        let unset = json!({"premium_type": 2, "user_profile": {"theme_colors": null}});
        assert_eq!(profile_colours(&unset, Some([3, 4]), None), Some((3, 4)));
        assert_eq!(
            profile_colours(&unset, None, None),
            Some((PRIMARY_530, PRIMARY_530))
        );
        let guild = json!({"premium_type": 2, "user_profile": {"theme_colors": [1, 2]},
            "guild_member_profile": {"guild_id": "10", "theme_colors": [5, 6]}});
        assert_eq!(profile_colours(&guild, None, None), Some((1, 2)));
        assert_eq!(profile_colours(&guild, None, Some(10)), Some((5, 6)));
    }

    #[test]
    fn black_theme_matches_the_captured_variables() {
        // design/dom/dm/loreanxavier-darker-cozy.json: both colours black.
        let (base, text) = custom_theme_colours(0, 0, true);
        assert_eq!(
            hex(base),
            0x0d0d0d,
            "--custom-theme-base-color-dark: rgb(13,13,13)"
        );
        assert_eq!(
            hex(text),
            0xf2f2f2,
            "--custom-theme-text-color-dark: rgb(242,242,242)"
        );
        let (base, text) = custom_theme_colours(0, 0, false);
        assert_eq!(
            hex(base),
            0xe6e6e6,
            "--custom-theme-base-color-light: rgb(230,230,230)"
        );
        assert_eq!(
            hex(text),
            0x262626,
            "--custom-theme-text-color-light: rgb(38,38,38)"
        );
        assert_eq!(modal_background(0, 0, None), 0, "hsla(0, 0%, 0%, 1)");
    }

    #[test]
    fn a_hued_theme_saturates_its_text_colour() {
        // Blue lighter colour (hue 240) in a dark theme: text at lightness .8, saturation 1.
        let (_, text) = custom_theme_colours(0x3030c0, 0x101010, true);
        let hsl = to_hsl(text);
        assert!((hsl.l - 0.8).abs() < 0.01);
        assert!((hsl.h.unwrap() - 240.).abs() < 1.);
    }

    #[test]
    fn the_sidebar_wears_the_viewers_theme_and_the_popout_its_own() {
        let sidebar =
            ProfileTheme::new((0xffffff, 0xeeeeee), Some(Theme::Dark), Theme::Dark, false);
        assert_eq!(sidebar.tokens, Theme::Dark);
        assert_eq!(sidebar.overlay.a, 0.6);
        assert_eq!(sidebar.overlay.r, 0.);
        let popout = ProfileTheme::new((0xffffff, 0xeeeeee), None, Theme::Dark, false);
        assert_eq!(popout.tokens, Theme::Light);
        assert_eq!(popout.overlay.r, 1.);
        let dark = ProfileTheme::new((0x101010, 0x202020), None, Theme::Dark, false);
        assert_eq!(dark.tokens, Theme::Darker);
        let synced = ProfileTheme::new((0xffffff, 0xeeeeee), None, Theme::Midnight, true);
        assert_eq!(synced.tokens, Theme::Midnight);
        assert_eq!(synced.overlay.a, 0.8);
    }

    #[test]
    fn themed_text_is_mixed_as_the_capture_shows() {
        // The capture's username: oklab(0.982692 ...) for --text-strong under the black theme on darker.
        let theme = ProfileTheme::new((0, 0), Some(Theme::Darker), Theme::Darker, false);
        let strong = to_oklab(theme.token("--text-strong"));
        assert!((strong[0] - 0.982_692).abs() < 0.002, "{strong:?}");
        // --text-default is overridden to white at .96.
        let default = theme.token("--text-default");
        assert_eq!((default.r, default.a), (1., 0.960_784_3));
    }
}
