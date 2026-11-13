//! When the overlay is drawn over a game.
//!
//! Ported from the pinned build:
//! - `eY` of `RunningGameStore` (module 397369): whether the overlay is enabled for one game, from the user's switch
//!   (`enableOverlay` / `enableOverlayV3`, keyed by `exePath:name`), the game database, and the global switches.
//! - `Q` of `OverlayRenderStore` (module 861862): the overlay method for a tracked game from the global switches, the
//!   game's enablement and its fullscreen type.
//! - The overlay window's own floor (module 427830): nothing is drawn in a window under 640 by 360.
//!
//! No voice call is needed: the overlay follows the focused game alone. What it then draws (the voice widget only in
//! a call, unless unlocked) is the widgets' business ([`crate::voice`]).

/// How the game's window is shown (`l.aI` of module 861862's import, reported by the native module).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullscreenType {
    Unknown,
    Windowed,
    Maximized,
    BorderlessFullscreen,
    /// Exclusive fullscreen: only the injected overlay can draw over it.
    Fullscreen,
    Minimized,
}

/// A screen rectangle in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The foreground window as the watcher reports it: its handle, its process, its client area on screen and how it is
/// shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Foreground {
    pub hwnd: isize,
    pub pid: u32,
    pub client: Rect,
    pub fullscreen: FullscreenType,
}

/// `T.Ue` (module 861862): how the overlay reaches the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayMethod {
    Disabled,
    /// The out-of-process window over the game.
    OutOfProcess,
    /// The same window with Compatibility Mode (`limitedInteractionOverride`): it never takes the focus.
    OutOfProcessLimitedInteraction,
    /// The injected legacy overlay.
    Hook,
}

impl OverlayMethod {
    /// The method is the out-of-process window, the only one gravecord draws.
    pub fn is_out_of_process(self) -> bool {
        matches!(
            self,
            Self::OutOfProcess | Self::OutOfProcessLimitedInteraction
        )
    }
}

/// `enabledOOP` and `enabledLegacy` of one game (`eY`'s result).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GameEnablement {
    pub oop: bool,
    pub legacy: bool,
}

/// What `eY` reads about one game.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameFacts {
    /// `isLauncher`: launchers never get the overlay.
    pub is_launcher: bool,
    /// `enableOverlay[key]`: the user's legacy switch for this game.
    pub user_legacy: Option<bool>,
    /// `enableOverlayV3[key]`: the user's switch for this game.
    pub user_oop: Option<bool>,
    /// The game is in the detectable games database (`eH` found a record): its `overlay` field, when known.
    pub database_overlay: Option<bool>,
    pub in_database: bool,
    /// `eV`: the game is on the out-of-process exclusion list. The list itself is not in the snapshot.
    pub excluded: bool,
}

/// `eY` (module 397369) for a game this process may draw over. `oop_supported` is `T6()` (the out-of-process module
/// is loaded and supported); `legacy_global` is `OverlayStore6.legacyEnabled`.
pub fn game_enablement(
    facts: &GameFacts,
    oop_supported: bool,
    legacy_global: bool,
) -> GameEnablement {
    if facts.is_launcher {
        return GameEnablement::default();
    }
    let usable = oop_supported && !facts.excluded;
    if facts.user_legacy.is_some() || facts.user_oop.is_some() {
        return GameEnablement {
            oop: facts.user_oop.unwrap_or(usable),
            legacy: facts.user_legacy.unwrap_or(legacy_global),
        };
    }
    if facts.in_database {
        // `eH`: `enabled: r.overlay ?? false`, `enabledOOP: T6() && !excluded || false`.
        return GameEnablement {
            oop: usable,
            legacy: facts.database_overlay.unwrap_or(false),
        };
    }
    GameEnablement {
        oop: usable,
        legacy: false,
    }
}

/// The value the Registered Games row's overlay switch shows (`overlay` of `ez`, module 397369): the user's own
/// switch when set, else whether the game is enabled either way.
pub fn row_switch(facts: &GameFacts, oop_supported: bool, legacy_global: bool) -> bool {
    if let Some(on) = facts.user_legacy.or(facts.user_oop) {
        return on;
    }
    let enabled = game_enablement(facts, oop_supported, legacy_global);
    enabled.legacy || enabled.oop
}

/// The global switches (`OverlayStore6`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Global {
    pub oop: bool,
    pub legacy: bool,
}

