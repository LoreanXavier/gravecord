//! Scripted input for headless tests. Steps go through `Window::dispatch_event`, the path real input takes, so clicks,
//! keys and drags are exercised without a visible window.
//!
//! `--script "click 36 157; wait 800; type hello; key enter; drag 450 330 600 330; note label"`.
//! `note` appends the app's state to `<capture>.log`. The clipboard is shared with the owner's desktop, so scripts never
//! press ctrl-c; `note` reports the selection directly.

use crate::editor::Editor;
use crate::ui::Root;
use gpui::{
    AnyWindowHandle, App, AsyncApp, Entity, EntityInputHandler, KeyDownEvent, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point, point, px,
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
        let composer = root.read(cx).composer();
        let _ = window.update(cx, |_, window, cx| {
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
            ["click", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                        dispatch(cx, window, up(position));
                    });
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
            ["note", label @ ..] => {
                let state = root.read_with(cx, |root, cx| root.describe(cx));
                let _ = writeln!(output, "{}: {state}", label.join(" "));
            }
            other => {
                let _ = writeln!(output, "unknown step: {other:?}");
            }
        }
        cx.background_executor().timer(STEP_GAP).await;
    }
    let _ = std::fs::write(log, output);
}
