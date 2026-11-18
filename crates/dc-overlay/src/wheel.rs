//! The soundboard wheel (module 177303) and when the overlay shows it (modules 944775, 528772, 596339).
//!
//! - Opened by the Open Soundboard (Hold) keybind (`SOUNDBOARD_HOLD`, Ctrl+` by default): while a game with the
//!   overlay is in front, the key going down shows the wheel over the overlay (`hK`: `SOUNDBOARD_SET_OVERLAY_ENABLED`
//!   with `keepOpen: false`; the v3 overlay is not unlocked for it) and the key going up hides it (`Lk`), which plays
//!   the sound the pointer was on. The toggle form (`SOUNDBOARD`) keeps it open (`keepOpen: true`) until pressed again.
//!   In the app (no game) it opens a popout at the pointer only under the user experiment
//!   `2026-05-global-soundboard-wheel-popout`, whose default config is off.
//! - The wheel (`container_kK3yeM`: centred over the whole overlay) is 424 by 424 (`ed` of module 980504) with a
//!   288-unit ring (radius 103.68, stroke 40.32, `--background-surface-high` at 0.9) and eight 96 by 52 sound buttons
//!   a page at the points of `O`, placed by `D`. Moving the pointer selects the button the ray from the wheel's centre
//!   through the pointer meets first (`E`, a segment-rectangle crossing test); the 28.8-unit dead zone in the middle
//!   selects nothing, and the indicator (`chatWheelCenter`, 28.8 at 0.6) follows the pointer within the ring. The
//!   wheel's scroll pages through the sounds; "Scroll for more" shows while there is more than one page. A click on
//!   the selected button plays it and closes the wheel.
//! - The sounds (`C`): the call server's available guild ids then the defaults (`"0"`), the favourites of each first,
//!   then the rest, each server's sorted by sound id; only available sounds.

/// `ed` of module 980504.
pub const SIZE: f32 = 424.;
/// The SVG's `viewBox` side.
pub const VIEW: f32 = 288.;
pub const RING_RADIUS: f32 = 103.68;
pub const RING_STROKE: f32 = 40.32;
pub const DEAD_ZONE: f32 = 28.8;
/// The sound buttons (`soundButton_JhiVh1`).
pub const ITEM_WIDTH: f32 = 96.;
pub const ITEM_HEIGHT: f32 = 52.;
/// `O`: the eight places of a page, as fractions of the wheel.
pub const PLACES: [(f32, f32); 8] = [
    (0.5, 0.05),
    (0.95, 0.2),
    (1., 0.5),
    (0.95, 0.8),
    (0.5, 0.95),
    (0.05, 0.8),
    (0., 0.5),
    (0.05, 0.2),
];
/// The user experiment that lets the keybind open the wheel in the app (module 596339 `Cp`), off by default.
pub const POPOUT_EXPERIMENT: &str = "2026-05-global-soundboard-wheel-popout";

/// `D`: an item's left (or top) edge for a place fraction.
pub fn place(fraction: f32, wheel: f32, item: f32) -> f32 {
    let at = fraction * wheel;
    if fraction > 0.5 {
        at - item
    } else if fraction < 0.5 {
        at
    } else {
        at - item / 2.
    }
}

/// The rectangle (left, top, width, height) of slot `index` of a page, in the wheel's pixels.
pub fn slot(index: usize, wheel: (f32, f32)) -> Option<(f32, f32, f32, f32)> {
    let (x, y) = *PLACES.get(index)?;
    Some((
        place(x, wheel.0, ITEM_WIDTH),
        place(y, wheel.1, ITEM_HEIGHT),
        ITEM_WIDTH,
        ITEM_HEIGHT,
    ))
}

/// The pages of `count` sounds (`chunk(f, 8)`).
pub fn pages(count: usize) -> usize {
    count.div_ceil(PLACES.len())
}

type Point = (f32, f32);

/// `w`: whether `c` lies counter-clockwise of the line from `a` to `b`.
fn ccw(a: Point, b: Point, c: Point) -> bool {
    (c.1 - a.1) * (b.0 - a.0) > (b.1 - a.1) * (c.0 - a.0)
}

