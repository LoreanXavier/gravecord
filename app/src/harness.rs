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

mod crawl;
mod grab;
mod hitch;
pub(crate) mod native;
mod ticks;

const STEP_GAP: Duration = Duration::from_millis(60);

fn at(x: &str, y: &str) -> Option<Point<gpui::Pixels>> {
    Some(point(px(x.parse().ok()?), px(y.parse().ok()?)))
}

fn dispatch(cx: &mut App, window: AnyWindowHandle, event: PlatformInput) {
    let _ = window.update(cx, |_, window, cx| {
        window.dispatch_event(event, cx);
    });
}

/// The primary button is down after `hold` and until `release`: moves in between carry it, as the system's do.
static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn moved(position: Point<gpui::Pixels>, pressed: Option<MouseButton>) -> PlatformInput {
    let held = HELD
        .load(std::sync::atomic::Ordering::Relaxed)
        .then_some(MouseButton::Left);
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: pressed.or(held),
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

fn right_up(position: Point<gpui::Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Right,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
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
            ["crawl", rest @ ..] => {
                let report = crawl::run(rest, window, &root, cx).await;
                output.push_str(&report);
            }
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
            ["mouse-exit"] => {
                cx.update(|cx| {
                    dispatch(
                        cx,
                        window,
                        PlatformInput::MouseExited(gpui::MouseExitEvent::default()),
                    )
                });
            }
            // `window size <w> <h>`: sets the content size of the window (the harness cannot drag an edge), as a
            // resize by hand does. `window size` alone prints the current one.
            ["window", "size", rest @ ..] => {
                let want = match rest {
                    [w, h] => w.parse::<f32>().ok().zip(h.parse::<f32>().ok()),
                    _ => None,
                };
                let now = cx.update(|cx| {
                    window
                        .update(cx, |_, window, _| {
                            if let Some((w, h)) = want {
                                window.resize(gpui::size(gpui::px(w), gpui::px(h)));
                            }
                            let size = window.viewport_size();
                            (f32::from(size.width), f32::from(size.height))
                        })
                        .unwrap_or_default()
                });
                let _ = writeln!(output, "window size: {} x {}", now.0, now.1);
            }
            ["native", "move", x, y] => {
                if let Some(position) = at(x, y) {
                    let result = native::move_pointer(window, position, cx).await;
                    let _ = writeln!(output, "native move: {result}");
                }
            }
            ["native", "leave"] => {
                let result = native::leave(window, cx).await;
                let _ = writeln!(output, "native leave: {result}");
            }
            ["native", "state"] => {
                let _ = writeln!(output, "native state: {}", native::state(window, cx));
            }
            ["native", "close-main"] => {
                let result = cx.update(|cx| {
                    let count = cx.windows().len();
                    window.update(cx, |_, window, cx| (count, native::close(window, cx)))
                });
                let _ = writeln!(output, "native close-main: {result:?}");
            }
            ["native", "windows"] => {
                let count = cx.update(|cx| cx.windows().len());
                let _ = writeln!(output, "native windows: {count}");
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
            ["tray", "check-updates"] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_tray_check_updates(cx)));
                let _ = writeln!(output, "{note}");
            }
            // Capture-only: the picture the tray or the taskbar overlay would get, written to a PNG; no shell call.
            ["tray", "render", rest @ ..] => {
                let note = cx.update(|cx| root.update(cx, |root, _| root.script_tray_render(rest)));
                let _ = writeln!(output, "{note}");
            }
            ["badge", "render", count, png] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, _| root.script_badge_render(count, png)));
                let _ = writeln!(output, "{note}");
            }
            ["accounts", rest @ ..] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_accounts(rest, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["favorites", rest @ ..] => {
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_favorites(rest, cx)));
                let _ = writeln!(output, "{note}");
            }
            ["rail", rest @ ..] => {
                let note = cx.update(|cx| root.update(cx, |root, cx| root.script_rail(rest, cx)));
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
                // A row outside the list's visible band is scrolled into view first, as a person would.
                let mut target = None;
                for _ in 0..8 {
                    target = cx.update(|cx| {
                        window
                            .update(cx, |_, window, cx| {
                                root.read(cx)
                                    .capture_channel_target(id, window.viewport_size())
                            })
                            .ok()
                            .flatten()
                    });
                    if target.is_some() {
                        break;
                    }
                    let delta = cx.update(|cx| {
                        window
                            .update(cx, |_, window, cx| {
                                root.read(cx)
                                    .capture_channel_scroll(id, window.viewport_size())
                            })
                            .ok()
                            .flatten()
                    });
                    let Some(delta) = delta else {
                        break;
                    };
                    let position = point(px(220.), px(400.));
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(
                            cx,
                            window,
                            PlatformInput::Scroll(ScrollEvent {
                                position,
                                delta: ScrollDelta::Pixels(point(px(0.), px(-delta))),
                                modifiers: Modifiers::default(),
                                touch_phase: TouchPhase::Moved,
                                source: ScrollSource::Pointer,
                            }),
                        );
                    });
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                }
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
            ["member-profile", user] => {
                let Ok(user) = user.parse::<u64>() else { break };
                let target = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.read(cx)
                                .capture_member_profile_target(user, window.viewport_size())
                        })
                        .ok()
                        .flatten()
                });
                let Some(position) = target else {
                    let _ = writeln!(
                        output,
                        "member profile: visible row unavailable; script stopped"
                    );
                    break;
                };
                cx.update(|cx| {
                    dispatch(cx, window, moved(position, None));
                    dispatch(cx, window, down(position));
                    dispatch(cx, window, up(position));
                });
                let selected = cx.update(|cx| root.read(cx).capture_member_profile_selected(user));
                let _ = writeln!(
                    output,
                    "member profile: selected={selected} at {position:?}"
                );
                if !selected {
                    break;
                }
            }
            ["context-spotify", who, surface] => {
                let note =
                    root.update(cx, |root, cx| root.script_spotify_context(who, surface, cx));
                let _ = writeln!(output, "{note}");
            }
            ["context-user"] | ["context-dm"] | ["context-self"] => {
                let kind = if step.ends_with("dm") {
                    "dm"
                } else if step.ends_with("self") {
                    "self"
                } else {
                    "user"
                };
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_context(kind, cx)));
                let _ = writeln!(output, "{note}");
            }
            // Capture only: `menu-run <row label with _ for spaces>` runs a row of the open message menu.
            ["menu-run", label] => {
                let label = label.replace('_', " ");
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_menu_run(&label, window, cx))
                    })
                });
                let _ = writeln!(output, "{note:?}");
            }
            // Capture only: a call tile's menu (`tilemenu open stream|user|options me|<id> [x y]`, `rows`, `run <label>`).
            ["tilemenu", rest @ ..] => {
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_tile_menu(rest, window, cx))
                    })
                });
                let _ = writeln!(output, "tilemenu: {note:?}");
            }
            ["context-sub-state"] => {
                let note = root.update(cx, |root, _| root.script_context_sub_state());
                let _ = writeln!(output, "{note}");
            }
            // Fixture server only: makes a coloured and a plain role and gives the coloured one to this account.
            ["fixture-roles"] => {
                let note = root.update(cx, |root, _| root.script_fixture_roles());
                let _ = writeln!(output, "{note}");
            }
            ["member-action", kind] => {
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_member_action(kind, window, cx))
                    })
                });
                let _ = writeln!(output, "{note:?}");
            }
            // Message Requests: fixture events only, nothing is sent (`ui/message_requests.rs`).
            // `report-issue open <app id|-> <name>`: the Report Issue dialog for that game; `report-issue pick <n>`.
            ["report-issue", rest @ ..] => {
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_report_issue(rest, window, cx))
                    })
                });
                let _ = writeln!(output, "{note:?}");
            }
            ["mr", rest @ ..] => {
                let note = cx.update(|cx| {
                    root.update(cx, |root, cx| root.script_message_requests(rest, cx))
                });
                let _ = writeln!(output, "{note}");
            }
            // Capture only: `gw <NAME> <json>` hands the root a gateway dispatch, as the socket would.
            // `game-profile <application id>`: opens that game's profile, as View Game Profile does.
            ["game-profile", game] => {
                let game = (*game).to_owned();
                cx.update(|cx| root.update(cx, |root, cx| root.open_game_profile(game, cx)));
                let _ = writeln!(output, "game-profile: opened");
            }
            // `channel-notice skip-invite`: the invite notice counts as dismissed, so the next candidate shows.
            ["channel-notice", "skip-invite"] => {
                crate::ui::channel_notice::SKIP_INVITE
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = writeln!(output, "channel-notice: invite skipped");
            }
            // `composer-ai staff`: the composer menu's AI rows show as for a staff account.
            ["composer-ai", "staff"] => {
                crate::ui::composer_ai::AS_STAFF.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = writeln!(output, "composer-ai: staff");
            }
            ["gw", name, json @ ..] => {
                let text = json.join(" ");
                let note = match serde_json::from_str(&text) {
                    Ok(data) => {
                        let name = (*name).to_owned();
                        cx.update(|cx| {
                            root.update(cx, |root, cx| {
                                root.update(crate::net::Update::Dispatch { name, data }, cx)
                            })
                        });
                        "gw: dispatched".to_owned()
                    }
                    Err(error) => format!("gw: not json: {error}"),
                };
                let _ = writeln!(output, "{note}");
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
                        dispatch(cx, window, right_up(position));
                    });
                }
            }
            // `dblclick x y`: two presses, the second with a click count of two, as the system reports a double press.
            ["dblclick", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                        dispatch(cx, window, up(position));
                        dispatch(
                            cx,
                            window,
                            PlatformInput::MouseDown(MouseDownEvent {
                                button: MouseButton::Left,
                                position,
                                modifiers: Modifiers::default(),
                                click_count: 2,
                                first_mouse: false,
                            }),
                        );
                        dispatch(cx, window, up(position));
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
            // `middle down|up <x> <y>`: the middle button, for autoscroll (`ui/autoscroll.rs`).
            [
                button @ ("middle" | "mouse-back" | "mouse-forward" | "mouse-right"),
                edge @ ("down" | "up"),
                x,
                y,
            ] => {
                let button = match *button {
                    "mouse-back" => MouseButton::Navigate(gpui::NavigationDirection::Back),
                    "mouse-forward" => MouseButton::Navigate(gpui::NavigationDirection::Forward),
                    "mouse-right" => MouseButton::Right,
                    _ => MouseButton::Middle,
                };
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        let event = if *edge == "down" {
                            PlatformInput::MouseDown(MouseDownEvent {
                                button,
                                position,
                                modifiers: Modifiers::default(),
                                click_count: 1,
                                first_mouse: false,
                            })
                        } else {
                            PlatformInput::MouseUp(MouseUpEvent {
                                button,
                                position,
                                modifiers: Modifiers::default(),
                                click_count: 1,
                            })
                        };
                        dispatch(cx, window, event);
                    });
                }
            }
            // The autoscroll mode and panning cursor, which a frame does not show.
            ["autoscroll", "state"] => {
                let line = root.read_with(cx, |root, _| root.autoscroll_state_line());
                let _ = writeln!(output, "{line}");
            }
            // The primary button goes down and stays down, so a frame can show the `:active` look; `release` lets go.
            ["hold", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, down(position));
                    });
                    HELD.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            // A move with the primary button held, after `hold`: what a drag sends between press and release.
            ["dragto", x, y] => {
                if let Some(position) = at(x, y) {
                    cx.update(|cx| dispatch(cx, window, moved(position, Some(MouseButton::Left))));
                }
            }
            ["release", x, y] => {
                HELD.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Some(position) = at(x, y) {
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
            // `modifiers shift` / `modifiers none`: the modifier keys now held, as the system reports a change.
            ["modifiers", held] => {
                let modifiers = gpui::Modifiers {
                    shift: held.contains("shift"),
                    control: held.contains("ctrl"),
                    alt: held.contains("alt"),
                    ..gpui::Modifiers::default()
                };
                cx.update(|cx| {
                    dispatch(
                        cx,
                        window,
                        PlatformInput::ModifiersChanged(gpui::ModifiersChangedEvent {
                            modifiers,
                            capslock: gpui::Capslock::default(),
                        }),
                    )
                });
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
            // `typed <text>`: each character is a key press that goes through the window like the system's keyboard:
            // the key down first (so type-to-focus and the shortcut table see it), then the character to whatever
            // input handler has focus. Unlike `type`, nothing falls back to the composer, so text that has nowhere to
            // land is lost, as it is in the window. A frame is drawn between characters.
            ["typed", ..] => {
                let text = step.trim_start_matches("typed").trim_start();
                for value in text.chars() {
                    let mut stroke = Keystroke {
                        modifiers: Modifiers::default(),
                        key: if value == ' ' {
                            "space".to_owned()
                        } else {
                            value.to_lowercase().to_string()
                        },
                        key_char: Some(value.to_string()),
                    };
                    stroke.modifiers.shift = value.is_uppercase();
                    cx.update(|cx| {
                        let _ = window
                            .update(cx, |_, window, cx| window.dispatch_keystroke(stroke, cx));
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                }
            }
            // `ime-key <stroke>`: one key press with the character the keyboard would deliver for it, like `typed`.
            ["ime-key", stroke] => {
                let stroke: &'static str = Box::leak(stroke.to_string().into_boxed_str());
                if let Ok(parsed) = Keystroke::parse(stroke) {
                    cx.update(|cx| {
                        let _ = window
                            .update(cx, |_, window, cx| window.dispatch_keystroke(parsed, cx));
                    });
                }
            }
            // One committed input operation, like CDP Input.insertText; no system clipboard.
            ["insert-text", ..] => {
                let text = step.trim_start_matches("insert-text").trim_start();
                with_composer(cx, window, &root, |editor, window, cx| {
                    editor.replace_text_in_range(None, text, window, cx);
                });
            }
            ["editor", "state", label @ ..] => {
                with_composer(cx, window, &root, |editor, window, cx| {
                    let selection = editor.selected_text_range(false, window, cx);
                    let _ = writeln!(output, "editor {}: {selection:?}", label.join(" "));
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
            // Capture only: the call simulator (`ui/callsim.rs`).
            #[cfg(feature = "capture")]
            ["callsim", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_callsim(rest, cx));
                let _ = writeln!(output, "callsim: {line}");
            }
            ["games", "demo"] => {
                root.update(cx, |root, cx| root.script_games_demo(cx));
            }
            // Capture only: the game overlay (`ui::overlay`). `overlay shot <png>` renders the overlay window, which
            // the main capture does not show; the others drive a made-up game in the foreground.
            ["overlay", "shot", path] => {
                let path = PathBuf::from(path);
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let line = match handle {
                    Some(handle) => {
                        cx.update(|cx| crate::write_capture(cx, handle, &path));
                        format!("overlay shot: {}", path.display())
                    }
                    None => "overlay shot: no overlay window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            ["rtcdebug", "shot", path] => {
                let path = PathBuf::from(path);
                let handle = root.read_with(cx, |root, cx| root.rtc_debug_window(cx));
                let line = match handle {
                    Some(handle) => {
                        cx.update(|cx| crate::write_capture(cx, handle, &path));
                        format!("rtcdebug shot: {}", path.display())
                    }
                    None => "rtcdebug shot: no rtc debug window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            ["rtcdebug", "click", x, y] => {
                let handle = root.read_with(cx, |root, cx| root.rtc_debug_window(cx));
                let line = match (handle, at(x, y)) {
                    (Some(win), Some(point)) => {
                        cx.update(|cx| {
                            dispatch(cx, win, moved(point, None));
                            dispatch(cx, win, down(point));
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(30))
                            .await;
                        cx.update(|cx| dispatch(cx, win, up(point)));
                        format!("rtcdebug click: {x},{y}")
                    }
                    _ => "rtcdebug click: no rtc debug window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            // `overlay drag <x1> <y1> <x2> <y2>`: a press, moves in 8 steps with a frame between, and a release, sent
            // to the overlay window (a widget's move while unlocked).
            ["overlay", "drag", x1, y1, x2, y2] => {
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let line = match (handle, at(x1, y1), at(x2, y2)) {
                    (Some(overlay), Some(from), Some(to)) => {
                        cx.update(|cx| {
                            dispatch(cx, overlay, moved(from, None));
                            dispatch(cx, overlay, down(from));
                        });
                        for step in 1..=8 {
                            cx.background_executor()
                                .timer(Duration::from_millis(30))
                                .await;
                            let t = step as f32 / 8.;
                            let point = gpui::point(
                                from.x + (to.x - from.x) * t,
                                from.y + (to.y - from.y) * t,
                            );
                            cx.update(|cx| {
                                dispatch(cx, overlay, moved(point, Some(MouseButton::Left)))
                            });
                        }
                        cx.update(|cx| dispatch(cx, overlay, up(to)));
                        format!("overlay drag: {x1},{y1} -> {x2},{y2}")
                    }
                    _ => "overlay drag: no overlay window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            // `overlay click <x> <y>`: a move there, a press and a release with a frame between, in the overlay window.
            ["overlay", "click", x, y] => {
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let line = match (handle, at(x, y)) {
                    (Some(overlay), Some(point)) => {
                        cx.update(|cx| {
                            dispatch(cx, overlay, moved(point, None));
                            dispatch(cx, overlay, down(point));
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(30))
                            .await;
                        cx.update(|cx| dispatch(cx, overlay, up(point)));
                        format!("overlay click: {x},{y}")
                    }
                    _ => "overlay click: no overlay window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            // `overlay key <stroke>`: a key press sent to the overlay window (Enter in the Chat widget's composer).
            ["overlay", "key", stroke] => {
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let stroke: &'static str = Box::leak(stroke.to_string().into_boxed_str());
                let line = match (handle, Keystroke::parse(stroke)) {
                    (Some(overlay), Ok(parsed)) => {
                        cx.update(|cx| key(cx, overlay, parsed));
                        format!("overlay key: {stroke}")
                    }
                    _ => "overlay key: no overlay window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            ["overlay", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_overlay(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Opens a settings page by its label in lower case with dashes, as the captures are named.
            ["settings", page] => {
                let page = (*page).to_owned();
                root.update(cx, |root, cx| root.open_settings_page(&page, cx));
            }
            // `settings-url`: the last address a settings link or connection button would have opened.
            ["settings-url"] => {
                let url = root.update(cx, |root, _| root.settings_last_url());
                let _ = writeln!(output, "settings url: {}", url.unwrap_or_default());
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
            // Calls in the test DM: starts, and official-shaped call dispatches fed to the stores (nothing is sent).
            ["active-now", variant] => {
                let line = root.update(cx, |root, cx| root.script_active_now(variant, cx));
                let _ = writeln!(output, "active-now: {line}");
            }
            ["file-card", variant] => {
                let line = root.update(cx, |root, cx| root.script_file_card(variant, cx));
                let _ = writeln!(output, "file-card: {line}");
            }
            ["upload-card", value] => {
                let line = root.update(cx, |root, cx| root.script_upload_card(value, cx));
                let _ = writeln!(output, "upload-card: {line}");
            }
            ["golive", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_go_live(rest, cx));
                let _ = writeln!(output, "golive: {line}");
            }
            ["dmcall", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_dmcall(rest, cx));
                let _ = writeln!(output, "dmcall: {line}");
            }
            ["pinggraph", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_ping_history(rest, cx));
                let _ = writeln!(output, "pinggraph: {line}");
            }
            // Capture only: the dialogs of the two-factor flow with injected answers (see `script_mfa`).
            // Capture only: the Gift Inventory code box (`gift type <text>`, `gift submit`, `gift accept`, `gift state`).
            ["gift", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_gift(rest, cx));
                let _ = writeln!(output, "gift: {line}");
            }
            // Capture only: the "Boost This Server" dialog as an account with free Boosts sees it (`script_boost_apply`).
            ["boost-apply", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_boost_apply(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["mfa", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_mfa(rest, cx));
                let _ = writeln!(output, "mfa: {line}");
            }
            // Fixture inspection, channel and message creation, and injected dispatches (`ui::ch_fixture`).
            ["ch", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_ch(rest);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "ch: {line}");
            }
            ["voice", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_voice(rest, cx));
                let _ = writeln!(output, "voice: {line}");
            }
            ["stream", "stop"] => {
                root.update(cx, |root, cx| root.stream_stop(cx));
            }
            ["badge", "catalog"] => {
                let line = root.update(cx, |root, _| root.script_badge_catalog());
                let _ = writeln!(output, "badge catalog: {line}");
            }
            ["badge", "shape"] => {
                let line = root.read_with(cx, |root, _| root.script_badge_shape());
                let _ = writeln!(output, "badge shape: {line}");
            }
            ["badge", "state"] => {
                let state = root.read_with(cx, |root, _| root.badge_state_line());
                let _ = writeln!(output, "badge: {state}");
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
            // Capture only: the interface language as the Language select would set it, on this run only (nothing is
            // sent). The catalog is loaded before the step returns, so the next frame is in that language.
            ["locale", code] => {
                let loaded = crate::intl::load_now(code);
                let line = root.update(cx, |root, cx| {
                    let line = root.script_locale(code);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "locale: {line} ({loaded})");
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
            // The calls the text-to-speech engine received (a capture build never speaks aloud).
            // `search last` / `search answer <total>`: the open search's request, and an answer to it.
            ["search", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_search(rest);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "search: {line}");
            }
            ["speech", "log"] => {
                let _ = writeln!(output, "speech: {:?}", crate::speech::spoken_log());
            }
            // Capture only: the account's variation of a user experiment, as a bucket override; and a made-up Nitro
            // profile for the DM's other person (`themed`, `light` or `plain`, then wishlist sku ids).
            ["experiment", name, variant] => {
                let variant = variant.parse::<i64>().unwrap_or(0);
                let name = (*name).to_owned();
                root.update(cx, |root, cx| root.script_experiment(&name, variant, cx));
            }
            ["profile", "demo", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_profile_demo(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["premium", tier] => {
                let tier = tier.parse::<u64>().ok().filter(|tier| *tier > 0);
                root.update(cx, |root, cx| root.script_premium(tier, cx));
            }
            ["spotify", "premium"] => {
                root.update(cx, |root, cx| root.script_spotify_premium(cx));
            }
            ["spotify", "state"] => {
                let state = root.read_with(cx, |root, _| root.script_spotify_state());
                let _ = writeln!(output, "spotify: {state}");
            }
            // Capture only: the body the subscriptions read would return, for one Nitro plan (a plan id, or `none`)
            // and a state (`ok`, `past_due`, `trial`, `pending`); nothing is sent.
            ["subscription", plan, state] => {
                let plan = plan.parse::<u64>().ok();
                let state = (*state).to_owned();
                root.update(cx, |root, cx| root.script_subscription(plan, &state, cx));
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
            // Capture only: animated attachments, DM messages and typing notices in memory (`Root::script_media`).
            ["media", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_media(rest, cx);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "media: {line}");
            }
            ["messages", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_messages(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["pins", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_pins(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `window minimize` / `window activate` change the window's own state, so the caret's wake
            // count can be read with the window minimised (`motion idle`).
            ["window", state @ ("minimize" | "activate")] => {
                let minimize = *state == "minimize";
                let _ = window.update(cx, |_, window, _| {
                    if minimize {
                        window.minimize_window();
                    } else {
                        window.activate_window();
                    }
                });
                let active = window.update(cx, |_, window, _| window.is_window_active());
                let _ = writeln!(output, "window: {state}, active={active:?}");
            }
            // Capture only: `popout click <x> <y>` clicks in the popped-out call window, `popout shot <png>` saves it.
            ["popout", "click", x, y] => {
                let handle = root.read_with(cx, |root, _| root.call_popout_window());
                let line = match (handle, at(x, y)) {
                    (Some(handle), Some(position)) => {
                        cx.update(|cx| {
                            dispatch(cx, handle, moved(position, None));
                            dispatch(cx, handle, down(position));
                            dispatch(cx, handle, up(position));
                        });
                        format!("popout click {x} {y}")
                    }
                    (None, _) => "popout click: no popout window".to_owned(),
                    (_, None) => "popout click: expected <x> <y>".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            ["popout", "shot", path] => {
                let path = PathBuf::from(path);
                let handle = root.read_with(cx, |root, _| root.call_popout_window());
                let line = match handle {
                    Some(handle) => {
                        cx.update(|cx| crate::write_capture(cx, handle, &path));
                        format!("popout shot: {}", path.display())
                    }
                    None => "popout shot: no popout window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            ["call-window", action] => {
                let state = window.update(cx, |_, window, cx| {
                    root.update(cx, |root, cx| root.script_call_window(action, window, cx))
                });
                let _ = writeln!(output, "call-window {action}: {state:?}");
            }
            // Capture only: `link <url>` opens a `discord://` or discord.com link in the app, as a click on it would.
            ["link", url] => {
                let line = root.update(cx, |root, cx| root.script_deep_link(url, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `webhook demo` posts the component messages of `webhook_demo.rs` to the fixture channel.
            ["webhook", "demo"] => {
                root.update(cx, |root, _| root.send_webhook_demo());
                let _ = writeln!(output, "webhook: demo requested");
            }
            // Capture only: `interaction press|create|success|failure|state` (`ui/interaction_flow.rs`).
            ["interaction", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_interaction(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `send hold|release|fail [<status> [<code> [<retry after>]]]|state` (`ui/send_state.rs`).
            ["send", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_send(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `poll demo|create|delete|gateway|state|voters|end` (`ui/poll.rs`).
            ["poll", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_poll(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `layers` logs the popouts and menus that are open, bottom to top (`ui/keyboard/layers.rs`).
            // Capture only: `inert` logs what inert mode refused or skipped (a link that would open the browser, ...).
            ["inert"] => {
                let lines = crate::inert::lines_since(0);
                let _ = writeln!(
                    output,
                    "inert: {} ({})",
                    if crate::inert::on() { "on" } else { "off" },
                    lines.join(" | ")
                );
            }
            // Capture only: `reset` closes everything that is open and goes Home with the window's handle focused
            // (`ui/crawl_reset.rs`), so a script starts from a known place without Escape.
            ["reset"] => {
                let line = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.update(cx, |root, cx| root.script_reset(window, cx))
                        })
                        .unwrap_or_default()
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `server-settings <page>` and `channel-settings <section>` open those full-window pages.
            ["server-settings", page] => {
                let line = root.update(cx, |root, cx| root.script_server_settings(page, cx));
                let _ = writeln!(output, "{line}");
            }
            ["channel-settings", section] => {
                let line = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.update(cx, |root, cx| {
                                root.script_channel_settings(section, window, cx)
                            })
                        })
                        .unwrap_or_default()
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: opens the Forward modal on the newest voice message of the open channel.
            ["vmfwd"] => {
                let line = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.update(cx, |root, cx| root.voice_forward_modal(window, cx))
                        })
                        .unwrap_or_default()
                });
                let _ = writeln!(output, "{line}");
            }
            ["layers"] => {
                let line = root.update(cx, |root, _| root.layers_report());
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `content demo <set>` puts messages with clickable content into the open channel (memory only).
            // Capture only: `vm demo|play|pause|seek|speed|volume|state` drive the voice message player.
            // Capture only: `au play|pause|seek|state` drive the audio attachment cards.
            ["au", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_au(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["vm", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_vm(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `embeds demo <set>` (see `ui/embed_demo.rs`).
            ["embeds", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_embeds(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["content", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_content(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `jump <message id>`, `jump reply` (the first reply whose original is not loaded),
            // `jump present`, `jump state` (see `ui/jump.rs`).
            ["jump", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_jump(rest);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: `threads facts` logs what decides each thread's row in the channel list.
            ["threads", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_threads_with(rest, cx);
                    cx.notify();
                    line
                });
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
            // Capture only: one call-tray Lottie icon drawn over the window (`ui/lottie_icon.rs`).
            ["lottie", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_lottie_icon(rest, cx));
                let _ = writeln!(output, "lottie: {line}");
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
                    "perf sample {}: wall_ms={} cpu_ms={} ui_thread_cpu_ms={} working_set={working} private={private} heap={heap} heap_peak={heap_peak} commit_image={image} commit_mapped={mapped} commit_private={committed_private} threads={} renders={renders}",
                    label.join(" "),
                    crate::perf::since_process_start_ms(),
                    crate::perf::cpu_ms(),
                    crate::perf::thread_cpu_ms(),
                    crate::perf::threads(),
                );
            }
            // `perf ticks <ms> <label>`: what the window does by itself for `ms` (see `ticks.rs`).
            ["perf", "ticks", ms, label @ ..] => {
                let line = ticks::run(ms.parse().unwrap_or(5000), &label.join(" "), cx).await;
                let _ = writeln!(output, "{line}");
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
            ["perf", "threads", which] => {
                let line = crate::perf::threads_report(*which == "begin");
                let _ = writeln!(output, "perf {line}");
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
            // `hitch start` ... `hitch stop <label>`: a frame ticker runs beside the steps in between and the stop reports
            // the frames over 16.7 and 33 ms and the worst ones (see `hitch.rs`).
            ["hitch", "start"] => {
                let line = hitch::start(window, root.clone(), cx);
                let _ = writeln!(output, "{line}");
            }
            ["hitch", "stop", label @ ..] => {
                let line = hitch::stop(&label.join(" "));
                let _ = writeln!(output, "{line}");
            }
            // `frames grab <ms> <x> <y> <w> <h> <label>`: frames back to back with one box read from each (see `grab.rs`).
            ["frames", "grab", rest @ ..] => {
                let line = grab::run(rest, window, cx).await;
                let _ = writeln!(output, "{line}");
            }
            // `frames animate <ms> <label>`: the main view is redrawn back to back for `ms`, as it is while a sticker
            // plays, and the process's CPU time, the frames' times and how much of them the app spent building its
            // views and rows are printed.
            ["frames", "animate", ms, label @ ..] => {
                let ms: u64 = ms.parse().unwrap_or(2000);
                let (builds0, build_ms0, rows0, rows_ms0) = crate::perf::build_totals();
                let cpu0 = crate::perf::cpu_ms();
                let phases0 = gpui::draw_phase_totals();
                let frames0 = gpui::frame_totals();
                let begun = std::time::Instant::now();
                let until = begun + Duration::from_millis(ms);
                let mut samples = Vec::new();
                while std::time::Instant::now() < until {
                    let started = std::time::Instant::now();
                    cx.update(|cx| {
                        root.update(cx, |_, cx| cx.notify());
                        let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
                    });
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                    let spent = started.elapsed();
                    cx.background_executor()
                        .timer(Duration::from_millis(16).saturating_sub(spent))
                        .await;
                }
                let (builds1, build_ms1, rows1, rows_ms1) = crate::perf::build_totals();
                let frames = samples.len().max(1) as f64;
                let phases1 = gpui::draw_phase_totals();
                let frames1 = gpui::frame_totals();
                let own = (frames1.2 - frames0.2).max(1) as f64;
                let draws = (phases1.3 - phases0.3).max(1) as f64;
                let phase = |now: u64, then: u64| (now - then) as f64 / 1e6 / draws;
                let total: f64 = samples.iter().sum();
                let _ = writeln!(
                    output,
                    "frames animate {}: {} frame_mean_ms={:.3} build_mean_ms={:.3} rows_per_frame={:.1} rows_mean_ms_per_frame={:.3} cpu_pct_of_wall={:.1} gpui_request_layout_ms={:.3} gpui_prepaint_ms={:.3} gpui_paint_ms={:.3} own_frames={} own_draw_ms={:.3} own_present_ms={:.3}",
                    label.join(" "),
                    crate::perf::summary(&mut samples, 16.7),
                    total / frames,
                    (build_ms1 - build_ms0) / (builds1 - builds0).max(1) as f64,
                    (rows1 - rows0) as f64 / frames,
                    (rows_ms1 - rows_ms0) / frames,
                    (crate::perf::cpu_ms() - cpu0) as f64 * 100.
                        / begun.elapsed().as_millis().max(1) as f64,
                    phase(phases1.0, phases0.0),
                    phase(phases1.1, phases0.1),
                    phase(phases1.2, phases0.2),
                    frames1.2 - frames0.2,
                    (frames1.0 - frames0.0) as f64 / 1e6 / own,
                    (frames1.1 - frames0.1) as f64 / 1e6 / own,
                );
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
            ["keyboard-explainer", command] => {
                let report =
                    root.update(cx, |root, cx| root.script_keyboard_explainer(command, cx));
                let _ = writeln!(output, "keyboard-explainer: {report}");
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
            ["unread", "posts", forum] => {
                // Every active post of the forum gets a message newer than anything acked.
                if let Ok(forum) = forum.parse::<u64>() {
                    cx.update(|cx| {
                        root.update(cx, |root, cx| {
                            root.script_unread_posts(forum, cx);
                        })
                    });
                }
            }
            ["unread", channel] => {
                // A message newer than anything acked arrived in `channel`.
                if let Ok(channel) = channel.parse::<u64>() {
                    cx.update(|cx| {
                        root.update(cx, |root, cx| {
                            root.script_unread(channel, cx);
                        })
                    });
                }
            }
            ["gif", "dump", dir] => {
                let dir = std::path::PathBuf::from(*dir);
                let _ = std::fs::create_dir_all(&dir);
                let count = root.read_with(cx, |root, _| root.gif_dump(&dir));
                let _ = writeln!(output, "gif dump: {count} files");
            }
            // `avatar state`: the cached users whose avatar is animated (`a_`), as user id and guild-or-global.
            // Capture only: `avatar demo` gives people, the open server's icon and one banner an animated picture.
            ["avatar", "demo"] => {
                let line = root.update(cx, |root, cx| root.script_avatar_demo(cx));
                let _ = writeln!(output, "{line}");
            }
            ["avatar", "state"] => {
                let report = root.read_with(cx, |root, _| root.avatar_report());
                let _ = writeln!(output, "avatar state: {report}");
            }
            ["avatar", "member", name] => {
                let found = root.update(cx, |root, cx| {
                    let found = root.avatar_member(name);
                    cx.notify();
                    found
                });
                let _ = writeln!(output, "avatar member {name}: {found:?}");
            }
            ["avatar", "goto", nth] => {
                let nth: usize = nth.parse().unwrap_or(0);
                let row = root.update(cx, |root, cx| {
                    let row = root.avatar_goto(nth);
                    cx.notify();
                    row
                });
                let _ = writeln!(output, "avatar goto {nth}: row={row:?}");
            }
            ["gif", "favorites"] => {
                let report = root.read_with(cx, |root, _| root.gif_favorites_report());
                let _ = writeln!(output, "gif favorites: {report}");
            }
            ["gif", "state"] => {
                let report = root.read_with(cx, |root, _| root.gif_report());
                let _ = writeln!(output, "gif state: {report}");
            }
            ["gif", "seed", url, src, format, width, height] => {
                if let (Ok(format), Ok(width), Ok(height)) =
                    (format.parse(), width.parse(), height.parse())
                {
                    root.update(cx, |root, cx| {
                        root.seed_gif_favorite_as(url, src, format, (width, height), true, cx)
                    });
                    let _ = writeln!(output, "gif favourite added in memory");
                }
            }
            // As `gif seed`, but nothing is requested: the list changes as a late settings arrival would.
            ["gif", "seed-late", url, src, format, width, height] => {
                if let (Ok(format), Ok(width), Ok(height)) =
                    (format.parse(), width.parse(), height.parse())
                {
                    root.update(cx, |root, cx| {
                        root.seed_gif_favorite_as(url, src, format, (width, height), false, cx)
                    });
                    let _ = writeln!(output, "gif favourite added late in memory");
                }
            }
            ["gif", "seed-url", url, width, height] => {
                if let (Ok(width), Ok(height)) = (width.parse(), height.parse()) {
                    root.update(cx, |root, cx| {
                        root.seed_gif_favorite(url, width, height, cx)
                    });
                    let _ = writeln!(output, "gif fixture added in memory");
                }
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
