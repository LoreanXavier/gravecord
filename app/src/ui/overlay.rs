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
//!   and shows the voice widget's header (`widgetHeaderContainer_DKMuWq`). Escape locks it again (module 427830).
//! - Kept out of screen captures (`WDA_EXCLUDEFROMCAPTURE`), out of the taskbar and Alt+Tab (`WS_EX_TOOLWINDOW`).
//!
//! A harness run never starts the foreground watcher and never registers the keybind (`kb_sync_hooks`); it opens the
//! window only on the harness's hidden desktop, and `overlay shot` renders it to a picture.

use super::{Root, avatar_source, kerning};
use dc_overlay::decision::{self, Foreground, OverlayMethod, Rect};
use dc_overlay::layout::{self, Layout};
use dc_overlay::store::{self, AvatarSize, DisplayNameMode, OverlaySettings};
use dc_overlay::voice;
use dc_state::running_games::Game;
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Render, Rgba, WeakEntity, Window, WindowHandle,
    div, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;

/// The harness's hidden desktop (`DESKTOP` of `xtask/src/headless.rs`).
const HIDDEN_DESKTOP: &str = "gravecord-headless";

/// `username_ieJ4Sq` and `hiddenVoiceStatesText_ieJ4Sq`: `rgba(19,19,24,.8)`; the header pills `bgShade_Ry5oWJ`: .9.
const PILL: u32 = 0x131318cc;
const SHADE: u32 = 0x131318e6;
/// `bgShade_Ry5oWJ` border.
const SHADE_BORDER: u32 = 0xffffff14;

/// The overlay is forced to the Onyx theme (`theme: ONYX`, drawn with the `theme-midnight` class in the capture).
fn onyx(name: &str) -> Rgba {
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
        let body = self
            .root
            .upgrade()
            .map(|root| root.update(cx, |root, cx| root.overlay_body(width, height, cx)));
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
            .children(body)
    }
}

/// A made-up game in the foreground, for the harness.
#[derive(Clone)]
struct Demo {
    foreground: Foreground,
    game: Game,
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
        }
    }
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

    /// Where the window is shown now and how, or none: `dc_overlay::decision::visible`.
    pub(super) fn overlay_target(&self) -> Option<(OverlayMethod, Rect)> {
        let global = self.overlay_global();
        let focused = self.overlay_focused_game().map(|(foreground, game)| {
            let (user_legacy, user_oop) = self.detect.store.overlay_override(&game);
            let facts = decision::GameFacts {
                is_launcher: game.is_launcher,
                user_legacy,
                user_oop,
                database_overlay: None,
                in_database: game.id.is_some(),
                excluded: false,
            };
            let enabled = decision::game_enablement(&facts, true, global.legacy);
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
                self.overlay.shown = Some((rect, locked));
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
                cx.notify();
            }
            None => {
                // A hidden overlay comes back locked (the window resets to locked when it is made again).
                self.overlay.locked = true;
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

    /// Each frame of the main view while a game has the focus: a setting changed on the page shows at once.
    pub(super) fn overlay_frame(&mut self, cx: &mut Context<Self>) {
        if self.overlay.focused.is_some() || self.overlay.shown.is_some() {
            self.overlay_sync(cx);
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
        tracing::info!("overlay locked={}", self.overlay.locked);
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
}

// ----- drawing -----

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

/// One person (`voiceUser_ieJ4Sq`): the avatar with its speaking outline, then the name pill with the voice icons.
fn voice_row(
    row: &Row,
    size: AvatarSize,
    locked: bool,
    preview: bool,
    names: DisplayNameMode,
) -> AnyElement {
    let avatar = size.pixels();
    let opacity = voice::row_opacity(locked, row.speaking, false);
    let name_shown = voice::name_shown(locked || preview, names, row.speaking);
    let icons = voice_icons(row);
    div()
        .flex()
        .items_center()
        .h(px(avatar))
        .opacity(opacity)
        .child(
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
                // `.speaking .avatarSpeakingOutline_ieJ4Sq`: a 1px `--green-300` outline 2px out.
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
                }),
        )
        .when(name_shown, |this| {
            this.child(
                div()
                    .ml(px(voice::PILL_MARGIN))
                    .h(px(voice::PILL_HEIGHT))
                    .px(px(voice::PILL_PADDING_X))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(onyx("--border-subtle"))
                    .bg(rgba(PILL))
                    .opacity(0.8)
                    .overflow_hidden()
                    .child(
                        div()
                            .whitespace_nowrap()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(onyx("--text-default"))
                            .child(row.name.clone()),
                    )
                    .when(!icons.is_empty(), |this| {
                        this.child(div().flex().gap(px(voice::ICON_GAP)).children(
                            icons.into_iter().map(|name| {
                                svg()
                                    .path(format!("icons/{name}.svg"))
                                    .size(px(voice::ICON))
                                    .flex_none()
                                    .text_color(gpui::white())
                                    .opacity(0.5)
                            }),
                        ))
                    }),
            )
        })
        .into_any_element()
}

/// The "+N others in voice" pill (`hiddenVoiceStates_ieJ4Sq`), its text hard-coded English in the official client.
fn others_pill(count: usize, locked: bool) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .when(locked, |this| this.opacity(0.4))
        .child(
            div()
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
                .child(format!("+{count} others in voice")),
        )
        .into_any_element()
}

/// The voice widget's list (`voiceUserContainer_ieJ4Sq`, module 502812 `ew`) for rows already in channel order.
pub(super) fn voice_list(
    rows: &[Row],
    settings: &OverlaySettings,
    max: i64,
    locked: bool,
    preview: bool,
) -> AnyElement {
    let members: Vec<voice::Member> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| voice::Member {
            id: row.id,
            speaking: row.speaking,
            current: index == 0 && !preview,
        })
        .collect();
    let ordered = voice::order(&members, max);
    let split = voice::split(&ordered, max, settings.display_user_mode);
    let find = |id: u64| rows.iter().find(|row| row.id == id);
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
                    voice::row_shown(locked || preview, settings.display_user_mode, row.speaking)
                })
                .map(|row| {
                    voice_row(
                        row,
                        settings.avatar_size_mode,
                        locked,
                        preview,
                        settings.display_name_mode,
                    )
                }),
        )
        .when(split.others.len() > 1, |this| {
            this.child(others_pill(split.others.len(), locked))
        })
        .into_any_element()
}

