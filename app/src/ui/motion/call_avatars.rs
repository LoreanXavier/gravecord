//! The avatars of a DM call coming and going (module 493387, the `b` wrapper of `VoiceCallParticipants`, inside a
//! `TransitionGroup`).
//!
//! * A person already in the call when the area mounts shows at once (`componentDidAppear`).
//! * One who joins (`componentWillEnter`): the value `scale` runs 0 to 1 over 300 ms `Easing.inOut(Easing.back())`
//!   and drives `transform: scale(.7 → 1)` and `opacity`; at the same time `width` runs 0 to 1 over 200 ms with
//!   Animated's default easing (`Easing.inOut(Easing.ease)`, module 452767) and drives the wrapper's width from 0 to
//!   the avatar's size.
//! * One who leaves (`componentWillLeave`): the sprite (`cross_IXpFcz`, white in a dark theme and grey otherwise,
//!   25 px frames of a 625 x 25 sheet) shows at once at position 0; `scale` runs 1 to 0 over 170 ms `Easing.quad`;
//!   then 23 timings of 17 ms move the sprite's `background-position` to `-26 * k` px (k = 0..22), each with the
//!   default easing; then `width` runs to 0 over 125 ms. The sprite stays until the wrapper is gone.
//!
//! The pinned client does not gate this on reduced motion; here reduced motion shows and removes the avatars at once,
//! as the app's other motion does.

use super::Ease;
use dc_state::Id;
use std::cell::RefCell;
use std::collections::HashMap;

pub const JOIN_SCALE_MS: f64 = 300.;
pub const JOIN_WIDTH_MS: f64 = 200.;
pub const LEAVE_SCALE_MS: f64 = 170.;
pub const SPRITE_STEPS: usize = 23;
pub const SPRITE_STEP_MS: f64 = 17.;
pub const SPRITE_STEP_PX: f32 = 26.;
pub const LEAVE_WIDTH_MS: f64 = 125.;
/// `SCALE_MIN`.
pub const SCALE_MIN: f32 = 0.7;
/// The sprite sheets (`.crossWhite_IXpFcz`, `.crossGrey_IXpFcz`), fetched from the asset host as the app does for
/// the client's other pictures; nothing is bundled.
pub const SPRITE_WHITE: &str = "https://discord.com/assets/dd2ccc47416b2b19.png";
pub const SPRITE_GREY: &str = "https://discord.com/assets/d085fdd3215d0dc0.png";
/// `.cross_IXpFcz`: one frame, and the sheet drawn at `background-size: 625px 25px`.
pub const SPRITE_FRAME: f32 = 25.;
pub const SPRITE_SHEET: f32 = 625.;
/// An area not drawn for this long mounts again: who is there shows at once.
const REMOUNT_MS: f64 = 1000.;

/// `Easing.inOut(Easing.ease)`, Animated's default: `ease` is `bezier(.42, 0, 1, 1)`.
pub fn default_ease(t: f64) -> f64 {
    let ease = |x: f64| Ease::Bezier(0.42, 0., 1., 1.).apply(x);
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        ease(t * 2.) / 2.
    } else {
        1. - ease((1. - t) * 2.) / 2.
    }
}

/// How one avatar is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The wrapper's share of the avatar's width.
    pub width: f32,
    pub scale: f32,
    pub opacity: f32,
    /// The sprite's `background-position` in px, when the sprite shows.
    pub sprite: Option<f32>,
}

impl Pose {
    pub const REST: Pose = Pose {
        width: 1.,
        scale: 1.,
        opacity: 1.,
        sprite: None,
    };
}

/// The pose `ms` after a join, or `None` once it has ended.
pub fn join_pose(ms: f64) -> Option<Pose> {
    if ms >= JOIN_SCALE_MS.max(JOIN_WIDTH_MS) {
        return None;
    }
    let s = super::back_in_out(ms / JOIN_SCALE_MS) as f32;
    Some(Pose {
        width: default_ease(ms / JOIN_WIDTH_MS) as f32,
        scale: SCALE_MIN + (1. - SCALE_MIN) * s,
        opacity: s.clamp(0., 1.),
        sprite: None,
    })
}