/// `E`: whether the segments `a`-`b` and `c`-`d` cross.
fn crosses(a: Point, b: Point, c: Point, d: Point) -> bool {
    ccw(a, c, d) != ccw(b, c, d) && ccw(a, b, c) != ccw(a, b, d)
}

/// Whether the segment crosses one of the rectangle's sides.
fn hits(a: Point, b: Point, rect: (f32, f32, f32, f32)) -> bool {
    let (x, y, w, h) = rect;
    let (tl, tr, bl, br) = ((x, y), (x + w, y), (x, y + h), (x + w, y + h));
    crosses(a, b, tl, tr) || crosses(a, b, tr, br) || crosses(a, b, br, bl) || crosses(a, b, bl, tl)
}

/// The slot the pointer selects (`K`), in wheel pixels with the wheel's top left at 0: the first slot of the page
/// whose rectangle the segment from the centre to a point `max(width, height)` past the pointer crosses. `None` in
/// the dead zone (the pointer within its circle, `m.current`) or when nothing is crossed.
pub fn select(pointer: Point, wheel: (f32, f32), page_items: usize) -> Option<usize> {
    let centre = (wheel.0 / 2., wheel.1 / 2.);
    let scale = wheel.0.max(wheel.1) / VIEW;
    let distance = ((centre.0 - pointer.0).powi(2) + (centre.1 - pointer.1).powi(2)).sqrt();
    if distance <= DEAD_ZONE * scale {
        return None;
    }
    let reach = wheel.0.max(wheel.1);
    let far = (
        pointer.0 + (pointer.0 - centre.0) / distance * reach,
        pointer.1 + (pointer.1 - centre.1) / distance * reach,
    );
    (0..page_items.min(PLACES.len()))
        .find(|index| slot(*index, wheel).is_some_and(|rect| hits(centre, far, rect)))
}

/// `G`: the indicator's offset from the centre (half the pointer's, kept inside the ring), in the units it is added
/// to the SVG's centre with.
pub fn indicator(pointer: Point, wheel: (f32, f32)) -> (f32, f32) {
    let centre = (wheel.0 / 2., wheel.1 / 2.);
    let delta = (pointer.0 - centre.0, pointer.1 - centre.1);
    let limit = wheel.0.max(wheel.1) / 2. - DEAD_ZONE;
    let angle = delta.1.abs().atan2(delta.0.abs());
    let bound = ((limit * angle.cos()).max(0.), (limit * angle.sin()).max(0.));
    let clamp = |value: f32, bound: f32| {
        if value < 0. {
            value.max(-bound)
        } else {
            value.min(bound)
        }
    };
    (clamp(delta.0, bound.0) / 2., clamp(delta.1, bound.1) / 2.)
}

/// `B`: the page after a wheel step (`delta_y` > 0 is the next page), kept inside the pages; the selection follows
/// to the same slot when the new page has it.
pub fn scroll(
    page: usize,
    delta_y: f32,
    count: usize,
    selected: Option<usize>,
) -> (usize, Option<usize>) {
    let next = if delta_y > 0. {
        page as i64 + 1
    } else {
        page as i64 - 1
    };
    if next < 0 || next as usize >= pages(count) {
        return (page, selected);
    }
    let next = next as usize;
    let on_page = count.saturating_sub(next * PLACES.len()).min(PLACES.len());
    (next, selected.filter(|slot| *slot < on_page))
}

/// One sound the wheel can list.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub id: String,
    /// `"0"` for the defaults.
    pub guild: String,
    pub available: bool,
}

/// `C`: the wheel's order over the given servers (the defaults are added last).
pub fn order(sounds: &[Sound], guilds: &[String], favorites: &[String]) -> Vec<Sound> {
    let mut ids: Vec<String> = guilds.to_vec();
    ids.push("0".to_owned());
    let mut out = Vec::new();
    for favorite_pass in [true, false] {
        for guild in &ids {
            let mut of_guild: Vec<&Sound> = sounds
                .iter()
                .filter(|sound| &sound.guild == guild)
                .collect();
            // `h.default.compare`: snowflakes by length, then as text.
            of_guild.sort_by(|a, b| a.id.len().cmp(&b.id.len()).then_with(|| a.id.cmp(&b.id)));
            for sound in of_guild {
                let favorite = favorites.contains(&sound.id);
                if favorite == favorite_pass && sound.available {
                    out.push(sound.clone());
                }
            }
        }
    }
    out
}

