//! Official messages of verified servers (`IS_GUILD_OFFICIAL`, message flag 1 << 19) and how the Accessibility
//! setting "Official Messages" (`AccessibilityStore.officialMessageStyle`, module 775602) shows them.
//!
//! A server shows them when it has the `VERIFIED` feature and its guild experiment `2026-03_guild_official_messages`
//! (module 472282) is in treatment 1 (`GP`/`bW` of module 521427). The channel chat then styles them with the server's
//! `official_message_color`, `3447003` when unset (`ChannelChat`, module 699344; `aj` of module 381941), through
//! `useGuildOfficialMessageStyle` (module 537174):
//!
//! - `default`: a 16% tint (`--custom-guild-official-message-alpha`), the dither layer, and the text colour made
//!   readable over the tinted background (`Fy` of module 521427);
//! - `no_text_color`: the tint and the dither, the text in `--text-default`;
//! - `no_gradient`: no tint and no dither (alpha and dither opacity 0), the readable text colour over the plain
//!   background;
//! - `hidden`: the message is not styled at all (`o` of 537174 drops `guildOfficialMessage_SY_P2T`).
//!
//! In stylesheet 524434 the tint is the hover and selected background of the message
//! (`hsl(var(--custom-guild-official-message-hsl)/var(--custom-guild-official-message-alpha))`), the text colour is the
//! content's (`.guildOfficialMessageContent` and its headings and `small`), and the dither is the list item's `:after`:
//! the colour masked by `/assets/13460a65f1622591.png`, repeated down the row. That image is not in the snapshot
//! (`design/snapshot/assets`), so the dither layer is not drawn.

use dc_state::Id;
use serde_json::Value;
use std::collections::HashMap;

/// `pr7.IS_GUILD_OFFICIAL`.
pub(super) const IS_GUILD_OFFICIAL: u64 = 1 << 19;
/// `aj` of module 381941: the colour of a server that chose none.
const DEFAULT_COLOR: u32 = 3_447_003;
const EXPERIMENT: &str = "2026-03_guild_official_messages";
/// `ExperimentFlags.UseAsEligibility`.
const USE_AS_ELIGIBILITY: u64 = 8;
/// The row key of "Official Messages".
pub(super) const STYLE_KEY: &str = "official_message_style";
/// `Tr.Text` of module 498596.
const TEXT_RATIO: f64 = 4.5;
/// `.16*!!o` of module 537174.
const TINT_ALPHA: f32 = 0.16;

/// `officialMessageStyle`, in the order the select lists it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Style {
    Default,
    NoTextColor,
    NoGradient,
    Hidden,
}

impl Style {
    pub(super) fn from_index(index: usize) -> Self {
        match index {
            1 => Self::NoTextColor,
            2 => Self::NoGradient,
            3 => Self::Hidden,
            _ => Self::Default,
        }
    }
}

/// What a server's official message is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    /// The hover and selected background (`hsl(... / alpha)`), transparent without the tint.
    pub hover: gpui::Rgba,
    /// The content's colour; `None` keeps `--text-default`.
    pub text: Option<gpui::Rgba>,
}

#[derive(Default)]
struct GuildOfficial {
    color: Option<u32>,
    enabled: bool,
}

/// Each server's official message colour and experiment, from READY and the guild events.
#[derive(Default)]
pub(crate) struct OfficialMessages {
    guilds: HashMap<Id, GuildOfficial>,
}

fn id_of(value: &Value) -> Option<Id> {
    value.as_str().and_then(|id| id.parse().ok())
}