/// The pose `ms` after a leave, or `None` once the wrapper is gone.
pub fn leave_pose(ms: f64) -> Option<Pose> {
    let ms = ms.max(0.);
    let sprite_end = LEAVE_SCALE_MS + SPRITE_STEPS as f64 * SPRITE_STEP_MS;
    if ms >= sprite_end + LEAVE_WIDTH_MS {
        return None;
    }
    let s = if ms < LEAVE_SCALE_MS {
        let t = ms / LEAVE_SCALE_MS;
        (1. - t * t) as f32
    } else {
        0.
    };
    let sprite = if ms < LEAVE_SCALE_MS {
        0.
    } else if ms < sprite_end {
        let into = ms - LEAVE_SCALE_MS;
        let step = (into / SPRITE_STEP_MS).floor() as usize;
        let local = (into - step as f64 * SPRITE_STEP_MS) / SPRITE_STEP_MS;
        let from = -SPRITE_STEP_PX * step.saturating_sub(1) as f32;
        let to = -SPRITE_STEP_PX * step as f32;
        from + (to - from) * default_ease(local) as f32
    } else {
        -SPRITE_STEP_PX * (SPRITE_STEPS - 1) as f32
    };
    let width = if ms < sprite_end {
        1.
    } else {
        1. - default_ease((ms - sprite_end) / LEAVE_WIDTH_MS) as f32
    };
    Some(Pose {
        width,
        scale: SCALE_MIN + (1. - SCALE_MIN) * s,
        opacity: s.clamp(0., 1.),
        sprite: Some(sprite),
    })
}

#[derive(Default)]
struct Area {
    /// Who was drawn last time, in order.
    shown: Vec<Id>,
    joined: HashMap<Id, f64>,
    /// Who left, when, and where they were in the row.
    left: Vec<(Id, f64, usize)>,
    drawn_at: f64,
}

thread_local! {
    static AREAS: RefCell<HashMap<Id, Area>> = RefCell::new(HashMap::new());
}

/// The row of the call area of `channel` this frame: everyone to draw, in order, with their pose. `people` is who is
/// in the call now; someone who left is kept until their leave has run. `reduced`: nobody moves.
pub fn frame(channel: Id, people: &[Id], now: f64, reduced: bool) -> Vec<(Id, Pose)> {
    AREAS.with(|areas| {
        let mut areas = areas.borrow_mut();
        let area = areas.entry(channel).or_default();
        let fresh = area.drawn_at == 0. || now - area.drawn_at > REMOUNT_MS || reduced;
        if fresh {
            area.joined.clear();
            area.left.clear();
        } else {
            for (index, id) in area.shown.iter().enumerate() {
                if !people.contains(id) && !area.left.iter().any(|(gone, _, _)| gone == id) {
                    area.left.push((*id, now, index));
                }
            }
            for id in people {
                if !area.shown.contains(id) {
                    area.joined.insert(*id, now);
                    area.left.retain(|(gone, _, _)| gone != id);
                }
            }
        }
        area.shown = people.to_vec();
        area.drawn_at = now;
        area.joined
            .retain(|_, start| join_pose(now - *start).is_some());
        area.left
            .retain(|(_, start, _)| leave_pose(now - *start).is_some());
        let mut row: Vec<(Id, Pose)> = people
            .iter()
            .map(|id| {
                let pose = area
                    .joined
                    .get(id)
                    .and_then(|start| join_pose(now - start))
                    .unwrap_or(Pose::REST);
                (*id, pose)
            })
            .collect();
        for (id, start, index) in &area.left {
            if let Some(pose) = leave_pose(now - start) {
                row.insert((*index).min(row.len()), (*id, pose));
            }
        }
        row
    })
}

