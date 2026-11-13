//! The game overlay window (Overlay v3, the pinned build's out-of-process overlay; `dc_overlay` holds the rules).
//!
//! One process, one state: the window is a second gpui window whose view reads the app's `Root`, as the popped-out
//! call does (`call_popout.rs`). No second gateway or voice connection exists for it.
//!
//! - Shown while a running game this client tracks is the foreground window, the overlay is on globally and for that
//!   game, and its window is shown windowed, maximized or borderless (`dc_overlay::decision`). No call is needed:
//!   locked with nobody to list, the voice widget draws nothing (module 502812).
//! - Placed over the game's client area, topmost, click-through and never activated while locked; unlocked (the
//!   `TOGGLE_OVERLAY_INPUT_LOCK` keybind, Shift+` by default) it takes input, dims the game with `--background-scrim`
//!   and shows the toolbar and each widget's frame (`overlay_widgets.rs`). Escape locks it again (module 427830).
//! - Locked, the clickable regions (module 302614) take clicks: the cursor is followed and the window is made
//!   click-through everywhere else (`dc_overlay::platform::set_click_through`).
//! - The widgets come from `LayoutStore`'s `overlay_v3_default` layout, created at version 4 when the window opens
//!   (module 237984 `Vz`); moved while unlocked and stored as fractions of the window (`dc_overlay::drag`).
//! - The notifications widget lists `OverlayNotificationsStore`'s queue (`dc_overlay::notify`): the welcome nudge a
//!   second after the overlay is mounted, text messages a desktop notification would announce, and calls that ring.
//! - Kept out of screen captures (`WDA_EXCLUDEFROMCAPTURE`), out of the taskbar and Alt+Tab (`WS_EX_TOOLWINDOW`).
//!
//! A harness run never starts the foreground watcher, the cursor follower or the keybind (`kb_sync_hooks`); it opens
//! the window only on the harness's hidden desktop, and `overlay shot` renders it to a picture.

use super::motion::{Spring, SpringConfig};
use super::{Root, avatar_source, kerning};
use dc_overlay::decision::{self, Foreground, OverlayMethod, Rect};
use dc_overlay::layout::{self, Layout};
use dc_overlay::notify::{self, Kind, Options, Priority, Queue};
use dc_overlay::store::{self, AvatarSize, DisplayNameMode, OverlaySettings};
use dc_overlay::{voice, zones};
use dc_state::Id;
use dc_state::running_games::Game;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, Render, Rgba, WeakEntity, Window,
    WindowHandle, canvas, div, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// The harness's hidden desktop (`DESKTOP` of `xtask/src/headless.rs`).
const HIDDEN_DESKTOP: &str = "gravecord-headless";

/// `username_ieJ4Sq` and `hiddenVoiceStatesText_ieJ4Sq`: `rgba(19,19,24,.8)`.
const PILL: u32 = 0x131318cc;
/// `connectedAnimationContainer_ieJ4Sq`: `#131318`.
const JOINED: u32 = 0x131318ff;

/// The overlay is forced to the Onyx theme (`themeOverride: ONYX`, drawn with the `theme-midnight` class).
pub(super) fn onyx(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

/// Whether the overlay window may open here: always in a normal run; in a harness run only on its hidden desktop.
pub(super) fn window_allowed_on(capture: bool, desktop: Option<&str>) -> bool {
    !capture || desktop == Some(HIDDEN_DESKTOP)
}

fn window_allowed() -> bool {
    #[cfg(windows)]
    let desktop = dc_overlay::platform::desktop_name();
    #[cfg(not(windows))]
    let desktop: Option<String> = None;
    // Until the window has been seen over a real game (click-through, transparency, staying on top), a normal run
    // opens it only when asked to with `GRAVECORD_OVERLAY=1`: a window that failed at any of those would cover the
    // game of whoever runs this build. BLOCKED.md, "Injected overlay anticheat compatibility".
    let capture = crate::storage::is_capture_run();
    if !capture && std::env::var_os("GRAVECORD_OVERLAY").is_none_or(|value| value != "1") {
        return false;
    }
    window_allowed_on(capture, desktop.as_deref())
}

/// The window's root view: the overlay of the app's `Root`, at this window's size.
pub(crate) struct OverlayView {
    root: WeakEntity<Root>,
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = window.viewport_size();
        let (width, height) = (f32::from(area.width), f32::from(area.height));
        let body = self.root.upgrade().map(|root| {
            root.update(cx, |root, cx| {
                root.overlay.scale = window.scale_factor();
                let body = root.overlay_body(width, height, cx);
                (body, root.overlay.animating)
            })
        });
        let animating = body.as_ref().is_some_and(|(_, animating)| *animating);
        if animating {
            window.request_animation_frame();
        }
        div()
            .id("overlay-root")
            .size_full()
            .font_family(super::SANS)
            .font_features(kerning())
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                // Escape locks the unlocked overlay (module 427830).
                if event.keystroke.key == "escape" {
                    let _ = this.root.update(cx, |root, cx| {
                        if !root.overlay.locked {
                            root.overlay_toggle_lock(cx);
                        }
                    });
                }
            }))
            .children(body.map(|(body, _)| body))
    }
}

/// A made-up game in the foreground, for the harness.
#[derive(Clone)]
struct Demo {
    foreground: Foreground,
    game: Game,
}

/// A widget held by the pointer.
pub(super) struct WidgetDrag {
    pub id: String,
    pub drag: dc_overlay::drag::Drag,
    pub min_size: (f32, f32),
}

/// What a toast's click or buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToastAction {
    /// The toast itself: unlocks a locked overlay, or opens the message's channel.
    Click,
    Confirm,
    Cancel,
    Dismiss,
}

/// The kinds of toast this client raises (module 348639).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToastKind {
    /// `OVERLAY_MOUNTED`'s welcome nudge.
    Welcome,
    /// `MESSAGE_CREATE`.
    Text,
    /// `CALL_CREATE` / `CALL_UPDATE` ringing this account.
    Call,
}

/// One toast's content (the props of module 376113's `x`).
#[derive(Clone, Debug)]
pub(super) struct Toast {
    pub kind: ToastKind,
    /// The image key of a 40 px picture.
    pub icon: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub hint: Option<String>,
    pub max_lines: Option<usize>,
    pub confirm: Option<String>,
    pub cancel: Option<String>,
    pub click: Option<ToastAction>,
    pub channel: Option<Id>,
    /// `disableClickableRegions` of the toast itself.
    pub no_zone: bool,
}

/// What the overlay holds between frames.
pub(super) struct Overlay {
    /// The foreground window the watcher last reported (none while unknown).
    foreground: Option<Foreground>,
    /// The game the overlay follows: kept while the overlay itself has the focus.
    focused: Option<Foreground>,
    /// `OverlayStore.isLocked(pid)`: locked unless unlocked by the keybind.
    pub(super) locked: bool,
    window: Option<WindowHandle<OverlayView>>,
    hwnd: Option<isize>,
    opening: bool,
    /// Where the window is shown now, and with which lock state its styles were set.
    shown: Option<(Rect, bool)>,
    #[cfg(windows)]
    watcher: Option<dc_overlay::platform::Watcher>,
    demo: Option<Demo>,
    /// The overlay's own clock (ms), for its transitions, springs and notification timers.
    clock: Instant,
    /// When the lock last changed (ms on the clock).
    lock_changed: f64,
    /// The window's size at its last draw (logical px) and its scale.
    pub(super) size: (f32, f32),
    scale: f32,
    pub(super) drag: Option<WidgetDrag>,
    /// Bounds of the widgets and toasts at their last paint, in the overlay window.
    pub(super) bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// The click zones painted in the last frame.
    zone_bounds: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    pub(super) notices: Queue<Toast>,
    tick_at: Option<f64>,
    /// The game the overlay was mounted for, and when (the welcome nudge's clock).
    mounted: Option<u32>,
    /// When each member of the call was first seen (`connectedOn` of `SortedVoiceStateStore`), per channel.
    connected: (Option<Id>, HashMap<Id, f64>),
    /// When each row appeared (the `eS` hook's mount).
    rows_mounted: HashMap<Id, f64>,
    springs: HashMap<(u64, u8), Spring>,
    /// The ringing channels toasts were raised for.
    calls: Vec<Id>,
    /// Click-through as last set while locked (the cursor follower).
    through: Option<bool>,
    follower: bool,
    pub(super) widgets_panel: bool,
    /// Something is still moving: the window asks for another frame.
    pub(super) animating: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            foreground: None,
            focused: None,
            locked: true,
            window: None,
            hwnd: None,
            opening: false,
            shown: None,
            #[cfg(windows)]
            watcher: None,
            demo: None,
            clock: Instant::now(),
            lock_changed: f64::NEG_INFINITY,
            size: (decision::MIN_WIDTH as f32, decision::MIN_HEIGHT as f32),
            scale: 1.,
            drag: None,
            bounds: Rc::default(),
            zone_bounds: Rc::default(),
            notices: Queue::default(),
            tick_at: None,
            mounted: None,
            connected: (None, HashMap::new()),
            rows_mounted: HashMap::new(),
            springs: HashMap::new(),
            calls: Vec::new(),
            through: None,
            follower: false,
            widgets_panel: false,
            animating: false,
        }
    }
}

