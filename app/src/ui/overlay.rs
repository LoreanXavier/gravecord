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
use super::{Root, avatar_source, color, icon, kerning};
use dc_overlay::decision::{self, Foreground, OverlayMethod, Rect};
use dc_overlay::layout::{self, Layout};
use dc_overlay::notify::{self, Kind, Options, Priority, Queue};
use dc_overlay::store::{self, AvatarSize, DisplayNameMode, OverlaySettings};
use dc_overlay::{voice, zones};
use dc_state::Id;
use dc_state::running_games::Game;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, Render, Rgba, SharedString,
    WeakEntity, Window, WindowHandle, canvas, div, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// `eR` of module 911024: the crash notice stays ten seconds.
const CRASH_NOTICE_MS: f64 = 10_000.;
/// The id the crash notice's toast answers to.
const CRASH_TOAST: u64 = u64::MAX;

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

/// Whether this run does any overlay work at all: a harness run, or any normal non-test desktop run unless
/// disabled with `GRAVECORD_OVERLAY=0` (for games whose anticheat objects to an overlay).
pub(super) fn gate_on(capture: bool, inert_or_test: bool, env: Option<&str>) -> bool {
    capture || (!inert_or_test && env != Some("0"))
}

/// [`gate_on`] for this run, read once.
pub(super) fn gate() -> bool {
    static GATE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *GATE.get_or_init(|| {
        let env = std::env::var("GRAVECORD_OVERLAY").ok();
        let inert_or_test = cfg!(test) || crate::inert::on();
        gate_on(
            crate::storage::is_capture_run(),
            inert_or_test,
            env.as_deref(),
        )
    })
}

fn window_allowed() -> bool {
    if !gate() {
        return false;
    }
    #[cfg(windows)]
    let desktop = dc_overlay::platform::desktop_name();
    #[cfg(not(windows))]
    let desktop: Option<String> = None;
    window_allowed_on(crate::storage::is_capture_run(), desktop.as_deref())
}

/// Runs `build`, giving `None` when it panicked (logged at error level, no content).
pub(super) fn guarded<T>(build: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::error!(
                "overlay: drawing the overlay failed; its window is closed for this session"
            );
            None
        }
    }
}

/// The window's root view: the overlay of the app's `Root`, at this window's size.
pub(crate) struct OverlayView {
    root: WeakEntity<Root>,
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area = window.viewport_size();
        let (width, height) = (f32::from(area.width), f32::from(area.height));
        // A panic while the overlay's elements are built closes the overlay window for the rest of the session; the
        // app goes on. It is caught inside the update so the app's state is handed back.
        let mut failed = false;
        let id = window.window_handle().window_id();
        let body = self.root.upgrade().and_then(|root| {
            root.update(cx, |root, cx| {
                if root.overlay.failed {
                    // Module 911024 `eM`: the crash notice stays ten seconds, then the window goes.
                    if let Some(notice) = root.overlay_crash_notice(cx) {
                        return Some((notice, false));
                    }
                    failed = true;
                    return None;
                }
                root.overlay.scale = window.scale_factor();
                match guarded(|| root.overlay_body(width, height, window, cx)) {
                    // The widgets' button tooltips (module 968898) are drawn in this window.
                    Some(body) => Some((
                        div()
                            .size_full()
                            .child(body)
                            .children(root.tip_layer_in(Some(id)))
                            .into_any_element(),
                        root.overlay.animating,
                    )),
                    None => {
                        root.overlay_failed();
                        let notice = root.overlay_crash_notice(cx);
                        failed = notice.is_none();
                        notice.map(|notice| (notice, false))
                    }
                }
            })
        });
        if failed {
            window.remove_window();
        }
        let animating = body.as_ref().is_some_and(|(_, animating)| *animating);
        if animating {
            window.request_animation_frame();
        }
        div()
            .id("overlay-root")
            .size_full()
            .font_family(super::SANS)
            .font_features(kerning())
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let _ = this.root.update(cx, |root, cx| {
                    // The Chat widget's composer is the app's own: its keys first (Enter sends).
                    if root.composer_key(&event.keystroke, window, cx) {
                        cx.notify();
                        return;
                    }
                    // `td` of module 427830: Escape locks (a menu or the dialog closes first).
                    if event.keystroke.key == "escape" && !root.overlay.locked {
                        if root.overlay.menu.is_some() || root.overlay.report.is_some() {
                            root.overlay.menu = None;
                            root.overlay.report = None;
                            cx.notify();
                        } else {
                            root.overlay_toggle_lock(cx);
                        }
                    }
                });
            }))
            // `td`: Alt held hides the widgets' extras while unlocked (`OVERLAY_V3_SHOW_WIDGETS {show: false}`), its
            // release shows them again. gpui reports a modifier alone as a modifiers change.
            .on_modifiers_changed(cx.listener(
                |this, event: &gpui::ModifiersChangedEvent, _, cx| {
                    let alt = event.modifiers.alt;
                    let _ = this.root.update(cx, |root, cx| {
                        if !root.overlay.locked || !alt {
                            root.overlay_show_widgets(!alt, cx);
                        }
                    });
                },
            ))
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
    pub(super) focused: Option<Foreground>,
    /// `OverlayStore.isLocked(pid)`: locked unless unlocked by the keybind.
    pub(super) locked: bool,
    pub(super) sync_dirty: bool,
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
    pub(super) springs: HashMap<(u64, u8), Spring>,
    /// The ringing channels toasts were raised for.
    calls: Vec<Id>,
    /// Click-through as last set while locked (the cursor follower).
    through: Option<bool>,
    follower: bool,
    pub(super) widgets_panel: bool,
    /// Something is still moving: the window asks for another frame.
    pub(super) animating: bool,
    /// A widget's Settings menu (modules 51444, 320490).
    pub(super) menu: Option<super::overlay_panel::OverlayMenu>,
    /// The Report a Problem dialog (module 967055).
    pub(super) report: Option<super::overlay_panel::ReportDialog>,
    /// `lockExtras` of every drag container: false while Alt is held (`OVERLAY_V3_SHOW_WIDGETS`).
    pub(super) show_widgets: bool,
    /// `Overlay-v3-Text-Chat-Store`.
    pub(super) text_chat: dc_overlay::text_chat::Store,
    /// The Chat widget's own list of the open channel's rows (the main window keeps its own), and what it was built
    /// for.
    pub(super) chat_list: gpui::ListState,
    pub(super) chat_rows: (Option<Id>, usize),
    /// Toasts drawn in the last frame, and the dismissed ones still leaving with when they left.
    pub(super) toasts_drawn: Vec<super::overlay_widgets::DrawnToast>,
    pub(super) leaving: Vec<(super::overlay_widgets::DrawnToast, f64)>,
    /// `disableClickableRegions` as last read: the cursor follower reads no storage.
    pub(super) clickable_disabled: bool,
    /// When the follower last looked at the game's window (ms on the clock).
    last_check: f64,
    /// Drawing the overlay panicked: the window is closed and not opened again this session.
    pub(super) failed: bool,
    /// The chat experiment's variation as the harness sets it (`overlay chat <n>`).
    pub(super) chat_variant: Option<Option<i64>>,
    /// The pointer in the window (logical px) at its last move: where a Settings menu opens.
    pub(super) pointer: (f32, f32),
    /// When drawing the overlay panicked (ms on the clock): the crash notice is up for ten seconds.
    pub(super) crashed: Option<f64>,
    /// The crash notice's end timer is running.
    pub(super) crash_timer: bool,
    /// The soundboard wheel (`SoundboardOverlayStore`, module 528772).
    pub(super) wheel: Option<super::overlay_wheel::Wheel>,
    /// A harness run: the sound the wheel would have played.
    pub(super) wheel_played: Option<String>,
    /// Overlay-specific Favorites (`OverlayFavoritesStore`, modules 406595 / 708653): separate from the app's
    /// Favorites server (`FRIENDS` tab user IDs and `MESSAGES` tab channel IDs).
    pub(super) favorite_users: std::collections::HashSet<Id>,
    pub(super) favorite_channels: std::collections::HashSet<Id>,
    /// Module 911024 (`eM`): Shift+click crash notice toggles the monospace stack trace box.
    pub(super) crash_show_trace: bool,
    pub(super) crash_trace: Option<String>,
    /// Module 907357 (`ts` invite button): timestamp (ms) when a play invite was sent to a user.
    pub(super) activity_invite_sent: HashMap<Id, f64>,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            foreground: None,
            focused: None,
            locked: true,
            sync_dirty: false,
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
            menu: None,
            report: None,
            show_widgets: true,
            text_chat: dc_overlay::text_chat::Store::default(),
            chat_list: gpui::ListState::new(0, gpui::ListAlignment::Bottom, px(1000.)),
            chat_rows: (None, 0),
            toasts_drawn: Vec::new(),
            leaving: Vec::new(),
            clickable_disabled: false,
            last_check: 0.,
            failed: false,
            chat_variant: None,
            pointer: (0., 0.),
            wheel: None,
            wheel_played: None,
            crashed: None,
            crash_timer: false,
            favorite_users: std::collections::HashSet::new(),
            favorite_channels: std::collections::HashSet::new(),
            crash_show_trace: false,
            crash_trace: None,
            activity_invite_sent: HashMap::new(),
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

    /// A menu opened in the overlay window goes with the overlay's lock or the overlay itself.
    pub(super) fn overlay_close_context_menu(&mut self) {
        if std::mem::take(&mut self.context.in_overlay) {
            self.context.open = None;
        }
    }

    /// The overlay's clock in ms.
    pub(super) fn overlay_now(&self) -> f64 {
        self.overlay.clock.elapsed().as_secs_f64() * 1000.
    }
}