/// Whether something in the row of `channel` still moves.
pub fn moving(row: &[(Id, Pose)]) -> bool {
    row.iter().any(|(_, pose)| *pose != Pose::REST)
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    AREAS.with(|areas| areas.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_ease_is_ease_in_out_of_bezier_42_0_1_1() {
        assert_eq!(default_ease(0.), 0.);
        assert_eq!(default_ease(1.), 1.);
        assert!((default_ease(0.5) - 0.5).abs() < 1e-9);
        let quarter = Ease::Bezier(0.42, 0., 1., 1.).apply(0.5) / 2.;
        assert!((default_ease(0.25) - quarter).abs() < 1e-9);
        assert!((default_ease(0.25) + default_ease(0.75) - 1.).abs() < 1e-9);
    }

    #[test]
    fn a_join_scales_from_0_7_and_widens_over_200ms() {
        let start = join_pose(0.).unwrap();
        assert_eq!(start.width, 0.);
        assert_eq!(start.scale, SCALE_MIN);
        assert_eq!(start.opacity, 0.);
        let mid = join_pose(100.).unwrap();
        assert!((mid.width - 0.5).abs() < 1e-6);
        // `inOut(back())` dips below 0 at first.
        assert!(join_pose(30.).unwrap().scale < SCALE_MIN);
        let late = join_pose(250.).unwrap();
        assert_eq!(late.width, 1.);
        assert!(late.scale > 1., "back overshoots: {}", late.scale);
        assert_eq!(join_pose(300.), None);
    }

    #[test]
    fn a_leave_shrinks_puffs_then_closes_its_slot() {
        let start = leave_pose(0.).unwrap();
        assert_eq!(
            (start.scale, start.opacity, start.sprite),
            (1., 1., Some(0.))
        );
        let half = leave_pose(85.).unwrap();
        assert!((half.opacity - 0.75).abs() < 1e-6);
        let shrunk = leave_pose(170.).unwrap();
        assert_eq!(
            (shrunk.opacity, shrunk.scale, shrunk.width),
            (0., SCALE_MIN, 1.)
        );
        // Step k (from 1) runs from -26(k-1) to -26k over 17 ms.
        let step3_mid = leave_pose(170. + 3. * 17. + 8.5).unwrap().sprite.unwrap();
        assert!((step3_mid - (-52. - 13.)).abs() < 1e-3, "{step3_mid}");
        let puffed = leave_pose(170. + 23. * 17.).unwrap();
        assert_eq!(puffed.sprite, Some(-572.));
        assert_eq!(puffed.width, 1.);
        let closing = leave_pose(170. + 23. * 17. + 62.5).unwrap();
        assert!((closing.width - 0.5).abs() < 1e-6);
        assert_eq!(leave_pose(170. + 23. * 17. + 125.), None);
    }

    #[test]
    fn the_first_row_shows_at_once_then_joins_and_leaves_move() {
        reset_for_tests();
        let row = frame(1, &[10, 20], 1000., false);
        assert_eq!(row, vec![(10, Pose::REST), (20, Pose::REST)]);
        assert!(!moving(&row));
        let row = frame(1, &[10, 20, 30], 1016., false);
        assert_eq!(row[2].0, 30);
        assert_eq!(row[2].1, join_pose(0.).unwrap());
        assert!(moving(&row));
        let row = frame(1, &[20, 30], 1032., false);
        assert_eq!(row[0].0, 10, "the leaver keeps its place");
        assert_eq!(row[0].1, leave_pose(0.).unwrap());
        let row = frame(1, &[20, 30], 1032. + 700., false);
        assert_eq!(row, vec![(20, Pose::REST), (30, Pose::REST)]);
        assert!(!moving(&row));
    }

    #[test]
    fn reduced_motion_and_a_remount_show_the_row_at_once() {
        reset_for_tests();
        frame(2, &[10], 1000., false);
        let row = frame(2, &[10, 20], 1016., true);
        assert!(!moving(&row));
        let row = frame(2, &[20], 1032., true);
        assert_eq!(row, vec![(20, Pose::REST)]);
        frame(3, &[10], 1000., false);
        let row = frame(3, &[10, 20], 5000., false);
        assert!(
            !moving(&row),
            "a remounted area does not animate who is there"
        );
    }
}
