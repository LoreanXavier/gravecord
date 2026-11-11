//! Shift+F10: the context menu of the element that has keyboard focus.
//!
//! Official rule: `OPEN_CONTEXT_MENU` (module 158781) dispatches a `contextmenu` event on `document.activeElement`
//! (nothing happens when the body has focus) with the pointer at 0, 0. The menu opener (module 442433 `jA`) then has no
//! pointer position and places the menu at the centre of the target's bounding box (`left + width / 2`,
//! `top + height / 2`), or at the end of the text selection inside it.

use crate::ui::Root;
use crate::ui::context_menu::Target;
use crate::ui::menu::Anchor;
use dc_state::Id;
use gpui::{Bounds, Context, FocusHandle, Pixels, Point, Window};

/// What a focus stop is, by the key it was made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    Message(Id),
    Guild(Id),
    Channel(Id),
    Category(Id),
    Member(Id),
}

/// Reads a stop's key (`message-{id}`, `guild-{id}`, `channel-row-{id}`, `category-{id}`, `member-row-member-{id}`).
pub(super) fn classify(key: &str) -> Option<Stop> {
    let number = |rest: &str| rest.parse::<Id>().ok();
    if let Some(rest) = key.strip_prefix("member-row-member-") {
        return number(rest).map(Stop::Member);
    }
    if let Some(rest) = key.strip_prefix("channel-row-") {
        return number(rest).map(Stop::Channel);
    }
    if let Some(rest) = key.strip_prefix("category-") {
        return number(rest).map(Stop::Category);
    }
    if let Some(rest) = key.strip_prefix("guild-") {
        return number(rest).map(Stop::Guild);
    }
    if let Some(rest) = key.strip_prefix("message-") {
        return number(rest).map(Stop::Message);
    }
    None
}

/// The key the stop's bounds are recorded under (`anchor_probe`).
pub(super) fn bounds_key(stop: Stop) -> String {
    match stop {
        Stop::Message(id) => format!("message-box-{id}"),
        Stop::Guild(id) => format!("stop-guild-{id}"),
        Stop::Channel(id) => format!("capture-channel-{id}"),
        Stop::Category(id) => format!("stop-category-{id}"),
        Stop::Member(id) => format!("stop-member-{id}"),
    }
}

/// `rect = new DOMRect(left + width / 2, top + height / 2, 0, 0)`.
pub(super) fn centre(bounds: Bounds<Pixels>) -> Point<Pixels> {
    Point {
        x: bounds.origin.x + bounds.size.width / 2.,
        y: bounds.origin.y + bounds.size.height / 2.,
    }
}

impl Root {
    /// `OPEN_CONTEXT_MENU`. True when a menu opened.
    pub(super) fn open_focused_context_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(focused) = window.focused(cx) else {
            return false;
        };
        self.open_context_of(&focused, cx)
    }

    fn open_context_of(&mut self, focused: &FocusHandle, cx: &mut Context<Self>) -> bool {
        let Some(stop) = self
            .keys
            .stops
            .key_of(focused)
            .and_then(|key| classify(&key))
        else {
            return false;
        };
        let Some(bounds) = self
            .tip_anchors
            .borrow()
            .get(bounds_key(stop).as_str())
            .copied()
        else {
            return false;
        };
        let at = centre(bounds);
        match stop {
            Stop::Message(id) => {
                let Some(channel) = self.channel else {
                    return false;
                };
                let own = self
                    .stores
                    .messages
                    .get(&channel)
                    .and_then(|list| list.iter().find(|message| message.id == id))
                    .zip(self.stores.current_user.as_ref())
                    .is_some_and(|(message, me)| message.author.id == me.id);
                self.open_menu(Anchor::Pointer(at), id, own, cx);
            }
            Stop::Guild(id) => self.open_context_menu(Target::Guild(id), at, cx),
            Stop::Channel(id) => self.open_context_menu(Target::Channel(id), at, cx),
            Stop::Category(id) => self.open_context_menu(Target::Category(id), at, cx),
            Stop::Member(user) => {
                let guild = self.guild;
                self.open_context_menu(Target::User { user, guild }, at, cx);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn stop_keys_name_what_the_menu_is_for() {
        assert_eq!(classify("message-12"), Some(Stop::Message(12)));
        assert_eq!(classify("guild-7"), Some(Stop::Guild(7)));
        assert_eq!(classify("channel-row-9"), Some(Stop::Channel(9)));
        assert_eq!(classify("category-4"), Some(Stop::Category(4)));
        assert_eq!(classify("member-row-member-5"), Some(Stop::Member(5)));
        // Controls without a context menu, and keys that only look alike.
        assert_eq!(classify("messages-entry"), None);
        assert_eq!(classify("guild-events"), None);
        assert_eq!(classify("home"), None);
    }

    #[test]
    fn the_menu_opens_at_the_centre_of_the_focused_box() {
        let bounds = Bounds {
            origin: point(px(10.), px(20.)),
            size: size(px(100.), px(40.)),
        };
        assert_eq!(centre(bounds), point(px(60.), px(40.)));
    }

    #[test]
    fn every_stop_has_a_bounds_key() {
        for stop in [
            Stop::Message(1),
            Stop::Guild(1),
            Stop::Channel(1),
            Stop::Category(1),
            Stop::Member(1),
        ] {
            assert!(bounds_key(stop).ends_with('1'));
        }
    }
}
