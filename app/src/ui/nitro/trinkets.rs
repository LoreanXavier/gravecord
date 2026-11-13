//! The hero's floating art on the Nitro subscriber home (`to` -> `tr` of module 886268, compact variant because the
//! hero has no banner): the flying Wumpus (`tl`) and the bolt, hammer, key and star trinkets (`ti`), each moved by
//! `te`: a looping vertical sine or cosine, a scale that runs between two values and back, a sideways wiggle, and a
//! parallax rise with fading as the hero scrolls out. Positions from `compact*_Rl3g05` (stylesheet 9a4971d6952f977a).

use crate::ui::motion;

/// `animationSpeedScale: 1 / .7` of `to`: every duration is this much longer.
pub(super) const SPEED: f64 = 1. / 0.7;

const CDN: &str = "https://cdn.discordapp.com/assets/content/";

/// `sine` or `cosine` of `yAxisAnimationData.path`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Path {
    Sine,
    Cosine,
}

/// One piece of art and the `te` props `tl`/`ti` give it.
pub(super) struct Piece {
    pub key: &'static str,
    pub file: &'static str,
    /// Width over height of the picture.
    pub aspect: f32,
    /// `scaleAnimationData`: start, end and duration (ms).
    pub scale: Option<(f32, f32, f64)>,
    /// `yAxisAnimationData`: range (px), duration (ms), path.
    pub bob: (f32, f64, Path),
    /// `parallaxAnimationData`: range (px), up, and whether opacity follows the hero's visibility.
    pub parallax: (f32, bool, bool),
}

/// The Wumpus first (`tl`), then `ti`'s bolt, hammer, key and star (no car: `to` gives it no class). The Wumpus is
/// drawn last: its container has `z-index: 1` (2 below 1140px).
pub(super) static PIECES: [Piece; 5] = [
    Piece {
        key: "nitro-home:wumpus",
        // The still `tl` shows when the browser plays HEVC with alpha; the `.webm` of other browsers is not played here.
        file: "082012af2fe8bfa66ce6630e1549a146738936af43a8e60c780f9976fa333d93.png",
        aspect: 1.,
        scale: Some((0.9, 1., 3000.)),
        bob: (20., 4000., Path::Sine),
        parallax: (200., true, false),
    },
    Piece {
        key: "nitro-art:bolt",
        file: "ae5638c61a572593c6b03b92e80d3846e0cfe7a9e893f3faf05aecd670a4017d.png",
        aspect: 62. / 87.,
        scale: Some((0.85, 1., 3000.)),
        bob: (15., 6000., Path::Cosine),
        parallax: (125., true, true),
    },
    Piece {
        key: "nitro-art:hammer",
        file: "b39a5f2755e6da320fce10c8a4a10bdebef9926b671256b1681976198c5656d4.png",
        aspect: 100. / 103.,
        scale: None,
        bob: (15., 6000., Path::Sine),
        parallax: (120., false, true),
    },
    Piece {
        key: "nitro-art:key",
        file: "7c23a220a6f31150648930e2ebb435aa7cc89ad57895275bed6f1900869f4de0.png",
        aspect: 78. / 72.,
        scale: Some((0.85, 1., 4000.)),
        bob: (15., 6000., Path::Sine),
        parallax: (200., true, true),
    },
    Piece {
        key: "nitro-art:star",
        file: "0b1a59149e615fc048010a3c7f109f8695c8b2004712e99417fcb0dec43fcb44.png",
        aspect: 83. / 90.,
        scale: Some((1., 0.85, 3000.)),
        bob: (20., 4000., Path::Cosine),
        parallax: (50., true, true),
    },
];

/// The still `tl` shows under reduced motion.
pub(super) const WUMPUS_STILL: &str =
    "46e72137fc3631c8024b00c33dbab5cf45740d4ab35f77bd96517830e727d0c5.png";
pub(super) const WUMPUS_STILL_KEY: &str = "nitro-home:wumpus-still";

impl Piece {
    pub(super) fn url(&self) -> String {
        format!("{CDN}{}", self.file)
    }
}

