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
pub(in crate::ui) use reminder::clip_session_key;
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
    /// Module 633906 — Clips Expressive / Mode Picker Coachmark Modal.
    pub coachmark: Option<ClipsCoachmarkState>,
}

/// Module 633906 modal variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(in crate::ui) enum ClipsCoachmarkVariant {
    ModePicker,
    ExpressiveNonNitro,
    ExpressiveNitro,
}

/// Module 633906 / 818349 capture mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ClipsCaptureMode {
    Off,
    Manual,
    Auto,
}

impl ClipsCaptureMode {
    pub(in crate::ui) fn from_settings(clips_enabled: bool, enable_autoclipping: bool) -> Self {
        if !clips_enabled {
            Self::Off
        } else if enable_autoclipping {
            Self::Auto
        } else {
            Self::Manual
        }
    }

    pub(in crate::ui) fn to_settings(self) -> (bool, bool) {
        match self {
            Self::Off => (false, false),
            Self::Manual => (true, false),
            Self::Auto => (true, true),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ClipsCoachmarkState {
    pub variant: ClipsCoachmarkVariant,
    pub mode: ClipsCaptureMode,
    pub keybind_label: String,
    pub save_error: bool,
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

    /// Module 633906 — Clips Expressive / Mode Picker Coachmark Modal:
    /// supports `"mode_picker"` (3-option capture mode radio cards: Off / Manual / Auto with Nitro
    /// gradient accent), `"expressive_non_nitro"`, and `"expressive_nitro"` (2-column feature cards).
    #[allow(dead_code)]
    pub(in crate::ui) fn open_clips_coachmark(
        &mut self,
        variant: ClipsCoachmarkVariant,
        cx: &mut Context<Self>,
    ) {
        let (enabled, auto) = self.clips.clips_capture_mode();
        let mode = ClipsCaptureMode::from_settings(enabled, auto);
        let art_url = "https://discord.com/assets/cc9b98ec3d958835.svg";
        self.request_image(art_url.to_owned(), art_url.to_owned());
        self.clips.engine.ui.coachmark = Some(ClipsCoachmarkState {
            variant,
            mode,
            keybind_label: "Alt+C".to_owned(),
            save_error: false,
        });
        cx.notify();
    }

    pub(in crate::ui) fn clips_coachmark_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.clips.engine.ui.coachmark.as_ref()?;
        let variant = state.variant;
        let mode = state.mode;
        let is_picker = variant == ClipsCoachmarkVariant::ModePicker;
        let heading = if variant == ClipsCoachmarkVariant::ExpressiveNitro
            || (is_picker && mode == ClipsCaptureMode::Auto)
        {
            clips_coach_intl("O9OeEg", "When your squad loses it, Auto Clips saves it.")
        } else {
            clips_coach_intl("Ggh4C0", "When your squad loses it, Clips saves it.")
        };
        let subtitle = match variant {
            ClipsCoachmarkVariant::ModePicker => clips_coach_intl(
                "BtyRCT",
                "Save the clutch plays and the chaos while you game with friends. As a Nitro member, you get Auto Clips first.",
            ),
            ClipsCoachmarkVariant::ExpressiveNonNitro => clips_coach_intl(
                "kWigdu",
                "Get early access to Clips & Auto Clips with Nitro. Save the clutch plays and the chaos while you game with friends, no extra software needed.",
            ),
            ClipsCoachmarkVariant::ExpressiveNitro => clips_coach_intl(
                "M8rOjI",
                "You're in early: Auto Clips are on for you with Nitro. We'll clip the clutch plays and the chaos while you game with friends, no keybinds required.",
            ),
        };
        let art_url = "https://discord.com/assets/cc9b98ec3d958835.svg";
        let art = self.images.get(art_url).cloned();
        let manual_desc = if state.keybind_label.is_empty() {
            clips_coach_intl(
                "VfTZ7q",
                "Set a keybind to save a Clip. Keybind can be customized anytime.",
            )
        } else if dc_intl::has_key("qYS+JH") {
            dc_intl::format(
                "qYS+JH",
                &[("keybind", state.keybind_label.as_str().into())],
            )
        } else {
            format!(
                "Press {} to save a Clip. Keybind can be customized anytime.",
                state.keybind_label
            )
        };
        let mode_card = |id: &'static str,
                         target: ClipsCaptureMode,
                         glyph: &'static str,
                         title: String,
                         desc: String,
                         nitro_accent: bool| {
            let selected = mode == target;
            div()
                .id(id)
                .flex_1()
                .min_w(px(0.))
                .p(px(16.))
                .rounded(px(12.))
                .border_2()
                .border_color(if selected {
                    if nitro_accent {
                        rgba(0xeb459eff)
                    } else {
                        color("--control-primary-background-default")
                    }
                } else {
                    color("--border-subtle")
                })
                .bg(if selected && nitro_accent {
                    gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0xeb459e33), 0.),
                        gpui::linear_color_stop(rgba(0x5865f233), 1.),
                    )
                } else {
                    color("--background-surface-high").into()
                })
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap(px(8.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(c) = this.clips.engine.ui.coachmark.as_mut() {
                        c.mode = target;
                        c.save_error = false;
                    }
                    cx.notify();
                }))
                .child(icon(
                    glyph,
                    22.,
                    if selected { "--white" } else { "--icon-muted" },
                ))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child(desc),
                )
        };

        let body_section: AnyElement = if is_picker {
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(13.))
                        .text_color(color("--text-subtle"))
                        .child(clips_coach_intl("utyPWV", "You can change this anytime in"))
                        .child(
                            div()
                                .id("clips-coachmark-open-settings")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .hover(|s| s.underline())
                                .child(clips_coach_intl("7kGdh5", "Clips Settings"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clips.engine.ui.coachmark = None;
                                    this.open_settings_page("clips", cx);
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .gap(px(12.))
                        .child(mode_card(
                            "clips-mode-off",
                            ClipsCaptureMode::Off,
                            "close",
                            clips_coach_intl("uO/2T5", "Clips Off"),
                            clips_coach_intl("l/u5Cq", "No Clips get saved."),
                            false,
                        ))
                        .child(mode_card(
                            "clips-mode-manual",
                            ClipsCaptureMode::Manual,
                            "clips_clips",
                            clips_coach_intl("2Uh8Zr", "Manual Clips Only"),
                            manual_desc,
                            false,
                        ))
                        .child(mode_card(
                            "clips-mode-auto",
                            ClipsCaptureMode::Auto,
                            "clips_auto",
                            clips_coach_intl("uEs0uv", "Auto & Manual"),
                            clips_coach_intl(
                                "MRWtw9",
                                "Saves your best moments using in-game events and voice activity. You can also use your keybind.",
                            ),
                            true,
                        )),
                )
                .when(state.save_error, |col| {
                    col.child(
                        div()
                            .text_size(px(12.))
                            .text_color(color("--text-feedback-critical"))
                            .child(clips_coach_intl(
                                "YXeYlM",
                                "We couldn't save your capture mode. Please try again.",
                            )),
                    )
                })
                .child(
                    div()
                        .text_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(format!(
                            "{} {}",
                            clips_coach_intl(
                                "03CMAJ",
                                "Clips runs on your device to help you capture your gaming moments. They stay on your computer until you share them."
                            ),
                            dc_intl::t("hvVgAZ")
                        )),
                )
                .into_any_element()
        } else {
            div()
                .w_full()
                .flex()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(16.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-muted"))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(icon("clips_auto", 20., "--brand-500"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(clips_coach_intl("pGoNG5", "Clips now clip themselves")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-subtle"))
                                .child(clips_coach_intl(
                                    "4Ta12C",
                                    "Auto Clips run on your PC, using in-game events and voice activity to save your best moments.",
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(16.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-muted"))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(icon("activity_game", 20., "--brand-500"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(clips_coach_intl("L0RzNn", "Works on thousands of PC games")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-subtle"))
                                .child(clips_coach_intl(
                                    "vngMNi",
                                    "Jump into your favorite multiplayer games and let Auto Clips handle the rest.",
                                )),
                        ),
                )
                .into_any_element()
        };

        let cta_label = if is_picker {
            clips_coach_intl("liWTC0", "Done")
        } else {
            clips_coach_intl("n9r54F", "Check It Out")
        };
        let is_expressive = variant == ClipsCoachmarkVariant::ExpressiveNonNitro;
        let pose = crate::ui::motion::modal_pose("clips-coachmark-modal");
        Some(
            deferred(
                div()
                    .id("clips-coachmark-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clips.engine.ui.coachmark = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("clips-coachmark-modal")
                            .occlude()
                            .relative()
                            .w(px(640.))
                            .rounded(px(12.))
                            .overflow_hidden()
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.32),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(28.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .flex()
                            .flex_col()
                            .child(
                                // Art banner (`art__vwHFq`, compact 168px)
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(168.))
                                    .bg(gpui::linear_gradient(
                                        135.,
                                        gpui::linear_color_stop(rgba(0x23253aff), 0.),
                                        gpui::linear_color_stop(rgba(0x47285eff), 1.),
                                    ))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(match art {
                                        Some(pic) => gpui::img(pic)
                                            .h(px(132.))
                                            .object_fit(gpui::ObjectFit::Contain)
                                            .into_any_element(),
                                        None => {
                                            icon("clips_auto", 52., "--white").into_any_element()
                                        }
                                    })
                                    .child(
                                        div()
                                            .id("clips-coachmark-close")
                                            .absolute()
                                            .top(px(10.))
                                            .right(px(10.))
                                            .size(px(32.))
                                            .rounded_full()
                                            .bg(rgba(0x00000066))
                                            .hover(|s| s.bg(rgba(0x00000099)))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("close", 18., "--white"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clips.engine.ui.coachmark = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(32.))
                                    .pt(px(20.))
                                    .pb(px(28.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(heading),
                                    )
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-subtle"))
                                            .child(subtitle),
                                    )
                                    .child(body_section)
                                    .when(variant == ClipsCoachmarkVariant::ExpressiveNitro, |col| {
                                        col.child(
                                            div()
                                                .text_center()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(clips_coach_intl(
                                                    "22Z9fP",
                                                    "Auto Clips is already on for you. You can change it to Keybind Only, or turn it off in Clips settings.",
                                                )),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("clips-coachmark-cta")
                                            .mt(px(4.))
                                            .w(px(360.))
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(if is_expressive {
                                                gpui::linear_gradient(
                                                    90.,
                                                    gpui::linear_color_stop(rgba(0xeb459eff), 0.),
                                                    gpui::linear_color_stop(rgba(0x5865f2ff), 1.),
                                                )
                                            } else {
                                                color("--brand-500").into()
                                            })
                                            .hover(|s| s.opacity(0.92))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(cta_label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if is_picker {
                                                    let (enabled, auto) = mode.to_settings();
                                                    this.clips
                                                        .set_clips_capture_mode(enabled, auto);
                                                    this.clips.engine.ui.coachmark = None;
                                                } else if is_expressive {
                                                    this.clips.engine.ui.coachmark = None;
                                                    this.open_purchase(false, cx);
                                                } else {
                                                    this.clips.engine.ui.coachmark = None;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .when(is_expressive, |col| {
                                        col.child(
                                            div()
                                                .text_center()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(clips_coach_intl(
                                                    "pMWdGA",
                                                    "Once you join Nitro, you can choose your capture mode in Clips settings.",
                                                )),
                                        )
                                    }),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_clips_coachmark(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.clips.engine.ui.coachmark = None;
                cx.notify();
                "clips-coachmark: closed".to_owned()
            }
            "non-nitro" => {
                self.open_clips_coachmark(ClipsCoachmarkVariant::ExpressiveNonNitro, cx);
                "clips-coachmark: non-nitro".to_owned()
            }
            "nitro" => {
                self.open_clips_coachmark(ClipsCoachmarkVariant::ExpressiveNitro, cx);
                "clips-coachmark: nitro".to_owned()
            }
            _ => {
                self.open_clips_coachmark(ClipsCoachmarkVariant::ModePicker, cx);
                if let Some(c) = self.clips.engine.ui.coachmark.as_mut() {
                    c.mode = ClipsCaptureMode::Auto;
                }
                cx.notify();
                "clips-coachmark: mode-picker".to_owned()
            }
        }
    }
}

fn clips_coach_intl(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        dc_intl::t(key).to_owned()
    } else {
        fallback.to_owned()
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

    #[test]
    fn clips_coachmark_633906_mode_roundtrip() {
        assert_eq!(
            ClipsCaptureMode::from_settings(false, false),
            ClipsCaptureMode::Off
        );
        assert_eq!(
            ClipsCaptureMode::from_settings(true, false),
            ClipsCaptureMode::Manual
        );
        assert_eq!(
            ClipsCaptureMode::from_settings(true, true),
            ClipsCaptureMode::Auto
        );
        assert_eq!(ClipsCaptureMode::Off.to_settings(), (false, false));
        assert_eq!(ClipsCaptureMode::Manual.to_settings(), (true, false));
        assert_eq!(ClipsCaptureMode::Auto.to_settings(), (true, true));
        assert!(dc_intl::has_key("hvVgAZ"));
    }
}
