//! The parts of Clips the person sees besides the settings page: the Quick Bar above the account panel and, later,
//! the reminder, the gallery and the editor (modules 596339, 657766, 423699). The capture and the save are
//! `settings::clips::engine`.
mod panels;

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// The Clips views own state.
#[derive(Default)]
pub(in crate::ui) struct State {
    /// The Quick Bar store `isOpen` (module 227628 `MZ`): opened by a save while a game is visible, closed when the
    /// bar has nothing to show.
    pub quick_bar_open: bool,
    /// Thumbnails by clip id, decoded from the clips data URLs when first drawn.
    pub pictures: RefCell<HashMap<String, Option<Arc<gpui::RenderImage>>>>,
}

impl Root {
    /// Runs once a second beside the capture sync: the Quick Bar closes when it has nothing to show (module 596339
    /// `I7`: it calls `closeQuickBar()` unless it is open, reminders are on and the game has clips).
    pub(in crate::ui) fn clips_ui_tick(&mut self, cx: &mut Context<Self>) {
        if self.clips.engine.ui.quick_bar_open && !self.clips_quick_bar_has_clips() {
            self.clips.engine.ui.quick_bar_open = false;
            cx.notify();
        }
    }

    /// `maybeOpenQuickBar` (module 516130), run for each saved clip: opens the bar when reminders are on and the
    /// visible game has a clip in this session.
    pub(in crate::ui) fn clips_after_save(&mut self, cx: &mut Context<Self>) {
        if self.clips_quick_bar_has_clips() {
            self.clips.engine.ui.quick_bar_open = true;
            cx.notify();
        }
    }
}