/// Where a piece sits in the art box and how tall it is: (left or right edge, top, height); `None` left means it is
/// placed from the right (`inset-inline-end`). `cqi` is the page container's width.
pub(super) fn place(index: usize, cqi: f32) -> (Option<f32>, Option<f32>, f32, f32) {
    let clamp = |min: f32, scale: f32, max: f32| (cqi * scale / 100.).clamp(min, max);
    if cqi > 1140. {
        // `compactFlyingWumpusContainer` etc. above 1140px.
        match index {
            0 => (Some(60.), None, -20., 250.),
            1 => (None, Some(100.), 150., 62.),
            2 => (Some(310.), None, 25., 32.),
            3 => (Some(20.), None, 120., 58.),
            _ => (Some(55.), None, -30., 60.),
        }
    } else {
        // `@container (max-width: 1140px)`.
        match index {
            0 => (Some(80.), None, -10., clamp(130., 22., 250.)),
            1 => (Some(80. + cqi * 0.26), None, 140., clamp(34., 5.5, 62.)),
            2 => (Some(80. + cqi * 0.24), None, 25., clamp(18., 2.8, 32.)),
            3 => (Some(55.), None, 110., clamp(32., 5.1, 58.)),
            _ => (Some(90.), None, -30., clamp(32., 5.3, 60.)),
        }
    }
}

/// The art box (`treatmentAArtContainer_aWZQh8`): 440 wide in the row above 1140px, then absolute at the top right
/// at `clamp(281px, 38.6cqi, 440px)`; gone at 720px and below.
pub(super) fn art_width(cqi: f32) -> Option<f32> {
    if cqi <= 720. {
        None
    } else if cqi <= 1140. {
        Some((cqi * 0.386).clamp(281., 440.))
    } else {
        Some(440.)
    }
}

/// `te` at `elapsed` ms after mount: (translate x, translate y, scale, opacity). `visibility` is the hero's
/// intersection ratio (`eg`).
pub(super) fn pose(
    piece: &Piece,
    index: usize,
    elapsed: f64,
    visibility: f32,
) -> (f32, f32, f32, f32) {
    let (range, duration, path) = piece.bob;
    // `{from: {y: 0}, to: {y: 1}, config: {duration}, loop: true}`: linear, restarting at 0.
    let y = (elapsed / (duration * SPEED)).fract();
    let wave = match path {
        Path::Sine => (y * std::f64::consts::TAU).sin(),
        Path::Cosine => (y * std::f64::consts::TAU).cos(),
    };
    let (parallax_range, up, fade) = piece.parallax;
    let shift = parallax_range * (1. - visibility);
    let parallax = if up { -shift } else { shift };
    // The scale runs start -> end, then end -> start (`onRest` flips the direction), linearly.
    let scale = piece.scale.map_or(1., |(start, end, duration)| {
        let leg = duration * SPEED;
        let runs = elapsed / leg;
        let t = runs.fract() as f32;
        if (runs as u64).is_multiple_of(2) {
            start + (end - start) * t
        } else {
            end + (start - end) * t
        }
    });
    (
        wiggle(index, elapsed),
        wave as f32 * range + parallax,
        scale,
        if fade { visibility } else { 1. },
    )
}

/// `xOffset`: every `A` ms (750 +/- 100, times the speed scale, drawn once per piece) the offset is set to the other
/// side, 2.5 to 5px out, and moves there linearly over `A`. The official draws with `Math.random`; a fixed sequence
/// per piece stands in for it.
fn wiggle(index: usize, elapsed: f64) -> f32 {
    let period = ((750. + (200. * unit(index as u64, 0) - 100.)) * SPEED).round();
    let tick = (elapsed / period).floor() as u64;
    let target = |tick: u64| -> f32 {
        if tick == 0 {
            return 0.;
        }
        let sign = if tick % 2 == 1 { 1. } else { -1. };
        sign * (0.5 * unit(index as u64, tick) as f32 * 5. + 2.5)
    };
    let from = target(tick);
    let to = target(tick + 1);
    let t = ((elapsed - tick as f64 * period) / period) as f32;
    from + (to - from) * t
}