/// What the overlay shows of the wheel (`SoundboardOverlayStore`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub keep_open: bool,
}

/// `SOUNDBOARD_HOLD`'s callback with a game in front: down shows (hold), up hides.
pub fn hold(down: bool) -> Option<Shown> {
    down.then_some(Shown { keep_open: false })
}

/// `SOUNDBOARD`'s callback (on the key going up): open closes, closed opens to stay.
pub fn toggle(open: bool) -> Option<Shown> {
    (!open).then_some(Shown { keep_open: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slots_sit_on_the_ring() {
        assert_eq!(slot(0, (SIZE, SIZE)), Some((164., 21.2, 96., 52.)));
        assert_eq!(slot(2, (SIZE, SIZE)), Some((328., 186., 96., 52.)));
        assert_eq!(slot(6, (SIZE, SIZE)), Some((0., 186., 96., 52.)));
        assert_eq!(slot(8, (SIZE, SIZE)), None);
        assert_eq!(pages(0), 0);
        assert_eq!(pages(8), 1);
        assert_eq!(pages(9), 2);
    }

    #[test]
    fn the_pointer_ray_selects_a_slot() {
        let wheel = (SIZE, SIZE);
        // Straight up from the centre: the top slot; right: slot 2; left: slot 6.
        assert_eq!(select((212., 120.), wheel, 8), Some(0));
        assert_eq!(select((320., 212.), wheel, 8), Some(2));
        assert_eq!(select((100., 212.), wheel, 8), Some(6));
        assert_eq!(select((212., 320.), wheel, 8), Some(4));
        // In the dead zone (28.8 units, 42.4 px): nothing.
        assert_eq!(select((220., 220.), wheel, 8), None);
        // Fewer sounds than slots: a ray to an empty slot selects nothing.
        assert_eq!(select((100., 212.), wheel, 3), None);
    }

    #[test]
    fn the_indicator_stays_inside_the_ring() {
        let wheel = (SIZE, SIZE);
        assert_eq!(indicator((232., 212.), wheel), (10., 0.));
        let (x, y) = indicator((2000., 212.), wheel);
        assert!((x - (212. - DEAD_ZONE) / 2.).abs() < 1e-3);
        assert!(y.abs() < 1e-3);
    }

    #[test]
    fn scrolling_pages_and_keeps_the_slot_when_it_exists() {
        assert_eq!(scroll(0, 1., 12, Some(2)), (1, Some(2)));
        assert_eq!(scroll(0, 1., 12, Some(6)), (1, None));
        assert_eq!(scroll(1, 1., 12, Some(1)), (1, Some(1)));
        assert_eq!(scroll(0, -1., 12, None), (0, None));
        assert_eq!(scroll(1, -1., 12, Some(3)), (0, Some(3)));
    }

    #[test]
    fn the_sounds_go_favourites_first_by_server_then_by_id() {
        let sound = |id: &str, guild: &str, available| Sound {
            id: id.to_owned(),
            guild: guild.to_owned(),
            available,
        };
        let sounds = [
            sound("30", "0", true),
            sound("4", "0", true),
            sound("200", "7", true),
            sound("100", "7", true),
            sound("5", "7", false),
            sound("9", "8", true),
        ];
        let ids = |list: Vec<Sound>| list.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(
            ids(order(&sounds, &["7".to_owned()], &["30".to_owned()])),
            ["30", "100", "200", "4"]
        );
        assert_eq!(hold(true), Some(Shown { keep_open: false }));
        assert_eq!(hold(false), None);
        assert_eq!(toggle(false), Some(Shown { keep_open: true }));
        assert_eq!(toggle(true), None);
    }
}