/// The "joined voice" pill of a row as drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Joined {
    pub opacity: f32,
    pub width: f32,
    /// `justConnected_ieJ4Sq`.
    pub just_connected: bool,
}

/// One row of the voice widget.
pub(super) struct Row {
    pub id: u64,
    pub name: String,
    pub avatar: Option<std::sync::Arc<gpui::Image>>,
    pub speaking: bool,
    pub mute: bool,
    pub server_mute: bool,
    pub deaf: bool,
    pub server_deaf: bool,
    pub video: bool,
    /// This account (`M`): it never shows the joined pill.
    pub current: bool,
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn hwnd_of(_: &Window) -> Option<isize> {
    None
}

// ----- the stores -----

impl Root {
    fn overlay_user(&self) -> Option<String> {
        self.stores
            .current_user
            .as_ref()
            .map(|user| user.id.to_string())
    }

    /// `OverlayStoreV2` for the signed-in account (the defaults when it holds nothing).
    pub(in crate::ui) fn overlay_settings(&self) -> OverlaySettings {
        let stored = crate::storage::get(store::OVERLAY_STORE).unwrap_or(Value::Null);
        self.overlay_user()
            .map(|user| store::settings_for(&stored, &user))
            .unwrap_or_default()
    }

    pub(in crate::ui) fn set_overlay_settings(&mut self, settings: &OverlaySettings) {
        let Some(user) = self.overlay_user() else {
            return;
        };
        let stored = crate::storage::get(store::OVERLAY_STORE).unwrap_or(Value::Null);
        crate::storage::set(
            store::OVERLAY_STORE,
            store::with_settings(&stored, &user, settings),
        );
    }

    /// `OverlayStore6`.
    pub(in crate::ui) fn overlay_global(&self) -> decision::Global {
        store::enabled(&crate::storage::get(store::ENABLED_STORE).unwrap_or(Value::Null))
    }

    pub(in crate::ui) fn overlay_layout(&self) -> Layout {
        Layout::from_json(&crate::storage::get(layout::STORE).unwrap_or(Value::Null))
    }

    pub(in crate::ui) fn save_overlay_layout(&mut self, layout: &Layout) {
        crate::storage::set(layout::STORE, layout.to_json());
    }

    /// A `LayoutStore` action on the overlay's layout, stored when it changed something.
    pub(super) fn overlay_change_layout(&mut self, change: impl FnOnce(&mut Layout) -> bool) {
        let mut layout = self.overlay_layout();
        if change(&mut layout) {
            self.save_overlay_layout(&layout);
        }
    }

    pub(in crate::ui) fn overlay_notification_disabled(&self, setting: &str) -> bool {
        store::notification_disabled(
            &crate::storage::get(store::SETTINGS_STORE).unwrap_or(Value::Null),
            setting,
        )
    }

    pub(in crate::ui) fn set_overlay_notification_disabled(
        &mut self,
        setting: &str,
        disabled: bool,
    ) {
        let stored = crate::storage::get(store::SETTINGS_STORE).unwrap_or(Value::Null);
        crate::storage::set(
            store::SETTINGS_STORE,
            store::set_notification_disabled(&stored, setting, disabled),
        );
    }

    /// The overlay's clock in ms.
    pub(super) fn overlay_now(&self) -> f64 {
        self.overlay.clock.elapsed().as_secs_f64() * 1000.
    }
}

/// A fresh widget id (`LayoutStore` uses random UUIDs).
pub(super) fn new_widget_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ----- the decision and the window -----

impl Root {
    /// The game the focused window belongs to, when it is a running game this client tracks.
    fn overlay_focused_game(&self) -> Option<(Foreground, Game)> {
        let focused = self.overlay.focused?;
        if let Some(demo) = &self.overlay.demo
            && demo.foreground.pid == focused.pid
        {
            return Some((focused, demo.game.clone()));
        }
        let game = self
            .detect
            .store
            .running_games()
            .iter()
            .find(|game| game.pid == Some(focused.pid))?
            .clone();
        Some((focused, game))
    }

    /// The running game shown on the Game Overlay page's Current Game row: the focused one, else the first running
    /// one (`i8` of module 158935 reads the store's current game).
    pub(in crate::ui) fn overlay_current_game(&self) -> Option<Game> {
        if let Some((_, game)) = self.overlay_focused_game() {
            return Some(game);
        }
        if let Some(demo) = &self.overlay.demo {
            return Some(demo.game.clone());
        }
        self.detect
            .store
            .running_games()
            .iter()
            .find(|game| !game.is_launcher)
            .cloned()
    }

    /// `eY` for a game as the overlay reads it.
    pub(in crate::ui) fn overlay_game_enablement(&self, game: &Game) -> decision::GameEnablement {
        let global = self.overlay_global();
        let (user_legacy, user_oop) = self.detect.store.overlay_override(game);
        let facts = decision::GameFacts {
            is_launcher: game.is_launcher,
            user_legacy,
            user_oop,
            database_overlay: None,
            in_database: game.id.is_some(),
            excluded: false,
        };
        decision::game_enablement(&facts, true, global.legacy)
    }

    /// Where the window is shown now and how, or none: `dc_overlay::decision::visible`.
    pub(super) fn overlay_target(&self) -> Option<(OverlayMethod, Rect)> {
        let global = self.overlay_global();
        let focused = self.overlay_focused_game().map(|(foreground, game)| {
            let enabled = self.overlay_game_enablement(&game);
            let size = foreground.client;
            (
                decision::Focused {
                    pid: foreground.pid,
                    fullscreen: foreground.fullscreen,
                    width: size.width.max(0) as u32,
                    height: size.height.max(0) as u32,
                },
                enabled,
                size,
            )
        });
        let inputs = decision::Inputs {
            authenticated: self.stores.current_user.is_some(),
            global,
            oop_supported: true,
            focused: focused.map(|(focused, enabled, _)| (focused, enabled)),
            limited_interaction: false,
        };
        let method = decision::visible(&inputs)?;
        Some((method, focused?.2))
    }

    /// The method the Current Game row reports for the focused game (`getTrackedGameByPid(pid).overlayMethod`).
    pub(in crate::ui) fn overlay_current_method(&self) -> Option<OverlayMethod> {
        self.overlay_target().map(|(method, _)| method)
    }

    /// The foreground watcher reported a change.
    pub(super) fn overlay_focus_changed(
        &mut self,
        foreground: Option<Foreground>,
        cx: &mut Context<Self>,
    ) {
        self.overlay.foreground = foreground;
        // The overlay window itself in front (unlocked) keeps the game it covers.
        let own = std::process::id();
        let next = decision::next_focus(
            self.overlay.focused.map(|focused| focused.pid),
            foreground.map(|foreground| foreground.pid),
            own,
        );
        self.overlay.focused = match (next, foreground) {
            (Some(pid), Some(foreground)) if foreground.pid == pid => Some(foreground),
            (Some(_), _) => self.overlay.focused,
            (None, _) => None,
        };
        self.overlay_sync(cx);
    }

