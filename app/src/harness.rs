//! Scripted input for headless tests. Steps go through `Window::dispatch_event`, the path real input takes, so clicks,
//! keys and drags are exercised without a visible window.
//!
//! `--script "click 36 157; wait 800; type hello; key enter; drag 450 330 600 330; note label"`.
//! `stream start` / `stream stop` run a Go Live stream from the synthetic source (only when alone in the channel).
//! `note` appends the app's state to `<capture>.log`. The clipboard is shared with the owner's desktop, so scripts never
//! press ctrl-c; `note` reports the selection directly.

use crate::editor::Editor;
use crate::ui::Root;
use gpui::{
    AnyWindowHandle, App, AsyncApp, Entity, EntityInputHandler, Focusable, KeyDownEvent, Keystroke,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point,
    ScrollDelta, ScrollEvent, ScrollSource, TouchPhase, point, px,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

const STEP_GAP: Duration = Duration::from_millis(60);

fn at(x: &str, y: &str) -> Option<Point<gpui::Pixels>> {
    Some(point(px(x.parse().ok()?), px(y.parse().ok()?)))
}

fn dispatch(cx: &mut App, window: AnyWindowHandle, event: PlatformInput) {
    let _ = window.update(cx, |_, window, cx| {
        window.dispatch_event(event, cx);
    });
}

fn moved(position: Point<gpui::Pixels>, pressed: Option<MouseButton>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: pressed,
        modifiers: Modifiers::default(),
    })
}

fn down(position: Point<gpui::Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    })
}

fn right_down(position: Point<gpui::Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Right,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    })
}

fn up(position: Point<gpui::Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
    })
}

fn key(cx: &mut App, window: AnyWindowHandle, stroke: Keystroke) {
    dispatch(
        cx,
        window,
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: stroke,
            is_held: false,
            prefer_character_input: false,
        }),
    );
}

/// Calls the composer through `EntityInputHandler`, the trait the OS input method calls.
fn with_composer(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    root: &Entity<Root>,
    edit: impl FnOnce(&mut Editor, &mut gpui::Window, &mut gpui::Context<Editor>),
) {
    cx.update(|cx| {
        let editors = root.read(cx).open_editors();
        let _ = window.update(cx, |_, window, cx| {
            // The field with focus takes the text; the composer (or the edit box) otherwise.
            let composer = editors
                .iter()
                .find(|editor| editor.read(cx).focus_handle(cx).is_focused(window))
                .unwrap_or_else(|| editors.last().expect("the composer is always open"))
                .clone();
            composer.update(cx, |composer, cx| edit(composer, window, cx));
        });
    });
}