/// A number in `0..1` from two integers (a stand-in for `Math.random`).
fn unit(seed: u64, step: u64) -> f64 {
    let mut x = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(step.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    x ^= x >> 31;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 29;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// The Wumpus's frames (an animated PNG), ready to draw, with the file's own timing.
pub(crate) struct Clip {
    timing: std::sync::Arc<crate::ui::nameplate_animation::Clip>,
    frames: Vec<std::sync::Arc<gpui::RenderImage>>,
}

impl Clip {
    /// Every frame of the file, in the BGRA order the renderer takes; `None` for a still PNG.
    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        let timing = crate::ui::nameplate_animation::decode(bytes)?;
        let frames = timing
            .frames
            .iter()
            .map(|frame| {
                let mut buffer = (**frame).clone();
                for pixel in buffer.pixels_mut() {
                    pixel.0.swap(0, 2);
                }
                std::sync::Arc::new(gpui::RenderImage::new(vec![image::Frame::new(buffer)]))
            })
            .collect();
        Some(Self { timing, frames })
    }

    /// The frame shown `elapsed` ms after the page mounted (the clip loops).
    pub(super) fn frame(&self, elapsed: f64) -> Option<std::sync::Arc<gpui::RenderImage>> {
        let length = self.timing.duration.as_secs_f64() * 1000.;
        if length <= 0. {
            return None;
        }
        let at = std::time::Duration::from_secs_f64(elapsed.rem_euclid(length) / 1000.);
        self.frames.get(self.timing.frame_at(at)).cloned()
    }
}

/// The hero keeps drawing while its art moves.
pub(super) fn keep_moving() {
    if !motion::reduced() {
        motion::keep_animating();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bob_loops_over_its_scaled_duration() {
        let star = &PIECES[4];
        // Cosine path: 20px down at the start of each loop of 4000 / .7 ms.
        let (_, y, _, _) = pose(star, 4, 0., 1.);
        assert!((y - 20.).abs() < 1e-4);
        let (_, y, _, _) = pose(star, 4, 4000. * SPEED / 2., 1.);
        assert!((y + 20.).abs() < 1e-3);
        let (_, y, _, _) = pose(star, 4, 4000. * SPEED, 1.);
        assert!((y - 20.).abs() < 1e-3);
    }

    #[test]
    fn the_scale_runs_there_and_back() {
        let bolt = &PIECES[1];
        let leg = 3000. * SPEED;
        assert!((pose(bolt, 1, 0., 1.).2 - 0.85).abs() < 1e-5);
        assert!((pose(bolt, 1, leg * 0.5, 1.).2 - 0.925).abs() < 1e-4);
        assert!((pose(bolt, 1, leg * 1.5, 1.).2 - 0.925).abs() < 1e-4);
        assert!((pose(bolt, 1, leg * 1.999, 1.).2 - 0.85).abs() < 1e-3);
        assert_eq!(pose(&PIECES[2], 2, leg, 1.).2, 1.);
    }

    #[test]
    fn parallax_rises_and_fades_as_the_hero_leaves() {
        let key = &PIECES[3];
        let (_, y_full, _, opacity_full) = pose(key, 3, 0., 1.);
        let (_, y_half, _, opacity_half) = pose(key, 3, 0., 0.5);
        assert!((y_full - y_half - 100.).abs() < 1e-3);
        assert_eq!((opacity_full, opacity_half), (1., 0.5));
        // The hammer moves down instead; the Wumpus keeps its opacity (`changeOpacity: false`).
        let (_, y_hammer, _, _) = pose(&PIECES[2], 2, 0., 0.5);
        let (_, y_hammer_full, _, _) = pose(&PIECES[2], 2, 0., 1.);
        assert!((y_hammer - y_hammer_full - 60.).abs() < 1e-3);
        let (_, _, _, wumpus) = pose(&PIECES[0], 0, 0., 0.3);
        assert_eq!(wumpus, 1.);
    }

    #[test]
    fn the_wiggle_stays_within_five_pixels() {
        for index in 0..5 {
            for step in 0..400 {
                let x = wiggle(index, step as f64 * 37.);
                assert!(x.abs() <= 5.0001, "{index} {step} {x}");
            }
        }
        assert_eq!(wiggle(0, 0.), 0.);
    }

    #[test]
    fn the_art_box_follows_the_container_queries() {
        assert_eq!(art_width(1400.), Some(440.));
        assert!((art_width(1000.).unwrap() - 386.).abs() < 1e-3);
        assert_eq!(art_width(720.), None);
        assert_eq!(place(0, 1400.), (Some(60.), None, -20., 250.));
        assert_eq!(place(1, 1400.).1, Some(100.));
        assert_eq!(place(0, 900.).3, 198.);
    }
}