/// `Q` (module 861862): the method for a running, tracked game.
pub fn method(
    global: Global,
    game: Option<GameEnablement>,
    oop_supported: bool,
    fullscreen: FullscreenType,
    limited_interaction: bool,
) -> OverlayMethod {
    if !(global.oop || global.legacy) {
        return OverlayMethod::Disabled;
    }
    let Some(game) = game else {
        return OverlayMethod::Disabled;
    };
    if !(game.oop || game.legacy) {
        return OverlayMethod::Disabled;
    }
    let legacy_method = |enabled: bool| {
        if enabled {
            OverlayMethod::Hook
        } else {
            OverlayMethod::Disabled
        }
    };
    if !global.oop {
        return legacy_method(game.legacy);
    }
    if !oop_supported {
        return legacy_method(game.legacy && global.legacy);
    }
    if !game.oop && game.legacy {
        return legacy_method(global.legacy);
    }
    match fullscreen {
        FullscreenType::Minimized
        | FullscreenType::Windowed
        | FullscreenType::Maximized
        | FullscreenType::BorderlessFullscreen => {
            if game.oop {
                if limited_interaction {
                    OverlayMethod::OutOfProcessLimitedInteraction
                } else {
                    OverlayMethod::OutOfProcess
                }
            } else {
                legacy_method(game.legacy && global.legacy)
            }
        }
        FullscreenType::Fullscreen => legacy_method(game.legacy && global.legacy),
        FullscreenType::Unknown => OverlayMethod::Disabled,
    }
}

/// Module 427830: the overlay draws nothing in a window under 640 by 360.
pub const MIN_WIDTH: u32 = 640;
pub const MIN_HEIGHT: u32 = 360;

/// The focused window, as the foreground watcher reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focused {
    pub pid: u32,
    pub fullscreen: FullscreenType,
    pub width: u32,
    pub height: u32,
}

/// Everything the window's visibility depends on.
#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    /// Signed in (the store only tracks games for an authenticated user).
    pub authenticated: bool,
    pub global: Global,
    pub oop_supported: bool,
    /// The focused window, when it belongs to a running game this client tracks; with that game's enablement.
    pub focused: Option<(Focused, GameEnablement)>,
    pub limited_interaction: bool,
}

/// Whether the overlay window is shown over the focused game, and with which method.
pub fn visible(inputs: &Inputs) -> Option<OverlayMethod> {
    if !inputs.authenticated {
        return None;
    }
    let (focused, game) = inputs.focused?;
    if focused.fullscreen == FullscreenType::Minimized {
        return None;
    }
    let method = method(
        inputs.global,
        Some(game),
        inputs.oop_supported,
        focused.fullscreen,
        inputs.limited_interaction,
    );
    (method.is_out_of_process() && focused.width >= MIN_WIDTH && focused.height >= MIN_HEIGHT)
        .then_some(method)
}

/// What the Game Overlay page's Current Game switch changes (module 158935 `pC`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchScope {
    /// The game's own switches (`RUNNING_GAME_TOGGLE_OVERLAY`, both methods).
    Game,
    /// The global switches (`OVERLAY_SET_ENABLED`, both methods).
    Global,
    Both,
}

/// The scope of the Current Game switch going to `on` from `checked`: turning it on where the game is on but its
/// method is off for all games turns the global switches on, and the game's too when some method is off for it.
pub fn current_game_scope(
    on: bool,
    checked: bool,
    global: Global,
    game: GameEnablement,
) -> SwitchScope {
    let turning_on = on && !checked;
    let game_on_global_off = (!global.oop && game.oop) || (!global.legacy && game.legacy);
    let game_off_global_on = (!game.oop && global.oop) || (!game.legacy && global.legacy);
    match (turning_on, game_on_global_off, game_off_global_on) {
        (true, true, true) => SwitchScope::Both,
        (true, true, false) => SwitchScope::Global,
        _ => SwitchScope::Game,
    }
}

