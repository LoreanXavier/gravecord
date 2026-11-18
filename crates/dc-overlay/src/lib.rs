//! The game overlay (Overlay v3, the out-of-process overlay of the pinned desktop build).
//!
//! The official client draws the overlay in a frameless, transparent, always-on-top Electron window of its own
//! (`DISCORD_OutOfProcessOverlay`, shown with `showInactive`) that the native module `discord_desktop_overlay` keeps
//! over the focused game and makes click-through while input is locked (`setInteractionEnabled`, module 222506). The
//! legacy overlay injects a hook DLL instead (`discord_overlay2`); gravecord does not inject (`BLOCKED.md`).
//!
//! - [`decision`]: when the overlay is drawn over a game (`OverlayRenderStore` `Q`, module 861862; the per-game
//!   enablement `eY` of `RunningGameStore`, module 397369).
//! - [`store`]: the persisted settings under their official keys.
//! - [`layout`]: the widget layout (`LayoutStore`, module 83880) and its anchor maths (module 5463).
//! - [`voice`]: the voice widget's ordering and visibility rules (module 502812).
//! - [`drag`]: moving and resizing a widget while unlocked (modules 5463, 129537).
//! - [`notify`]: the notifications widget's queue, timers and stacking (modules 348639, 415173, 57019).
//! - [`zones`]: the clickable regions of a locked overlay (module 302614).
//! - [`widgets`]: the widget registry, the Widgets panel's list and the chat experiment (modules 776162, 415173,
//!   589051).
//! - [`menus`]: the voice and notifications widgets' Settings menus (modules 51444, 320490).
//! - [`text_chat`]: `Overlay-v3-Text-Chat-Store` (module 288641), the Chat widget's chats.
//! - [`video`]: the Video and Go Live widgets' tiles and lists (module 415173 `iS`, `e2`; module 163432).
//! - `platform` (Windows): the window styles, the foreground watcher, the click-through switch and the desktop check.

pub mod decision;
pub mod drag;
pub mod layout;
pub mod menus;
pub mod notify;
#[cfg(windows)]
pub mod platform;
pub mod store;
pub mod text_chat;
pub mod video;
pub mod voice;
pub mod widgets;
pub mod zones;
