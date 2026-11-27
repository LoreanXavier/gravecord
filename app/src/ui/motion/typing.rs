//! Not in the pinned client: the typing indicator's row fades in and out, as in Discord's native client ("Phoenix"). The pinned build mounts and unmounts it at once (`.typing_U9Tvvz` has no transition).
//!
//! Phoenix's `chat/typing.rs` wraps the row (`typing-row`) in a presence with the slot `chat-typing-fade`: the same
//! transition for entering and leaving, 500 ms with curve 4 (`ease-in-out`, spec at RVA `0x154da7d8`), drives the
//! row's opacity, and the row stays mounted while it fades out. Under reduced motion it shows and goes at once.

use super::{Css, Ease, key, value_from};
use dc_state::Id;
use std::cell::RefCell;
use std::collections::HashMap;

/// `chat-typing-fade`: 500 ms, `ease-in-out`, in both directions.
pub const FADE: Css = Css::new(500., Ease::EaseInOut).full_motion();

thread_local! {
    /// The people the row last named, per channel, so a fading row keeps its text.
    static LAST: RefCell<HashMap<Id, Vec<Id>>> = RefCell::new(HashMap::new());
}

/// The row of `channel` this frame: the people to name and its opacity, or `None` when it is not drawn. `typing` is
/// who types now; once nobody does the row fades out with the names it had.
pub fn row(channel: Id, typing: Vec<Id>) -> Option<(Vec<Id>, f32)> {
    let shown = !typing.is_empty();
    let opacity = value_from(
        key(&("chat-typing-fade", channel)),
        0.,
        if shown { 1. } else { 0. },
        FADE,
    );
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if shown {
            last.insert(channel, typing.clone());
            return Some((typing, opacity));
        }
        if opacity <= 0. {
            last.remove(&channel);
            return None;
        }
        last.get(&channel).cloned().map(|names| (names, opacity))
    })
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    LAST.with(|last| last.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests as reset_motion};

    fn at(ms: f64, reduced: bool) {
        freeze(Some(ms));
        begin_frame(reduced);
    }

    #[test]
    fn the_row_fades_in_over_500ms_ease_in_out() {
        reset_motion();
        reset_for_tests();
        at(0., false);
        assert_eq!(row(1, Vec::new()), None);
        at(16., false);
        assert_eq!(row(1, vec![7]), Some((vec![7], 0.)));
        at(266., false);
        let (_, half) = row(1, vec![7]).expect("shown");
        assert!((half - 0.5).abs() < 0.01, "{half}");
        at(516., false);
        assert_eq!(row(1, vec![7]), Some((vec![7], 1.)));
    }

    #[test]
    fn the_row_keeps_its_names_while_it_fades_out() {
        reset_motion();
        reset_for_tests();
        at(0., false);
        let _ = row(2, vec![7, 8]);
        at(600., false);
        assert_eq!(row(2, vec![7, 8]), Some((vec![7, 8], 1.)));
        at(616., false);
        let (names, opacity) = row(2, Vec::new()).expect("fading");
        assert_eq!(names, vec![7, 8]);
        assert_eq!(opacity, 1.);
        at(866., false);
        let (_, opacity) = row(2, Vec::new()).expect("fading");
        assert!((opacity - 0.5).abs() < 0.01, "{opacity}");
        at(1116., false);
        assert_eq!(row(2, Vec::new()), None);
    }

    #[test]
    fn reduced_motion_shows_and_hides_at_once() {
        reset_motion();
        reset_for_tests();
        at(0., true);
        assert_eq!(row(3, vec![7]), Some((vec![7], 1.)));
        at(16., true);
        assert_eq!(row(3, Vec::new()), None);
    }
}