/// The guild experiment's treatment from the guild's own `experiments.assignments`
/// (`[hashedName, variantId, flags, revision]`).
fn experiment_enabled(guild: &Value) -> bool {
    let hash = u64::from(dc_state::experiments::hash(EXPERIMENT));
    guild["experiments"]["assignments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .find(|entry| entry.first().and_then(Value::as_u64) == Some(hash))
        .is_some_and(|entry| {
            let flags = entry.get(2).and_then(Value::as_u64).unwrap_or(0);
            flags & USE_AS_ELIGIBILITY == 0 && entry.get(1).and_then(Value::as_i64) == Some(1)
        })
}

/// `official_message_color`, in READY's `properties` or on the guild itself.
fn color_of(guild: &Value) -> Option<Option<u32>> {
    let field = if guild["properties"].is_object() {
        guild["properties"].get("official_message_color")
    } else {
        guild.get("official_message_color")
    }?;
    Some(field.as_u64().and_then(|color| u32::try_from(color).ok()))
}

impl OfficialMessages {
    pub(crate) fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                self.guilds.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    self.guild_created(guild);
                }
            }
            "GUILD_CREATE" => self.guild_created(data),
            "GUILD_UPDATE" => {
                if let Some(guild) = id_of(&data["id"]) {
                    let entry = self.guilds.entry(guild).or_default();
                    if let Some(color) = color_of(data) {
                        entry.color = color;
                    }
                    if data["experiments"].is_object() {
                        entry.enabled = experiment_enabled(data);
                    }
                }
            }
            "GUILD_DELETE" if data["unavailable"] != true => {
                if let Some(guild) = id_of(&data["id"]) {
                    self.guilds.remove(&guild);
                }
            }
            _ => {}
        }
    }

    fn guild_created(&mut self, guild: &Value) {
        let Some(id) = id_of(&guild["id"]) else {
            return;
        };
        self.guilds.insert(
            id,
            GuildOfficial {
                color: color_of(guild).flatten(),
                enabled: experiment_enabled(guild),
            },
        );
    }

    /// `GP`: the server is verified and in the experiment. The colour when it is.
    pub(super) fn color_for(&self, guild: Id, verified: bool) -> Option<u32> {
        let entry = self.guilds.get(&guild)?;
        (verified && entry.enabled).then_some(entry.color.unwrap_or(DEFAULT_COLOR))
    }
}

impl super::Root {
    /// How a message of the open channel is drawn as an official one: the flag, a verified server in the experiment
    /// (`te.bW(guild_id, "ChatMessage")`), and a style other than `hidden` (`guildOfficialMessage_SY_P2T`).
    pub(super) fn official_look(&self, message: &dc_state::Message) -> Option<Look> {
        if message.flags & IS_GUILD_OFFICIAL == 0 {
            return None;
        }
        let guild = self.current_guild()?;
        let verified = guild.features.iter().any(|feature| feature == "VERIFIED");
        let color = self.official_messages.color_for(guild.id, verified)?;
        let style = Style::from_index(self.pref_index(STYLE_KEY, 0));
        let saturation = super::saturation();
        let text_saturation = if self.pref_on("desaturate_custom_colors", false) {
            saturation
        } else {
            1.
        };
        look(
            color,
            style,
            super::color("--background-base-lower"),
            saturation,
            text_saturation,
        )
    }
}

/// `useGuildOfficialMessageStyle` (module 537174 `A`) for one colour: `None` for `hidden`. `background` is
/// `BACKGROUND_BASE_LOWER`; `saturation` is `--saturation-factor`, `text_saturation` the factor `Fy` gets (the
/// saturation when "Apply saturation to custom colors" is on, else 1).
pub(super) fn look(
    color: u32,
    style: Style,
    background: gpui::Rgba,
    saturation: f32,
    text_saturation: f32,
) -> Option<Look> {
    if style == Style::Hidden {
        return None;
    }
    let tinted = style != Style::NoGradient;
    let alpha = if tinted { TINT_ALPHA } else { 0. };
    // `BK(e, !0)`: `h calc(var(--saturation-factor, 1) * s%) l%`.
    let (h, s, l) = tf(color);
    let hover: gpui::Rgba = gpui::Hsla {
        h: h as f32 / 360.,
        s: s as f32 * saturation,
        l: l as f32,
        a: alpha,
    }
    .into();
    let text = (style != Style::NoTextColor).then(|| {
        let bg = [
            f64::from(background.r) * 255.,
            f64::from(background.g) * 255.,
            f64::from(background.b) * 255.,
        ];
        let [r, g, b] = readable(color, bg, f64::from(text_saturation), f64::from(alpha));
        gpui::Rgba {
            r: r as f32 / 255.,
            g: g as f32 / 255.,
            b: b as f32 / 255.,
            a: 1.,
        }
    });
    Some(Look { hover, text })
}

