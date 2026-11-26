//! LimitlessScreenshare (Equicord `src/equicordplugins/limitlessScreenshare`, GPL-3.0-or-later): every Go Live
//! resolution and frame rate open whatever the account is (`canStreamWithSettings` answers yes: no Nitro wheels, no
//! upsell, the Gaming and Screenshare presets at their top pairs), and in the Go Live picker's Screen Resolution and
//! Frame Rate menus and the running stream's Stream Quality menu a slider (3p to Max Resolution in 1p or 10p steps,
//! 1 to Max FPS) above the plugin's own preset lists. A choice off the official table is sent as it is: the capture
//! is scaled to that height and the encoder runs at that rate. The slider moves a running stream at most once a
//! second (`COOLDOWN_MS`). Not part of the pinned client.

use super::super::Root;
use super::super::go_live::{Prefs, Preset as Mode};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::limitless_screenshare::{self as rules, Presets, Throttle};
use dc_media::screen::{Resolution, Settings, Source};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div,
    prelude::*, px,
};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::time::Instant;

const ID: &str = "LimitlessScreenshare";

/// An add box of the card and what keeps it to digits.
type AddBox = (Entity<Editor>, Subscription);

thread_local! {
    /// When the running stream was last moved by a slider, and the value waiting for the cooldown.
    static LAST_MOVE: Cell<Option<Instant>> = const { Cell::new(None) };
    static PENDING: Cell<Option<(bool, u32)>> = const { Cell::new(None) };
    static SCHEDULED: Cell<bool> = const { Cell::new(false) };
    /// The add boxes of the settings card (frame rate, resolution), made the first time the card shows.
    static ADDING: RefCell<Option<[AddBox; 2]>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn number_of(state: &crate::plugins::State, key: &str) -> u32 {
    state.value(ID, key).as_f64().unwrap_or_default() as u32
}

fn round() -> bool {
    crate::plugins::with(|state| state.value(ID, "roundResolution") == Value::Bool(true))
}

/// A height of the lists or the slider as the picker's resolution: the official ones as themselves, 0 as Source.
fn resolution_of(height: u32) -> Resolution {
    Resolution::from_height(height).unwrap_or(Resolution::Custom(height))
}

/// `canStreamWithSettings` answers yes.
pub(in crate::ui) fn limitless_unlocked() -> bool {
    on()
}

/// The capture settings for any pair the encoder takes, while the plugin is on.
pub(in crate::ui) fn limitless_settings(
    source: &Source,
    resolution: Resolution,
    fps: u32,
    audio: bool,
) -> Option<Settings> {
    if !on() {
        return None;
    }
    crate::plugins::note(ID);
    Settings::unlocked(source, resolution, fps, true, audio)
}

/// A stored height off the official table, read back while the plugin is on.
pub(in crate::ui) fn limitless_stored_resolution(height: u32) -> Option<Resolution> {
    (on() && (rules::MIN_RESOLUTION..=rules::RESOLUTION_CEILING).contains(&height))
        .then_some(Resolution::Custom(height))
}

/// A stored frame rate off the official list, read back while the plugin is on.
pub(in crate::ui) fn limitless_stored_fps(fps: u32) -> bool {
    on() && (rules::MIN_FPS..=rules::FPS_CEILING).contains(&fps)
}

/// The plugin's resolution presets in place of the menu's list.
pub(in crate::ui) fn limitless_resolutions() -> Option<Vec<Resolution>> {
    on().then(|| {
        Presets::load()
            .resolutions
            .iter()
            .map(|preset| resolution_of(preset.value))
            .collect()
    })
}

/// The plugin's frame rate presets in place of the menu's list.
pub(in crate::ui) fn limitless_fpss() -> Option<Vec<u32>> {
    on().then(|| {
        Presets::load()
            .fpss
            .iter()
            .map(|preset| preset.value)
            .collect()
    })
}

/// The in-call Stream Quality menu's slider for the resolution (`true`) or the frame rate.
pub(in crate::ui) fn limitless_tile_slider(resolution: bool) -> Option<(&'static str, String)> {
    on().then(|| {
        if resolution {
            ("limitless-resolution", "Resolution".to_owned())
        } else {
            ("limitless-fps", "Frame Rate".to_owned())
        }
    })
}

/// The slider's range and value in its own units, the step and the text of a value.
fn slider_of(resolution: bool, current: u32) -> (u32, u32, u32, u32, &'static str) {
    let (top, suffix) = if resolution {
        (
            crate::plugins::with(|state| number_of(state, "maxResolution")),
            "p",
        )
    } else {
        (
            crate::plugins::with(|state| number_of(state, "maxFPS")),
            " FPS",
        )
    };
    let (low, high, step) = rules::slider_units(resolution, top, resolution && round());
    let value = (current / step).clamp(low, high);
    (low, high, value, step, suffix)
}

impl Root {
    /// A slider of the Go Live picker's Screen Resolution (`true`) or Frame Rate menu (`OptionsRange`).
    pub(in crate::ui) fn limitless_picker_slider(
        &self,
        resolution: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        let prefs = self.go_live.as_ref()?.prefs;
        let current = if resolution {
            match prefs.resolution {
                Resolution::Source => 720,
                other => other.height(),
            }
        } else {
            prefs.fps
        };
        let (low, high, value, step, suffix) = slider_of(resolution, current);
        let format = std::rc::Rc::new(move |value: u32| format!("{}{suffix}", value * step));
        Some(self.menu_slider_row(
            super::super::menu_slider::MenuSliderConfig {
                id: if resolution {
                    "stream-option-resolution-custom".into()
                } else {
                    "stream-option-frame-rate-custom".into()
                },
                label: format(value),
                min: low,
                max: high,
                value,
                palette: super::super::menu_slider::ThemePalette::Default,
                format,
                on_change: std::rc::Rc::new(move |this, value, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        if resolution {
                            state.prefs.resolution = resolution_of(value * step);
                        } else {
                            state.prefs.fps = value * step;
                        }
                    }
                    cx.notify();
                }),
            },
            cx,
        ))
    }

    /// The range of the Stream Quality menu's slider: (min, max, value, text of a value).
    fn limitless_tile_range(
        &self,
        resolution: bool,
        running: (u32, u32),
    ) -> (u32, u32, u32, std::rc::Rc<dyn Fn(u32) -> String>) {
        let current = PENDING
            .with(Cell::get)
            .filter(|(kind, _)| *kind == resolution)
            .map(|(_, value)| value)
            .unwrap_or(if resolution {
                if running.0 == 0 { 720 } else { running.0 }
            } else {
                running.1
            });
        let (low, high, value, step, suffix) = slider_of(resolution, current);
        (
            low,
            high,
            value,
            std::rc::Rc::new(move |value| format!("{}{suffix}", value * step)),
        )
    }

    /// The Stream Quality menu's slider row (`SettingsRange`).
    pub(in crate::ui) fn limitless_tile_slider_row(
        &self,
        key: &str,
        resolution: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = self
            .own_started()
            .map(|started| (started.quality.0.height(), started.quality.1))
            .unwrap_or((720, 30));
        let (min, max, value, format) = self.limitless_tile_range(resolution, running);
        self.menu_slider_row(
            super::super::menu_slider::MenuSliderConfig {
                id: format!("tile-slider-{key}").into(),
                label: format(value),
                min,
                max,
                value,
                palette: super::super::menu_slider::ThemePalette::Default,
                format,
                on_change: std::rc::Rc::new(move |this, value, cx| {
                    this.limitless_tile_slid(resolution, value, cx);
                }),
            },
            cx,
        )
    }

    /// The Stream Quality slider moved: the running stream follows, at most once a second (`lodash.throttle`).
    fn limitless_tile_slid(&mut self, resolution: bool, value: u32, cx: &mut Context<Self>) {
        let step = if resolution && round() { 10 } else { 1 };
        PENDING.with(|pending| pending.set(Some((resolution, value * step))));
        let since = LAST_MOVE
            .with(Cell::get)
            .map(|last| last.elapsed().as_millis() as u64);
        match rules::throttle(since.map(|_| 0), since.unwrap_or(0)) {
            Throttle::Now => self.limitless_apply_pending(cx),
            Throttle::After(wait) => {
                if SCHEDULED.with(|scheduled| scheduled.replace(true)) {
                    return;
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(wait))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        SCHEDULED.with(|scheduled| scheduled.set(false));
                        this.limitless_apply_pending(cx);
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn limitless_apply_pending(&mut self, cx: &mut Context<Self>) {
        let Some((resolution, value)) = PENDING.with(|pending| pending.take()) else {
            return;
        };
        LAST_MOVE.with(|last| last.set(Some(Instant::now())));
        if resolution {
            self.limitless_stream_choice(Some(value), None, cx);
        } else {
            self.limitless_stream_choice(None, Some(value), cx);
        }
    }

    /// A Stream Quality choice while the plugin is on: the running stream restarts at that resolution or rate,
    /// whatever the account (`changeStream(value)`). False when the plugin is off and the pinned rules decide.
    pub(in crate::ui) fn limitless_tile_choice(
        &mut self,
        resolution: Option<u32>,
        fps: Option<u32>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !on() {
            return false;
        }
        self.limitless_stream_choice(resolution, fps, cx);
        true
    }

    fn limitless_stream_choice(
        &mut self,
        resolution: Option<u32>,
        fps: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        crate::plugins::note(ID);
        let mut prefs = Prefs::load();
        prefs.preset = Mode::Custom;
        if let Some(height) = resolution {
            prefs.resolution = resolution_of(height);
        }
        if let Some(fps) = fps {
            prefs.fps = fps;
        }
        self.restart_stream_with(prefs, cx);
    }

    /// The add box of a list (`TextInput type="number"`): digits only, Enter adds.
    fn limitless_add_box(resolution: bool, cx: &mut Context<Self>) -> Entity<Editor> {
        let made = ADDING.with(|adding| {
            adding
                .borrow()
                .as_ref()
                .map(|boxes| boxes[usize::from(resolution)].0.clone())
        });
        if let Some(editor) = made {
            return editor;
        }
        let make = |resolution: bool, cx: &mut Context<Self>| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(super::super::editor_style(), cx);
                editor.set_single_line(true);
                editor
            });
            let typing = cx.subscribe(&editor, move |_, editor, event: &EditorEvent, cx| {
                // `onBeforeInput`: only digits go in.
                let text = editor.read(cx).text().to_owned();
                let digits: String = text.chars().filter(char::is_ascii_digit).take(5).collect();
                if digits != text {
                    editor.update(cx, |editor, _| editor.set_text(&digits));
                }
                if matches!(event, EditorEvent::Submit(_)) {
                    Self::limitless_add(resolution, cx);
                }
                cx.notify();
            });
            (editor, typing)
        };
        let boxes = [make(false, cx), make(true, cx)];
        let editor = boxes[usize::from(resolution)].0.clone();
        ADDING.with(|adding| *adding.borrow_mut() = Some(boxes));
        editor
    }

    /// `handleAddPreset`: the typed value joins its list and the box empties.
    fn limitless_add(resolution: bool, cx: &mut Context<Self>) {
        let editor = Self::limitless_add_box(resolution, cx);
        let Some(value) = editor
            .read(cx)
            .text()
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0 && rules::add_error(resolution, *value).is_none())
        else {
            return;
        };
        let mut presets = Presets::load();
        presets.add(resolution, value);
        presets.save();
        editor.update(cx, |editor, _| editor.set_text(""));
        cx.notify();
    }

    /// The card's own section: the frame rate and resolution presets (`fpsComponent`, `resolutionComponent`), each
    /// a deletable chip, and a box to add one.
    pub(in crate::ui) fn limitless_screenshare_settings(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::super::color;
        let presets = Presets::load();
        let typed = |resolution: bool, cx: &mut Context<Self>| {
            Self::limitless_add_box(resolution, cx)
                .read(cx)
                .text()
                .to_owned()
        };
        let (fps_text, resolution_text) = (typed(false, cx), typed(true, cx));
        let section = |resolution: bool, typed: String, cx: &mut Context<Self>| {
            let list = if resolution {
                &presets.resolutions
            } else {
                &presets.fpss
            };
            let value = typed.parse::<u32>().ok();
            let error = value.and_then(|value| rules::add_error(resolution, value));
            let kind = if resolution { "resolution" } else { "fps" };
            let mut chips = div().flex().flex_wrap().gap(px(8.));
            for (index, preset) in list.iter().enumerate() {
                let source = preset.label == "Source";
                chips = chips.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(preset.label.clone()),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "limitless-delete-{kind}-{index}"
                                )))
                                .size(px(28.))
                                .rounded(px(8.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(source, |this| this.opacity(0.5))
                                .when(!source, |this| {
                                    this.cursor_pointer()
                                        .hover(|style| style.bg(color("--background-mod-subtle")))
                                })
                                .child(super::super::icon(
                                    "menu_delete_message",
                                    20.,
                                    "--text-feedback-critical",
                                ))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    let mut presets = Presets::load();
                                    presets.remove(resolution, index);
                                    presets.save();
                                    cx.notify();
                                })),
                        ),
                );
            }
            let can_add = value.is_some_and(|value| value > 0) && error.is_none();
            let editor = Self::limitless_add_box(resolution, cx);
            let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
            let adder = div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(96.))
                                .h(px(32.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(if error.is_some() {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--input-border-default")
                                })
                                .bg(color("--input-background-default"))
                                .flex()
                                .items_center()
                                .cursor_text()
                                .text_size(px(14.))
                                .text_color(color("--text-default"))
                                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                                    window.focus(&focus, cx);
                                })
                                .child(editor),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("limitless-add-{kind}")))
                                .size(px(32.))
                                .rounded(px(8.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(color("--control-primary-background-default"))
                                .when(!can_add, |this| this.opacity(0.5))
                                .when(can_add, |this| this.cursor_pointer())
                                .child(super::super::icon(
                                    "plugin_plus",
                                    20.,
                                    "--control-primary-text-default",
                                ))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    if can_add {
                                        Self::limitless_add(resolution, cx);
                                    }
                                })),
                        ),
                )
                .children(error.map(|error| {
                    div()
                        .text_size(px(12.))
                        .text_color(color("--text-feedback-critical"))
                        .child(error)
                }));
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(if resolution {
                            "Resolution presets"
                        } else {
                            "Frame rate presets"
                        }),
                )
                .child(chips.child(adder))
        };
        div()
            .mt(px(8.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(false, fps_text, cx))
            .child(section(true, resolution_text, cx))
            .into_any_element()
    }
}