    /// The watcher runs while signed in, outside harness runs.
    fn overlay_watch(&mut self) {
        #[cfg(windows)]
        if self.overlay.watcher.is_none()
            && !crate::storage::is_capture_run()
            && self.stores.current_user.is_some()
        {
            let updates = self.updates.clone();
            match dc_overlay::platform::watch(move |foreground| {
                let _ = updates.send(crate::net::Update::OverlayFocus(foreground));
            }) {
                Ok(watcher) => self.overlay.watcher = Some(watcher),
                Err(error) => tracing::warn!("overlay foreground watcher not started: {error}"),
            }
        }
    }

    /// Brings the window in line with the decision: opened, placed and styled, or hidden (and locked again).
    pub(super) fn overlay_sync(&mut self, cx: &mut Context<Self>) {
        self.overlay_watch();
        let target = self.overlay_target();
        match target {
            Some((_, rect)) => {
                if self.overlay.window.is_none() {
                    self.overlay_open_window(cx);
                    return;
                }
                let locked = self.overlay.locked;
                if self.overlay.shown == Some((rect, locked)) {
                    return;
                }
                let first = self.overlay.shown.is_none();
                self.overlay.shown = Some((rect, locked));
                self.overlay.through = None;
                #[cfg(windows)]
                if let Some(hwnd) = self.overlay.hwnd {
                    dc_overlay::platform::style(hwnd, locked);
                    dc_overlay::platform::place(hwnd, rect);
                }
                tracing::info!(
                    "overlay shown {}x{} at {},{} locked={locked}",
                    rect.width,
                    rect.height,
                    rect.x,
                    rect.y
                );
                if first {
                    self.overlay_mounted(cx);
                }
                cx.notify();
            }
            None => {
                // A hidden overlay comes back locked (the window resets to locked when it is made again).
                self.overlay.locked = true;
                self.overlay.drag = None;
                if self.overlay.shown.take().is_some() {
                    #[cfg(windows)]
                    if let Some(hwnd) = self.overlay.hwnd {
                        dc_overlay::platform::hide(hwnd);
                    }
                    tracing::info!("overlay hidden");
                    cx.notify();
                }
            }
        }
    }