/// `tf` of module 317097: whole-degree hue, saturation and lightness to three places.
fn tf(color: u32) -> (f64, f64, f64) {
    let r = f64::from((color >> 16) & 255) / 255.;
    let g = f64::from((color >> 8) & 255) / 255.;
    let b = f64::from(color & 255) / 255.;
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let delta = max - min;
    let mut h = if delta == 0. {
        0.
    } else if max == r {
        ((g - b) / delta) % 6.
    } else if max == g {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    };
    h = (60. * h).round();
    if h < 0. {
        h += 360.;
    }
    let l = (max + min) / 2.;
    let s = if delta == 0. {
        0.
    } else {
        delta / (1. - (2. * l - 1.).abs())
    };
    (h, round3(s), round3(l))
}

fn round3(value: f64) -> f64 {
    (value * 1000.).round() / 1000.
}

/// chroma-js `luminance`.
pub(super) fn luminance([r, g, b]: [f64; 3]) -> f64 {
    let channel = |x: f64| {
        let x = x / 255.;
        if x <= 0.03928 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// chroma-js `contrast`.
pub(super) fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// chroma-js `rgb2hsl`: hue in degrees, saturation and lightness 0 to 1.
pub(super) fn to_hsl([r, g, b]: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (r / 255., g / 255., b / 255.);
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let l = (max + min) / 2.;
    if max == min {
        return [f64::NAN, 0., l];
    }
    let s = if l < 0.5 {
        (max - min) / (max + min)
    } else {
        (max - min) / (2. - max - min)
    };
    let mut h = if r == max {
        (g - b) / (max - min)
    } else if g == max {
        2. + (b - r) / (max - min)
    } else {
        4. + (r - g) / (max - min)
    };
    h *= 60.;
    if h < 0. {
        h += 360.;
    }
    [h, s, l]
}

/// chroma-js `hsl2rgb` (no rounding).
pub(super) fn from_hsl([h, s, l]: [f64; 3]) -> [f64; 3] {
    if s == 0. || h.is_nan() {
        return [l * 255.; 3];
    }
    let t3 = [h / 360. + 1. / 3., h / 360., h / 360. - 1. / 3.];
    let t2 = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let t1 = 2. * l - t2;
    t3.map(|mut t| {
        if t < 0. {
            t += 1.;
        }
        if t > 1. {
            t -= 1.;
        }
        let c = if 6. * t < 1. {
            t1 + (t2 - t1) * 6. * t
        } else if 2. * t < 1. {
            t2
        } else if 3. * t < 2. {
            t1 + (t2 - t1) * (2. / 3. - t) * 6.
        } else {
            t1
        };
        (c * 255.).clamp(0., 255.)
    })
}

/// `Fy` of module 521427 with `lZ` of module 998304: the colour mixed into the background at the tint's alpha (rgb);
/// of that and the plain background, the one the colour stands out from least; then the colour stepped 5% lighter (over
/// a dark background) or darker until it reaches the text contrast of 4.5, ten steps at most.
fn readable(color: u32, background: [f64; 3], saturation: f64, alpha: f64) -> [u8; 3] {
    let base = [
        f64::from((color >> 16) & 255),
        f64::from((color >> 8) & 255),
        f64::from(color & 255),
    ];
    let mixed = [0, 1, 2].map(|i| background[i] + alpha * (base[i] - background[i]));
    let mut against = if contrast(base, mixed) < contrast(base, background) {
        mixed
    } else {
        background
    };
    let mut text = base;
    if saturation < 1. {
        let mut hsl = to_hsl(text);
        hsl[1] *= saturation;
        text = from_hsl(hsl);
        let mut hsl = to_hsl(against);
        hsl[1] *= saturation;
        against = from_hsl(hsl);
    }
    let dark = luminance(against) <= 0.5;
    let mut ratio = contrast(text, against);
    for _ in 0..10 {
        if ratio >= TEXT_RATIO {
            break;
        }
        let mut hsl = to_hsl(text);
        if dark {
            if hsl[2] >= 0.95 {
                break;
            }
            hsl[2] += 0.05;
        } else {
            if hsl[2] <= 0.05 {
                break;
            }
            hsl[2] -= 0.05;
        }
        text = from_hsl(hsl);
        ratio = contrast(text, against);
    }
    text.map(|channel| channel.round().clamp(0., 255.) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ready(assignment: Value, color: Value) -> Value {
        json!({"guilds": [{"id": "5", "properties": {"official_message_color": color},
            "experiments": {"assignments": [assignment]}}]})
    }

    #[test]
    fn a_verified_server_in_the_experiment_styles_with_its_colour() {
        let hash = dc_state::experiments::hash(EXPERIMENT);
        let mut store = OfficialMessages::default();
        store.dispatch("READY", &ready(json!([hash, 1, 0, 1]), json!(0xff0000)));
        assert_eq!(store.color_for(5, true), Some(0xff0000));
        assert_eq!(store.color_for(5, false), None, "only verified servers");
        // No colour chosen: the default blue.
        store.dispatch("READY", &ready(json!([hash, 1, 0, 1]), json!(null)));
        assert_eq!(store.color_for(5, true), Some(DEFAULT_COLOR));
        // The control group, or an eligibility-only assignment, keeps the default config (off).
        store.dispatch("READY", &ready(json!([hash, 0, 0, 1]), json!(1)));
        assert_eq!(store.color_for(5, true), None);
        store.dispatch("READY", &ready(json!([hash, 1, 8, 1]), json!(1)));
        assert_eq!(store.color_for(5, true), None);
        store.dispatch("READY", &ready(json!([hash, 1, 0, 1]), json!(1)));
        store.dispatch(
            "GUILD_UPDATE",
            &json!({"id": "5", "official_message_color": 0x00ff00}),
        );
        assert_eq!(store.color_for(5, true), Some(0x00ff00));
        store.dispatch("GUILD_DELETE", &json!({"id": "5"}));
        assert_eq!(store.color_for(5, true), None);
    }

    #[test]
    fn each_style_changes_what_it_names() {
        let dark = gpui::Rgba {
            r: 0x32 as f32 / 255.,
            g: 0x33 as f32 / 255.,
            b: 0x38 as f32 / 255.,
            a: 1.,
        };
        let default = look(DEFAULT_COLOR, Style::Default, dark, 1., 1.).unwrap();
        assert!((default.hover.a - 0.16).abs() < 1e-6);
        let text = default.text.unwrap();
        // The blue is made light enough to read over the tinted dark background.
        let rgb = [text.r, text.g, text.b].map(|c| f64::from(c) * 255.);
        assert!(contrast(rgb, [0x32 as f64, 0x33 as f64, 0x38 as f64]) >= 4.0);
        let no_text = look(DEFAULT_COLOR, Style::NoTextColor, dark, 1., 1.).unwrap();
        assert_eq!(no_text.text, None);
        assert_eq!(no_text.hover, default.hover);
        let no_tint = look(DEFAULT_COLOR, Style::NoGradient, dark, 1., 1.).unwrap();
        assert_eq!(no_tint.hover.a, 0.);
        assert!(no_tint.text.is_some());
        assert_eq!(look(DEFAULT_COLOR, Style::Hidden, dark, 1., 1.), None);
        assert_eq!(Style::from_index(2), Style::NoGradient);
        assert_eq!(Style::from_index(9), Style::Default);
    }

    #[test]
    fn hsl_values_match_the_bundles_rounding() {
        // 3447003 is #3498db: hue 204, saturation 0.699, lightness 0.531.
        assert_eq!(tf(DEFAULT_COLOR), (204., 0.699, 0.531));
        let back = from_hsl(to_hsl([52., 152., 219.]));
        assert!(
            back.iter()
                .zip([52., 152., 219.])
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
}
