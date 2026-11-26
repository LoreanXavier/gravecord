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
mod settle;
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
            // `ready [ms]`: waits (at most `ms`, 20 s by default) for the live READY. With `GRAVECORD_CACHE=1` a script
            // starts on the cached screen, and the live READY that follows replaces what earlier `gw` steps put in
            // stores READY owns (CONNECTION_OPEN replaces the geo-restricted servers of module 543897, for one).
            ["ready", rest @ ..] => {
                let limit: u64 = rest
                    .first()
                    .and_then(|ms| ms.parse().ok())
                    .unwrap_or(20_000);
                let started = std::time::Instant::now();
                let mut live = false;
                while started.elapsed() < Duration::from_millis(limit) {
                    live = cx.update(|cx| root.read(cx).live_ready());
                    if live {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                }
                let _ = writeln!(
                    output,
                    "ready: {} after {} ms",
                    if live { "live" } else { "timed out" },
                    started.elapsed().as_millis()
                );
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
            // `native deactivate` / `native activate`: the window loses and regains activation as with Alt+Tab
            // (`WM_ACTIVATE` through its own procedure, on the hidden desktop only).
            ["native", which @ ("activate" | "deactivate")] => {
                let active = *which == "activate";
                let result = window
                    .update(cx, |_, window, _| native::activate(window, active))
                    .map_err(|error| error.to_string())
                    .and_then(|result| result);
                let _ = writeln!(output, "native {which}: {result:?}");
            }
            // `blur`: nothing in the window has focus, as `Window::blur` leaves it.
            ["blur"] => {
                let _ = window.update(cx, |_, window, cx| window.blur(cx));
                let _ = writeln!(output, "blurred");
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
            ["context-user"]
            | ["context-dm"]
            | ["context-self"]
            | ["context-overlay-dm"]
            | ["context-game-shop-sku"] => {
                let kind = step.strip_prefix("context-").unwrap_or("user");
                let note =
                    cx.update(|cx| root.update(cx, |root, cx| root.script_context(kind, cx)));
                let _ = writeln!(output, "{note}");
            }
            // Capture only: `full-profile <user id> [board|activity|wishlist]` opens that person's full profile.
            ["full-profile", user, rest @ ..] => {
                let user = user.parse::<u64>().unwrap_or(0);
                let tab = rest.first().copied();
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| {
                            root.script_full_profile(user, tab, window, cx)
                        })
                    })
                });
                let _ = writeln!(output, "{note:?}");
            }
            // Capture only: `profile-popout <user id>` opens that person's popout as if their name at 400,80 was pressed.
            ["profile-popout", user] => {
                let user = user.parse::<u64>().unwrap_or(0);
                let note = root.update(cx, |root, cx| root.script_profile_popout(user, cx));
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
            // `server-welcome new`: the server in view counts as created just now (module 302125's cards).
            ["server-welcome", "new"] => {
                crate::ui::server_welcome_new::AS_NEW
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = writeln!(output, "server-welcome: new");
            }
            // `tutorial reset [confirmed ids...]`: the account's tutorial tips unsuppressed (READY's `tutorial`, nothing
            // sent); `tutorial open <id>` opens a shown tip's popout, `tutorial close` closes it (`ui/tutorial_tips.rs`).
            ["tutorial", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_tutorial(rest, cx));
                let _ = writeln!(output, "tutorial: {line}");
            }
            // `view-as-role start <role id|everyone> [guild id]`, `view-as-role picker|stop|state`: View Server As
            // Role, local only (`ui/view_as_role.rs`).
            ["view-as-role", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_view_as_role(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // `limited-access`: a request refused for limited access, as `net` reports it (module 218642).
            ["limited-access"] => {
                cx.update(|cx| {
                    root.update(cx, |root, cx| {
                        root.update(crate::net::Update::LimitedAccess, cx)
                    })
                });
                let _ = writeln!(output, "limited-access: reported");
            }
            // Whether the stored login held for the whole run (`net::login_refused`).
            ["signed-in"] => {
                let refused = crate::net::login_refused();
                let _ = writeln!(output, "signed-in: {}", if refused { "no" } else { "yes" });
            }
            // Capture only: `fetched <key> <json>` hands the view an answer of one of its reads as if the network had
            // brought it (local fixtures for pages the test account has no data for, such as its connections).
            ["fetched", key, json @ ..] => {
                let text = json.join(" ");
                let note = match serde_json::from_str(&text) {
                    Ok(body) => {
                        let key = (*key).to_owned();
                        cx.update(|cx| {
                            root.update(cx, |root, cx| {
                                root.update(crate::net::Update::Fetched { key, body }, cx)
                            })
                        });
                        "fetched: delivered".to_owned()
                    }
                    Err(error) => format!("fetched: not json: {error}"),
                };
                let _ = writeln!(output, "{note}");
            }
            ["gw", name, json @ ..] => {
                // `{dm_user}` stands for the open DM's recipient (local fixtures for someone else's presence).
                let recipient = root.read_with(cx, |root, _| root.script_dm_user());
                let mut text = json.join(" ");
                if let Some(id) = recipient {
                    text = text.replace("{dm_user}", &id);
                }
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
            // Capture only: `join-request <SUBMITTED|REJECTED|none> [reason...]` makes the account a pending member of
            // the open server with that request of its own (`ui/screening.rs`), nothing sent.
            ["join-request", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_join_request(rest, cx));
                let _ = writeln!(output, "join-request: {line}");
            }
            // Capture only: `invite-error <code>`: an invite card's Join answered with that abort code.
            // Capture only: the header invite as an account without Create Invite sees it (module 132610).
            // Capture only: the open server under a verification level (`ui/verification_gate.rs`).
            ["verification", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_verification(rest, cx));
                let _ = writeln!(output, "verification: {line}");
            }
            ["invite-permission"] => {
                root.update(cx, |root, cx| root.script_invite_permission(cx));
                let _ = writeln!(output, "invite-permission: shown");
            }
            // Capture only: `frames-announcement` opens "New in the Shop: Profile Frames" (module 627781) whatever the
            // account's dismissals.
            // Capture only: `storefront <app id>` opens the application storefront modal (module 719847) as its Store
            // button does.
            ["storefront", app] => {
                let app = (*app).to_owned();
                root.update(cx, |root, cx| root.open_app_storefront(app, None, cx));
                let _ = writeln!(output, "storefront: opened");
            }
            // Capture only: `not-spam <channel>` presses Mark as Not Spam of that spam request (module 190857).
            ["not-spam", channel] => {
                let channel = channel.parse::<u64>().unwrap_or(0);
                root.update(cx, |root, cx| root.mark_not_spam(channel, cx));
                let _ = writeln!(output, "not-spam: pressed");
            }
            ["frames-announcement"] => {
                root.update(cx, |root, cx| root.script_frames_announcement(cx));
                let _ = writeln!(output, "frames-announcement: shown");
            }
            ["invite-error", code] => {
                let line = root.update(cx, |root, cx| root.script_invite_error(code, cx));
                let _ = writeln!(output, "invite-error: {line}");
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
            // `click-with ctrl|shift|alt|ctrl+shift x y`: a click whose press and release carry those keys, as a click
            // with the keys held does (window.modifiers() reads the last event's).
            ["click-with", held, x, y] => {
                if let Some(position) = at(x, y) {
                    let modifiers = Modifiers {
                        shift: held.contains("shift"),
                        control: held.contains("ctrl"),
                        alt: held.contains("alt"),
                        ..Modifiers::default()
                    };
                    let with = |input: PlatformInput| match input {
                        PlatformInput::MouseDown(mut event) => {
                            event.modifiers = modifiers;
                            PlatformInput::MouseDown(event)
                        }
                        PlatformInput::MouseUp(mut event) => {
                            event.modifiers = modifiers;
                            PlatformInput::MouseUp(event)
                        }
                        other => other,
                    };
                    cx.update(|cx| {
                        dispatch(cx, window, moved(position, None));
                        dispatch(cx, window, with(down(position)));
                        dispatch(cx, window, with(up(position)));
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
            // The file dialog of an image upload site answers with this file (the dialog is never opened in a capture
            // run): the cropper of module 142630 opens over it.
            ["image-pick", path] => {
                let path = std::path::PathBuf::from(*path);
                root.update(cx, |root, cx| root.crop_file_picked(&path, cx));
            }
            // The file the Create Server dialog's icon input (module 356343) would get from the system dialog.
            ["add-server-icon", path] => {
                let path = std::path::PathBuf::from(*path);
                root.update(cx, |root, cx| root.add_server_icon_file(path, cx));
            }
            // `image-crop state`: the open image dialogs, the cropper's transform and its last announcement.
            ["image-crop", "state"] => {
                let line = root.update(cx, |root, _| root.image_crop_state());
                let _ = writeln!(output, "{line}");
            }
            // `image-crop recent <n>`: stand-in recent avatars for the select modal.
            ["image-crop", "recent", count] => {
                let count = count.parse().unwrap_or(0);
                root.update(cx, |root, cx| root.harness_recent_avatars(count, cx));
            }
            // `image-crop open <purpose>`: the select image modal of a site whose button is not on a reachable page.
            ["image-crop", "open", purpose] => {
                let purpose = purpose.to_string();
                root.update(cx, |root, cx| root.harness_open_image_select(&purpose, cx));
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
            // Drags a file over the window to a point and holds it there (no drop): the drop area shows.
            ["dragover", path, x, y] => {
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
                    });
                }
            }
            // The dragged file leaves the window.
            ["dragexit"] => {
                cx.update(|cx| {
                    dispatch(
                        cx,
                        window,
                        PlatformInput::FileDrop(gpui::FileDropEvent::Exited),
                    );
                });
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
            // `overlay rightclick <x> <y>`: a right press and release in the overlay window (a context menu).
            ["overlay", "rightclick", x, y] => {
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let line = match (handle, at(x, y)) {
                    (Some(overlay), Some(point)) => {
                        cx.update(|cx| {
                            dispatch(cx, overlay, moved(point, None));
                            dispatch(cx, overlay, right_down(point));
                            dispatch(cx, overlay, right_up(point));
                        });
                        format!("overlay rightclick: {x},{y}")
                    }
                    _ => "overlay rightclick: no overlay window".to_owned(),
                };
                let _ = writeln!(output, "{line}");
            }
            // `overlay move <x> <y>`: the pointer moves there in the overlay window (a hover, a tooltip).
            ["overlay", "move", x, y] => {
                let handle = root.read_with(cx, |root, _| root.overlay_window());
                let line = match (handle, at(x, y)) {
                    (Some(overlay), Some(point)) => {
                        cx.update(|cx| dispatch(cx, overlay, moved(point, None)));
                        format!("overlay move: {x},{y}")
                    }
                    _ => "overlay move: no overlay window".to_owned(),
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
            // Built-in plugins (`ui/settings/mods.rs`): state, switches, settings, an unfolded card, an import.
            ["plugin", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_plugin(rest, cx);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "plugin: {line}");
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
            ["connect-error", kind] => {
                let line = root.update(cx, |root, cx| root.script_connect_error(kind, cx));
                let _ = writeln!(output, "{line}");
            }
            ["connect-open", kind] => {
                let line = root.update(cx, |root, cx| root.script_connect_open(kind, cx));
                let _ = writeln!(output, "{line}");
            }
            ["connect-state"] => {
                let line = root.read_with(cx, |root, _| root.script_connect_state());
                let _ = writeln!(output, "{line}");
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
            // Capture only: the call, stream and report surveys (`ui::feedback`).
            #[cfg(feature = "capture")]
            ["feedback", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_feedback(rest, cx));
                let _ = writeln!(output, "feedback: {line}");
            }
            // Capture only: a stream's Report a problem (`ui::stream_report`).
            #[cfg(feature = "capture")]
            ["streamreport", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_stream_report(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // Capture only: the AV warnings (`ui::av_errors`) and a watched stream that fails (`av fail <owner>`).
            #[cfg(feature = "capture")]
            ["av", "fail", owner] => {
                // `focused`: the owner of the stream on screen.
                let owner = if *owner == "focused" {
                    root.read_with(cx, |root, _| root.focused_tile_owner())
                } else {
                    owner.parse::<u64>().ok()
                };
                let line = match owner {
                    Some(owner) => root.update(cx, |root, cx| root.script_stream_fail(owner, cx)),
                    None => "usage: av fail <owner id|focused>".to_owned(),
                };
                let _ = writeln!(output, "av: {line}");
            }
            #[cfg(feature = "capture")]
            ["av", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_av(rest, cx));
                let _ = writeln!(output, "av: {line}");
            }
            // Capture only: the video picture-in-picture window (`ui::video_pip`).
            #[cfg(feature = "capture")]
            ["pip", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_pip(rest, cx));
                let _ = writeln!(output, "pip: {line}");
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
            // Capture only: `image state <prefix>`: the pictures whose key starts with the prefix and how they stand.
            ["image", "state", prefix @ ..] => {
                let prefix = prefix.join(" ");
                let state = root.read_with(cx, |root, _| root.image_state_line(&prefix));
                let _ = writeln!(output, "image: {state}");
            }
            // Capture only: `image online`: the gateway connected again, as after a network drop.
            ["image", "online"] => {
                cx.update(|cx| {
                    root.update(cx, |root, cx| {
                        root.update(crate::net::Update::GatewayConnected(true), cx)
                    })
                });
                let _ = writeln!(output, "image: online");
            }
            ["badge", "state"] => {
                let state = root.read_with(cx, |root, _| root.badge_state_line());
                let _ = writeln!(output, "badge: {state}");
            }
            ["activity", "state"] => {
                let state = root.read_with(cx, |root, _| root.activity_state_line());
                let _ = writeln!(output, "activity: {state}");
            }
            ["activity", "failed", code @ ..] => {
                let code = code.first().and_then(|code| code.parse().ok());
                let line = root.update(cx, |root, cx| {
                    let line = root.script_activity_failed(code);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "activity {line}");
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
            // `graphic urgent | download <url>`: the dialogs of `ui/graphic_dialogs.rs`.
            // `birthday stage | open <source> | nuf | set | pick | confirm | loading | submit | answer | state`: the age
            // gate of `ui/birthday.rs` on a staged "no date of birth" state; nothing is ever sent.
            ["birthday", rest @ ..] => {
                let note = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_birthday(rest, window, cx))
                    })
                });
                let _ = writeln!(output, "{note:?}");
            }
            ["graphic", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_graphic(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            // `unread-notice young | sent | state`: the store of `ui/unread_notice.rs`, in memory.
            ["unread-notice", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_unread_notice(rest);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // `voice-silence-dialog`: "We can't hear you!" with its Don't show checkbox (`ui/settings/voice/extra.rs`).
            ["voice-silence-dialog"] => {
                root.update(cx, |root, cx| {
                    root.script_voice_silence_dialog();
                    cx.notify();
                });
            }
            // `onboarding-blocked`: the open server in the onboarding blocked state (`ui/member_onboarding.rs`), in memory.
            ["onboarding-blocked"] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_onboarding_blocked();
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // `emoji bulk-errors`: "Failed to upload files" with one row of each kind (`ui/server_emoji.rs`).
            ["emoji", "bulk-errors"] => {
                let line = root.update(cx, |root, cx| root.script_emoji_bulk_errors(cx));
                let _ = writeln!(output, "{line}");
            }
            // `emoji-studio [path]`: opens Emoji Studio (module 894399) on a file or synthetic test PNG.
            ["emoji-studio", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_open_emoji_studio(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `forum-demo [upsell|book|game]`: opens Forum Channels demo modal (module 774538).
            ["forum-demo", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_open_forum_demo(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `soundboard-echo [popout|details]`: stages Soundboard Echo pills & details popout (module 120246).
            ["soundboard-echo", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_soundboard_echo(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `role-verify [step0|step1|step2]`: opens Role Connection Verification modal (module 488358).
            ["role-verify", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_role_connection_verify(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `safety-tools [actions|tips|about]`: opens DM Safety Tools modal (module 556127).
            ["safety-tools", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_safety_tools(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `invite-accept [expanded|stream|app|group|friend|error]`: opens Invite Accept modal (module 700156).
            ["invite-accept", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_invite_accept(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `oauth-success [guild|user|external|error]`: opens OAuth2 Authorized Success page (module 595244).
            ["oauth-success", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_oauth_success(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-confirm [activated|updated|basic|credits|guild|pending]`: opens Nitro confirmation (module 231308).
            ["nitro-confirm", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_confirm(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `invite-no-suggestions`: opens Invite modal in the no-suggestions variant (module 137811).
            ["invite-no-suggestions"] => {
                let line = root.update(cx, |root, cx| root.script_invite_no_suggestions(cx));
                let _ = writeln!(output, "{line}");
            }
            // `invite-target [stream|activity|stage|event]`: opens targeted Invite modal (module 520006 / 137811).
            ["invite-target", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_invite_target(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `onboarding-connections`: opens Guild Onboarding with connection cards (module 528275).
            ["onboarding-connections"] => {
                let line = root.update(cx, |root, cx| root.script_onboarding_connections(cx));
                let _ = writeln!(output, "{line}");
            }
            // `activity-share`: opens Activity / Frame Share modal (module 529999).
            ["activity-share"] => {
                let line = root.update(cx, |root, cx| root.script_activity_share(cx));
                let _ = writeln!(output, "{line}");
            }
            // `phase3ah [trial-upsell|affinity|guide-bar]`: seeds Phase 3ah visual surfaces.
            ["phase3ah", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_phase3ah(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `monetization-eligibility`: opens Server Subscriptions monetization eligibility checklist (module 715395).
            ["monetization-eligibility"] => {
                let line = root.update(cx, |root, cx| root.script_monetization_eligibility(cx));
                let _ = writeln!(output, "{line}");
            }
            // `command-permissions [lockout|picker]`: opens Application Command Permissions editor (module 365200).
            ["command-permissions", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_command_permissions(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-bar <expand|collapse|console|console-connected|warning|clear>`: controls the Quest Bar (66246).
            ["quest-bar", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_bar(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `billing-address [us|ca|errors|ca-errors|intl]`: opens Billing Address form modal (module 718718).
            ["billing-address", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_billing_address(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-upsell [sticker|trial|upload|length|emoji|burst|theme]`: opens Nitro offer/upsell modal (811611).
            ["nitro-upsell", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_upsell(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `gift-ready [ready|sent|bulk|error]`: opens Gift Ready delivery modal (module 998035).
            ["gift-ready", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_gift_ready(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `restricted-profile [blocked|ignored]`: opens blocked/ignored profile confirmation modal (148540).
            ["restricted-profile", rest @ ..] => {
                let arg = rest.join(" ");
                let line = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| {
                            root.script_restricted_profile(&arg, window, cx)
                        })
                    })
                    .unwrap_or_default();
                let _ = writeln!(output, "{line}");
            }
            // `boost-cancel [confirm|preview|canceled|external|fractional|error]`: Cancel Server Boost modal (983511).
            ["boost-cancel", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_boost_cancel(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `library-game-menu [installed|uninstalled|hide|fallback]`: Game Library context menu (873079).
            ["library-game-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_library_game_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `channel-link [edit|confirm|noperm]`: Channel Link edit / unlink modals (106895, 428185, 422542).
            ["channel-link", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_channel_link(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `hub-add-server [choose|templates|new|last|confirm]`: Student Hubs Add a Server modal (668822).
            ["hub-add-server", rest @ ..] => {
                let arg = rest.join(" ");
                let line = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_hub_add_server(&arg, window, cx))
                    })
                    .unwrap_or_default();
                let _ = writeln!(output, "{line}");
            }
            // `role-sub [cards|tip|welcome]`: Role Subscription Tier Cards & Welcome modal (31637).
            ["role-sub", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_role_sub(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `search-feedback [bad|step2|good]`: Search Feedback Survey modal (770950).
            ["search-feedback", rest @ ..] => {
                let arg = rest.join(" ");
                let line = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_search_feedback(&arg, window, cx))
                    })
                    .unwrap_or_default();
                let _ = writeln!(output, "{line}");
            }
            // `payment-picker [default|streamer]`: Payment Source Picker modal (637141).
            ["payment-picker", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_payment_picker(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `notification-center [rows|empty]`: Notification Center Inbox overlay (617823).
            ["notification-center", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_notification_center(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `shop-gift-cta [friend|self|generic|connected|error]`: Wishlist & Game Shop Gift CTA modal (909004).
            ["shop-gift-cta", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_shop_gift_cta(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `soundmoji [nitro|join|default|volume|noperm]`: Soundmoji popout & volume slider (456086).
            ["soundmoji", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_soundmoji(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `server-shop [products|subs|empty|preview]`: Server Shop storefront modal (38137).
            ["server-shop", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_server_shop(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `onboarding-flow [cover|connections|prompt|rules]`: Member-facing Server Onboarding flow (215764).
            ["onboarding-flow", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_onboarding_flow(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-referral [select|remind|sent|error|rowerr|bannererr]`: Share Nitro Trial Friend Picker (581846).
            ["nitro-referral", rest @ ..] => {
                let arg = rest.join(" ");
                let line = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_nitro_referral(&arg, window, cx))
                    })
                    .unwrap_or_default();
                let _ = writeln!(output, "{line}");
            }
            // `shop-checkout-review [open|closed|paypal]`: Shop Checkout Review modal (582695).
            ["shop-checkout-review", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_shop_checkout_review(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `join-request-page [rejected|submitted|incomplete|withdraw|discard]`: Join Request applicant page (840336).
            ["join-request-page", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_join_request_page(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `app-sub-cancel [survey|confirm]`: Premium App Subscription Cancel & Survey modal (548750).
            ["app-sub-cancel", rest @ ..] => {
                let arg = rest.join(" ");
                let line = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_app_sub_cancel(&arg, window, cx))
                    })
                    .unwrap_or_default();
                let _ = writeln!(output, "{line}");
            }
            // `violation-detail [eligible|coppa|expired|appealed]`: Safety Hub Violation Detail modal (163002).
            ["violation-detail", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_violation_detail(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `purchase-waivers [trial|discount|err|unchecked]`: Purchase Legal Waivers & Install modal (357465).
            ["purchase-waivers", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_purchase_waivers(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `game-gift-redeem [success|link|claim|owned|exhausted]`: Game Item Gift Redeem modal (173971).
            ["game-gift-redeem", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_game_gift_redeem(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `subscription-pause [select|paused|confirm|error]`: Pause/Cancel Nitro Subscription modal (318998).
            ["subscription-pause", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_subscription_pause(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `violation-appeal [speed-bump|signal|confirm|sent|close]`: Violation Appeal modal (894683 + 351876).
            ["violation-appeal", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_violation_appeal(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `suspended-hub [underage|verified|manual-review|loading|error|close]`: Suspended Safety Hub (187686 + 37380).
            ["suspended-hub", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_suspended_safety_hub(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `trials-picker [select|ineligible|error|reminder|confirm|close]`: Send Nitro Trials Picker (303682 + 810852).
            ["trials-picker", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_trials_picker(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `user-activity-card [open|close]`: Legacy UserActivity card preview (663249 + 961272).
            ["user-activity-card", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_user_activity_card(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `link-context [href]`: Standalone native link context menu (945896).
            ["link-context", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_link_context(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `iframe-modal [open|close]`: Interaction iframe modal (949548).
            ["iframe-modal", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_iframe_modal(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `age-verify-modal [default|stage|close]`: Age Verification Get Started modal (929180).
            ["age-verify-modal", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_age_verify_modal(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `premium-announcement [open|close]`: Nitro Announcement Modal Variant 1 (962263).
            ["premium-announcement", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_premium_announcement(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `guild-space [open|edit|close]`: Guild Space Server Hub page & widget editor (930417).
            ["guild-space", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_guild_space(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `checkpoint-2026 [home|summary|squad|close]`: Checkpoint 2026 full-screen layer (925433).
            ["checkpoint-2026", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_checkpoint_2026(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `game-server-success [created|downgrade|close]`: Game Server Created/Updated modal (978084).
            ["game-server-success", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_game_server_success(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `library-launch-menu [open|close]`: Library Application Launch Options menu (980411).
            ["library-launch-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_library_launch_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `soundboard-nux [open|close]`: Soundboard Desktop NUX coachmark popover (987933).
            ["soundboard-nux", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_soundboard_nux(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `copy-combo <tier 1..11|reset>`: Copy combo easter egg green tooltip (206845).
            ["copy-combo", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_copy_combo(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `premium-uncancel [notice|modal|modal-tier1|confirmed|close]`: Keep Nitro winback notice & modal (174705).
            ["premium-uncancel", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_premium_uncancel(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-menu [open|disclosure|error|unverified|close]`: Quest card context menu & disclosure modal (270045 + 348559 + 201805).
            ["quest-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-promoted [open|blocked|close]`: Quest Promoted Card & enrollment blocked modal (107543 + 124232).
            ["quest-promoted", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_promoted(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-split [connect|connected|error|close]`: SplitQuestModal account connect flow (968779 + 976019 + 491145).
            ["quest-split", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_split(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `claim-account [form|required|error|success|close]`: Unclaimed account Finish Signing Up modal (110744).
            ["claim-account", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_claim_account(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `transfer-ownership [confirm|monetized|pincode|ineligible|close]`: Guild Transfer Ownership flow (922269 + 79779).
            ["transfer-ownership", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_transfer_ownership(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-mgmt [alerts|squad-primary|squad-member|discount|preview-cancel|preview-cancel-expanded|pending-change|welcome-back|close]`: Nitro subscription management & cancel preview (488110 + 347378 + 95337 + 702698 + 412314 + 456459).
            ["nitro-mgmt", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_mgmt(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `shop-collections [promos|vault|close]`: Shop Collection Detail, Vault, Related Drops & Seasonal Promos (19576 + 367234).
            ["shop-collections", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_shop_collections(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `changelog [loaded|loading|out-of-date|failed|close]`: What's New changelog modal (837601).
            ["changelog", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_changelog(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-survey [open|video|submitted|close]`: Quest dismiss survey modal (321538 + 798899).
            ["quest-survey", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_survey(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `app-test-mode [open|proxy|active|error|close]`: Application Test Mode modal (868511).
            ["app-test-mode", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_app_test_mode(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `accept-invite [loading|invalid|valid|limit|activity|close]`: Accept Invite modal (654000 + 700156).
            ["accept-invite", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_accept_invite(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `schedule-upsell [open|close]`: Scheduled message non-Nitro limit upsell modal (859403 + 853861).
            ["schedule-upsell", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_schedule_upsell(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `app-widget-state [syncing|connect|reconnect|close]`: Application Game Stats Widget sync/connect/reconnect states (591410).
            ["app-widget-state", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_app_widget_state(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `forum-tag-editor [create|edit|close]`: Forum Tag Editor modal (950989).
            ["forum-tag-editor", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_forum_tag_editor(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-call-sheet [call-unenrolled|call-progress|call-completed|sheet|sheet-error|close]`: Quest Call Header Card & Sponsored Quest Sheet (10394 + 194267).
            ["quest-call-sheet", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_call_sheet(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `game-leaderboard [podium|empty|sharing|popular|close]`: Guild Space Game Leaderboard & Popular Games/Music (220744).
            ["game-leaderboard", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_game_leaderboard(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `icymi [open|close]`: ICYMI ("In Case You Missed It") feed page (411831).
            ["icymi", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_icymi(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `verify-email [resend|resent|change|close]`: Verify by Email modal (9124).
            ["verify-email", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_verify_email(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `server-shop-success [open|error|close]`: Server Shop Product Purchase Success modal (779457).
            ["server-shop-success", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_server_shop_success(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-share-trial [profile|hd|hd-error|confirmed|close]`: Single-recipient Share a Nitro Trial modal (536110).
            ["nitro-share-trial", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_share_trial(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `raid-alerts [paused-both|paused-dms|paused-invites|raid|dm-spam|modal|close]`: Raid alerts & Security Actions (731204 + 671576).
            ["raid-alerts", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_raid_alerts(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-resubscribe [confirm|gift|gift-tax|error|success|close]`: Resubscribe to Nitro 2-step modal (674809).
            ["nitro-resubscribe", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_resubscribe(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-reward-modal [claimed|loading|error|member-completed|member-progress|member-available|close]`: Quest Reward Split Modal (670092) & Members List Quest Popout (198525).
            ["quest-reward-modal", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_reward_modal(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `boost-uncancel [confirm|credits|error|saved|close]`: Uncancel Server Boost 2-step modal (342744).
            ["boost-uncancel", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_boost_uncancel(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `server-subs [storefront|past-due|mobile|not-member|going-away|close]`: Server Subscriptions Storefront (602844 + 456540).
            ["server-subs", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_server_subscriptions(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `monetization-terms [open|demonetized|error|close]`: Creator Monetization Terms Update modal (710113).
            ["monetization-terms", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_monetization_terms(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nitro-offer-header [trial|two-week|discount|gift-payment|gift-review|close]`: Nitro Purchase/Gift modal header & offer pills (683166).
            ["nitro-offer-header", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nitro_offer_header(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `third-party-promo [locked|unlocked|claimed|error|close]`: Partner Promotions modal (452417 / 116011).
            ["third-party-promo", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_third_party_promo(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `overlay-voice-switcher [open|empty|search <q>|close]`: Overlay Voice Channel Switcher popout (594831).
            ["overlay-voice-switcher", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_overlay_voice_switcher(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `overlay-stream-menu [open|close]`: Overlay Stream Video widget context menu (663912).
            ["overlay-stream-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_overlay_stream_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `pick-guild-settings [default|onboarding|template|close]`: Guild Settings Picker / Creator Onboarding (623509).
            ["pick-guild-settings", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_pick_guild_settings(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `orb-wallet [active|cleared|error|unclaimed|delivered|delivered-gift|close]`: Orb Wallet Popout (150849) & Orbs Delivered modal (717983).
            ["orb-wallet", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_orb_wallet(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `clear-activity-history [game|watch|listening|error|close]`: Clear Activity History modal (839785).
            ["clear-activity-history", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_clear_activity_history(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `badge-menu [reorder|staff|tenure|hidden|close]`: Badge Customization menu (842810).
            ["badge-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_badge_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `moderation-user-menu [open|completed|close]`: Raid / Safety Alert User menu (485304).
            ["moderation-user-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_moderation_user_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `powerup-perk [theme|roles|tag|tag-active|vanity|disable-confirm|close]`: GuildPowerupPerkModal (945942).
            ["powerup-perk", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_powerup_perk(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `powerup-level [1|2|3|active|disabled|close]`: GuildPowerupLevelModal (784140).
            ["powerup-level", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_powerup_level(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `powerup-multi-perk [open|close]`: GuildPowerupsMultiPerkModal (789267).
            ["powerup-multi-perk", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_powerup_multi_perk(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `coupon-complete [storefront|pdp|close]`: Social Layer Coupon Complete Modal (783481).
            ["coupon-complete", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_coupon_complete(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `game-shop-popover [sku|asset|video|close]`: Guild Sidebar Game Shop Promo Popover (849134).
            ["game-shop-popover", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_game_shop_popover(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `member-bulk-bar [open|modal|close]`: Member Safety Bulk Selection Bar (856768).
            ["member-bulk-bar", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_member_bulk_bar(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `text-context [text]`: Standalone Text Selection Context Menu (898811).
            ["text-context", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_text_selection_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `activity-overflow [perms|plain]`: Activity Popout Overflow Menu (913525).
            ["activity-overflow", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_activity_overflow_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `friends-popout [open|all|pending|close]`: Title Bar Friends List Popout (824042).
            ["friends-popout", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_friends_popout(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `gift-promo-notice [open|close]`: Gift Promotion Reminder Nagbar (823901).
            ["gift-promo-notice", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_gift_promo_notice(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `shop-nagbar [default|nitro|close]`: Collectibles Shop Nagbar (814124).
            ["shop-nagbar", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_shop_nagbar(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `partner-reward [locked|claimed|error|close]`: Recurring Partner Reward Code Card (116011).
            ["partner-reward", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_partner_reward(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `wishlist-nux [sku|close]`: "Added to your Wishlist!" NUX modal (38884).
            ["wishlist-nux", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_wishlist_nux(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `add-connection [open|close]`: Add Connection modal with Apps & Claimed Games (58829).
            ["add-connection", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_add_connection(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `research-survey [active|exit|done|loading|close]`: Discord Research Survey modal (215978).
            ["research-survey", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_research_survey(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `guild-theme-nux [guild|personal|error|close]`: Guild Theme NUX modal (166998).
            ["guild-theme-nux", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_guild_theme_nux(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `conjure-menu [menu|delete|unpublish|remove|close]`: Conjure Project Actions Menu (753329).
            ["conjure-menu", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_conjure_menu(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nuf-complete [open|close]`: NUF server-ready celebration slide (109766).
            ["nuf-complete", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_nuf_complete(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `e2ee-verify [new|verified|devices|close]`: E2EE User Verification modal (730713).
            ["e2ee-verify", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_e2ee_verify(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-conn-helper [detected|error|close]`: Quest Connection Helper modal (988436).
            ["quest-conn-helper", rest @ ..] => {
                let arg = rest.join(" ");
                let line =
                    root.update(cx, |root, cx| root.script_quest_connection_helper(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `role-icon-upload [upload|error|emoji|close]`: Custom Role Icon Upload / Expression Picker modal (126061).
            ["role-icon-upload", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_role_icon_upload(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-activity-header [coachmark|popout|unenrolled|close]`: Quest Activity Header & Coachmark (131862).
            ["quest-activity-header", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_activity_header(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `web-presence [overview|picker|review|close]`: Server Web Presence / Discoverable Content picker (284305).
            ["web-presence", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_web_presence(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `evolving-tenure [subscriber|upsell|other|close]`: Nitro Evolving Tenure Badge modal (384048).
            ["evolving-tenure", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_evolving_tenure(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `guild-space-nux [published|draft|close]`: Welcome to your Server Hub NUX modal (725041).
            ["guild-space-nux", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_guild_space_nux(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `tenure-level-up [tier_idx|close]`: Tiered Nitro Tenure Badge Level-Up Popout (748579).
            ["tenure-level-up", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_tenure_level_up(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `stream-privacy-code [loading|close]`: Stream Privacy Code modal (773262).
            ["stream-privacy-code", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_stream_privacy_code(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `voice-invite-suggestions [open|close]`: Voice Invite Suggestions popover (778768).
            ["voice-invite-suggestions", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| {
                    root.script_voice_invite_suggestions(&arg, cx)
                });
                let _ = writeln!(output, "{line}");
            }
            // `purchase-email-auth [pending|resent|error|close]`: Checkout Email Verification & Predicate Error (615456).
            ["purchase-email-auth", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_purchase_email_auth(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `quest-bounty [card|menu|close]`: Quest Bar Bounty Card & popout menu (682908).
            ["quest-bounty", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_quest_bounty(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `activity-invite [spotify|stream|game|expired|close]`: Activity invite / listening party / stream request embeds (584283).
            ["activity-invite", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_activity_invite(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `grant-access [text|stage|skip|close]`: Grant Channel Access modal (685374).
            ["grant-access", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_grant_channel_access(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `sensitive-media [manage|restricted|age|close]`: Why we hide some media explainer (743461).
            ["sensitive-media", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_sensitive_media(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `forum-post-perms [open|error|close]`: Who can create a post? forum modal (698325).
            ["forum-post-perms", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_forum_post_perms(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `monetization-team [teams|empty|terms|close]`: Server monetization team onboarding (75834).
            ["monetization-team", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_monetization_team(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `app-guild-sub [open|empty|close]`: App guild-subscription Server Selection modal (399143).
            ["app-guild-sub", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_app_guild_sub(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `nuf-channel-prompt [open|filled|close]`: NUF CHANNEL_PROMPT slide (362311).
            ["nuf-channel-prompt", rest @ ..] => {
                let arg = rest.join(" ");
                let line = cx.update(|cx| {
                    window.update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| {
                            root.script_nuf_channel_prompt(&arg, window, cx)
                        })
                    })
                });
                let _ = writeln!(output, "{line:?}");
            }
            // `orb-multiplier [upsell|nitro|xbox|close]`: Quest Orbs Multiplier Info / Upsell modal (50292).
            ["orb-multiplier", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_orb_multiplier(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `forum-default-reaction [open|picker|error|close]`: Choose a default reaction modal (939940).
            ["forum-default-reaction", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_forum_default_reaction(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `shop-embed [buy|orbs|sub-current|unavailable|close]`: Social Layer product embed & plan buttons (50777 + 376374).
            ["shop-embed", rest @ ..] => {
                let arg = rest.join(" ");
                let line = root.update(cx, |root, cx| root.script_shop_embed(&arg, cx));
                let _ = writeln!(output, "{line}");
            }
            // `search last` / `search answer <total>`: the open search's request, and an answer to it;
            // `search page <n>`: the pager turns to page n.
            ["search", rest @ ..] => {
                let line = root.update(cx, |root, cx| {
                    let line = match rest {
                        ["page", page] => root.script_search_page(page.parse().unwrap_or(1), cx),
                        _ => root.script_search(rest),
                    };
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
            // Capture only: a guild's bucket of a guild experiment, as the client's guild bucket override
            // (`guild-experiment <name> <guild> <bucket>`, -1 not eligible).
            ["guild-experiment", name, guild, bucket] => {
                let line = root.update(cx, |root, cx| {
                    root.script_guild_experiment(name, guild, bucket, cx)
                });
                let _ = writeln!(output, "guild-experiment: {line}");
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
            // Capture only: a DM recipient as a bot with an authorized application (module 891522 `s9`).
            ["app-dm", user, application, name @ ..] => {
                let user = user.parse().unwrap_or(0);
                let name = name.join(" ");
                let line = root.update(cx, |root, cx| {
                    let line = root.script_app_dm(user, application, &name);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: a voice alert by name (`reload-activity`).
            ["voice-alert", name] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_voice_alert(name);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: a step of the Nitro cancellation flow (module 673761) over the open settings.
            ["cancel-nitro", step] => {
                let line = root.update(cx, |root, cx| {
                    let line = root.script_cancel_nitro(step);
                    cx.notify();
                    line
                });
                let _ = writeln!(output, "{line}");
            }
            // Capture only: saved payment sources for the Billing page (`demo` or `none`); nothing is sent.
            ["payment-sources", which] => {
                let demo = *which == "demo";
                root.update(cx, |root, cx| root.script_payment_sources(demo, cx));
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
            // Capture only: `window restore` restores the window after `window minimize`; `window move <x> <y>` puts
            // its top-left corner at a screen point, as a drag of the title bar does.
            ["window", "iconic"] => {
                let result =
                    window.update(cx, |_, window, _| native::minimize_or_restore(window, true));
                let _ = writeln!(output, "window: iconic {result:?}");
            }
            ["window", "restore"] => {
                let result = window.update(cx, |_, window, _| {
                    native::minimize_or_restore(window, false)
                });
                let _ = writeln!(output, "window: restore {result:?}");
            }
            ["window", "move", x, y] => {
                let result = match (x.parse::<i32>(), y.parse::<i32>()) {
                    (Ok(x), Ok(y)) => window
                        .update(cx, |_, window, _| native::move_window(window, x, y))
                        .map_err(|error| error.to_string())
                        .and_then(|result| result),
                    _ => Err("expected <x> <y>".to_owned()),
                };
                let _ = writeln!(output, "window: move {result:?}");
            }
            // Capture only: `webframes` reads the web views' windows back from Win32 against the chat's viewport.
            ["webframes"] => {
                let chat = root.read_with(cx, |root, _| root.script_chat_viewport());
                let report = window
                    .update(cx, |_, window, _| native::web_surfaces(window, chat))
                    .unwrap_or_else(|error| format!("webframes: {error}\n"));
                output.push_str(&report);
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
            // What the shortcuts and Escape act on (`Root::script_keys_state`).
            ["keys", "state", label @ ..] => {
                let report = window
                    .update(cx, |_, window, cx| root.read(cx).script_keys_state(window, cx))
                    .unwrap_or_else(|_| "window closed".to_owned());
                let _ = writeln!(output, "keys {}: {report}", label.join(" "));
            }
            // Capture only: `popout key <stroke>` presses a key in the popped-out call window.
            ["popout", "key", stroke] => {
                let handle = root.read_with(cx, |root, _| root.call_popout_window());
                let stroke: &'static str = Box::leak(stroke.to_string().into_boxed_str());
                let line = match (handle, Keystroke::parse(stroke)) {
                    (Some(handle), Ok(parsed)) => {
                        cx.update(|cx| key(cx, handle, parsed));
                        format!("popout key {stroke}")
                    }
                    (None, _) => "popout key: no popout window".to_owned(),
                    (_, Err(_)) => format!("popout key: cannot parse {stroke}"),
                };
                let _ = writeln!(output, "{line}");
            }
            // `shot <png>`: the main window as it is now, so one run can keep several frames (one sign-in for many
            // checks).
            ["shot", path] => {
                let path = PathBuf::from(path);
                cx.update(|cx| crate::write_capture(cx, window, &path));
                let _ = writeln!(output, "shot: {}", path.display());
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
            // Capture only: `send hold|release|fail [<status> [<code> [<retry after>]]]|edit-fail <status> <code>|state`
            // (`ui/send_state.rs`).
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
            // The features of every server the account is in, as the stores hold them.
            ["guild-features"] => {
                let lines = root.read_with(cx, |root, _| root.script_guild_features());
                let _ = writeln!(output, "{lines}");
            }
            ["invites", "demo"] => {
                let line = root.update(cx, |root, cx| root.script_invites_demo(cx));
                let _ = writeln!(output, "{line}");
            }
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
            // `gifs still`: every GIF tile shows its first frame from now on (`gif_tiles.rs`), whatever the real
            // clock says; plugins-off uses it so an animation cannot tell two runs apart.
            ["gifs", "still"] => {
                crate::ui::gif_tiles::hold_still(true);
                cx.update(|cx| root.update(cx, |_, cx| cx.notify()));
                let _ = writeln!(output, "gifs: still");
            }
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
            // Capture only: `family-center on` turns the Family Center tab setting on locally (nothing is sent in an
            // inert run), `family-center disable` opens Disable Family Center (module 371158).
            // Capture only: `standin <name>` opens a dialog or menu whose opener the fixture cannot reach (see
            // `script_standin`): `follow-success`, `geo-unavailable`, ...
            ["standin", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_standin(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["family-center", which] => {
                let line = root.update(cx, |root, cx| root.script_family_center(which, cx));
                let _ = writeln!(output, "{line}");
            }
            ["profile-editor", rest @ ..] => {
                let line = root.update(cx, |root, cx| root.script_profile_editor(rest, cx));
                let _ = writeln!(output, "{line}");
            }
            ["image-context", rest @ ..] => {
                let url = rest
                    .first()
                    .copied()
                    .unwrap_or("https://media.discordapp.net/attachments/1/2/shot.png");
                let line = root.update(cx, |root, cx| root.script_image_context(url, cx));
                let _ = writeln!(output, "{line}");
            }
            // Measurements, see `perf.rs`: `perf sample <label>` (process counters), `perf caches` (sizes of the app's
            // own caches), `perf start` (when the content and the loading screen's end showed).
            ["perf", "watch", "begin"] => crate::perf::watch_begin(),
            ["perf", "watch", "end", label @ ..] => {
                let _ = writeln!(
                    output,
                    "perf watch {}: {}",
                    label.join(" "),
                    crate::perf::watch_end()
                );
            }
            ["perf", "sample", label @ ..] => {
                let renders = crate::perf::RENDERS.load(std::sync::atomic::Ordering::Relaxed);
                let (working, private, private_peak) = crate::perf::memory();
                let (heap, heap_peak) = crate::perf::heap();
                let (image, mapped, committed_private) = crate::perf::committed();
                let _ = writeln!(
                    output,
                    "perf sample {}: wall_ms={} cpu_ms={} ui_thread_cpu_ms={} working_set={working} private={private} private_peak={private_peak} heap={heap} heap_peak={heap_peak} commit_image={image} commit_mapped={mapped} commit_private={committed_private} threads={} renders={renders}",
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
                    "perf start: fonts_ms={} window_ms={} ready_ms={} content_ms={} splash_done_ms={} {} {}",
                    crate::perf::FONTS_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::WINDOW_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::READY_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::CONTENT_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::SPLASH_DONE_MS.load(std::sync::atomic::Ordering::Relaxed),
                    crate::perf::checkpoints(),
                    crate::start_trace::fields(),
                );
            }
            // `perf timeline`: every start stage reached so far (`start_trace`), in milliseconds since the process
            // started.
            ["perf", "timeline"] => {
                let _ = writeln!(output, "perf timeline: {}", crate::start_trace::fields());
            }
            // `perf pictures`: how many of the pictures the start waits for are absent, queued, decoding or decoded,
            // by the kind of key (the part before its first `:`).
            ["perf", "pictures"] => {
                let states = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.start_picture_states(window, cx))
                    })
                    .unwrap_or_default();
                let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
                for (key, state) in states {
                    let kind = if key.starts_with("http") {
                        "url"
                    } else {
                        key.split(':').next().unwrap_or("")
                    };
                    *counts.entry(format!("{kind}.{state}")).or_default() += 1;
                }
                let line: Vec<String> = counts.iter().map(|(k, n)| format!("{k}={n}")).collect();
                let _ = writeln!(output, "perf pictures: {}", line.join(" "));
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
            // `settle <max ms>`: until the window has stopped changing for a second (see `settle.rs`).
            ["settle", rest @ ..] => {
                let line = settle::run(rest, window, cx).await;
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
            ["sticker-upsell", mode] => {
                let msg = root.update(cx, |root, cx| root.script_sticker_upsell(mode, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["nitro-required", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_nitro_required(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["template-accept", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_template_accept(&arg, window, cx))
                    })
                    .unwrap_or_else(|_| "template-accept: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
            }
            ["shop-error", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_shop_error(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["stage-preview"] => {
                let msg = root.update(cx, |root, cx| root.script_stage_preview(cx));
                let _ = writeln!(output, "{msg}");
            }
            ["clips-coachmark", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_clips_coachmark(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["conjure-secrets", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_conjure_secrets(&arg, window, cx))
                    })
                    .unwrap_or_else(|_| "conjure-secrets: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
            }
            ["content-warning", mode] => {
                let msg = root.update(cx, |root, cx| root.script_content_warning(mode, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["video-quest", mode] => {
                let msg = root.update(cx, |root, cx| root.script_video_quest(mode, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["event-rsvp-picker", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_event_rsvp_picker(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["activity-pip", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_activity_pip(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["game-platform-picker", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_game_platform_picker(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["lurker-popout", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_lurker_popout(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["legacy-chat-modal", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_legacy_chat_modal(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["clips-capture-mode", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_clips_capture_mode(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["storefront-share", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| {
                            root.script_storefront_share(&arg, window, cx)
                        })
                    })
                    .unwrap_or_else(|_| "storefront-share: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
            }
            ["rail-overflow"] => {
                let msg = root.update(cx, |root, cx| root.script_rail_overflow(cx));
                let _ = writeln!(output, "{msg}");
            }
            ["rail-scroll-bottom"] => {
                let msg = root.update(cx, |root, cx| root.script_rail_scroll_bottom(cx));
                let _ = writeln!(output, "{msg}");
            }
            ["community-updates-popout", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| {
                    root.script_community_updates_popout(&arg, cx)
                });
                let _ = writeln!(output, "{msg}");
            }
            ["family-request", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| {
                    root.script_family_request_connection(&arg, cx)
                });
                let _ = writeln!(output, "{msg}");
            }
            ["family-accept", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_family_accept_link(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["guild-room-selector", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_guild_room_selector(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["gift-card-promo", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_gift_card_promo(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["game-org-error", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_game_org_error(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["app-channel", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_app_channel(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["picker-upsell", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_picker_upsell(&arg, window, cx))
                    })
                    .unwrap_or_else(|_| "picker-upsell: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
            }
            ["join-request-reject", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_join_request_reject(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["guild-friends", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_guild_friends(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["powerup-coachmark", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_powerup_coachmark(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["safety-flows", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_safety_flows(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["conjure-workspace", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_conjure_workspace(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["family-disconnect", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_family_disconnect_link(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["family-spending-cap", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_family_spending_cap(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["in-game-auth-check", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_in_game_auth_check(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["conjure-agent-browser", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_conjure_agent_browser(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["conjure-aux", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_conjure_aux_modal(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["payment-auth", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_payment_auth(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["gift-coachmark", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_gift_coachmark(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["family-teen-qr", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_family_teen_qr(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["family-downtime", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_family_downtime(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["add-payment", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_add_payment_method(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["avatar-stack", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_avatar_stack(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["keyboard-dnd", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_keyboard_dnd(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["how-boosting-works", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = root.update(cx, |root, cx| root.script_how_boosting_works(&arg, cx));
                let _ = writeln!(output, "{msg}");
            }
            ["share-moment", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.script_share_moment(&arg, window, cx))
                    })
                    .unwrap_or_else(|_| "share-moment: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
            }
            ["share-custom-theme", rest @ ..] => {
                let arg = rest.join(" ");
                let msg = window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| {
                            root.script_share_custom_theme(&arg, window, cx)
                        })
                    })
                    .unwrap_or_else(|_| "share-custom-theme: window closed".to_owned());
                let _ = writeln!(output, "{msg}");
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