/// The widget header (`widgetHeaderContainer_DKMuWq`): the title pill and the pin button, 4px apart.
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
                .border_color(rgba(SHADE_BORDER))
                .bg(rgba(SHADE))
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
                .border_color(rgba(SHADE_BORDER))
                .bg(rgba(SHADE))
                .child(
                    svg()
                        .path("icons/overlay_pin.svg")
                        .size(px(12.))
                        .text_color(onyx("--text-strong"))
                        .when(!pinned, |this| this.opacity(0.5)),
                ),
        )
        .into_any_element()
}

impl Root {
    /// The people of the current call as the voice widget lists them, this account first when it is in the call.
    fn overlay_call_rows(&mut self) -> Vec<Row> {
        let Some((guild, channel)) = self.call.as_ref().map(|call| (call.guild, call.channel))
        else {
            return Vec::new();
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let mut states: Vec<dc_state::VoiceState> = self
            .stores
            .voice_members(guild, channel)
            .into_iter()
            .cloned()
            .collect();
        states.sort_by_key(|state| Some(state.user_id) != me);
        let mut rows = Vec::new();
        for state in states {
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
            let (key, url) = avatar_source(&user);
            if !self.images.contains_key(&key) {
                self.request_image(key.clone(), url);
            }
            rows.push(Row {
                id: state.user_id,
                name: user.display_name().to_owned(),
                avatar: self.images.get(&key).cloned(),
                speaking: self.speaking.contains(&state.user_id)
                    || self.soundboard.user_playing(state.user_id),
                mute: state.self_mute,
                server_mute: state.mute || state.suppress,
                deaf: state.self_deaf,
                server_deaf: state.deaf,
                video: state.self_video,
            });
        }
        rows
    }

    /// The overlay window's content at its size: the scrim while unlocked, then the voice widget at its anchor.
    fn overlay_body(&mut self, width: f32, height: f32, _cx: &mut Context<Self>) -> AnyElement {
        let locked = self.overlay.locked;
        let settings = self.overlay_settings();
        let layout = self.overlay_layout();
        let widget = layout.widget(layout::VOICE).filter(|widget| !widget.hidden);
        let rows = self.overlay_call_rows();
        let in_channel = self.call.is_some();
        let voice = widget.map(|widget| {
            let at = layout::anchor_pixels(widget.anchor, width, height);
            let drawn = voice::widget_shown(!rows.is_empty(), in_channel, locked, widget.pinned);
            let title = self
                .call
                .as_ref()
                .and_then(|call| {
                    let guild = self.stores.guild_of_channel(call.channel)?;
                    let channel = guild
                        .channels
                        .iter()
                        .find(|known| known.id == call.channel)?;
                    Some(channel.name.clone())
                })
                .unwrap_or_default();
            let body = if drawn {
                Some(voice_list(
                    &rows,
                    &settings,
                    layout.voice_max_shown(),
                    locked,
                    false,
                ))
            } else if voice::shows_empty_state(drawn, locked, true) {
                Some(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(16.))
                        .bg(rgba(PILL))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(onyx("--text-default"))
                        .child(dc_intl::tr(
                            "Connect to a voice channel to see a list of users here.",
                        ))
                        .into_any_element(),
                )
            } else {
                None
            };
            div()
                .absolute()
                .when_some(at.left, |this, left| this.left(px(left)))
                .when_some(at.right, |this, right| this.right(px(right)))
                .when_some(at.top, |this, top| this.top(px(top)))
                .when_some(at.bottom, |this, bottom| this.bottom(px(bottom)))
                .opacity(widget.opacity)
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(body)
                .when(!locked, |this| {
                    this.child(widget_header(title, widget.pinned))
                })
        });
        div()
            .id("overlay-body")
            .relative()
            .size_full()
            // `.overlayBackground` with `--background-scrim` while unlocked.
            .when(!locked, |this| this.bg(onyx("--background-scrim")))
            .children(voice)
            .into_any_element()
    }
}

// ----- harness -----

impl Root {
    /// Capture only: `overlay demo [w h]`, `overlay away`, `overlay toggle`, `overlay game on|off|clear`,
    /// `overlay speak`, `overlay state`. `overlay shot <png>` is handled by the harness.
    #[cfg(feature = "capture")]
    pub(crate) fn script_overlay(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let demo_game = || Game {
            name: Some("Demo Game".to_owned()),
            exe_path: "c:/games/demo/demo.exe".to_owned(),
            pid: Some(4242),
            ..Game::default()
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
            ["state"] => {
                let rows = self.overlay_call_rows().len();
                format!(
                "overlay state: target={:?} window={} hwnd={} locked={} shown={:?} allowed={} rows={}",
                self.overlay_target(),
                self.overlay.window.is_some(),
                self.overlay.hwnd.is_some(),
                self.overlay.locked,
                self.overlay.shown,
                window_allowed(),
                rows
            )
            }
            _ => "usage: overlay demo [w h] | away | toggle | game on|off | speak | state | shot <png>".to_owned(),
        }
    }

    /// The overlay window, for `overlay shot`.
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