    /// The overlay came up over a game (module 427830 `tf`): the layout exists at version 4 (module 237984), the
    /// cursor follower starts, and a second later the welcome nudge (`overlayMounted`, `OVERLAY_MOUNTED`).
    fn overlay_mounted(&mut self, cx: &mut Context<Self>) {
        let mut layout = self.overlay_layout();
        let before = layout.to_json();
        layout.ensure(&mut new_widget_id);
        if layout.to_json() != before {
            self.save_overlay_layout(&layout);
        }
        self.overlay_follow_cursor(cx);
        let pid = self.overlay.focused.map(|focused| focused.pid);
        if pid.is_none() || pid == self.overlay.mounted {
            return;
        }
        self.overlay.mounted = pid;
        let now = self.overlay_now();
        self.overlay.notices.tidy_text(0, now);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(notify::MOUNT_DELAY as u64))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.overlay.shown.is_some() {
                    this.overlay_welcome(cx);
                }
            });
        })
        .detach();
    }

    /// Each frame of the main view while a game has the focus: a setting changed on the page shows at once, and the
    /// calls that ring us are matched with their toasts.
    pub(super) fn overlay_frame(&mut self, cx: &mut Context<Self>) {
        if self.overlay.focused.is_some() || self.overlay.shown.is_some() {
            self.overlay_sync(cx);
        }
        if self.overlay.shown.is_some() {
            self.overlay_calls(cx);
        }
    }

    fn overlay_open_window(&mut self, cx: &mut Context<Self>) {
        if self.overlay.opening || !window_allowed() {
            return;
        }
        self.overlay.opening = true;
        let root = cx.entity().downgrade();
        // The window draws its first frame as it opens, and that frame reads this view.
        cx.defer(move |cx| {
            let bounds = gpui::Bounds::new(
                gpui::point(px(0.), px(0.)),
                gpui::size(
                    px(decision::MIN_WIDTH as f32),
                    px(decision::MIN_HEIGHT as f32),
                ),
            );
            let view = root.clone();
            let opened = cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    focus: false,
                    show: false,
                    kind: gpui::WindowKind::PopUp,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_background: gpui::WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|cx| {
                        if let Some(entity) = view.upgrade() {
                            // Whatever changes the call or the settings redraws this window too.
                            cx.observe(&entity, |_, _, cx| cx.notify()).detach();
                        }
                        OverlayView { root: view }
                    })
                },
            );
            let hwnd = opened
                .as_ref()
                .ok()
                .and_then(|handle| handle.update(cx, |_, window, _| hwnd_of(window)).ok())
                .flatten();
            let _ = root.update(cx, |root, cx| {
                root.overlay.opening = false;
                match opened {
                    Ok(handle) => {
                        root.overlay.window = Some(handle);
                        root.overlay.hwnd = hwnd;
                        root.overlay.shown = None;
                        root.overlay_sync(cx);
                    }
                    Err(error) => tracing::warn!("overlay window not opened: {error}"),
                }
            });
        });
    }

    /// `TOGGLE_OVERLAY_INPUT_LOCK` (module 596339): `setInputLocked(!isLocked(pid), pid)` for the focused game. With
    /// no overlay over a game it does nothing.
    pub(in crate::ui) fn overlay_toggle_lock(&mut self, cx: &mut Context<Self>) {
        if self.overlay_target().is_none() {
            return;
        }
        self.overlay.locked = !self.overlay.locked;
        self.overlay.drag = None;
        self.overlay.lock_changed = self.overlay_now();
        tracing::info!("overlay locked={}", self.overlay.locked);
        // `OVERLAY_SET_INPUT_LOCKED` in the notifications store.
        let now = self.overlay_now();
        let locked = self.overlay.locked;
        self.overlay.notices.set_locked(locked, now);
        self.overlay_schedule_tick(cx);
        self.overlay_sync(cx);
        // Unlocked, the overlay takes the keyboard; locked, the game gets it back.
        #[cfg(windows)]
        if !crate::storage::is_capture_run() {
            let to = if self.overlay.locked {
                self.overlay.focused.map(|focused| focused.hwnd)
            } else {
                self.overlay.hwnd
            };
            if let Some(hwnd) = to {
                dc_overlay::platform::focus(hwnd);
            }
        }
    }

    /// The clickable regions of the last frame, as zones of the window.
    fn overlay_zones(&self) -> Vec<zones::Zone> {
        if self.overlay_settings().disable_clickable_regions {
            return Vec::new();
        }
        let (width, height) = self.overlay.size;
        self.overlay
            .zone_bounds
            .borrow()
            .iter()
            .map(|bounds| {
                zones::zone_of(
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.origin.x + bounds.size.width),
                    f32::from(bounds.origin.y + bounds.size.height),
                    width,
                    height,
                )
            })
            .collect()
    }

    /// Who takes a click at a point of the window (logical px).
    pub(super) fn overlay_hit(&self, x: f32, y: f32) -> zones::Hit {
        let disabled = self.overlay_settings().disable_clickable_regions;
        zones::hit(
            self.overlay.locked,
            disabled,
            &self.overlay_zones(),
            (x, y),
            self.overlay.size,
        )
    }

    /// While the overlay is shown, the cursor is followed every 16 ms: locked, the window takes clicks only over a
    /// zone. Never in a harness run.
    fn overlay_follow_cursor(&mut self, cx: &mut Context<Self>) {
        if self.overlay.follower || crate::storage::is_capture_run() {
            return;
        }
        self.overlay.follower = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let going = this.update(cx, |this, _| this.overlay_follow_step());
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look at the cursor; false once the overlay is hidden.
    fn overlay_follow_step(&mut self) -> bool {
        if self.overlay.shown.is_none() {
            self.overlay.follower = false;
            return false;
        }
        #[cfg(windows)]
        if self.overlay.locked
            && let Some(hwnd) = self.overlay.hwnd
            && let Some(((x, y), (width, height))) = dc_overlay::platform::cursor(hwnd)
            && width > 0.
            && height > 0.
        {
            let (logical_w, logical_h) = self.overlay.size;
            let point = (x / width * logical_w, y / height * logical_h);
            let through = self.overlay_hit(point.0, point.1) == zones::Hit::Through;
            if self.overlay.through != Some(through) {
                self.overlay.through = Some(through);
                dc_overlay::platform::set_click_through(hwnd, through);
            }
        }
        true
    }

    /// Records the bounds of a click zone (`data-click-zone`) of this frame.
    pub(super) fn overlay_zone_probe(&self) -> impl IntoElement {
        let zones = self.overlay.zone_bounds.clone();
        canvas(
            move |bounds, _, _| zones.borrow_mut().push(bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

// ----- springs -----

impl Root {
    /// A spring keyed by `key`, created at `from` the first time, moving to `to`.
    fn overlay_spring(
        &mut self,
        key: (u64, u8),
        from: f32,
        to: f32,
        config: SpringConfig,
        now: f64,
    ) -> f32 {
        let spring = self
            .overlay
            .springs
            .entry(key)
            .or_insert_with(|| Spring::new(config, f64::from(from), now));
        spring.config = config;
        spring.retarget(f64::from(to), now);
        spring.step_to(now);
        if !spring.is_done() {
            self.overlay.animating = true;
        }
        spring.position() as f32
    }

    /// A toast's pose from its springs (`Ru` of module 57019): it comes in from `start` and rests at `rest`.
    pub(super) fn overlay_toast_pose(
        &mut self,
        id: u64,
        rest: notify::Pose,
        start: notify::Pose,
        now: f64,
    ) -> notify::Pose {
        let (tension, friction, mass) = notify::SPRING;
        let config = SpringConfig::new(tension, friction).mass(mass);
        // Not measured yet: drawn at rest, unseen, until its height is known.
        if rest.height == 0. && !self.overlay.springs.contains_key(&(id, 0)) {
            self.overlay.animating = true;
            return notify::Pose {
                opacity: 0.,
                ..rest
            };
        }
        notify::Pose {
            opacity: self.overlay_spring((id, 0), start.opacity, rest.opacity, config, now),
            scale: self.overlay_spring((id, 1), start.scale, rest.scale, config, now),
            offset: self.overlay_spring((id, 2), start.offset, rest.offset, config, now),
            content_opacity: self.overlay_spring(
                (id, 3),
                start.content_opacity,
                rest.content_opacity,
                config,
                now,
            ),
            height: self.overlay_spring((id, 4), start.height, rest.height, config, now),
        }
    }

    /// The joined pill of a row (`eI` and `eS` of module 502812).
    fn overlay_joined(&mut self, row: &Row, now: f64) -> Option<Joined> {
        if row.current {
            return None;
        }
        let connected = self
            .overlay
            .connected
            .1
            .get(&row.id)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);
        let mounted = *self.overlay.rows_mounted.entry(row.id).or_insert(now);
        let (shown, entering) = voice::connected_animation(connected, mounted, now, false);
        if connected == f64::NEG_INFINITY || now - mounted > voice::CONNECTED_ANIMATION_MS + 1000. {
            return None;
        }
        if now - mounted < voice::CONNECTED_ANIMATION_MS + voice::CONNECTED_STEP_MS {
            self.overlay.animating = true;
        }
        let inner = self
            .overlay
            .bounds
            .borrow()
            .get(&format!("joined-{}", row.id))
            .map_or(0., |bounds| f32::from(bounds.size.width));
        let pill = self
            .overlay
            .bounds
            .borrow()
            .get(&format!("pill-{}", row.id))
            .map_or(0., |bounds| f32::from(bounds.size.width));
        let (width_config, opacity_config) = if entering {
            (voice::WIDTH_ENTER, voice::OPACITY_ENTER)
        } else {
            (voice::WIDTH_EXIT, voice::OPACITY_EXIT)
        };
        let spring = |(tension, friction, mass): (f64, f64, f64), clamp: bool| {
            SpringConfig::new(tension, friction).mass(mass).clamp(clamp)
        };
        let width = self.overlay_spring(
            (row.id, 8),
            pill,
            if shown { inner } else { pill },
            spring(width_config, !entering),
            now,
        );
        // The opacity leaves 300 ms after the pill closes.
        let leave_at = mounted
            + voice::CONNECTED_ANIMATION_MS
            + voice::CONNECTED_STEP_MS
            + voice::OPACITY_EXIT_DELAY_MS;
        let target = if shown || now < leave_at { 1. } else { 0. };
        let opacity = self.overlay_spring(
            (row.id, 9),
            0.,
            if shown || !entering { target } else { 0. },
            spring(opacity_config, !entering),
            now,
        );
        Some(Joined {
            opacity,
            width,
            just_connected: shown,
        })
    }
}

// ----- notifications -----

impl Root {
    /// Adds a toast and arms the timers.
    fn overlay_push(&mut self, toast: Toast, options: Options, cx: &mut Context<Self>) -> u64 {
        let now = self.overlay_now();
        let id = self.overlay.notices.push(toast, options, now);
        self.overlay_schedule_tick(cx);
        cx.notify();
        id
    }

    /// The next notification timer fires on time.
    fn overlay_schedule_tick(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.overlay.notices.next_deadline() else {
            return;
        };
        if self.overlay.tick_at.is_some_and(|at| at <= deadline) {
            return;
        }
        self.overlay.tick_at = Some(deadline);
        let wait = (deadline - self.overlay_now()).max(0.);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(wait.ceil() as u64))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.overlay.tick_at = None;
                let now = this.overlay_now();
                if this.overlay.notices.tick(now) {
                    cx.notify();
                }
                this.overlay_schedule_tick(cx);
            });
        })
        .detach();
    }

    /// `OVERLAY_MOUNTED`'s welcome nudge: "Use Discord from the overlay while playing", "Press {keybind} to show",
    /// eight seconds, urgent. Off with the Welcome switch. Its Invite and Go Live buttons need the game's gaming id
    /// and Go Live, which this client does not have for a detected game: neither is drawn.
    fn overlay_welcome(&mut self, cx: &mut Context<Self>) {
        if self.overlay_notification_disabled(store::WELCOME_GENERAL) {
            return;
        }
        let hint = dc_intl::format("z8/sgJ", &[("keybind", self.overlay_keybind_text().into())]);
        let toast = Toast {
            kind: ToastKind::Welcome,
            icon: None,
            title: Some(dc_intl::t("KWDIrh").to_owned()),
            body: None,
            hint: Some(hint),
            max_lines: None,
            confirm: None,
            cancel: None,
            click: Some(ToastAction::Click),
            channel: None,
            no_zone: false,
        };
        self.overlay_push(
            toast,
            Options {
                priority: Priority::Urgent,
                kind: Kind::Nudge,
                duration: notify::NUDGE_DURATION,
                ..Options::default()
            },
            cx,
        );
    }

    /// A message the desktop notification announces (`MESSAGE_CREATE` with `lx`): a text toast, its expiry externally
    /// managed (it only times out), with "Click here or {keybind} to reply". Off with Text Chat Notifications.
    pub(super) fn overlay_text_notice(
        &mut self,
        channel: Id,
        author: &dc_state::User,
        title: String,
        body: String,
        cx: &mut Context<Self>,
    ) {
        if self.overlay.shown.is_none() || self.overlay_notification_disabled(store::TEXT_CHAT) {
            return;
        }
        let (key, url) = avatar_source(author);
        if !self.images.contains_key(&key) {
            self.request_image(key.clone(), url);
        }
        let hint = dc_intl::format("ykjOAJ", &[("keybind", self.overlay_keybind_text().into())]);
        let toast = Toast {
            kind: ToastKind::Text,
            icon: Some(key),
            title: Some(title),
            body: Some(body),
            hint: Some(hint),
            max_lines: Some(2),
            confirm: None,
            cancel: None,
            click: Some(ToastAction::Click),
            channel: Some(channel),
            no_zone: false,
        };
        self.overlay_push(
            toast,
            Options {
                kind: Kind::Text,
                externally_managed: true,
                channel: Some(channel),
                ..Options::default()
            },
            cx,
        );
        let now = self.overlay_now();
        self.overlay.notices.tidy_text(3, now);
    }

    /// `eO` and `CALL_DELETE`: a call that rings us gets its toast (Join Call / Decline), gone when it stops. None in
    /// Do Not Disturb or quiet mode.
    fn overlay_calls(&mut self, cx: &mut Context<Self>) {
        let dnd = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| self.stores.status(me.id) == "dnd");
        let ringing: Vec<Id> = self
            .stores
            .calls
            .get_incoming_calls(dnd || self.stores.notification_quiet_mode())
            .iter()
            .map(|call| call.channel)
            .collect();
        if ringing == self.overlay.calls {
            return;
        }
        let now = self.overlay_now();
        for gone in self.overlay.calls.clone() {
            if !ringing.contains(&gone)
                && let Some(id) = self.overlay.notices.call_of(gone)
            {
                self.overlay
                    .notices
                    .update_status(id, notify::Status::Dismissed, now);
            }
        }
        for channel in ringing.clone() {
            if self.overlay.notices.call_of(channel).is_some() {
                continue;
            }
            let toast = self.overlay_call_toast(channel);
            self.overlay_push(
                toast,
                Options {
                    priority: Priority::High,
                    kind: Kind::IncomingCall,
                    externally_managed: true,
                    channel: Some(channel),
                    ..Options::default()
                },
                cx,
            );
        }
        self.overlay.calls = ringing;
        cx.notify();
    }

    /// The call toast of a channel: its name and picture (`m1`, `Y`), "Incoming call...".
    fn overlay_call_toast(&mut self, channel: Id) -> Toast {
        let known = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .cloned();
        let other = known
            .as_ref()
            .and_then(|known| known.recipients.first())
            .and_then(|id| self.stores.users.get(id))
            .cloned();
        let title = match (&known, &other) {
            (Some(known), _) if known.kind == 3 => known.name.clone().unwrap_or_else(|| {
                known
                    .recipients
                    .iter()
                    .filter_map(|id| self.stores.users.get(id))
                    .map(|user| user.display_name().to_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
            (_, Some(user)) => self.dm_display_name(user),
            _ => String::new(),
        };
        let icon = other.map(|user| {
            let (key, url) = avatar_source(&user);
            if !self.images.contains_key(&key) {
                self.request_image(key.clone(), url);
            }
            key
        });
        Toast {
            kind: ToastKind::Call,
            icon,
            title: Some(title),
            body: Some(dc_intl::t("ssrVzG").to_owned()),
            hint: None,
            max_lines: None,
            confirm: Some(dc_intl::t("0D/6Rz").to_owned()),
            cancel: Some(dc_intl::t("BVN4pL").to_owned()),
            click: None,
            channel: Some(channel),
            no_zone: false,
        }
    }

    /// A toast was clicked, confirmed, declined or dismissed.
    pub(super) fn overlay_toast_action(
        &mut self,
        id: u64,
        action: ToastAction,
        cx: &mut Context<Self>,
    ) {
        let now = self.overlay_now();
        let Some(toast) = self
            .overlay
            .notices
            .entries()
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.content.clone())
        else {
            return;
        };
        let dismiss = |this: &mut Self| {
            this.overlay
                .notices
                .update_status(id, notify::Status::Dismissed, now);
        };
        match (action, toast.kind) {
            (ToastAction::Dismiss, _) => dismiss(self),
            // Join Call (`call` / `selectVoiceChannel`) and Decline (`stopRinging`).
            (ToastAction::Confirm, ToastKind::Call) => {
                if let Some(channel) = toast.channel {
                    self.join_dm_call(channel, false, cx);
                }
                dismiss(self);
            }
            (ToastAction::Cancel, ToastKind::Call) => {
                if let Some(channel) = toast.channel {
                    self.stop_ringing(channel, None);
                }
            }
            // A text toast: a locked overlay unlocks, an unlocked one opens the channel in the app.
            (ToastAction::Click, ToastKind::Text) => {
                if self.overlay.locked {
                    self.overlay_toggle_lock(cx);
                } else if let Some(channel) = toast.channel {
                    self.open_notification_channel(channel, cx);
                }
            }
            (ToastAction::Click, _) if self.overlay.locked => self.overlay_toggle_lock(cx),
            _ => {}
        }
        self.overlay_schedule_tick(cx);
        cx.notify();
    }

    /// The lock keybind as the overlay writes it (`dI(shortcut, true)`), `???` without one.
    pub(super) fn overlay_keybind_text(&self) -> String {
        self.overlay_lock_text()
    }
}

// ----- drawing the voice widget -----

/// The voice icons of a row (`Kc`, module 293277): the microphone (server or local mute shows the deny icon), then
/// the headphones (server deafened: the deny icon), then the camera.
pub(super) fn voice_icons(row: &Row) -> Vec<&'static str> {
    let mut icons = Vec::new();
    if row.mute || row.server_mute {
        icons.push(if row.server_mute {
            "overlay_server_muted"
        } else {
            "voice_member_muted"
        });
    }
    if row.deaf || row.server_deaf {
        icons.push(if row.server_deaf {
            "overlay_server_deafened"
        } else {
            "headphones_deafened"
        });
    }
    if row.video {
        icons.push("voice_member_camera");
    }
    icons
}

