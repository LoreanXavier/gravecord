//! The shortcuts that work on the message that has keyboard focus: `tj` in module 891522, the ones the Keyboard
//! Shortcuts help lists under "Messages" (158781), and the focus the message list gives them (116903, 891522).
//!
//! Official rules this follows:
//! - The handler runs only in keyboard mode and only when the message itself has focus (`target === currentTarget`),
//!   never while a field has it.
//! - E edits (own messages), Backspace deletes (with the confirmation dialog), P pins (Shift skips the
//!   confirmation), + opens the reaction picker, R replies, F forwards, S speaks a message that has text, T starts
//!   or opens a thread, Ctrl+C copies the text, Alt+Enter marks unread from here and Escape gives the text area
//!   focus. Each acts as the same item of the message's context menu does, and is offered only when that menu offers
//!   it.
//! - Up and Down move between the list's items (messages and day dividers); Down from the last one goes to the text
//!   area; Home and End scroll to the start or the end and focus the first or the last item.

use super::Keystroke;
use crate::ui::Root;
use crate::ui::menu::{Action, Anchor, Item, OpenMenu, SavedState, message_items_saved};
use dc_state::Id;
use gpui::{Context, FocusHandle, Window, div, prelude::*, px, rgba};

/// A shortcut that was pressed, and whether Shift was down with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pressed {
    pub action: Action,
    pub shift: bool,
}

/// The actions a message's context menu offers, which is what the shortcuts may do.
pub(super) fn offered(own: bool, saved: SavedState) -> Vec<Action> {
    message_items_saved(own, saved)
        .into_iter()
        .filter_map(|item| match item {
            Item::Entry(_, _, action, _) | Item::Danger(_, _, action) => Some(action),
            Item::Separator => None,
        })
        .collect()
}

/// What a key means to a focused message (the `switch` of `tj`), when the menu offers it.
pub(super) fn pressed(
    keystroke: &Keystroke,
    offered: &[Action],
    has_text: bool,
) -> Option<Pressed> {
    let modifiers = keystroke.modifiers;
    let none = !modifiers.alt && !modifiers.control && !modifiers.platform && !modifiers.shift;
    let alt = modifiers.alt && !modifiers.control && !modifiers.platform && !modifiers.shift;
    let ctrl = modifiers.control && !modifiers.alt && !modifiers.platform && !modifiers.shift;
    let shift = modifiers.shift && !modifiers.alt && !modifiers.control && !modifiers.platform;
    let key = keystroke.key.to_lowercase();
    let (action, with_shift) = match key.as_str() {
        "backspace" if none => (Action::Delete, false),
        "c" if ctrl => (Action::CopyText, false),
        "e" if none => (Action::Edit, false),
        "p" if none || shift => (Action::Pin, shift),
        // `+` is Shift and `=` on a US layout.
        "+" | "=" if none || shift => (Action::AddReaction, shift),
        "r" if none || shift => (Action::Reply, shift),
        "f" if none || shift => (Action::Forward, shift),
        "s" if none && has_text => (Action::Speak, false),
        "t" if none => (Action::Thread, false),
        "enter" if alt => (Action::MarkUnread, false),
        _ => return None,
    };
    // `=` without Shift is not the plus key.
    if key == "=" && !modifiers.shift {
        return None;
    }
    offered.contains(&action).then_some(Pressed {
        action,
        shift: with_shift,
    })
}

/// The request a shortcut sends without a dialog: pinning with Shift held (module 891522, `rS`). Delete always asks
/// first, so it has none.
pub(super) fn unconfirmed(pressed: Pressed) -> bool {
    pressed.action == Action::Pin && pressed.shift
}

