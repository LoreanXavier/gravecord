//! The composer's emoji button (`y` in module 375499, class module 346089). While the pointer is over it (or its
//! picker is open) the grey mask sprite gives way to a colored one, a cell of the same 20 by 4 sheet picked at
//! random (`Math.floor(77 * Math.random())`) each time the pointer enters, and the whole sprite scales from 1 to
//! 1.14 on a react-spring `{tension: 800, friction: 24}`. The swap has no transition; reduced motion does not
//! change the spring.

use super::{SpringConfig, hovered, key, spring_from};
use std::cell::Cell;

/// `N` in module 375499.
pub const SPRING: SpringConfig = SpringConfig::new(800., 24.);

/// The sprite sheet holds 77 emoji cells for the button.
pub const CELLS: usize = 77;

/// The key of the button's hover probe.
pub fn hover_key() -> u64 {
    key("composer-emoji-button")
}

thread_local! {
    static STATE: Cell<(bool, usize)> = const { Cell::new((false, 50)) };
}

/// The cell for a random draw in `0..1`.
pub fn cell_for(draw: f64) -> usize {
    ((draw.clamp(0., 0.999_999) * CELLS as f64).floor() as usize).min(CELLS - 1)
}

fn draw() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    f64::from(nanos % 10_000) / 10_000.
}

/// Whether the colored sprite shows, its scale and its cell this frame. `open` is the `active` prop (the picker
/// is open on this button).
pub fn emoji_button(open: bool) -> (bool, f32, usize) {
    let over = hovered(hover_key());
    let (was, index) = STATE.with(Cell::get);
    let shown = over || open;
    let index = if over && !was {
        cell_for(draw())
    } else {
        index
    };
    STATE.with(|state| state.set((over, index)));
    let grown = spring_from(
        key("composer-emoji-scale"),
        0.,
        if shown { 1. } else { 0. },
        SPRING,
    );
    (shown, 1. + 0.14 * grown, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests, set_hover};

    #[test]
    fn the_cell_is_one_of_the_77() {
        assert_eq!(cell_for(0.), 0);
        assert_eq!(cell_for(0.5), 38);
        assert_eq!(cell_for(1.), 76);
    }

    #[test]
    fn hovering_scales_the_sprite_to_1_14_on_the_spring_and_shows_the_colored_cell() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        assert_eq!(emoji_button(false), (false, 1., 50));
        set_hover(hover_key(), true);
        freeze(Some(1.));
        begin_frame(false);
        let (shown, scale, index) = emoji_button(false);
        assert!(shown && index < CELLS && scale >= 1.);
        freeze(Some(60.));
        begin_frame(false);
        let (_, middle, _) = emoji_button(false);
        assert!(middle > 1.0 && middle < 1.2, "{middle}");
        freeze(Some(2000.));
        begin_frame(false);
        let (_, end, _) = emoji_button(false);
        assert!((end - 1.14).abs() < 1e-3, "{end}");
        set_hover(hover_key(), false);
        freeze(Some(2001.));
        begin_frame(false);
        let _ = emoji_button(false);
        freeze(Some(4000.));
        begin_frame(false);
        assert_eq!(emoji_button(false), (false, 1., STATE.with(Cell::get).1));
    }

    #[test]
    fn an_open_picker_keeps_the_colored_sprite() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        assert!(emoji_button(true).0);
    }
}
