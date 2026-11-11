//! The parts of Clips the person sees besides the settings page: the Quick Bar above the account panel, the gallery
//! modal and the editor inside it (modules 596339, 657766, 423699). The capture and the save are
//! `settings::clips::engine`.
mod actions;
mod editor;
mod editor_view;
mod gallery;
mod gallery_view;
mod model;
mod panels;
mod player;
mod reminder;
mod widgets;

use super::*;
pub(in crate::ui) use actions::ShareClip;
pub(in crate::ui) use gallery::Facet;
use gpui::deferred;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// The modal's key in the layer stack (module 696016 `nm`).
const MODAL_ID: &str = "clips-gallery";

/// The Clips views own state.
#[derive(Default)]
pub(in crate::ui) struct State {
    /// The Quick Bar store `isOpen` (module 227628 `MZ`): opened by a save while a game is visible, closed when the
    /// bar has nothing to show.
    pub quick_bar_open: bool,
    /// Thumbnails by clip id, decoded from the clips data URLs when first drawn.
    pub pictures: RefCell<HashMap<String, Option<Arc<gpui::RenderImage>>>>,
    /// The gallery modal (module 657766), open while a clip is edited or the gallery is shown.
    pub modal: Option<Modal>,
    /// What the last write of a clip's document did, for the harness.
    pub last_write: String,
    /// What the last export or Show in Folder did, for the harness.
    pub last_export: String,
    /// The clip a Delete dialog asks about.
    pub confirm_delete: Option<String>,
    /// The editor's More menu.
    pub menu: actions::Menu,
    /// The reminder panel store (module 468550).
    pub reminder: Option<reminder::Reminder>,
}

/// The gallery modal: the editor panel slides over the gallery grid. Opened straight into the editor (the Quick Bar's
/// thumbnails), closing the editor closes the modal; opened from the grid, it goes back to the grid
/// (`initialEditingClipId`, module 657766 `e3`).
pub(in crate::ui) struct Modal {
    pub editor: Option<editor::Editor>,
    pub direct: bool,
    /// The gallery behind the editor; none when the modal opened straight into the editor.
    pub gallery: Option<gallery::State>,
}

impl Root {
    /// Runs once a second beside the capture sync: the Quick Bar closes when it has nothing to show (module 596339
    /// `I7`: it calls `closeQuickBar()` unless it is open, reminders are on and the game has clips).
    pub(in crate::ui) fn clips_ui_tick(&mut self, cx: &mut Context<Self>) {
        self.clips_reminder_tick(cx);
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

    /// The gallery modal is open (a dialog for the keyboard layer's counting).
    pub(in crate::ui) fn clips_modal_open(&self) -> bool {
        self.clips.engine.ui.modal.is_some() || self.clips.engine.ui.confirm_delete.is_some()
    }

    /// Opens the editor on `clip` (module 271597 `p` with `initialEditingClipId`): the modal opens straight into it.
    pub(in crate::ui) fn open_clip_editor(&mut self, clip: dc_clips::Clip, cx: &mut Context<Self>) {
        self.clips_close_menus();
        let editor = editor::Editor::open(&clip, cx);
        self.clips.engine.ui.modal = Some(Modal {
            editor: Some(editor),
            direct: true,
            gallery: None,
        });
        self.clips_start_polling(cx);
        cx.notify();
    }

    /// Closes the modal; an edit that changed something is written to the clip first.
    pub(in crate::ui) fn close_clips_modal(&mut self, cx: &mut Context<Self>) {
        self.clips_close_editor(cx);
        self.clips.engine.ui.modal = None;
        cx.notify();
    }

    /// `e3`: closing the editor closes the modal when it was opened into the editor, else returns to the grid.
    fn clips_editor_closed(&mut self, cx: &mut Context<Self>) {
        let direct = self
            .clips
            .engine
            .ui
            .modal
            .as_ref()
            .is_none_or(|modal| modal.direct);
        self.clips_close_editor(cx);
        if direct {
            self.clips.engine.ui.modal = None;
        }
        cx.notify();
    }

    fn clips_close_menus(&mut self) {
        self.clips_close_menu();
    }

    /// The harness steps of the Clips views: `clips open [index|id]` opens the editor on a clip of the library (the
    /// first by default) and `clips editor` reports its state; `clips close` closes the modal as its close button does.
    #[cfg(feature = "capture")]
    pub(in crate::ui) fn clips_script_ui(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> Option<String> {
        match words {
            ["open", rest @ ..] => {
                // Newest first, as the gallery lists them.
                let mut clips: Vec<&dc_clips::Clip> = self.clips.engine.clips.values().collect();
                clips.sort_by_key(|clip| std::cmp::Reverse(clip.meta.created_at));
                let clip = match rest.first() {
                    None => clips.first().copied(),
                    Some(which) => clips
                        .iter()
                        .copied()
                        .find(|clip| clip.meta.id == *which)
                        .or_else(|| {
                            which
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| clips.get(index).copied())
                        }),
                }
                .cloned();
                Some(match clip {
                    Some(clip) => {
                        let id = clip.meta.id.clone();
                        self.open_clip_editor(clip, cx);
                        format!("opened {id}")
                    }
                    None => "no such clip".to_owned(),
                })
            }
            ["editor"] => Some(format!(
                "{} | last write: {} | last export: {}",
                self.clips_editor_status(),
                self.clips.engine.ui.last_write,
                self.clips.engine.ui.last_export
            )),
            ["gallery", rest @ ..] => {
                let facet = match rest.first() {
                    None | Some(&"all") => Facet::All,
                    Some(&"favourites") => Facet::Favorites,
                    Some(game) => Facet::Game((*game).to_owned()),
                };
                self.clips_open_gallery(facet, cx);
                Some("gallery".to_owned())
            }
            ["appid", id] => {
                if let Some(game) = self.clips.engine.game.as_mut() {
                    game.application_id = Some((*id).to_owned());
                }
                Some(format!("appid {id}"))
            }
            ["reminder", "open", id] => {
                self.clips_session_stop(Some((*id).to_owned()), cx);
                Some(format!("reminder {}", self.clips_reminder_open()))
            }
            ["reminder", "age", ms] => {
                if let (Some(reminder), Ok(ms)) =
                    (self.clips.engine.ui.reminder.as_mut(), ms.parse::<f64>())
                {
                    reminder.opened_at = motion::now() - ms;
                    reminder.paused_ms = 0.;
                }
                Some("reminder aged".to_owned())
            }
            ["reminder"] => Some(format!("reminder open {}", self.clips_reminder_open())),
            ["grid"] => Some(self.clips_grid_status()),
            ["close"] => {
                self.close_clips_modal(cx);
                Some("closed".to_owned())
            }
            _ => None,
        }
    }

    /// A key press while the modal is open. True when the modal took it.
    pub(in crate::ui) fn clips_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Share Clip (the forward modal) sits above the editor and has its own keys.
        if self.forward.is_some() {
            return false;
        }
        let escape = event.keystroke.key == "escape";
        // The delete dialog is above the editor and takes Escape first (its Enter reaches its focused button).
        if self.clips.engine.ui.confirm_delete.is_some() {
            if escape {
                self.clips.engine.ui.confirm_delete = None;
                cx.notify();
                return true;
            }
            return false;
        }
        if self.clips.engine.ui.modal.is_none() {
            return false;
        }
        if escape && self.clips_menu_open() {
            self.clips_close_menu();
            cx.notify();
            return true;
        }
        let grid_only = self
            .clips
            .engine
            .ui
            .modal
            .as_ref()
            .is_some_and(|modal| modal.editor.is_none());
        if grid_only {
            // The grid: Escape closes the modal.
            if escape {
                self.close_clips_modal(cx);
                return true;
            }
            return false;
        }
        self.clips_editor_key(event, window, cx)
    }