/// What a row is drawn with besides its person.
#[derive(Clone, Copy)]
pub(super) struct RowLook {
    pub size: AvatarSize,
    pub locked: bool,
    pub preview: bool,
    pub names: DisplayNameMode,
    /// `flipped` (`!anchorLeft`): the row reads from the right, its avatar last.
    pub flipped: bool,
}

/// The avatar (`avatar_ieJ4Sq`) with the stylesheet's speaking outline: 1px `--green-300` 2px out (no volume level is
/// known, see `voice::ring_widths`).
fn voice_avatar(row: &Row, avatar: f32) -> AnyElement {
    div()
        .relative()
        .flex_none()
        .size(px(avatar))
        .rounded_full()
        .children(
            row.avatar
                .clone()
                .map(|picture| img(picture).size(px(avatar)).rounded_full()),
        )
        .when(row.speaking, |this| {
            this.child(
                div()
                    .absolute()
                    .top(px(-3.))
                    .left(px(-3.))
                    .size(px(avatar + 6.))
                    .rounded_full()
                    .border_1()
                    .border_color(onyx("--green-300")),
            )
        })
        .into_any_element()
}

/// One person (`voiceUser_ieJ4Sq`): the avatar, the joined pill over the row, the name pill with the voice icons;
/// mirrored when flipped. The pill's `box-shadow: var(--shadow-border) var(--shadow-high)` is two shadows without a
/// comma, which the browser rejects: none is drawn (the capture's computed `box-shadow` is `none`).
fn voice_row(
    row: &Row,
    look: RowLook,
    joined: Option<Joined>,
    bounds: &Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
) -> AnyElement {
    let avatar = look.size.pixels();
    let just = joined.is_some_and(|joined| joined.just_connected);
    let opacity = voice::row_opacity(look.locked, row.speaking, just);
    let name_shown = voice::name_shown(look.locked || look.preview, look.names, row.speaking);
    let icons = voice_icons(row);
    let icon_row = (!icons.is_empty()).then(|| {
        div()
            .flex()
            .gap(px(voice::ICON_GAP))
            .when(look.flipped, |this| this.mr(px(4.)))
            .children(icons.into_iter().map(|name| {
                svg()
                    .path(format!("icons/{name}.svg"))
                    .size(px(voice::ICON))
                    .flex_none()
                    .text_color(gpui::white())
                    .opacity(0.5)
            }))
    });
    let name = div()
        .whitespace_nowrap()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(500.))
        .text_color(onyx("--text-default"))
        .child(row.name.clone());
    let pill = name_shown.then(move || {
        let base = div()
            .relative()
            .h(px(voice::PILL_HEIGHT))
            .when(look.flipped, |this| this.mr(px(4.)).pl(px(4.)).pr(px(8.)))
            .when(!look.flipped, |this| {
                this.ml(px(voice::PILL_MARGIN))
                    .px(px(voice::PILL_PADDING_X))
            })
            .flex()
            .flex_shrink(1.)
            .items_center()
            .justify_center()
            .gap(px(8.))
            .rounded(px(16.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(rgba(PILL))
            .opacity(0.8)
            .overflow_hidden();
        let base = if look.flipped {
            base.children(icon_row).child(name)
        } else {
            base.child(name).children(icon_row)
        };
        base.child(super::overlay_widgets::probe(
            bounds,
            format!("pill-{}", row.id),
        ))
    });
    // `connectedAnimationContainer_ieJ4Sq`: 36px from the avatar's side, 4px in from the top and bottom.
    let joined_pill = joined.filter(|joined| joined.opacity > 0.).map(|joined| {
        let text = dc_intl::format("uFcReq", &[("nick", row.name.clone().into())]);
        div()
            .absolute()
            .top(px(4.))
            .bottom(px(4.))
            .when(look.flipped, |this| this.right(px(36.)))
            .when(!look.flipped, |this| this.left(px(36.)))
            .w(px(joined.width.max(0.)))
            .overflow_hidden()
            .rounded(px(16.))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .bg(rgba(JOINED))
            .opacity(joined.opacity)
            .child(
                // `width: fit-content`: the text's own width, whatever the clipping box is now.
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .when(look.flipped, |this| this.right_0())
                    .when(!look.flipped, |this| this.left_0())
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .whitespace_nowrap()
                    .when(look.flipped, |this| this.pr(px(4.)))
                    .when(!look.flipped, |this| this.pl(px(8.)))
                    // The lottie (`animation_ieJ4Sq`, 24 px) is kept as its space.
                    .when(look.flipped, |this| {
                        this.child(div().size(px(24.)).flex_none())
                    })
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(onyx("--text-default"))
                            .child(text),
                    )
                    .when(!look.flipped, |this| {
                        this.child(div().size(px(24.)).flex_none())
                    })
                    .child(super::overlay_widgets::probe(
                        bounds,
                        format!("joined-{}", row.id),
                    )),
            )
    });
    div()
        .relative()
        .flex()
        .items_center()
        .when(look.flipped, |this| this.justify_end())
        .h(px(avatar))
        .opacity(opacity)
        .when(!look.flipped, |this| this.child(voice_avatar(row, avatar)))
        .children(pill)
        .when(look.flipped, |this| this.child(voice_avatar(row, avatar)))
        // `z-index: 100`: the joined pill is drawn over the name pill.
        .children(joined_pill)
        .into_any_element()
}