/// The focus moved to the overlay window itself (while unlocked): the game it covers stays the focused one, as the
/// native module excludes its own window. Any other window replaces it.
pub fn next_focus(current: Option<u32>, new_pid: Option<u32>, own_pid: u32) -> Option<u32> {
    match new_pid {
        Some(pid) if pid == own_pid => current,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Global = Global {
        oop: true,
        legacy: true,
    };

    fn focused(fullscreen: FullscreenType) -> Focused {
        Focused {
            pid: 42,
            fullscreen,
            width: 1920,
            height: 1080,
        }
    }

    fn inputs(game: GameEnablement, fullscreen: FullscreenType) -> Inputs {
        Inputs {
            authenticated: true,
            global: ON,
            oop_supported: true,
            focused: Some((focused(fullscreen), game)),
            limited_interaction: false,
        }
    }

    #[test]
    fn a_game_is_enabled_by_default_and_never_a_launcher() {
        let plain = GameFacts::default();
        assert_eq!(
            game_enablement(&plain, true, true),
            GameEnablement {
                oop: true,
                legacy: false
            }
        );
        let launcher = GameFacts {
            is_launcher: true,
            user_oop: Some(true),
            ..GameFacts::default()
        };
        assert_eq!(
            game_enablement(&launcher, true, true),
            GameEnablement::default()
        );
        // Excluded from the out-of-process overlay, or no module: off unless the user says otherwise.
        let excluded = GameFacts {
            excluded: true,
            ..GameFacts::default()
        };
        assert!(!game_enablement(&excluded, true, true).oop);
        assert!(!game_enablement(&plain, false, true).oop);
    }

    #[test]
    fn the_users_switch_for_a_game_wins() {
        let off = GameFacts {
            user_oop: Some(false),
            ..GameFacts::default()
        };
        let enabled = game_enablement(&off, true, true);
        assert!(!enabled.oop);
        // The legacy half follows the global legacy switch when only the v3 switch was set.
        assert!(enabled.legacy);
        assert!(!row_switch(&off, true, true));
        let database = GameFacts {
            in_database: true,
            database_overlay: Some(true),
            ..GameFacts::default()
        };
        assert_eq!(
            game_enablement(&database, true, false),
            GameEnablement {
                oop: true,
                legacy: true
            }
        );
        assert!(row_switch(&GameFacts::default(), true, true));
    }

    #[test]
    fn the_method_follows_the_fullscreen_type() {
        let oop = GameEnablement {
            oop: true,
            legacy: false,
        };
        for kind in [
            FullscreenType::Windowed,
            FullscreenType::Maximized,
            FullscreenType::BorderlessFullscreen,
            FullscreenType::Minimized,
        ] {
            assert_eq!(
                method(ON, Some(oop), true, kind, false),
                OverlayMethod::OutOfProcess
            );
        }
        assert_eq!(
            method(
                ON,
                Some(oop),
                true,
                FullscreenType::BorderlessFullscreen,
                true
            ),
            OverlayMethod::OutOfProcessLimitedInteraction
        );
        // Exclusive fullscreen is the injected overlay's, which this game does not have.
        assert_eq!(
            method(ON, Some(oop), true, FullscreenType::Fullscreen, false),
            OverlayMethod::Disabled
        );
        let both = GameEnablement {
            oop: true,
            legacy: true,
        };
        assert_eq!(
            method(ON, Some(both), true, FullscreenType::Fullscreen, false),
            OverlayMethod::Hook
        );
        assert_eq!(
            method(ON, Some(both), true, FullscreenType::Unknown, false),
            OverlayMethod::Disabled
        );
        // Globally off, or no game.
        let off = Global {
            oop: false,
            legacy: false,
        };
        assert_eq!(
            method(off, Some(both), true, FullscreenType::Windowed, false),
            OverlayMethod::Disabled
        );
        assert_eq!(
            method(ON, None, true, FullscreenType::Windowed, false),
            OverlayMethod::Disabled
        );
        // Only the legacy switch on: the hook, never the window.
        let legacy_only = Global {
            oop: false,
            legacy: true,
        };
        assert_eq!(
            method(
                legacy_only,
                Some(both),
                true,
                FullscreenType::Windowed,
                false
            ),
            OverlayMethod::Hook
        );
    }

    #[test]
    fn the_window_shows_for_a_focused_enabled_game_only() {
        let oop = GameEnablement {
            oop: true,
            legacy: false,
        };
        assert_eq!(
            visible(&inputs(oop, FullscreenType::BorderlessFullscreen)),
            Some(OverlayMethod::OutOfProcess)
        );
        // Not focused (another window in front), signed out, minimized, disabled for the game, or too small.
        let mut nothing = inputs(oop, FullscreenType::Windowed);
        nothing.focused = None;
        assert_eq!(visible(&nothing), None);
        let mut signed_out = inputs(oop, FullscreenType::Windowed);
        signed_out.authenticated = false;
        assert_eq!(visible(&signed_out), None);
        assert_eq!(visible(&inputs(oop, FullscreenType::Minimized)), None);
        assert_eq!(
            visible(&inputs(GameEnablement::default(), FullscreenType::Windowed)),
            None
        );
        let mut small = inputs(oop, FullscreenType::Windowed);
        if let Some((focused, _)) = small.focused.as_mut() {
            focused.width = 639;
        }
        assert_eq!(visible(&small), None);
        let mut global_off = inputs(oop, FullscreenType::Windowed);
        global_off.global.oop = false;
        assert_eq!(visible(&global_off), None);
    }

    #[test]
    fn the_current_game_switch_reaches_the_global_switches_when_needed() {
        let on_off = GameEnablement {
            oop: true,
            legacy: false,
        };
        let global_off = Global {
            oop: false,
            legacy: false,
        };
        // Game on, OOP off for all games, legacy off for the game and for all: only the global switches.
        assert_eq!(
            current_game_scope(true, false, global_off, on_off),
            SwitchScope::Global
        );
        // Legacy on for all but off for the game: both.
        let legacy_on = Global {
            oop: false,
            legacy: true,
        };
        assert_eq!(
            current_game_scope(true, false, legacy_on, on_off),
            SwitchScope::Both
        );
        // Turning off, or the game alone off: the game.
        assert_eq!(
            current_game_scope(false, true, ON, on_off),
            SwitchScope::Game
        );
        assert_eq!(
            current_game_scope(true, false, ON, GameEnablement::default()),
            SwitchScope::Game
        );
    }

    #[test]
    fn focusing_the_overlay_keeps_the_game_focused() {
        assert_eq!(next_focus(Some(42), Some(7), 7), Some(42));
        assert_eq!(next_focus(Some(42), Some(9), 7), Some(9));
        assert_eq!(next_focus(Some(42), None, 7), None);
    }
}