    /// The pointer moved with a drag possibly under way.
    pub(in crate::ui) fn clips_mouse_move(
        &mut self,
        event: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if self.clips.engine.ui.modal.is_some() {
            self.clips_editor_pointer(event.position, event.pressed_button, cx);
        }
    }

    /// The primary button was let go.
    pub(in crate::ui) fn clips_mouse_up(&mut self, cx: &mut Context<Self>) {
        if self.clips.engine.ui.modal.is_some() {
            self.clips_editor_release(cx);
        }
    }

    /// The modal's layer: the scrim and the box (`outerContainer`/`modal` in stylesheet F9ieB: the viewport less 40 px on
    /// each side, 1400 wide at most; full-bleed when the window is 1080 wide or less).
    pub(in crate::ui) fn clips_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.clips.engine.ui.modal.as_ref()?;
        let (width, height, full) = modal_size(self.viewport);
        let pose = motion::modal_pose(MODAL_ID);
        let content = match (modal.editor.as_ref(), modal.gallery.as_ref()) {
            (Some(editor), _) => self.clips_editor_view(editor, (width, height), window, cx),
            (None, Some(state)) => self.clips_gallery_view(state, (width, height), cx),
            (None, None) => div().into_any_element(),
        };
        let shell = div()
            .id(MODAL_ID)
            .parity_with_border("clips-modal", 1.)
            .occlude()
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                this.clips_mouse_move(event, cx);
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                    this.clips_mouse_up(cx);
                }),
            )
            .when(!full, |this| {
                // `container_dvN-MP`: 1px `--border-subtle`, `--radius-md`, `--shadow-high`.
                this.border_1()
                    .border_color(color("--border-subtle"))
                    .rounded(px(12.))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            })
            .child(content);
        Some(
            deferred(
                div()
                    .id("clips-gallery-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::Rgba {
                        a: f32::from(0xb8u8) / 255. * pose.scrim,
                        ..rgba(0x000000ff)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    // A drag in the editor goes on while the pointer leaves the strip: the layer covers the window, so
                    // it sees every move (the root's own listener is blocked by this occluding layer).
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        this.clips_mouse_move(event, cx);
                    }))
                    // A drag let go over the scrim ends there too.
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                            this.clips_mouse_up(cx);
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.close_clips_modal(cx)))
                    .child(shell),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// The modal's width, height and whether it is full-bleed (stylesheet F9ieB: `--custom-viewport-padding` 40 px a
/// side below 1600 px wide; at 1080 px or less the modal fills the window).
pub(in crate::ui) fn modal_size(viewport: (f32, f32)) -> (f32, f32, bool) {
    let (viewport_width, viewport_height) = viewport;
    if viewport_width <= 1080. {
        return (viewport_width, viewport_height, true);
    }
    let padding = 40.;
    (
        (viewport_width - padding * 2.).min(1400.),
        viewport_height - padding * 2.,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modal_leaves_forty_pixels_and_fills_a_narrow_window() {
        assert_eq!(modal_size((1280., 720.)), (1200., 640., false));
        assert_eq!(modal_size((2000., 1200.)), (1400., 1120., false));
        assert_eq!(modal_size((1000., 700.)), (1000., 700., true));
    }
}