impl Root {
    /// A key with a message focused. True when it was a message shortcut.
    pub(super) fn message_key(
        &mut self,
        keystroke: &Keystroke,
        focused: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.keys.keyboard_modality {
            return false;
        }
        let Some(message) = self.keys.stops.focused_message(focused) else {
            return false;
        };
        if keystroke.key == "escape" {
            // `TEXTAREA_FOCUS`; the Escape chain still runs after it.
            self.focus_composer(window, cx);
            return false;
        }
        let Some(channel) = self.channel else {
            return false;
        };
        let Some(known) = self
            .stores
            .messages
            .get(&channel)
            .and_then(|messages| messages.iter().find(|known| known.id == message))
        else {
            return false;
        };
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.id == known.author.id);
        let has_text = !known.content.is_empty();
        let offered = offered(own, self.saved_flags(message));
        let Some(pressed) = pressed(keystroke, &offered, has_text) else {
            return false;
        };
        if pressed.action == Action::AddReaction {
            // The picker opens beside the message, where the hover bar's own button would put it.
            let bounds = self
                .tip_anchors
                .borrow()
                .get(format!("message-box-{message}").as_str())
                .copied();
            if let Some(bounds) = bounds {
                self.last_menu = Some(OpenMenu {
                    anchor: Anchor::MoreButton(gpui::point(
                        bounds.origin.x + bounds.size.width - px(48.),
                        bounds.origin.y,
                    )),
                    message,
                    own,
                });
            }
        }
        if unconfirmed(pressed) {
            self.run_unconfirmed(pressed.action, message, cx);
        } else {
            self.run_action(pressed.action, message, window, cx);
        }
        cx.notify();
        true
    }

    /// Makes the message row a focus stop of the message list: one tab stop for the list, the arrows between the
    /// items, and the ring the message gets in keyboard mode (`FocusRing` with a 4px side offset, 491182).
    pub(in crate::ui) fn message_stop<E: InteractiveElement + gpui::ParentElement>(
        &self,
        element: E,
        message: Id,
        row: usize,
        cx: &mut gpui::App,
    ) -> E {
        let key = format!("message-{message}");
        let handle = self.stop(&key, cx);
        self.join_member(
            "messages",
            super::Nav::List,
            &handle,
            Some(row),
            Some(message),
        );
        let focused = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        // The arrows reach the other messages; the list's one tab stop is its entry.
        let element = super::stops::tab_stop(element, &handle, false);
        // The box the reaction picker and Shift+F10's menu open beside (`message-box-{id}`, read in `message_key`).
        let element = element.child(self.anchor_probe(format!("message-box-{message}")));
        if focused {
            element.child(ring_overlay())
        } else {
            element
        }
    }

    /// The invisible tab stop in front of the message list: the official list is one tab stop (`tabIndex 0` on the
    /// `ol`), and focusing it moves focus to the first message in view (116903).
    pub(in crate::ui) fn messages_entry(&self, cx: &mut gpui::App) -> impl IntoElement {
        let handle = self.stop("messages-entry", cx);
        super::stops::tab_stop(
            div().id("messages-entry").absolute().size(px(0.)),
            &handle,
            self.modal_open() == 0,
        )
    }

    /// Tab landed on the message list's entry: focus moves on to the first message fully in view, or the newest one
    /// when the view is at the end. True when it moved.
    pub(super) fn enter_messages(
        &mut self,
        focused: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if *focused != self.stop("messages-entry", cx) {
            return false;
        }
        let first = self.keys.stops.first_row.get();
        let landing = self.keys.stops.entry_message(if first >= self.rows.len() {
            None
        } else {
            Some(first)
        });
        match landing {
            Some((handle, row)) => {
                window.focus(&handle, cx);
                self.list.scroll_to_reveal_item(row);
                true
            }
            None => false,
        }
    }
}

/// The hollow ring of a message: the 4px ring of the official focus ring, around the article inset 4px at each side.
fn ring_overlay() -> impl IntoElement {
    div()
        .absolute()
        .left(px(0.))
        .right(px(0.))
        .top(px(-4.))
        .bottom(px(-4.))
        .rounded(px(8.))
        .border(px(4.))
        .border_color(rgba(0x00b0f4ff))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    fn all() -> Vec<Action> {
        offered(true, SavedState::default())
    }

    #[test]
    fn each_listed_shortcut_means_its_menu_item() {
        let all = all();
        let case = |key: &'static str| pressed(&stroke(key), &all, true).map(|p| p.action);
        assert_eq!(case("e"), Some(Action::Edit));
        assert_eq!(case("backspace"), Some(Action::Delete));
        assert_eq!(case("p"), Some(Action::Pin));
        assert_eq!(case("r"), Some(Action::Reply));
        assert_eq!(case("f"), Some(Action::Forward));
        assert_eq!(case("s"), Some(Action::Speak));
        assert_eq!(case("t"), Some(Action::Thread));
        assert_eq!(case("ctrl-c"), Some(Action::CopyText));
        assert_eq!(case("alt-enter"), Some(Action::MarkUnread));
        assert_eq!(case("shift-="), Some(Action::AddReaction));
        assert_eq!(case("+"), Some(Action::AddReaction));
    }

    #[test]
    fn delete_never_skips_its_confirmation() {
        let all = all();
        // `tj` takes Backspace only with no modifier, so Shift+Backspace does nothing, and Delete is never
        // an unconfirmed request.
        assert_eq!(pressed(&stroke("shift-backspace"), &all, true), None);
        let delete = pressed(&stroke("backspace"), &all, true).unwrap();
        assert!(!unconfirmed(delete));
    }

    #[test]
    fn pin_asks_unless_shift_is_held() {
        let all = all();
        let plain = pressed(&stroke("p"), &all, true).unwrap();
        let shifted = pressed(&stroke("shift-p"), &all, true).unwrap();
        assert!(!unconfirmed(plain));
        assert!(unconfirmed(shifted));
    }

    #[test]
    fn other_peoples_messages_cannot_be_edited_or_deleted() {
        let theirs = offered(false, SavedState::default());
        assert_eq!(pressed(&stroke("e"), &theirs, true), None);
        assert_eq!(pressed(&stroke("backspace"), &theirs, true), None);
        assert!(pressed(&stroke("r"), &theirs, true).is_some());
    }

    #[test]
    fn an_empty_message_is_not_spoken() {
        let all = all();
        assert_eq!(pressed(&stroke("s"), &all, false), None);
    }

    #[test]
    fn unrelated_modifiers_and_keys_do_nothing() {
        let all = all();
        for key in [
            "ctrl-e",
            "alt-p",
            "ctrl-r",
            "shift-e",
            "x",
            "enter",
            "ctrl-alt-c",
            "=",
        ] {
            assert_eq!(pressed(&stroke(key), &all, true), None, "{key}");
        }
    }

    #[test]
    fn mark_unread_is_offered_to_everyone() {
        let theirs = offered(false, SavedState::default());
        assert_eq!(
            pressed(&stroke("alt-enter"), &theirs, true).map(|p| p.action),
            Some(Action::MarkUnread)
        );
    }
}
