//! System-wide push to talk: the keybind recorded on Voice & Video keeps working while another window has the focus.
//!
//! On Windows a low-level keyboard and mouse hook (`SetWindowsHookExW`) reads what is pressed, the way the official
//! client's native input does, and reports when the whole keybind is down. Nothing is swallowed and nothing is logged.
//! [`matcher`] holds the platform independent part.

pub mod idle;
pub mod matcher;
#[cfg(windows)]
mod windows;

pub use matcher::{
    GAMEPAD_BUTTON, KEYBOARD_KEY, KEYBOARD_MODIFIER_KEY, MOUSE_BUTTON, Matcher, Part,
};
#[cfg(windows)]
pub use windows::{Hook, start};
