//! Which keys and mouse buttons are down, and whether they make up a keybind. Pure state: the Windows hook feeds it
//! raw events and nothing here touches the system.
//!
//! A keybind is the official `[deviceType, code, platform]` list (module 650583): keyboard codes are Windows virtual
//! keys, mouse codes are the native input's button numbers. Push to talk is held while every part of the keybind is
//! down (`CC` in module 596339 tracks held binds), and released as soon as one is let go.

/// `KEYBOARD_KEY`, `MOUSE_BUTTON`, `KEYBOARD_MODIFIER_KEY`, `GAMEPAD_BUTTON` (650583 `zY`).
pub const KEYBOARD_KEY: u32 = 0;
pub const MOUSE_BUTTON: u32 = 1;
pub const KEYBOARD_MODIFIER_KEY: u32 = 2;
pub const GAMEPAD_BUTTON: u32 = 3;

/// One part of a keybind: `[deviceType, code, platform]`.
pub type Part = [u32; 3];

/// Left and right of each modifier: shift, control, alt, windows. The recorder stores the left one (a window cannot
/// tell them apart), so either side satisfies it.
const MODIFIER_PAIRS: [(u32, u32); 4] = [(160, 161), (162, 163), (164, 165), (91, 92)];

fn sibling(code: u32) -> Option<u32> {
    MODIFIER_PAIRS.iter().find_map(|&(left, right)| {
        if code == left {
            Some(right)
        } else if code == right {
            Some(left)
        } else {
            None
        }
    })
}

#[derive(Clone, Debug, Default)]
pub struct Matcher {
    shortcut: Vec<Part>,
    keys: Vec<u32>,
    buttons: Vec<u32>,
    held: bool,
}

impl Matcher {
    pub fn new(shortcut: Vec<Part>) -> Self {
        Self {
            shortcut,
            ..Self::default()
        }
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    /// A new keybind. Returns the new held state when it changed.
    pub fn set_shortcut(&mut self, shortcut: Vec<Part>) -> Option<bool> {
        self.shortcut = shortcut;
        self.update()
    }

    /// A key went down or up (a Windows virtual key code).
    pub fn key(&mut self, code: u32, down: bool) -> Option<bool> {
        set(&mut self.keys, code, down);
        self.update()
    }

    /// A mouse button went down or up (the native button number).
    pub fn button(&mut self, code: u32, down: bool) -> Option<bool> {
        set(&mut self.buttons, code, down);
        self.update()
    }

    /// Forget everything that was down, as when the hook stops.
    pub fn release_all(&mut self) -> Option<bool> {
        self.keys.clear();
        self.buttons.clear();
        self.update()
    }

    fn key_down(&self, code: u32) -> bool {
        self.keys.contains(&code) || sibling(code).is_some_and(|other| self.keys.contains(&other))
    }

    fn update(&mut self) -> Option<bool> {
        let held = !self.shortcut.is_empty()
            && self.shortcut.iter().all(|[kind, code, _]| match *kind {
                KEYBOARD_KEY | KEYBOARD_MODIFIER_KEY => self.key_down(*code),
                MOUSE_BUTTON => self.buttons.contains(code),
                // No gamepad input is read.
                _ => false,
            });
        (std::mem::replace(&mut self.held, held) != held).then_some(held)
    }
}

fn set(list: &mut Vec<u32>, code: u32, down: bool) {
    match (down, list.iter().position(|held| *held == code)) {
        (true, None) => list.push(code),
        (false, Some(at)) => {
            list.remove(at);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: u32 = 162;
    const F9: u32 = 120;

    fn ctrl_f9() -> Matcher {
        Matcher::new(vec![[KEYBOARD_KEY, CTRL, 1], [KEYBOARD_KEY, F9, 1]])
    }

    #[test]
    fn held_while_every_part_is_down_and_released_with_the_first_to_go() {
        let mut matcher = ctrl_f9();
        assert_eq!(matcher.key(CTRL, true), None);
        assert_eq!(matcher.key(F9, true), Some(true));
        assert!(matcher.is_held());
        // Key repeat sends more downs: no new change.
        assert_eq!(matcher.key(F9, true), None);
        assert_eq!(matcher.key(65, true), None, "other keys do not matter");
        assert_eq!(matcher.key(CTRL, false), Some(false));
        assert_eq!(matcher.key(CTRL, true), Some(true), "back again");
        assert_eq!(matcher.key(F9, false), Some(false));
    }

    #[test]
    fn either_side_of_a_modifier_counts() {
        let mut matcher = ctrl_f9();
        matcher.key(163, true);
        assert_eq!(matcher.key(F9, true), Some(true));
        assert_eq!(matcher.key(163, false), Some(false));
        // Left satisfies a keybind recorded on the right, too.
        let mut right = Matcher::new(vec![[KEYBOARD_MODIFIER_KEY, 163, 1]]);
        assert_eq!(right.key(162, true), Some(true));
    }

    #[test]
    fn mouse_buttons_and_keys_combine() {
        let mut matcher = Matcher::new(vec![[MOUSE_BUTTON, 3, 1]]);
        assert_eq!(matcher.button(3, true), Some(true));
        assert_eq!(matcher.button(3, false), Some(false));
        let mut both = Matcher::new(vec![[KEYBOARD_KEY, CTRL, 1], [MOUSE_BUTTON, 4, 1]]);
        both.button(4, true);
        assert!(!both.is_held());
        assert_eq!(both.key(CTRL, true), Some(true));
        // A key code is not a button number.
        assert_eq!(both.button(4, false), Some(false));
        assert_eq!(both.key(4, true), None);
    }

    #[test]
    fn an_empty_keybind_and_a_gamepad_part_are_never_held() {
        let mut empty = Matcher::new(Vec::new());
        assert_eq!(empty.key(CTRL, true), None);
        let mut pad = Matcher::new(vec![[GAMEPAD_BUTTON, 1, 1]]);
        assert_eq!(pad.button(1, true), None);
        assert!(!pad.is_held());
    }

    #[test]
    fn a_new_keybind_takes_what_is_already_down_and_release_all_lets_go() {
        let mut matcher = Matcher::new(Vec::new());
        matcher.key(CTRL, true);
        matcher.key(F9, true);
        assert_eq!(
            matcher.set_shortcut(vec![[KEYBOARD_KEY, CTRL, 1], [KEYBOARD_KEY, F9, 1]]),
            Some(true)
        );
        assert_eq!(matcher.release_all(), Some(false));
        assert_eq!(matcher.release_all(), None);
        assert_eq!(matcher.key(F9, true), None, "the old state is gone");
    }
}
