//! The System page's desktop switches (module 158935, `OS_OPEN_ON_STARTUP`, `OS_START_MINIMIZED`,
//! `OS_MINIMIZE_TO_TRAY`). Open on startup is the Run registry value (`autostart.rs`), read back each time the page
//! shows so the switch tells the truth even when the entry was removed in Windows' Startup Apps. Start minimized is a
//! local preference that also sets the argument in that value; it shows on only with open on startup on, and is
//! disabled without it (`useValue: openOnStartup && startMinimized`, `useDisabled: !openOnStartup`). Minimize to tray
//! is the local preference `refresh_tray` already hands the tray (`tray::State::close_to_tray`).

use crate::ui::Root;

pub(super) const OPEN_ON_STARTUP: &str = "os_open_on_startup";
pub(super) const START_MINIMIZED: &str = "os_start_minimized";

/// What the two startup switches show.
pub(super) fn startup_view(open: bool, start_minimized: bool, key: &str) -> Option<(bool, bool)> {
    match key {
        OPEN_ON_STARTUP => Some((open, false)),
        START_MINIMIZED => Some((open && start_minimized, !open)),
        _ => None,
    }
}

impl Root {
    fn startup_open(&self) -> bool {
        // A capture run has no registry (`autostart::switch_value` is `None`): the page's own choice stands.
        crate::autostart::switch_value().unwrap_or_else(|| self.pref_on(OPEN_ON_STARTUP, false))
    }

    /// The switch's value and whether it is disabled, for the rows this page owns.
    pub(super) fn system_switch(&self, key: &str) -> Option<(bool, bool)> {
        if key != OPEN_ON_STARTUP && key != START_MINIMIZED {
            return None;
        }
        startup_view(
            self.startup_open(),
            self.pref_on(START_MINIMIZED, false),
            key,
        )
    }

    /// A switch of the page changed: the registry follows. Start minimized's own value is saved by `pref_changed`.
    pub(super) fn system_setting_changed(&mut self, key: &str, on: bool) {
        match key {
            OPEN_ON_STARTUP => crate::autostart::set(on, self.pref_on(START_MINIMIZED, false)),
            START_MINIMIZED => crate::autostart::start_minimized_changed(on),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_minimized_follows_open_on_startup() {
        assert_eq!(
            startup_view(false, true, START_MINIMIZED),
            Some((false, true))
        );
        assert_eq!(
            startup_view(true, true, START_MINIMIZED),
            Some((true, false))
        );
        assert_eq!(
            startup_view(true, false, START_MINIMIZED),
            Some((false, false))
        );
        assert_eq!(
            startup_view(true, false, OPEN_ON_STARTUP),
            Some((true, false))
        );
        assert_eq!(
            startup_view(false, true, OPEN_ON_STARTUP),
            Some((false, false))
        );
        assert_eq!(startup_view(true, true, "os_minimize_to_tray"), None);
    }
}