/// The "+N others in voice" pill (`hiddenVoiceStates_ieJ4Sq`): the first two hidden people's pictures on a diagonal
/// (`DiagonalFacepile`), then the count, its text hard-coded English in the official client.
fn others_pill(
    count: usize,
    pictures: [Option<std::sync::Arc<gpui::Image>>; 2],
    look: RowLook,
) -> AnyElement {
    let size = look.size.pixels();
    let part = size / 1.5;
    let offset = part / 2.;
    let [back, front] = pictures;
    let facepile = div()
        .relative()
        .flex_none()
        .size(px(size))
        .children(back.map(|image| {
            img(image)
                .absolute()
                .left_0()
                .top_0()
                .size(px(part))
                .rounded_full()
        }))
        .children(front.map(|image| {
            img(image)
                .absolute()
                .left(px(offset))
                .top(px(offset))
                .size(px(part))
                .rounded_full()
        }));
    let text = div()
        .px(px(8.))
        .py(px(4.))
        .rounded(px(16.))
        .border_1()
        .border_color(onyx("--border-subtle"))
        .bg(rgba(PILL))
        .opacity(0.8)
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(500.))
        .text_color(onyx("--text-default"))
        .child(format!("+{count} others in voice"));
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .when(look.flipped, |this| this.flex_row_reverse())
        .when(look.locked, |this| this.opacity(0.4))
        .child(facepile)
        .child(text)
        .into_any_element()
}

/// The voice widget's list (`voiceUserContainer_ieJ4Sq`, module 502812 `ew`) for rows already in channel order.
pub(super) fn voice_list(
    rows: &[Row],
    settings: &OverlaySettings,
    max: i64,
    look: RowLook,
    joined: &HashMap<u64, Joined>,
    bounds: &Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
) -> AnyElement {
    let members: Vec<voice::Member> = rows
        .iter()
        .map(|row| voice::Member {
            id: row.id,
            speaking: row.speaking,
            current: row.current,
        })
        .collect();
    let ordered = voice::order(&members, max);
    let split = voice::split(&ordered, max, settings.display_user_mode);
    let find = |id: u64| rows.iter().find(|row| row.id == id);
    let pictures = [
        split
            .others
            .first()
            .and_then(|id| find(*id))
            .and_then(|row| row.avatar.clone()),
        split
            .others
            .get(1)
            .and_then(|id| find(*id))
            .and_then(|row| row.avatar.clone()),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(voice::ROW_GAP))
        .children(
            split
                .rows
                .iter()
                .filter_map(|id| find(*id))
                .filter(|row| {
                    voice::row_shown(
                        look.locked || look.preview,
                        settings.display_user_mode,
                        row.speaking,
                    )
                })
                .map(|row| voice_row(row, look, joined.get(&row.id).copied(), bounds)),
        )
        .when(split.others.len() > 1, |this| {
            this.child(others_pill(split.others.len(), pictures, look))
        })
        .into_any_element()
}

/// The settings page's preview header (`widgetHeaderContainer_DKMuWq`): the title pill and the pin button, 4px apart.
pub(super) fn widget_header(title: String, pinned: bool) -> AnyElement {
    div()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(4.))
        .child(
            div()
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(12.))
                .border_1()
                .border_color(rgba(0xffffff14))
                .bg(rgba(0x131318e6))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(onyx("--text-strong"))
                .whitespace_nowrap()
                .child(title),
        )
        .child(
            div()
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if pinned {
                    onyx("--background-brand")
                } else {
                    rgba(0xffffff14)
                })
                .bg(rgba(0x131318e6))
                .child(
                    svg()
                        .path("icons/overlay_pin.svg")
                        .size(px(12.))
                        .text_color(if pinned {
                            onyx("--background-brand")
                        } else {
                            onyx("--interactive-text-active")
                        }),
                ),
        )
        .into_any_element()
}

