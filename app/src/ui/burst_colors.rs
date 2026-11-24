//! The colours of a super reaction's pill (module 478840 `V` with the helpers of module 998304 and the colour class of
//! module 325335): the most vivid of the reaction's first three `burst_colors`, lifted until it reads on Discord's dark
//! grey for the count and the glow, and a darker copy of it, at a low opacity, for the pill's fill.

use super::official_messages::{contrast, from_hsl, luminance, to_hsl};

/// The light and dark references of module 478840 (`s`, `c`); the client draws the dark palette in a dark theme.
const DARK_REFERENCE: [f64; 3] = [0x36 as f64, 0x39 as f64, 0x3e as f64];

/// The pill's colours in a dark theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BurstLook {
    /// The count's colour and the glow's (`accentColor`).
    pub accent: [u8; 3],
    /// The fill, drawn at `opacity`, and the border while the account has super reacted (`backgroundColor`).
    pub background: [u8; 3],
    pub opacity: f32,
}

fn parse_hex(hex: &str) -> Option<[f64; 3]> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok().map(f64::from);
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The HSV of `h6`, rounded as it rounds: hue to a degree, saturation and value to hundredths of a percent.
fn hsv_score(rgb: [f64; 3]) -> f64 {
    let [r, g, b] = rgb.map(|channel| channel / 255.);
    let max = r.max(g).max(b);
    let spread = max - r.min(g).min(b);
    let saturation = if spread == 0. { 0. } else { spread / max };
    let round = |value: f64| (value * 100.).round() / 100.;
    round(100. * saturation) + round(100. * max)
}

/// `h6`: of the first three colours, the one with the most saturation plus value (the first such on a tie).
fn most_vivid(colors: &[String]) -> Option<[f64; 3]> {
    let mut best: Option<([f64; 3], f64)> = None;
    for color in colors.iter().take(3) {
        let rgb = parse_hex(color).unwrap_or([0.; 3]);
        let score = if parse_hex(color).is_some() {
            hsv_score(rgb)
        } else {
            0.
        };
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((rgb, score));
        }
    }
    best.map(|(rgb, _)| rgb)
}

/// Module 998304 `l` and `o` (the colour class rounds every channel).
fn round_through_hsl(hsl: [f64; 3]) -> [f64; 3] {
    from_hsl(hsl).map(f64::round)
}

/// `fE(color, 0.6, false)`: 0.6 darker, or 0.1 when that would pass black.
fn darken(rgb: [f64; 3]) -> [f64; 3] {
    let mut hsl = to_hsl(rgb);
    hsl[2] = if hsl[2] - 0.6 < 0. { 0.1 } else { hsl[2] - 0.6 };
    round_through_hsl(hsl)
}

/// `lZ`: with the saturation preference applied to both, the colour stepped 5% lighter (on a dark reference) until
/// it reaches `ratio` against it, ten steps at most.
fn readable_against(color: [f64; 3], reference: [f64; 3], ratio: f64, saturation: f64) -> [f64; 3] {
    let (mut color, mut reference) = (color, reference);
    if saturation < 1. {
        let mut hsl = to_hsl(color);
        hsl[1] *= saturation;
        color = from_hsl(hsl);
        let mut hsl = to_hsl(reference);
        hsl[1] *= saturation;
        reference = from_hsl(hsl);
    }
    let dark = luminance(reference) <= 0.5;
    let mut current = contrast(color, reference);
    for _ in 0..10 {
        if current >= ratio {
            break;
        }
        let mut hsl = to_hsl(color);
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
        color = from_hsl(hsl);
        current = contrast(color, reference);
    }
    color
}

fn to_bytes(rgb: [f64; 3]) -> [u8; 3] {
    rgb.map(|channel| channel.round().clamp(0., 255.) as u8)
}

/// Module 478840 `V` for a dark theme: `None` without colours.
pub(super) fn burst_look(colors: &[String], saturation: f64) -> Option<BurstLook> {
    let base = most_vivid(colors)?;
    let fill = readable_against(darken(base), DARK_REFERENCE, 5., saturation);
    let accent = readable_against(base, DARK_REFERENCE, 7., saturation);
    // `WN`: the fill's rounded channels with the saturation preference.
    let mut hsl = to_hsl(fill.map(f64::round));
    if hsl[0].is_nan() {
        hsl[0] = 0.;
    }
    hsl[1] *= saturation;
    let background = round_through_hsl(hsl);
    let grey = to_hsl(base)[1] < 0.1;
    Some(BurstLook {
        accent: to_bytes(accent),
        background: to_bytes(background),
        opacity: if grey { 0.5 } else { 0.2 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(list: &[&str]) -> Vec<String> {
        list.iter().map(|color| (*color).to_owned()).collect()
    }

    #[test]
    fn no_colours_no_look() {
        assert_eq!(burst_look(&[], 1.), None);
    }

    #[test]
    fn the_most_vivid_of_the_first_three_wins() {
        let picked = most_vivid(&colors(&["#404040", "#ff0000", "#808080", "#00ff00"]));
        assert_eq!(picked, Some([255., 0., 0.]));
    }

    #[test]
    fn a_vivid_colour_keeps_its_hue_and_reads_on_the_dark_grey() {
        let look = burst_look(&colors(&["#ff0000"]), 1.).unwrap();
        assert_eq!(look.opacity, 0.2);
        let accent = look.accent.map(f64::from);
        assert!(contrast(accent, DARK_REFERENCE) >= 7. || to_hsl(accent)[2] >= 0.95);
        assert!(look.accent[0] > look.accent[1] && look.accent[0] > look.accent[2]);
        assert!(look.background[0] > look.background[1]);
    }

    #[test]
    fn a_grey_gets_the_stronger_fill() {
        let look = burst_look(&colors(&["#777777"]), 1.).unwrap();
        assert_eq!(look.opacity, 0.5);
    }

    #[test]
    fn darkening_stops_at_a_tenth() {
        assert_eq!(darken([40., 40., 40.]), [26., 26., 26.]);
    }
}
