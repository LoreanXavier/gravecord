//! Scripted input for headless tests. Steps go through `Window::dispatch_event`, the path real input takes, so clicks,
//! keys and drags are exercised without a visible window.
//!
//! `--script "click 36 157; wait 800; type hello; key enter; drag 450 330 600 330; note label"`.
//! `stream start` / `stream stop` run a Go Live stream from the synthetic source (only when alone in the channel).
//! `note` appends the app's state to `<capture>.log`. The clipboard is shared with the owner's desktop, so scripts never
//! press ctrl-c; `note` reports the selection directly.
//!
//! `native move x y` sends Win32 hit-test/move messages to this window. `native state` records the
//! cached position and bounded move/leave trace. These do not move the OS pointer: Windows may
//! generate extra leave messages, so this verifies message forwarding, not physical mouse input.

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

mod native;

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
            ["splash", rest @ ..] => {
                let note = cx.update(|cx| root.update(cx, |root, _| root.script_splash(rest)));
                let _ = writeln!(output, "{note}");
            }
            ["move", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| dispatch(cx, window, moved(position, None)));
                }
            }
            ["native", "move", x, y] => {
                if let Some(position) = at(x, y) {
                    let result = native::move_pointer(window, position, cx).await;
                    let _ = writeln!(output, "native move: {result}");
                }
            }
            ["native", "state"] => {
                let _ = writeln!(output, "native state: {}", native::state(window, cx));
            }
            ["discover", "dump"] => {
                let note = cx.update(|cx| root.update(cx, |root, _| root.discover_dump()));
                let _ = writeln!(output, "{note}");
            }
            ["shop", "dump"] => {
                let note = cx.update(|cx| root.update(cx, |root, _| root.shop_dump()));
                let _ = writeln!(output, "{note}");
            }
            ["shop", "checkout", rest @ ..] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_shop_checkout(rest, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["tray", "preview"] => {
                let note = cx.update(|cx| root.update(cx, |root, _| root.script_tray()));
                let _ = writeln!(output, "{note}");
            }
            ["badge", "fixture", rest @ ..] => {
                let note = cx.update(|cx| root.update(cx, |root, cx| root.script_badge(rest, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["badge", "dm", rest @ ..] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_dm_rail(rest, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["badge", "requests", value] => {
                let note = cx
                    .update(|cx| root.update(cx, |root, cx| root.script_request_badge(value, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["nickname", mode] => {
                let result = cx.update(|cx| {
                    root.update(cx, |root, cx| root.script_nickname_preview(mode, cx))
                });
                let _ = writeln!(output, "nickname: {result}");
            }
            ["call-background", "state"] => {
                let state = cx.update(|cx| root.read(cx).call_background_status());
                let _ = writeln!(output, "call background: {state}");
            }
            ["inventory", "state"] => {
                let state = cx.update(|cx| root.read(cx).inventory_status());
                let _ = writeln!(output, "inventory: {state}");
            }
            ["inventory", "popout", "state"] => {
                let state = cx.update(|cx| root.read(cx).inventory_popout_status());
                let _ = writeln!(output, "inventory popout: {state}");
            }
            ["channel", "click", id] => {
                let Ok(id) = id.parse::<u64>() else {
                    break;
                };
                let target = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.read(cx)
                                .capture_channel_target(id, window.viewport_size())
                        })
                        .ok()
                        .flatten()
                });
                let Some(position) = target else {
                    let _ = writeln!(
                        output,
                        "channel click: row unavailable or excluded; script stopped"
                    );
                    break;
                };
                cx.update(|cx| {
                    dispatch(cx, window, moved(position, None));
                    dispatch(cx, window, down(position));
                    dispatch(cx, window, up(position));
                });
                let selected = cx.update(|cx| root.read(cx).capture_channel_selected(id));
                let _ = writeln!(output, "channel click: selected={selected} at {position:?}");
                if !selected {
                    break;
                }
            }
            ["member-profile", "mobile"] => {
                let target = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.read(cx).mobile_member_target(window.viewport_size())
                        })
                        .ok()
                        .flatten()
                });
                if let Some((position, name)) = target {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                        dispatch(cx, window, up(position));
                    });
                    let _ = writeln!(output, "mobile member profile: {name} at {position:?}");
                } else {
                    let _ = writeln!(output, "mobile member profile: no visible mobile member");
                }
            }
            ["friends", tab] => {
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_friends_tab(tab, window, cx))
                    })
                });
                let _ = writeln!(output, "{note:?}");
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
            // One committed input operation, like CDP Input.insertText; no system clipboard.
            ["insert-text", ..] => {
                let text = step.trim_start_matches("insert-text").trim_start();
                with_composer(cx, window, &root, |editor, window, cx| {
                    editor.replace_text_in_range(None, text, window, cx);
                });
            }
            // Pastes a picture file as if it were on the clipboard (the system clipboard is never touched).
            ["paste-image", path] => {
                let path = std::path::PathBuf::from(*path);
                if let Ok(bytes) = std::fs::read(&path) {
                    let format = match path.extension().and_then(|ext| ext.to_str()) {
                        Some("jpg" | "jpeg") => gpui::ImageFormat::Jpeg,
                        Some("gif") => gpui::ImageFormat::Gif,
                        Some("bmp") => gpui::ImageFormat::Bmp,
                        _ => gpui::ImageFormat::Png,
                    };
                    let item =
                        gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(format, bytes));
                    root.update(cx, |root, cx| root.paste_clipboard_item(&item, cx));
                }
            }
            // Pastes files, as copying them in the file manager does.
            ["paste-files", path] => {
                let paths = gpui::ExternalPaths(vec![PathBuf::from(*path)].into());
                let item = gpui::ClipboardItem {
                    entries: vec![gpui::ClipboardEntry::ExternalPaths(paths)],
                };
                root.update(cx, |root, cx| root.paste_clipboard_item(&item, cx));
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
            // Registered Games with a running game, two seen ones and a hand-added one, in memory only.
            ["games", "demo"] => {
                root.update(cx, |root, cx| root.script_games_demo(cx));
            }
            // Opens a settings page by its label in lower case with dashes, as the captures are named.
            ["settings", page] => {
                let page = (*page).to_owned();
                root.update(cx, |root, cx| root.open_settings_page(&page, cx));
            }
            // Go Live from the synthetic pattern, only when nobody else is in the channel; ends by itself after ten
            // seconds. The outcome goes to the log.
            ["stream", "start"] => {
                let outcome = root.update(cx, |root, cx| root.stream_start_synthetic(false, cx));
                let _ = writeln!(output, "stream start: {outcome}");
            }
            // The same pattern with the owner in the channel, for a live test he asked for: it holds nothing of
            // this machine. Ends by itself like the other.
            ["stream", "start", "with-others"] => {
                let outcome = root.update(cx, |root, cx| root.stream_start_synthetic(true, cx));
                let _ = writeln!(output, "stream start: {outcome}");
            }
            // The fixture guild's General voice channel, the only one a script can name.
            ["voice", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_voice(rest, cx));
                let _ = writeln!(output, "voice: {line}");
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
            // A preference of the proto settings, read as the account holds it, or changed as its row would.
            ["pref", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_pref(rest);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "pref: {line}");
            }
            // Capture only: the account's `premium_type` is set locally (none, 1, 2 or 3) to see the subscriber
            // states of the Nitro screens. Nothing is sent, and a normal build has no such step.
            // The Clips page's own steps: `clips get`, `clips location <path>` (a folder dialog's answer), and the
            // engine's: `clips save` (the keybind), `clips status`, `clips list`.
            ["clips", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_clips(rest, cx);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "clips: {line}");
            }
            ["premium", tier] => {
                let tier = tier.parse::<u64>().ok().filter(|tier| *tier > 0);
                root.update(cx, |root, cx| root.script_premium(tier, cx));
            }
            // Local cooldown only; no request or channel setting is sent.
            ["slowmode", "reject"] => {
                root.update(cx, |root, cx| root.script_slowmode_rejection(cx));
            }
            ["notification", action] => {
                root.update(cx, |root, cx| root.script_notification(action, cx));
            }
            ["icons", choice] => {
                crate::branding::select(crate::branding::IconSet::from_name(Some(choice)));
                root.update(cx, |_, cx| cx.notify());
            }
            ["browser", action] => {
                let state = root.update(cx, |root, cx| root.script_browser(action, cx));
                let _ = writeln!(output, "browser: {state}");
            }
            ["slowmode", millis] => {
                if let Ok(millis) = millis.parse::<u64>() {
                    root.update(cx, |root, cx| root.script_slowmode(millis, cx));
                }
            }
            ["slowmode", "fixture", millis] => {
                if let Ok(millis) = millis.parse::<u64>() {
                    root.update(cx, |root, cx| root.script_slowmode_fixture(millis, cx));
                }
            }
            // Shows the profile effect of a collectibles product over the open profile, in this process only (the
            // product is read with a GET): `profile-effect <sku> [at <ms>]`, `profile-effect off`, `profile-effect state`.
            // Capture only: `sticker clock <ms>|off` freezes every playing sticker at that time, `sticker state` prints
            // what the sticker player holds.
            // Capture only: `messages demo <set>` puts message kinds the fixture server lacks into the open channel (memory only).
            ["messages", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_messages(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["stickerpopout", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_sticker_popout(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["sticker", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_sticker_player(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only, the motion layer (`ui/motion.rs`): `motion clock <ms>` stands the animation clock still at
            // that many ms (a frame is drawn at the old time first, so what the last input started has begun), the
            // time of every later input; `motion clock real` lets it run. `motion shot <path>` writes the frame at
            // the current time. `motion reduced on|off|system` forces the reduced-motion decision. `motion idle <ms>`
            // waits and prints how many renders and frames the driver asked for in that time. `motion state` prints
            // the clock.
            ["motion", rest @ ..] => {
                let line = crate::ui::motion::step(rest, window, &root, cx).await;
                let _ = writeln!(output, "{line}");
            }
            ["profile-effect", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_profile_effect(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: one styled name in the three places of the official style editor's preview, at their
            // positions on plain panels: `name-sheet <font id> <effect id> <colours> [at <ms>] [bg a b c]`, `off`.
            ["name-sheet", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_name_sheet(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // The profile editor: `profile-editor open`, `nitro`, `preset <name>`, `pick <kind> [sku]`, ... (see
            // `script_profile_editor`).
            ["profile-editor", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_profile_editor(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Measurements, see `perf.rs`: `perf sample <label>` (process counters), `perf caches` (sizes of the app's
            // own caches), `perf start` (when the content and the loading screen's end showed).
            ["perf", "sample", label @ ..] => {
                let renders = crate::perf::RENDERS.load(std::sync::atomic::Ordering::Relaxed);
                let (working, private) = crate::perf::memory();
                let (heap, heap_peak) = crate::perf::heap();
                let (image, mapped, committed_private) = crate::perf::committed();
                let _ = writeln!(
                    output,
                    "perf sample {}: wall_ms={} cpu_ms={} working_set={working} private={private} heap={heap} heap_peak={heap_peak} commit_image={image} commit_mapped={mapped} commit_private={committed_private} threads={} renders={renders}",
                    label.join(" "),
                    crate::perf::since_process_start_ms(),
                    crate::perf::cpu_ms(),
                    crate::perf::threads(),
                );
            }
            ["perf", "start"] => {
                let _ = writeln!(
                    output,
                    "perf start: fonts_ms={} window_ms={} ready_ms={} content_ms={} splash_done_ms={} {}",
                    crate::perf::FONTS_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::WINDOW_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::READY_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::CONTENT_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::SPLASH_DONE_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::checkpoints(),
                );
            }
            ["perf", "ws"] => {
                for line in crate::perf::working_set_report() {
                    let _ = writeln!(output, "perf ws: {line}");
                }
            }
            ["perf", "heap"] => {
                for line in crate::perf::big_allocations() {
                    let _ = writeln!(output, "perf heap: {line}");
                }
            }
            ["perf", "badge"] => {
                let line = cx.update(|cx| root.update(cx, |root, _| root.perf_badge()));
                let _ = writeln!(output, "perf {line}");
            }
            ["perf", "caches", label @ ..] => {
                let line = cx.update(|cx| root.read(cx).perf_caches());
                let _ = writeln!(output, "perf caches {}: {line}", label.join(" "));
            }
            // `go home|dm|shop|discover|quests|settings <page>`: the places the rail and the sidebars open.
            ["go", rest @ ..] => {
                let line = cx.update(|cx| root.update(cx, |root, cx| root.script_go(rest, cx)));
                let _ = writeln!(output, "go {}: {line}", rest.join(" "));
            }
            // `frames scroll <x> <y> <dy> <count> <label>`: `count` wheel ticks of `dy` pixels, and the time each took to
            // reach a finished frame (the event, the render and the paint, not the GPU).
            ["frames", "scroll", x, y, dy, count, label @ ..] => {
                if let (Some(position), Ok(dy), Ok(count)) =
                    (at(x, y), dy.parse::<f32>(), count.parse::<usize>())
                {
                    let mut samples = Vec::with_capacity(count);
                    cx.update(|cx| dispatch(cx, window, moved(position, None)));
                    for _ in 0..count {
                        let started = std::time::Instant::now();
                        cx.update(|cx| {
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
                            let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                        });
                        samples.push(started.elapsed().as_secs_f64() * 1000.);
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                    }
                    let _ = writeln!(
                        output,
                        "frames scroll {}: {}",
                        label.join(" "),
                        crate::perf::summary(&mut samples, 16.7)
                    );
                }
            }
            // `frames watch <ms> <label>`: finished frames drawn back to back for `ms`, as a window does while things
            // load in, and the time each took.
            ["frames", "watch", ms, label @ ..] => {
                let ms: u64 = ms.parse().unwrap_or(1000);
                let until = std::time::Instant::now() + Duration::from_millis(ms);
                let mut samples = Vec::new();
                while std::time::Instant::now() < until {
                    let started = std::time::Instant::now();
                    cx.update(|cx| {
                        let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                    });
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
                let _ = writeln!(
                    output,
                    "frames watch {}: {}",
                    label.join(" "),
                    crate::perf::summary(&mut samples, 16.7)
                );
            }
            // `frames sweep <x> <y1> <y2> <steps> <label>`: the pointer moves from `y1` to `y2` down the column at `x` in
            // `steps` moves, one every 16 ms, and the time each took to reach a finished frame (what a pointer sweeping
            // a list costs, hover transitions included). The frames are drawn the way the window does after an event.
            ["frames", "sweep", x, y1, y2, steps, label @ ..] => {
                if let (Ok(x), Ok(y1), Ok(y2), Ok(steps)) = (
                    x.parse::<f32>(),
                    y1.parse::<f32>(),
                    y2.parse::<f32>(),
                    steps.parse::<usize>(),
                ) {
                    let mut samples = Vec::with_capacity(steps);
                    for step in 0..steps {
                        let y = y1 + (y2 - y1) * step as f32 / steps.max(1) as f32;
                        let position = point(px(x), px(y));
                        let started = std::time::Instant::now();
                        cx.update(|cx| {
                            dispatch(cx, window, moved(position, None));
                            let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                        });
                        samples.push(started.elapsed().as_secs_f64() * 1000.);
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                    }
                    let _ = writeln!(
                        output,
                        "frames sweep {}: {}",
                        label.join(" "),
                        crate::perf::summary(&mut samples, 16.7)
                    );
                }
            }
            // `frames alternate <count> <x1> <y1> <x2> <y2> <label>`: clicks the two places in turn, `count` times each, the
            // way a fast server switch does, and the time from each click to the finished frame after it.
            ["frames", "alternate", count, x1, y1, x2, y2, label @ ..] => {
                if let (Ok(count), Some(first), Some(second)) =
                    (count.parse::<usize>(), at(x1, y1), at(x2, y2))
                {
                    let mut samples = Vec::new();
                    for _ in 0..count {
                        for position in [first, second] {
                            cx.update(|cx| dispatch(cx, window, moved(position, None)));
                            let started = std::time::Instant::now();
                            cx.update(|cx| {
                                dispatch(cx, window, down(position));
                                dispatch(cx, window, up(position));
                                let _ =
                                    window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                            });
                            samples.push(started.elapsed().as_secs_f64() * 1000.);
                            cx.background_executor()
                                .timer(Duration::from_millis(120))
                                .await;
                        }
                    }
                    let _ = writeln!(
                        output,
                        "frames watch {}: {}",
                        label.join(" "),
                        crate::perf::summary(&mut samples, 16.7)
                    );
                }
            }
            // `frames click <x> <y> <label>`: the time from a click to the finished frame that follows it.
            ["frames", "click", x, y, label @ ..] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| dispatch(cx, window, moved(position, None)));
                    let started = std::time::Instant::now();
                    cx.update(|cx| {
                        dispatch(cx, window, down(position));
                        dispatch(cx, window, up(position));
                        let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                    });
                    let _ = writeln!(
                        output,
                        "frames click {}: ms={:.2}",
                        label.join(" "),
                        started.elapsed().as_secs_f64() * 1000.
                    );
                }
            }
            // `frames type <text>`: each character goes in through the input handler, as the system's keyboard
            // delivers it, and the time to the finished frame that shows it is taken.
            ["frames", "type", ..] => {
                let text = step.trim_start_matches("frames").trim_start();
                let text = text.trim_start_matches("type").trim_start();
                let mut samples = Vec::new();
                for value in text.chars() {
                    let started = std::time::Instant::now();
                    with_composer(cx, window, &root, |composer, window, cx| {
                        composer.replace_text_in_range(None, &value.to_string(), window, cx);
                    });
                    cx.update(|cx| {
                        let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                    });
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                }
                let _ = writeln!(
                    output,
                    "frames type: {}",
                    crate::perf::summary(&mut samples, 16.0)
                );
            }
            // Which element has focus, by name and kind, for asserting keyboard focus in the run log.
            ["focus", label @ ..] => {
                let report = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.focus_report(window, cx))
                    })
                    .unwrap_or_else(|_| "focus=unknown".to_owned());
                let _ = writeln!(output, "{}: {report}", label.join(" "));
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