impl Root {
    /// The people of the current call in `SortedVoiceStateStore`'s order (module 607567): streamers first, then by
    /// the shown name in lower case, then by id. Each one's first sighting is their `connectedOn`; whoever was there
    /// when the overlay first looked at the channel counts as long connected.
    fn overlay_call_rows(&mut self) -> Vec<Row> {
        let Some((guild, channel)) = self.call.as_ref().map(|call| (call.guild, call.channel))
        else {
            return Vec::new();
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let states: Vec<dc_state::VoiceState> = self
            .stores
            .voice_members(guild, channel)
            .into_iter()
            .cloned()
            .collect();
        let now = self.overlay_now();
        let first_look = self.overlay.connected.0 != Some(channel);
        if first_look {
            self.overlay.connected = (Some(channel), HashMap::new());
            self.overlay.rows_mounted.clear();
        }
        let mut rows = Vec::new();
        for state in &states {
            let user = self
                .stores
                .users
                .get(&state.user_id)
                .cloned()
                .or_else(|| {
                    self.stores
                        .current_user
                        .clone()
                        .filter(|user| user.id == state.user_id)
                })
                .unwrap_or_default();
            let nick = self
                .stores
                .members
                .get(guild, state.user_id)
                .and_then(|member| member.nick.clone());
            let name = nick.unwrap_or_else(|| user.display_name().to_owned());
            self.overlay
                .connected
                .1
                .entry(state.user_id)
                .or_insert(if first_look { f64::NEG_INFINITY } else { now });
            let (key, url) = avatar_source(&user);
            if !self.images.contains_key(&key) {
                self.request_image(key.clone(), url);
            }
            rows.push((
                voice::sort_key(state.self_stream, &name, state.user_id),
                Row {
                    id: state.user_id,
                    name,
                    avatar: self.images.get(&key).cloned(),
                    speaking: self.speaking.contains(&state.user_id)
                        || self.soundboard.user_playing(state.user_id),
                    mute: state.self_mute,
                    server_mute: state.mute || state.suppress,
                    deaf: state.self_deaf,
                    server_deaf: state.deaf,
                    video: state.self_video,
                    current: Some(state.user_id) == me,
                },
            ));
        }
        let present: std::collections::HashSet<Id> =
            states.iter().map(|state| state.user_id).collect();
        self.overlay
            .rows_mounted
            .retain(|id, _| present.contains(id));
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// The voice widget's content: the list, or unlocked with nobody to list the empty state (`O.g`: the voice icon at
    /// 24% under "Connect to a voice channel to see a list of users here.").
    fn overlay_voice_content(
        &mut self,
        widget: &layout::Widget,
        locked: bool,
        flipped: bool,
    ) -> AnyElement {
        let settings = self.overlay_settings();
        let rows = self.overlay_call_rows();
        let in_channel = self.call.is_some();
        let drawn = voice::widget_shown(!rows.is_empty(), in_channel, locked, widget.pinned);
        if drawn {
            let now = self.overlay_now();
            let mut joined = HashMap::new();
            for row in &rows {
                if let Some(motion) = self.overlay_joined(row, now) {
                    joined.insert(row.id, motion);
                }
            }
            let max = widget
                .meta("voiceStatesMaxShown")
                .and_then(Value::as_i64)
                .unwrap_or(layout::VOICE_MAX_SHOWN_DEFAULT);
            let look = RowLook {
                size: settings.avatar_size_mode,
                locked,
                preview: false,
                names: settings.display_name_mode,
                flipped,
            };
            return voice_list(&rows, &settings, max, look, &joined, &self.overlay.bounds);
        }
        if voice::shows_empty_state(drawn, locked, true) {
            return div()
                .w_full()
                .p(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/overlay_voice_normal.svg")
                        .size(px(24.))
                        .text_color(onyx("--white"))
                        .opacity(0.24),
                )
                .into_any_element();
        }
        div().into_any_element()
    }

    /// The overlay window's content at its size: the scrim, the widgets of the layout in z order, the toolbar and
    /// the empty state while unlocked.
    fn overlay_body(&mut self, width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        // The game's client area the window was placed over is the overlay's size (`windowSize`), whatever the
        // window reported last.
        let scale = self.overlay.scale.max(0.1);
        let (width, height) = self.overlay.shown.map_or((width, height), |(rect, _)| {
            (rect.width as f32 / scale, rect.height as f32 / scale)
        });
        self.overlay.size = (width, height);
        self.overlay.animating = false;
        self.overlay.zone_bounds.borrow_mut().clear();
        let locked = self.overlay.locked;
        let now = self.overlay_now();
        let since = ((now - self.overlay.lock_changed) / super::overlay_widgets::SCRIM.duration_ms)
            .clamp(0., 1.);
        let eased = super::overlay_widgets::SCRIM.ease.apply(since) as f32;
        let scrim = if locked { 1. - eased } else { eased };
        let extras_t =
            ((now - self.overlay.lock_changed) / super::overlay_widgets::EXTRAS_MS).clamp(0., 1.);
        let extras = super::overlay_widgets::EXTRAS.apply(extras_t) as f32;
        let rise = if super::motion::reduced() {
            0.
        } else {
            6. * (1. - super::motion::Ease::EaseOut.apply(extras_t) as f32)
        };
        if since < 1. || extras_t < 1. {
            self.overlay.animating = true;
        }
        let frame = super::overlay_widgets::Frame {
            locked,
            width,
            height,
            unlocked: if locked { 0. } else { extras },
            rise,
        };
        let layout = self.overlay_layout();
        let mut widgets = Vec::new();
        for widget in layout.drawn() {
            let anchored_left = match self
                .overlay
                .drag
                .as_ref()
                .filter(|held| held.id == widget.id && held.drag.started)
            {
                Some(held) => held.drag.anchor.left.is_some(),
                None => widget.anchor.left.is_some(),
            };
            let (title, content) = match widget.kind.as_str() {
                layout::VOICE => (
                    "Voice",
                    self.overlay_voice_content(&widget, locked, !anchored_left),
                ),
                layout::NOTIFICATIONS => (
                    dc_intl::tr("Notifications"),
                    self.overlay_notifications(locked, widget.pinned, true, cx),
                ),
                // The other widgets (quick actions, activity, video, streams, chat, friends) are not built.
                _ => continue,
            };
            widgets.push(self.overlay_widget_frame(&widget, title, content, &frame, cx));
        }
        let keybind = self.overlay_keybind_text();
        let empty = widgets.is_empty() && !locked;
        div()
            .id("overlay-body")
            .relative()
            .w(px(width))
            .h(px(height))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if this.overlay_drag_move(event.position) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.overlay.drag.is_some() {
                        this.overlay_drag_end();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.overlay.drag.is_some() {
                        this.overlay_drag_end();
                        cx.notify();
                    }
                }),
            )
            // `.overlayBackground` with `--background-scrim`; a press on it locks the overlay (`tu`).
            .child(
                div()
                    .id("overlay-background")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(onyx("--background-scrim"))
                    .opacity(scrim)
                    .when(!locked, |this| {
                        this.on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                if !this.overlay.locked && this.overlay.drag.is_none() {
                                    this.overlay_toggle_lock(cx);
                                }
                            }),
                        )
                    }),
            )
            .children(widgets)
            .when(empty, |this| this.child(self.overlay_empty_state(cx)))
            .when(!locked, |this| {
                this.child(self.overlay_toolbar(keybind, cx))
            })
            .into_any_element()
    }
}

// ----- harness -----