pub async fn run(
    script: &str,
    window: AnyWindowHandle,
    root: Entity<Root>,
    log: PathBuf,
    cx: &mut AsyncApp,
) {
    let mut output = String::new();
    for step in script
        .split(';')
        .map(str::trim)
        .filter(|step| !step.is_empty())
    {
        let words: Vec<&str> = step.split_whitespace().collect();
        match words.as_slice() {
            ["wait", ms] => {
                let ms = ms.parse().unwrap_or(0);
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
            }
            ["move", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| dispatch(cx, window, moved(position, None)));
                }
            }
            // Positive `dy` scrolls the content up, as a wheel turned toward the user does.
            ["scroll", x, y, dy] => {
                if let (Some(position), Ok(dy)) = (at(x, y), dy.parse::<f32>()) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(
                            cx,
                            window,
                            PlatformInput::Scroll(ScrollEvent {
                                position,
                                delta: ScrollDelta::Pixels(point(px(0.), px(-dy))),
                                modifiers: Modifiers::default(),
                                touch_phase: TouchPhase::Moved,
                                source: ScrollSource::Pointer,
                            }),
                        );
                    });
                }
            }
            ["hscroll", x, y, dx] => {
                if let (Some(position), Ok(dx)) = (at(x, y), dx.parse::<f32>()) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(
                            cx,
                            window,
                            PlatformInput::Scroll(ScrollEvent {
                                position,
                                delta: ScrollDelta::Pixels(point(px(-dx), px(0.))),
                                modifiers: Modifiers::default(),
                                touch_phase: TouchPhase::Moved,
                                source: ScrollSource::Pointer,
                            }),
                        );
                    });
                }
            }
            ["rightclick", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, right_down(position));
                    });
                }
            }
            ["click", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                        dispatch(cx, window, up(position));
                    });
                }
            }
            // A click with a frame between press and release, for text links: they register the release listener
            // only once the press has been painted.
            ["press", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    cx.update(|cx| dispatch(cx, window, up(position)));
                }
            }
            ["drag", x1, y1, x2, y2] => {
                if let (Some(from), Some(to)) = (at(x1, y1), at(x2, y2)) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(from, None));
                        dispatch(cx, window, down(from));
                        dispatch(cx, window, moved(to, Some(MouseButton::Left)));
                        dispatch(cx, window, up(to));
                    });
                }
            }
            ["key", stroke] => {
                // The parser wants a static string; a test script is short-lived and small.
                let stroke: &'static str = Box::leak(stroke.to_string().into_boxed_str());
                if let Ok(parsed) = Keystroke::parse(stroke) {
                    cx.update(|cx| key(cx, window, parsed));
                }
            }
            // Lets a key go, for push to talk.
            ["keyup", stroke] => {
                let stroke: &'static str = Box::leak(stroke.to_string().into_boxed_str());
                if let Ok(parsed) = Keystroke::parse(stroke) {
                    cx.update(|cx| {
                        dispatch(
                            cx,
                            window,
                            PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke: parsed }),
                        )
                    });
                }
            }
            ["type", ..] => {
                let text = step.trim_start_matches("type").trim_start();
                with_composer(cx, window, &root, |composer, window, cx| {
                    for value in text.chars() {
                        composer.replace_text_in_range(None, &value.to_string(), window, cx);
                    }
                });
            }
            ["compose", ..] => {
                let text = step.trim_start_matches("compose").trim_start();
                with_composer(cx, window, &root, |composer, window, cx| {
                    composer.replace_and_mark_text_in_range(None, text, None, window, cx);
                });
            }
            ["commit", ..] => {
                let text = step.trim_start_matches("commit").trim_start();
                with_composer(cx, window, &root, |composer, window, cx| {
                    composer.replace_text_in_range(None, text, window, cx);
                });
            }
            // Puts a file in the composer's upload area, as the file dialog or a drop does.
            ["attach", path] => {
                let path = std::path::PathBuf::from(*path);
                root.update(cx, |root, cx| root.attach_files(vec![path], cx));
            }
            // Drops a file on the window at a point, as the platform does when files are dragged in.
            ["drop", path, x, y] => {
                if let Some(position) = at(x, y) {
                    let paths = gpui::ExternalPaths(vec![PathBuf::from(*path)].into());
                    cx.update(|cx| {
                        dispatch(
                            cx,
                            window,
                            PlatformInput::FileDrop(gpui::FileDropEvent::Entered {
                                position,
                                paths,
                            }),
                        );
                        dispatch(
                            cx,
                            window,
                            PlatformInput::FileDrop(gpui::FileDropEvent::Pending { position }),
                        );
                        dispatch(
                            cx,
                            window,
                            PlatformInput::FileDrop(gpui::FileDropEvent::Submit { position }),
                        );
                    });
                }
            }
            // Opens a settings page by its label in lower case with dashes, as the captures are named.
            ["settings", page] => {
                let page = (*page).to_owned();
                root.update(cx, |root, cx| root.open_settings_page(&page, cx));
            }
            // Go Live from the synthetic pattern, only when nobody else is in the channel; ends by itself after ten
            // seconds. The outcome goes to the log.
            ["stream", "start"] => {
                let outcome = root.update(cx, |root, cx| root.stream_start_synthetic(cx));
                let _ = writeln!(output, "stream start: {outcome}");
            }
            ["stream", "stop"] => {
                root.update(cx, |root, cx| root.stream_stop(cx));
            }
            ["activity", "state"] => {
                let state = root.read_with(cx, |root, _| root.activity_state_line());
                let _ = writeln!(output, "activity: {state}");
            }
            ["activity", "launch", name @ ..] => {
                let name = name.join(" ");
                let outcome = root.update(cx, |root, cx| root.activity_launch_named(&name, cx));
                let _ = writeln!(output, "activity launch: {outcome}");
            }
            // The fixture server cannot pass Discovery's requirements: this shows the listing form as if it did.
            ["discovery", "eligible"] => {
                crate::ui::FORCE_ELIGIBLE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            // The fixture is neither discoverable nor published: this shows the Server Web Page in either state.
            ["landing", state] => {
                crate::ui::FORCE_LANDING.store(
                    if *state == "published" { 2 } else { 1 },
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
            ["note", label @ ..] => {
                let state = root.read_with(cx, |root, cx| root.describe(cx));
                let _ = writeln!(output, "{}: {state}", label.join(" "));
            }
            other => {
                let _ = writeln!(output, "unknown step: {other:?}");
            }
        }
        let _ = std::fs::write(&log, &output);
        cx.background_executor().timer(STEP_GAP).await;
    }
    let _ = std::fs::write(log, output);
}