/// The 40 px icon of a game's application (module 744700's `getApplicationIconURL` with `size: 40`) from the
/// `/applications/public` answer.
pub(super) fn game_icon_url(id: &str, body: &Value) -> Option<String> {
    let app = body
        .as_array()?
        .iter()
        .find(|app| app["id"].as_str() == Some(id))?;
    let icon = app["icon"].as_str()?;
    Some(format!(
        "https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=40"
    ))
}

/// Hiding an unlocked overlay locks its notifications store too (`OVERLAY_SET_INPUT_LOCKED {locked: true}`): the
/// focused toast gets its timer back and the held ones are let in. Returns whether it was unlocked.
pub(super) fn relock_on_hide<T: Clone>(notices: &mut Queue<T>, locked: bool, now: f64) -> bool {
    if locked {
        return false;
    }
    notices.set_locked(true, now);
    true
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

    /// The game's application icon (`getApplicationIconURL`, 40 px), once `/applications/public` and the picture
    /// have arrived.
    pub(in crate::ui) fn overlay_game_icon(
        &self,
        id: Option<&str>,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        let url = game_icon_url(id?, self.fetched.get(&format!("presence-app:{}", id?))?)?;
        self.images.get(&url).cloned()
    }

    /// Asks for what [`Self::overlay_game_icon`] reads (the application, then its picture), each once.
    pub(in crate::ui) fn overlay_request_game_icon(&mut self, id: &str) {
        if id.parse::<u64>().is_err() {
            return;
        }
        let key = format!("presence-app:{id}");
        if !self.fetched.contains_key(&key) {
            self.fetched.insert(key.clone(), Value::Null);
            let _ = self.requests.send(crate::net::Request::Fetch {
                key,
                route: format!("/applications/public?application_ids={id}"),
            });
            return;
        }
        if let Some(url) = self
            .fetched
            .get(&key)
            .and_then(|body| game_icon_url(id, body))
            && !self.images.contains_key(&url)
        {
            self.request_image(url.clone(), url);
        }
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
        let (inputs, rect) = self.overlay_inputs();
        let method = decision::visible(&inputs)?;
        Some((method, rect?))
    }

    /// Why the overlay is not shown now, for the log.
    pub(super) fn overlay_hidden_reason(&self) -> &'static str {
        decision::hidden_reason(&self.overlay_inputs().0).unwrap_or("shown")
    }

    /// Drawing panicked (the error boundary `eP` of module 911024): the overlay locks (`OVERLAY_CRASHED`,
    /// `OVERLAY_SET_INPUT_LOCKED`) and shows "Discord Overlay has crashed!" with Reload Overlay for ten seconds
    /// (`eR`); dismissed or timed out, the window is let go and not opened again this session.
    pub(super) fn overlay_failed(&mut self) {
        self.overlay.failed = true;
        self.overlay.crashed = Some(self.overlay_now());
        self.overlay.crash_timer = false;
        self.overlay.locked = true;
        self.overlay.drag = None;
        self.overlay.menu = None;
        self.overlay.report = None;
        self.overlay.wheel = None;
        self.overlay_close_context_menu();
    }

    /// The crash notice while it is up, else `None` (and the window's handles are let go).
    fn overlay_crash_notice(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let since = self.overlay.crashed.map(|at| self.overlay_now() - at);
        if since.is_none_or(|since| since >= CRASH_NOTICE_MS) {
            self.overlay.crashed = None;
            self.overlay.window = None;
            self.overlay.hwnd = None;
            self.overlay.shown = None;
            return None;
        }
        // One timer for the notice's end, armed on its first draw (not one per redraw).
        if !std::mem::replace(&mut self.overlay.crash_timer, true) {
            let wait = CRASH_NOTICE_MS - since.unwrap_or(0.);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(wait.ceil() as u64))
                    .await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        let toast = Toast {
            kind: ToastKind::Welcome,
            // `X.A` of module 144400 at 40 by 40.
            icon: Some("icon:overlay_crashed".to_owned()),
            title: Some(dc_intl::t("U38qZj").to_owned()),
            body: None,
            hint: None,
            max_lines: None,
            confirm: Some(dc_intl::t("TzAl1a").to_owned()),
            cancel: None,
            click: None,
            channel: None,
            no_zone: false,
        };
        let (width, height) = self.overlay.size;
        let anchor = layout::anchor_pixels(
            layout::default_of(layout::NOTIFICATIONS)
                .map(|widget| widget.anchor)
                .unwrap_or_default(),
            width,
            height,
        );
        let show_trace = self.overlay.crash_show_trace;
        let trace_text = self
            .overlay
            .crash_trace
            .clone()
            .unwrap_or_else(|| "OverlayView::render panic (captured by boundary)".to_owned());
        Some(
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("overlay-crash-notice")
                        .absolute()
                        .left(px(anchor.left.unwrap_or(0.) + 10.))
                        .top(px(anchor.top.unwrap_or(0.) + 10.))
                        .w(px(300.))
                        // Module 911024 (`eM`): Shift+click toggles crash stack trace panel.
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                if event.modifiers.shift {
                                    this.overlay.crash_show_trace = !this.overlay.crash_show_trace;
                                    cx.notify();
                                }
                            }),
                        )
                        .child(self.overlay_toast(CRASH_TOAST, &toast, cx))
                        .child(self.overlay_zone_probe()),
                )
                .when(show_trace, |root| {
                    root.child(
                        div()
                            .id("overlay-crash-trace")
                            .absolute()
                            .left(px(anchor.left.unwrap_or(0.) + 10.))
                            .top(px(anchor.top.unwrap_or(0.) + 140.))
                            .w(px(420.))
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(rgba(0x131318f2))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .font_family("Consolas")
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(trace_text),
                    )
                })
                .into_any_element(),
        )
    }

    /// Reload Overlay (`location.reload`): the overlay is drawn again from scratch.
    fn overlay_reload(&mut self, cx: &mut Context<Self>) {
        tracing::info!("overlay: reloaded after a crash");
        self.overlay.failed = false;
        self.overlay.crashed = None;
        self.overlay.springs.clear();
        self.overlay.toasts_drawn.clear();
        self.overlay.leaving.clear();
        cx.notify();
    }

    /// `OVERLAY_V3_SHOW_WIDGETS` (Alt held and released while unlocked).
    pub(super) fn overlay_show_widgets(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.overlay.show_widgets != show {
            self.overlay.show_widgets = show;
            tracing::debug!(
                "overlay extras {}",
                if show { "shown" } else { "hidden (Alt)" }
            );
            cx.notify();
        }
    }

    /// `WidgetRenderConfig` (`2025-11-overlay-chat`) for this account, or what the harness set.
    pub(super) fn overlay_chat_config(&self) -> dc_overlay::widgets::ChatConfig {
        let variant = self.overlay.chat_variant.unwrap_or_else(|| {
            self.stores
                .experiments
                .variant(dc_overlay::widgets::CHAT_EXPERIMENT)
        });
        dc_overlay::widgets::ChatConfig::of(variant)
    }

    fn overlay_inputs(&self) -> (decision::Inputs, Option<Rect>) {
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
        (inputs, focused.map(|(_, _, rect)| rect))
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
        // The overlay window itself in front (unlocked) keeps the game it covers; any other window replaces it, the
        // app's own main window included (same process, another window).
        let next = decision::next_focus_window(
            self.overlay.focused.map(|focused| focused.pid),
            foreground.map(|foreground| (foreground.hwnd, foreground.pid)),
            self.overlay.hwnd,
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
        if gate()
            && self.overlay.watcher.is_none()
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
        if !gate() || self.overlay.failed {
            return;
        }
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
                    if !dc_overlay::platform::place_checked(hwnd, rect) {
                        tracing::warn!(
                            "overlay: the window does not cover the game after three placements"
                        );
                    }
                }
                tracing::info!(
                    "overlay shown {}x{} at {},{} locked={locked} first={first}",
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
                // A hidden overlay comes back locked (the window resets to locked when it is made again), and so does
                // its notifications store: a focused toast gets its timer back and held ones are released.
                let now = self.overlay_now();
                if relock_on_hide(&mut self.overlay.notices, self.overlay.locked, now) {
                    self.overlay.lock_changed = now;
                }
                self.overlay.locked = true;
                self.overlay.drag = None;
                self.overlay.menu = None;
                self.overlay.report = None;
                self.overlay.widgets_panel = false;
                self.overlay.wheel = None;
                self.overlay_close_context_menu();
                if self.overlay.shown.take().is_some() {
                    #[cfg(windows)]
                    if let Some(hwnd) = self.overlay.hwnd {
                        dc_overlay::platform::hide(hwnd);
                    }
                    tracing::info!("overlay hidden: {}", self.overlay_hidden_reason());
                    self.overlay_schedule_tick(cx);
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
        self.overlay_text_chat_mount();
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
        if !gate() {
            return;
        }
        if std::mem::take(&mut self.overlay.sync_dirty)
            || self.overlay.focused.is_some()
            || self.overlay.shown.is_some()
        {
            self.overlay_sync(cx);
        }
        if self.overlay.shown.is_some() {
            self.overlay_calls(cx);
        }
    }

    fn overlay_open_window(&mut self, cx: &mut Context<Self>) {
        if self.overlay.opening || self.overlay.failed || !window_allowed() {
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
            tracing::debug!("overlay lock ignored: {}", self.overlay_hidden_reason());
            return;
        }
        self.overlay.locked = !self.overlay.locked;
        self.overlay.drag = None;
        self.overlay.menu = None;
        self.overlay_close_context_menu();
        self.overlay.show_widgets = true;
        self.overlay.lock_changed = self.overlay_now();
        tracing::info!(
            "overlay {} (lock keybind, Escape, a close button or a click)",
            if self.overlay.locked {
                "locked"
            } else {
                "unlocked"
            }
        );
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
        if self.overlay.clickable_disabled {
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
        let disabled = self.overlay.clickable_disabled;
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
                let going = this.update(cx, |this, cx| this.overlay_follow_step(cx));
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look at the cursor; false once the overlay is hidden. Every 250 ms the game's window is looked at too: gone
    /// (the game quit), or resized by itself (a resolution change), the decision is taken again; and the overlay's own
    /// window is placed again when something (a DPI change) moved it off the game.
    fn overlay_follow_step(&mut self, cx: &mut Context<Self>) -> bool {
        if self.overlay.shown.is_none() || self.overlay.failed {
            self.overlay.follower = false;
            return false;
        }
        let now = self.overlay_now();
        if now - self.overlay.last_check >= 250. {
            self.overlay.last_check = now;
            #[cfg(windows)]
            if let Some(focused) = self.overlay.focused {
                if !dc_overlay::platform::window_alive(focused.hwnd) {
                    tracing::info!("overlay: the game's window is gone");
                    self.overlay_focus_changed(None, cx);
                } else if let Some(client) = dc_overlay::platform::client_rect_of(focused.hwnd)
                    && client != focused.client
                {
                    let moved = Foreground { client, ..focused };
                    self.overlay.focused = Some(moved);
                    tracing::info!("overlay: the game's window moved or changed size");
                }
                if let (Some(hwnd), Some((rect, _))) = (self.overlay.hwnd, self.overlay.shown)
                    && dc_overlay::platform::client_rect_of(hwnd) != Some(rect)
                {
                    tracing::info!("overlay: the window was moved off the game, placed again");
                    self.overlay.shown = None;
                }
            }
            self.overlay_sync(cx);
            if self.overlay.shown.is_none() {
                self.overlay.follower = false;
                return false;
            }
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
        let hint = self.overlay_keybind_hint_text("z8/sgJ");
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

    /// Module 911024 (`t9`): Activity Invite notification (`aB5xLy`, `VJlc0S` "Join", 30s).
    #[allow(dead_code)]
    pub(super) fn overlay_activity_invite_notice(
        &mut self,
        channel: Id,
        title: String,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let hint = self.overlay_keybind_hint_text("aB5xLy");
        let toast = Toast {
            kind: ToastKind::Text,
            icon: None,
            title: Some(title),
            body: Some(body),
            hint: Some(hint),
            max_lines: Some(2),
            confirm: Some(dc_intl::t("VJlc0S").to_owned()),
            cancel: None,
            click: Some(ToastAction::Click),
            channel: Some(channel),
            no_zone: false,
        };
        self.overlay_push(
            toast,
            Options {
                priority: Priority::High,
                kind: Kind::Generic,
                duration: notify::ACTIVITY_DURATION,
                externally_managed: true,
                channel: Some(channel),
                ..Options::default()
            },
            cx,
        );
    }

    /// Module 911024 (`t9`): Activity Join Request (`VDODnv`, `fgP/wX` "Yeah" / `tpXzJ+` "Nope", 30s).
    #[allow(dead_code)]
    pub(super) fn overlay_activity_join_request_notice(
        &mut self,
        username: &str,
        game: &str,
        cx: &mut Context<Self>,
    ) {
        let body = dc_intl::format(
            "VDODnv",
            &[("username", username.into()), ("game", game.into())],
        );
        let toast = Toast {
            kind: ToastKind::Welcome,
            icon: None,
            title: Some(game.to_owned()),
            body: Some(body),
            hint: None,
            max_lines: Some(2),
            confirm: Some(dc_intl::t("fgP/wX").to_owned()),
            cancel: Some(dc_intl::t("tpXzJ+").to_owned()),
            click: Some(ToastAction::Click),
            channel: None,
            no_zone: false,
        };
        self.overlay_push(
            toast,
            Options {
                priority: Priority::High,
                kind: Kind::Generic,
                duration: notify::ACTIVITY_DURATION,
                ..Options::default()
            },
            cx,
        );
    }

    /// Module 911024 (`t9`): User Joining Game (`Yk+uYG` + `WRj1Wn`).
    #[allow(dead_code)]
    pub(super) fn overlay_activity_user_join_notice(
        &mut self,
        username: &str,
        cx: &mut Context<Self>,
    ) {
        let title = dc_intl::format("Yk+uYG", &[("username", username.into())]);
        let toast = Toast {
            kind: ToastKind::Welcome,
            icon: None,
            title: Some(title),
            body: Some(dc_intl::t("WRj1Wn").to_owned()),
            hint: None,
            max_lines: Some(2),
            confirm: None,
            cancel: None,
            click: None,
            channel: None,
            no_zone: false,
        };
        self.overlay_push(toast, Options::default(), cx);
    }

    /// Module 911024 (`t9`): Go Live nudge (`z9znpa` in voice / `0SVWgF` solo, CTA `U76Ft2` "Go Live", 8s).
    #[allow(dead_code)]
    pub(super) fn overlay_go_live_nudge(
        &mut self,
        game: &str,
        server: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let title = match server {
            Some(guild) if !guild.is_empty() => {
                dc_intl::format("z9znpa", &[("game", game.into()), ("server", guild.into())])
            }
            _ => dc_intl::format("0SVWgF", &[("game", game.into())]),
        };
        let toast = Toast {
            kind: ToastKind::Welcome,
            icon: None,
            title: Some(title),
            body: None,
            hint: None,
            max_lines: Some(2),
            confirm: Some(dc_intl::t("U76Ft2").to_owned()),
            cancel: None,
            click: Some(ToastAction::Confirm),
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

    /// Module 911024 (`STREAM_START`): Clips keybind reminder toast (`S5uhCN`).
    #[allow(dead_code)]
    pub(super) fn overlay_clips_reminder_notice(&mut self, cx: &mut Context<Self>) {
        let body = self.overlay_keybind_hint_text("S5uhCN");
        let toast = Toast {
            kind: ToastKind::Welcome,
            icon: None,
            title: Some(dc_intl::t("NBMK9m").to_owned()),
            body: Some(body),
            hint: None,
            max_lines: Some(2),
            confirm: None,
            cancel: None,
            click: None,
            channel: None,
            no_zone: false,
        };
        self.overlay_push(toast, Options::default(), cx);
    }

    /// Module 907357 (`OverlayActivityWidget` + `OverlayFriendsPlayingList`, `.container_DKsPd0` 300px Onyx):
    /// renders current user's active game card + `"Invite Friends"` (`6Qgrev`) and friend rows
    /// (`y9eo7a`, `"Ask to Join"` `OKsSCR`, `"Invite to Join"` `3fRySx` / `"Message this person..."` `XHxDIV`,
    /// sending `UVBA9g` `"Want to play {gameName}?"` or `DwAcMz` `"Want to play?"`).
    fn overlay_activity_content(&self, locked: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if locked {
            return None;
        }
        let me_id = self.stores.me().map(|u| u.id);
        let my_game = me_id
            .and_then(|id| self.stores.presence.get(id, None))
            .and_then(|p| {
                p.activities
                    .iter()
                    .find(|a| a["type"].as_u64() == Some(0))
                    .and_then(|a| a["name"].as_str())
                    .map(str::to_owned)
            })
            .or_else(|| self.overlay.demo.as_ref().and_then(|d| d.game.name.clone()));

        let now = self.overlay_now();
        let mut friends: Vec<(Id, String, Option<String>, bool, bool)> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == super::friends::RELATIONSHIP_FRIEND)
            .filter_map(|(&uid, _)| {
                let user = self.stores.users.get(&uid)?;
                let presence = self.stores.presence.get(uid, None);
                let act = presence
                    .and_then(|p| p.activities.iter().find(|a| a["type"].as_u64() == Some(0)));
                let streaming = presence
                    .is_some_and(|p| p.activities.iter().any(|a| a["type"].as_u64() == Some(1)));
                let can_ask_join = act.is_some_and(|a| a["party"]["id"].is_string());
                let game_match = my_game.as_deref().is_some_and(|g| {
                    act.and_then(|a| a["name"].as_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case(g))
                });
                if !game_match && act.is_none() {
                    return None;
                }
                let (av_key, _) = avatar_source(user);
                Some((
                    uid,
                    user.display_name().to_owned(),
                    Some(av_key),
                    streaming,
                    can_ask_join,
                ))
            })
            .take(12)
            .collect();
        friends.sort_by(|a, b| a.1.cmp(&b.1));

        if my_game.is_none() && friends.is_empty() {
            return None;
        }
        let game_name = my_game.unwrap_or_default();
        let invite_rows: Vec<AnyElement> = friends
            .into_iter()
            .map(|(uid, name, av_key, streaming, can_ask_join)| {
                let av = av_key.and_then(|k| self.images.get(&k).cloned());
                let sent_at = self.overlay.activity_invite_sent.get(&uid).copied();
                let redirect_ready = sent_at.is_some_and(|at| now - at >= 2000.);
                let sent = sent_at.is_some();
                let game_for_msg = game_name.clone();
                div()
                    .id(SharedString::from(format!("overlay-activity-friend-{uid}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded_full()
                            .bg(color("--background-mod-subtle"))
                            .overflow_hidden()
                            .children(
                                av.map(|img_data| img(img_data).size(px(32.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .mx(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(name),
                            )
                            .when(streaming, |row| {
                                row.child(
                                    div()
                                        .px(px(6.))
                                        .h(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--status-danger"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(10.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child("LIVE"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(can_ask_join, |btns| {
                                btns.child(
                                    div()
                                        .id(SharedString::from(format!("overlay-ask-join-{uid}")))
                                        .aria_label(dc_intl::t("OKsSCR"))
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("user_play", 14., "--text-strong")),
                                )
                            })
                            .child(
                                div()
                                    .id(SharedString::from(format!("overlay-invite-play-{uid}")))
                                    .aria_label(if sent {
                                        dc_intl::t("3fRySx")
                                    } else {
                                        dc_intl::t("XHxDIV")
                                    })
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .bg(if sent {
                                        color("--status-positive")
                                    } else {
                                        color("--control-secondary-background-default")
                                    })
                                    .hover(|s| {
                                        s.bg(if sent {
                                            color("--status-positive")
                                        } else {
                                            color("--control-secondary-background-hover")
                                        })
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        if redirect_ready {
                                            "members_nav"
                                        } else if sent {
                                            "select_check"
                                        } else {
                                            "user_plus"
                                        },
                                        14.,
                                        "--white",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if redirect_ready {
                                            this.open_dm_with(uid, cx);
                                        } else {
                                            let ts = this.overlay_now();
                                            this.overlay.activity_invite_sent.insert(uid, ts);
                                            let content = if game_for_msg.is_empty() {
                                                dc_intl::t("DwAcMz").to_owned()
                                            } else {
                                                dc_intl::format(
                                                    "UVBA9g",
                                                    &[("gameName", game_for_msg.as_str().into())],
                                                )
                                            };
                                            if let Some(dm) = this
                                                .stores
                                                .private_channels
                                                .iter()
                                                .find(|c| {
                                                    c.kind == 1
                                                        && c.recipients.len() == 1
                                                        && c.recipients[0] == uid
                                                })
                                                .map(|c| c.id)
                                            {
                                                let _ =
                                                    this.requests.send(crate::net::Request::Rest {
                                                        key: None,
                                                        method: reqwest::Method::POST,
                                                        route: format!("/channels/{dm}/messages"),
                                                        body: Some(serde_json::json!({
                                                            "content": content,
                                                        })),
                                                    });
                                            }
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        Some(
            div()
                .id("overlay-activity-widget")
                .w(px(300.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgba(0x131318f2))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .gap(px(12.))
                .when(!game_name.is_empty(), |card| {
                    card.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(game_name.clone()),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .h(px(28.))
                                    .rounded(px(6.))
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("6Qgrev")),
                            ),
                    )
                })
                .when(!invite_rows.is_empty(), |card| {
                    card.child(
                        div()
                            .pt(px(12.))
                            .border_t_1()
                            .border_color(color("--background-mod-strong"))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("y9eo7a")),
                            )
                            .children(invite_rows),
                    )
                })
                .into_any_element(),
        )
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
        let hint = self.overlay_keybind_hint_text("ykjOAJ");
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

    /// Overlay toast notification when a clip is saved or fails to save.
    pub(in crate::ui) fn overlay_clip_notice(
        &mut self,
        title: String,
        body: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if crate::storage::is_capture_run() || self.overlay.shown.is_none() {
            return;
        }
        let toast = Toast {
            kind: ToastKind::Text,
            icon: Some("icon:overlay_clips".to_owned()),
            title: Some(title),
            body,
            hint: None,
            max_lines: Some(2),
            confirm: None,
            cancel: None,
            click: None,
            channel: None,
            no_zone: true,
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
        self.overlay_schedule_tick(cx);
        cx.notify();
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
        // The crash notice: Reload Overlay reloads; its X (`hideNotification`) lets the window go.
        if id == CRASH_TOAST {
            match action {
                ToastAction::Confirm => self.overlay_reload(cx),
                ToastAction::Dismiss => {
                    self.overlay.crashed = None;
                    cx.notify();
                }
                _ => {}
            }
            return;
        }
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

    /// Module 119191 (`sI`) / `RunningGameStore.canShowAdminWarning`: on Windows, keybinds cannot reach an elevated
    /// game unless our client is elevated too (no Discord input system service is installed).
    pub(super) fn overlay_keybind_admin_warning(&self) -> bool {
        let game_elevated = self
            .detect
            .store
            .visible_game()
            .is_some_and(|game| game.elevated);
        overlay_admin_warning_needed(
            cfg!(windows),
            game_elevated,
            dc_detect::is_current_process_elevated(),
        )
    }

    /// Formats an overlay keybind hint message (`z8/sgJ`, `ykjOAJ`, `1XA04X`) or falls back to module 119191's
    /// elevated-game admin warning (`PIeSHF` "Keybinds disabled — requires admin privileges").
    pub(super) fn overlay_keybind_hint_text(&self, key: &str) -> String {
        if self.overlay_keybind_admin_warning() {
            dc_intl::t("PIeSHF").to_owned()
        } else {
            dc_intl::format(key, &[("keybind", self.overlay_keybind_text().into())])
        }
    }

    /// Module 119191 `sI`: elevated-game admin warning line (`12/16` semibold, `--text-feedback-warning` when
    /// `highlight` else `--text-muted`). When `can_open_settings` is true, renders clickable `MVS9pV` opening Game
    /// Overlay settings and locking the overlay; otherwise renders `PIeSHF`.
    pub(super) fn overlay_keybind_admin_banner(
        &self,
        highlight: bool,
        can_open_settings: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.overlay_keybind_admin_warning() {
            return None;
        }
        let tint = onyx(if highlight {
            "--text-feedback-warning"
        } else {
            "--text-muted"
        });
        let text = if can_open_settings {
            dc_intl::t("MVS9pV")
                .replace("$[", "")
                .replace("](onSettingsClick)", "")
        } else {
            dc_intl::t("PIeSHF").to_owned()
        };
        let row = div()
            .id("overlay-keybind-admin-warning")
            .px(px(10.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(onyx("--background-surface-higher"))
            .border_1()
            .border_color(onyx("--border-subtle"))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(tint)
            .when(can_open_settings, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.overlay.locked {
                            this.overlay_toggle_lock(cx);
                        }
                        this.open_settings_section(dc_intl::tr("Game Overlay"), cx);
                    }))
            })
            .child(text);
        Some(row.into_any_element())
    }
}

/// Pure predicate for module 119191 (`showAdminWarning`).
pub(super) fn overlay_admin_warning_needed(
    is_windows: bool,
    visible_game_elevated: bool,
    self_elevated: bool,
) -> bool {
    is_windows && visible_game_elevated && !self_elevated
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
            super::voice_marks::HEADPHONES_SLASH
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

    /// Module 588860: opens the Overlay User context menu on right-clicking a voice row in the overlay.
    pub(super) fn open_overlay_user_menu(
        &mut self,
        user: Id,
        at: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (guild, channel) = self
            .call
            .as_ref()
            .map(|c| (Some(c.guild), Some(c.channel)))
            .unwrap_or((self.guild, self.channel));
        self.open_context_menu(
            super::context_menu::Target::OverlayUser {
                user,
                guild,
                channel,
            },
            at,
            cx,
        );
        self.context.in_overlay = true;
        cx.notify();
    }

    /// The voice widget's content: the list, or unlocked with nobody to list the empty state (`O.g`: the voice icon at
    /// 24% under "Connect to a voice channel to see a list of users here.").
    fn overlay_voice_content(
        &mut self,
        widget: &layout::Widget,
        locked: bool,
        flipped: bool,
        show_empty: bool,
        cx: &mut Context<Self>,
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
            let bounds = &self.overlay.bounds;
            return div()
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
                        .map(|row| {
                            let uid = row.id;
                            div()
                                .id(gpui::SharedString::from(format!("overlay-voice-row-{uid}")))
                                .on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.open_overlay_user_menu(uid, ev.position, cx);
                                    }),
                                )
                                .child(voice_row(row, look, joined.get(&uid).copied(), bounds))
                        }),
                )
                .when(split.others.len() > 1, |this| {
                    this.child(others_pill(split.others.len(), pictures, look))
                })
                .into_any_element();
        }
        if voice::shows_empty_state(drawn, locked, show_empty) {
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
    fn overlay_body(
        &mut self,
        width: f32,
        height: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        self.overlay.clickable_disabled = self.overlay_settings().disable_clickable_regions;
        let config = self.overlay_chat_config();
        let mut widgets = Vec::new();
        let mut gated_out = 0;
        let mut drawn_count = 0;
        for widget in dc_overlay::widgets::drawn(&layout, config) {
            drawn_count += 1;
            let anchored_left = match self
                .overlay
                .drag
                .as_ref()
                .filter(|held| held.id == widget.id && held.drag.started)
            {
                Some(held) => held.drag.anchor.left.is_some(),
                None => widget.anchor.left.is_some(),
            };
            // `showEmpty` is the container's `lockExtras`: Alt held hides the empty states too.
            let show_empty = self.overlay.show_widgets;
            let (title, content) = match widget.kind.as_str() {
                layout::VOICE => (
                    dc_intl::tr("Voice"),
                    self.overlay_voice_content(&widget, locked, !anchored_left, show_empty, cx),
                ),
                layout::NOTIFICATIONS => (
                    dc_intl::tr("Notifications"),
                    self.overlay_notifications(locked, widget.pinned, show_empty, cx),
                ),
                layout::QUICK_ACTIONS => (
                    dc_intl::tr("Quick Actions"),
                    self.overlay_quick_actions(locked, cx),
                ),
                layout::TEXT_CHAT => {
                    // `containerRenderGate`: only unlocked, with a voice channel or a chat to show.
                    let voice = self.call.as_ref().map(|call| call.channel);
                    let entries = !self.overlay.text_chat.entries().0.is_empty();
                    if !dc_overlay::widgets::chat_container_shown(locked, voice.is_some(), entries)
                    {
                        gated_out += 1;
                        continue;
                    }
                    (
                        dc_intl::tr("Chat"),
                        self.overlay_text_chat(&widget, frame.width, frame.height, cx),
                    )
                }
                // Video and Streams (`iS`, `e2` of module 415173): with nothing to draw the frame stays, empty.
                layout::VIDEO => (
                    dc_intl::tr("Video"),
                    self.overlay_video_content(&widget, locked, show_empty, (width, height), cx)
                        .unwrap_or_else(|| div().into_any_element()),
                ),
                layout::GO_LIVE => (
                    dc_intl::tr("Streams"),
                    self.overlay_go_live_content(&widget, locked, show_empty, (width, height), cx)
                        .unwrap_or_else(|| div().into_any_element()),
                ),
                // Module 907357 (`OverlayActivityWidget` `Ay`): hidden while locked or when empty.
                layout::ACTIVITY => {
                    let Some(content) = self.overlay_activity_content(locked, cx) else {
                        gated_out += 1;
                        continue;
                    };
                    (dc_intl::tr("Activity"), content)
                }
                _ => continue,
            };
            widgets.push(self.overlay_widget_frame(&widget, title, content, &frame, cx));
        }
        let keybind = self.overlay_keybind_text();
        // `e9`: the empty state, panel closed, while every drawn widget's render gate says no (none drawn: all gone).
        let empty = !locked && drawn_count == gated_out && !self.overlay.widgets_panel;
        div()
            .id("overlay-body")
            .relative()
            .w(px(width))
            .h(px(height))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                this.overlay.pointer = (f32::from(event.position.x), f32::from(event.position.y));
                if this.overlay_drag_move(event.position) {
                    cx.notify();
                }
                if this.overlay.menu.as_ref().is_some_and(|menu| menu.sliding) && event.dragging() {
                    this.overlay_menu_slide(f32::from(event.position.x));
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
                    if let Some(menu) = this.overlay.menu.as_mut() {
                        menu.sliding = false;
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
            .when(!locked && self.overlay.widgets_panel, |this| {
                this.child(self.overlay_widgets_panel(height, config, cx))
            })
            .children(
                (!locked)
                    .then(|| self.overlay_menu_layer(width, height, cx))
                    .flatten(),
            )
            .children(
                (!locked)
                    .then(|| self.overlay_report_layer(width, height, cx))
                    .flatten(),
            )
            // A rail entry's menu opened in this window (the app's `context_menu_layer`).
            .children(
                (!locked && self.context.in_overlay)
                    .then(|| self.context_menu_layer(window, cx))
                    .flatten(),
            )
            // `eQ` of module 427830: the soundboard wheel over everything, locked or not.
            .children(self.overlay_wheel_layer(cx))
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
            "quick" => Some(layout::QUICK_ACTIONS),
            "chat" => Some(layout::TEXT_CHAT),
            "video" => Some(layout::VIDEO),
            "golive" => Some(layout::GO_LIVE),
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
                        let hint = self.overlay_keybind_hint_text("ykjOAJ");
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
                            "{} anchor={:?} size={:?} pinned={} hidden={} z={} opacity={} horizontal={}",
                            widget.kind,
                            widget.anchor,
                            widget.size,
                            widget.pinned,
                            widget.hidden,
                            widget.z_index,
                            widget.opacity,
                            widget.horizontal()
                        )
                    })
                    .collect();
                format!("overlay layout: {}", lines.join(" | "))
            }
            // The Widgets panel, as the toolbar's Widgets button toggles it.
            ["panel"] => {
                self.overlay.widgets_panel = !self.overlay.widgets_panel;
                cx.notify();
                let kinds: Vec<&str> = dc_overlay::widgets::panel(self.overlay_chat_config())
                    .iter()
                    .map(|entry| entry.kind)
                    .collect();
                format!("overlay panel: open={} rows={kinds:?}", self.overlay.widgets_panel)
            }
            // `overlay menu voice|notifications <x> <y>`: the widget's Settings menu at that point.
            ["menu", kind, x, y] => {
                self.overlay.pointer = (x.parse().unwrap_or(0.), y.parse().unwrap_or(0.));
                let menu = match *kind {
                    "voice" => dc_overlay::menus::Menu::Voice,
                    "video" => dc_overlay::menus::Menu::Video,
                    "golive" => dc_overlay::menus::Menu::GoLive,
                    "notifications" => dc_overlay::menus::Menu::Notifications,
                    _ => return "overlay menu voice|notifications|video|golive <x> <y>".to_owned(),
                };
                self.overlay.menu = None;
                self.overlay_open_menu(menu);
                cx.notify();
                format!("overlay menu: {:?}", self.overlay.menu.as_ref().map(|menu| menu.kind))
            }
            // The chat experiment's variation: `none` for the default, else 1 to 3.
            ["chat", variant] => {
                self.overlay.chat_variant = Some(variant.parse().ok());
                cx.notify();
                format!("overlay chat: {:?}", self.overlay_chat_config())
            }
            ["chat-select", channel] => {
                if let Ok(channel) = channel.parse() {
                    self.overlay_chat_select(channel, cx);
                }
                cx.notify();
                let (entries, _) = self.overlay.text_chat.entries();
                format!(
                    "overlay chat-select: selected={:?} entries={}",
                    self.overlay.text_chat.selected(),
                    entries.len()
                )
            }
            ["report"] => {
                self.overlay_open_report(cx);
                cx.notify();
                "overlay report: open".to_owned()
            }
            ["wheel", rest @ ..] => self.script_overlay_wheel(rest, cx),
            // What the error boundary does when drawing fails (the crash notice), and its Reload Overlay.
            ["crash"] => {
                self.overlay_failed();
                cx.notify();
                format!("overlay crash: failed={} notice={:?}", self.overlay.failed, self.overlay.crashed.is_some())
            }
            ["reload"] => {
                self.overlay_toast_action(CRASH_TOAST, ToastAction::Confirm, cx);
                format!("overlay reload: failed={}", self.overlay.failed)
            }
            ["alt", value] => {
                self.overlay_show_widgets(*value != "on", cx);
                format!("overlay alt: show_widgets={}", self.overlay.show_widgets)
            }
            ["state"] => {
                let rows = self.overlay_call_rows().len();
                format!(
                    "overlay state: target={:?} window={} hwnd={} locked={} shown={:?} allowed={} rows={} notices={} size={:?} scale={} panel={} menu={:?} report={} chat={:?} leaving={} reason={}",
                    self.overlay_target(),
                    self.overlay.window.is_some(),
                    self.overlay.hwnd.is_some(),
                    self.overlay.locked,
                    self.overlay.shown,
                    window_allowed(),
                    rows,
                    self.overlay.notices.entries().len(),
                    self.overlay.size,
                    self.overlay.scale,
                    self.overlay.widgets_panel,
                    self.overlay.menu.as_ref().map(|menu| menu.kind),
                    self.overlay.report.is_some(),
                    self.overlay.text_chat.selected(),
                    self.overlay.leaving.len(),
                    self.overlay_hidden_reason()
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
    fn overlay_gate_runs_capture_and_normal_desktop_unless_disabled() {
        assert!(gate_on(true, false, None));
        assert!(gate_on(true, true, None));
        assert!(gate_on(false, false, None));
        assert!(gate_on(false, false, Some("1")));
        assert!(!gate_on(false, false, Some("0")));
        assert!(!gate_on(false, true, None));
        // Unit tests run with `inert_or_test = true`, so a non-capture test never starts a watcher.
        if !crate::storage::is_capture_run() {
            assert!(!gate());
            let mut overlay = Overlay::default();
            assert!(overlay.watcher.is_none());
            overlay.locked = false;
            assert!(!overlay.locked);
        }
    }

    #[test]
    fn the_game_icon_comes_from_its_application() {
        let body = serde_json::json!([{"id": "9", "icon": "abc"}, {"id": "7", "icon": null}]);
        assert_eq!(
            game_icon_url("9", &body).as_deref(),
            Some("https://cdn.discordapp.com/app-icons/9/abc.png?size=40")
        );
        assert_eq!(game_icon_url("7", &body), None);
        assert_eq!(game_icon_url("8", &body), None);
        assert_eq!(game_icon_url("9", &Value::Null), None);
    }

    #[test]
    fn hiding_while_unlocked_lets_held_toasts_in() {
        let text = || Options {
            kind: Kind::Text,
            externally_managed: true,
            ..Options::default()
        };
        let mut queue: Queue<&str> = Queue::default();
        let first = queue.push("first", text(), 0.);
        queue.set_locked(false, 10.);
        let held = queue.push("held", Options::default(), 20.);
        assert!(queue.entries().iter().all(|entry| entry.id != held));
        // Shown again after an alt-tab while unlocked: nothing stuck.
        assert!(relock_on_hide(&mut queue, false, 30.));
        assert!(queue.entries().iter().any(|entry| entry.id == held));
        assert!(
            queue
                .entries()
                .iter()
                .all(|entry| entry.status != notify::Status::Focused)
        );
        assert!(queue.entries().iter().any(|entry| entry.id == first));
        assert!(queue.next_deadline().is_some(), "the timers run again");
        // Already locked: nothing to do.
        assert!(!relock_on_hide(&mut queue, true, 40.));
    }

    #[test]
    fn a_panic_while_drawing_is_caught() {
        assert_eq!(guarded(|| 4), Some(4));
        let caught: Option<u8> = guarded(|| panic!("drawing failed"));
        assert_eq!(caught, None);
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
                "overlay_headphones_slash",
                "voice_member_camera"
            ]
        );
        let mut server_deaf = row(3);
        server_deaf.server_deaf = true;
        assert_eq!(voice_icons(&server_deaf), ["overlay_server_deafened"]);
        assert!(voice_icons(&row(4)).is_empty());
    }

    #[test]
    fn quick_actions_and_text_chat_default_boxes_clear_at_720p_and_1080p() {
        let mut layout = layout::Layout::default();
        layout.ensure(&mut new_widget_id);
        let quick = layout.widget(layout::QUICK_ACTIONS).unwrap();
        let chat = layout.widget(layout::TEXT_CHAT).unwrap();
        let qa_h = crate::ui::overlay_chat::QUICK_ACTIONS_WRAPPER_HEIGHT;
        assert_eq!(qa_h, 84.);
        let extras_h = 10. + 24.;

        let q720 = layout::anchor_pixels(quick.anchor, 1280., 720.);
        let c720 = layout::anchor_pixels(chat.anchor, 1280., 720.);
        let (_, ch720) =
            dc_overlay::drag::requested_size(chat.size.pixels(1280., 720.), chat.min_size);
        let qe720 = dc_overlay::drag::resolve(q720, 1280., 720., 356., qa_h);
        let ce720 = dc_overlay::drag::resolve(c720, 1280., 720., 500., ch720.unwrap());
        assert_eq!((qe720.top, qe720.top + qa_h), (14., 98.));
        assert_eq!((ce720.top, ce720.top + ch720.unwrap()), (100., 472.));
        assert!(qe720.top + qa_h < ce720.top);

        let q1080 = layout::anchor_pixels(quick.anchor, 1920., 1080.);
        let c1080 = layout::anchor_pixels(chat.anchor, 1920., 1080.);
        let (_, ch1080) =
            dc_overlay::drag::requested_size(chat.size.pixels(1920., 1080.), chat.min_size);
        let qe1080 = dc_overlay::drag::resolve(q1080, 1920., 1080., 356., qa_h);
        let ce1080 = dc_overlay::drag::resolve(c1080, 1920., 1080., 500., ch1080.unwrap());
        assert_eq!((qe1080.top, qe1080.top + qa_h), (21., 105.));
        assert_eq!((ce1080.top, ce1080.top + ch1080.unwrap()), (151., 523.));
        assert!(qe1080.top + qa_h + extras_h < ce1080.top);
    }

    #[test]
    fn locked_overlay_hides_quick_actions_and_text_chat() {
        let mut layout = layout::Layout::default();
        layout.ensure(&mut new_widget_id);
        let chat = layout.widget(layout::TEXT_CHAT).unwrap();
        let quick = layout.widget(layout::QUICK_ACTIONS).unwrap();
        assert!(!chat.pinned);
        assert!(!quick.pinned);
        assert!(!dc_overlay::widgets::chat_container_shown(true, true, true));
    }

    #[test]
    fn elevated_game_triggers_admin_keybind_warning_only_when_unprivileged_on_windows() {
        assert!(overlay_admin_warning_needed(true, true, false));
        assert!(!overlay_admin_warning_needed(true, false, false));
        assert!(!overlay_admin_warning_needed(true, true, true));
        assert!(!overlay_admin_warning_needed(false, true, false));
        assert!(!dc_intl::t("PIeSHF").is_empty());
        assert!(!dc_intl::t("MVS9pV").is_empty());
    }

    #[test]
    fn overlay_activity_907357_and_notifications_911024_intl() {
        for key in [
            "3fRySx", "6Qgrev", "DwAcMz", "OKsSCR", "XHxDIV", "y9eo7a", "VJlc0S", "fgP/wX",
            "tpXzJ+", "WRj1Wn", "U76Ft2",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
        assert!(dc_intl::format("UVBA9g", &[("gameName", "Valorant".into())]).contains("Valorant"));
        assert!(
            dc_intl::format(
                "VDODnv",
                &[("username", "Ada".into()), ("game", "Apex".into())]
            )
            .contains("Ada")
        );
        assert!(dc_intl::format("Yk+uYG", &[("username", "Ada".into())]).contains("Ada"));
        assert!(
            dc_intl::format(
                "z9znpa",
                &[("game", "Apex".into()), ("server", "Squad".into())]
            )
            .contains("Squad")
        );
        assert!(dc_intl::format("0SVWgF", &[("game", "Apex".into())]).contains("Apex"));
        assert!(dc_intl::has_key("aB5xLy"));
        assert!(dc_intl::has_key("S5uhCN"));
    }
}