impl Root {
    /// Capture only: `overlay demo [w h]`, `overlay away`, `overlay toggle`, `overlay game on|off|clear`,
    /// `overlay speak`, `overlay notify welcome|text|call|join|playing`, `overlay max <n>`, `overlay pin <kind>`,
    /// `overlay hide <kind>`, `overlay reset`, `overlay hit <x> <y>`, `overlay layout`, `overlay state`.
    /// `overlay shot <png>` and `overlay drag <x1> <y1> <x2> <y2>` are handled by the harness.
    #[cfg(feature = "capture")]
    pub(crate) fn script_overlay(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let demo_game = || Game {
            name: Some("Demo Game".to_owned()),
            exe_path: "c:/games/demo/demo.exe".to_owned(),
            pid: Some(4242),
            ..Game::default()
        };
        let kind_of = |word: &str| match word {
            "voice" => Some(layout::VOICE),
            "notifications" => Some(layout::NOTIFICATIONS),
            _ => None,
        };
        match words {
            ["demo", rest @ ..] => {
                let size = |at: usize, default: i32| {
                    rest.get(at).and_then(|value| value.parse().ok()).unwrap_or(default)
                };
                let foreground = Foreground {
                    hwnd: 0,
                    pid: 4242,
                    client: Rect {
                        x: 0,
                        y: 0,
                        width: size(0, 1280),
                        height: size(1, 720),
                    },
                    fullscreen: decision::FullscreenType::BorderlessFullscreen,
                };
                self.overlay.demo = Some(Demo {
                    foreground,
                    game: demo_game(),
                });
                self.overlay_focus_changed(Some(foreground), cx);
                format!("overlay demo: target={:?}", self.overlay_target())
            }
            ["away"] => {
                self.overlay_focus_changed(None, cx);
                format!("overlay away: target={:?}", self.overlay_target())
            }
            ["toggle"] => {
                self.overlay_toggle_lock(cx);
                format!("overlay toggle: locked={}", self.overlay.locked)
            }
            ["game", value] => {
                let game = demo_game();
                match *value {
                    "on" => self.detect.store.toggle_overlay(&game, false, Some(true)),
                    "off" => self.detect.store.toggle_overlay(&game, false, Some(false)),
                    _ => {}
                }
                self.overlay_sync(cx);
                format!("overlay game {value}: target={:?}", self.overlay_target())
            }
            ["speak"] => {
                let others: Vec<u64> = self
                    .call
                    .as_ref()
                    .map(|call| {
                        self.stores
                            .voice_members(call.guild, call.channel)
                            .iter()
                            .map(|state| state.user_id)
                            .collect()
                    })
                    .unwrap_or_default();
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                for id in others.into_iter().filter(|id| Some(*id) != me) {
                    self.speaking.insert(id);
                }
                cx.notify();
                "overlay speak: the others speak".to_owned()
            }
            // Made-up toasts of each kind, as their stores would raise them.
            ["notify", kind] => {
                let me = self.stores.current_user.clone().unwrap_or_default();
                let made_up = |title: &str, body: Option<String>| Toast {
                    kind: ToastKind::Text,
                    icon: Some(avatar_source(&me).0),
                    title: Some(title.to_owned()),
                    body,
                    hint: None,
                    max_lines: Some(2),
                    confirm: None,
                    cancel: None,
                    click: Some(ToastAction::Click),
                    channel: None,
                    no_zone: false,
                };
                match *kind {
                    "welcome" => self.overlay_welcome(cx),
                    "text" => {
                        let hint = dc_intl::format("ykjOAJ", &[("keybind", self.overlay_keybind_text().into())]);
                        let toast = Toast {
                            hint: Some(hint),
                            ..made_up(
                                me.display_name(),
                                Some("Ready when you are, the lobby is open.".to_owned()),
                            )
                        };
                        self.overlay_push(
                            toast,
                            Options {
                                kind: Kind::Text,
                                externally_managed: true,
                                channel: Some(1),
                                ..Options::default()
                            },
                            cx,
                        );
                    }
                    "call" => {
                        let toast = Toast {
                            kind: ToastKind::Call,
                            body: Some(dc_intl::t("ssrVzG").to_owned()),
                            confirm: Some(dc_intl::t("0D/6Rz").to_owned()),
                            cancel: Some(dc_intl::t("BVN4pL").to_owned()),
                            click: None,
                            channel: Some(2),
                            ..made_up(me.display_name(), None)
                        };
                        self.overlay_push(
                            toast,
                            Options {
                                priority: Priority::High,
                                kind: Kind::IncomingCall,
                                externally_managed: true,
                                channel: Some(2),
                                ..Options::default()
                            },
                            cx,
                        );
                    }
                    // `ACTIVITY_USER_ACTION` with JOIN: "{username} is joining your game via Discord.", the hint.
                    "join" => {
                        let toast = Toast {
                            hint: Some(dc_intl::t("WRj1Wn").to_owned()),
                            click: None,
                            ..made_up(
                                me.username.as_str(),
                                Some(dc_intl::format("Yk+uYG", &[("username", "".into())])),
                            )
                        };
                        self.overlay_push(
                            toast,
                            Options {
                                priority: Priority::Urgent,
                                ..Options::default()
                            },
                            cx,
                        );
                    }
                    // `ev`: "{username} started playing {gameName}", one line, no click zone without chat.
                    "playing" => {
                        let toast = Toast {
                            title: None,
                            max_lines: Some(1),
                            click: None,
                            no_zone: true,
                            ..made_up(
                                "",
                                Some(dc_intl::format(
                                    "q7/rgv",
                                    &[
                                        ("username", me.display_name().to_owned().into()),
                                        ("gameName", "Demo Game".into()),
                                    ],
                                )),
                            )
                        };
                        self.overlay_push(toast, Options::default(), cx);
                    }
                    _ => {}
                }
                format!("overlay notify {kind}: {} listed", self.overlay.notices.entries().len())
            }
            // The first other member joins again: their `connectedOn` is now, so their row plays the joined pill.
            ["rejoin"] => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let other = self
                    .overlay_call_rows()
                    .into_iter()
                    .map(|row| row.id)
                    .find(|id| Some(*id) != me);
                let now = self.overlay_now();
                if let Some(id) = other {
                    self.overlay.connected.1.insert(id, now);
                    self.overlay.rows_mounted.remove(&id);
                    self.overlay.springs.remove(&(id, 8));
                    self.overlay.springs.remove(&(id, 9));
                }
                cx.notify();
                format!("overlay rejoin: {other:?}")
            }
            ["max", value] => {
                let value: f32 = value.parse().unwrap_or(8.);
                self.overlay_change_layout(|layout| {
                    layout.set_voice_max_shown(value, &mut new_widget_id);
                    true
                });
                cx.notify();
                format!("overlay max: {}", self.overlay_layout().voice_max_shown())
            }
            ["pin", kind] | ["hide", kind] => {
                let Some(kind) = kind_of(kind) else {
                    return "overlay pin|hide voice|notifications".to_owned();
                };
                let Some(widget) = self.overlay_layout().widget(kind) else {
                    return "overlay: no such widget".to_owned();
                };
                let pin = words[0] == "pin";
                self.overlay_change_layout(|layout| {
                    if pin {
                        layout.set_pinned(&widget.id, None)
                    } else {
                        layout.set_hidden(std::slice::from_ref(&widget.id), true)
                    }
                });
                cx.notify();
                let after = self.overlay_layout().widget(kind);
                format!(
                    "overlay {}: pinned={:?} hidden={:?}",
                    words[0],
                    after.as_ref().map(|widget| widget.pinned),
                    after.as_ref().map(|widget| widget.hidden)
                )
            }
            ["reset"] => {
                self.overlay_change_layout(|layout| {
                    layout.reset(&mut new_widget_id);
                    true
                });
                cx.notify();
                "overlay reset".to_owned()
            }
            ["hit", x, y] => {
                let (x, y) = (x.parse().unwrap_or(0.), y.parse().unwrap_or(0.));
                format!(
                    "overlay hit {x},{y}: {:?} zones={:?}",
                    self.overlay_hit(x, y),
                    self.overlay_zones()
                )
            }
            ["layout"] => {
                let layout = self.overlay_layout();
                let lines: Vec<String> = layout
                    .widgets()
                    .iter()
                    .map(|widget| {
                        format!(
                            "{} anchor={:?} pinned={} hidden={} z={}",
                            widget.kind, widget.anchor, widget.pinned, widget.hidden, widget.z_index
                        )
                    })
                    .collect();
                format!("overlay layout: {}", lines.join(" | "))
            }
            ["state"] => {
                let rows = self.overlay_call_rows().len();
                format!(
                    "overlay state: target={:?} window={} hwnd={} locked={} shown={:?} allowed={} rows={} notices={} size={:?} scale={}",
                    self.overlay_target(),
                    self.overlay.window.is_some(),
                    self.overlay.hwnd.is_some(),
                    self.overlay.locked,
                    self.overlay.shown,
                    window_allowed(),
                    rows,
                    self.overlay.notices.entries().len(),
                    self.overlay.size,
                    self.overlay.scale
                )
            }
            _ => "usage: overlay demo [w h] | away | toggle | game on|off | speak | notify <kind> | max <n> | pin|hide <kind> | reset | hit <x> <y> | layout | state | shot <png> | drag <x1> <y1> <x2> <y2>".to_owned(),
        }
    }

    /// The overlay window, for `overlay shot` and `overlay drag`.
    #[cfg(feature = "capture")]
    pub(crate) fn overlay_window(&self) -> Option<gpui::AnyWindowHandle> {
        self.overlay.window.map(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64) -> Row {
        Row {
            id,
            name: String::new(),
            avatar: None,
            speaking: false,
            mute: false,
            server_mute: false,
            deaf: false,
            server_deaf: false,
            video: false,
            current: false,
        }
    }

    #[test]
    fn a_harness_run_opens_the_window_only_on_the_hidden_desktop() {
        assert!(window_allowed_on(false, Some("Default")));
        assert!(window_allowed_on(false, None));
        assert!(!window_allowed_on(true, Some("Default")));
        assert!(!window_allowed_on(true, None));
        assert!(window_allowed_on(true, Some(HIDDEN_DESKTOP)));
        // A unit test is not on the hidden desktop: in a capture build it may not open the window.
        if cfg!(feature = "capture") {
            assert!(!window_allowed());
        }
    }

    #[test]
    fn voice_icons_follow_the_official_order() {
        let mut muted = row(1);
        muted.mute = true;
        assert_eq!(voice_icons(&muted), ["voice_member_muted"]);
        muted.server_mute = true;
        assert_eq!(voice_icons(&muted), ["overlay_server_muted"]);
        let mut deaf = row(2);
        deaf.mute = true;
        deaf.deaf = true;
        deaf.video = true;
        assert_eq!(
            voice_icons(&deaf),
            [
                "voice_member_muted",
                "headphones_deafened",
                "voice_member_camera"
            ]
        );
        let mut server_deaf = row(3);
        server_deaf.server_deaf = true;
        assert_eq!(voice_icons(&server_deaf), ["overlay_server_deafened"]);
        assert!(voice_icons(&row(4)).is_empty());
    }
}
