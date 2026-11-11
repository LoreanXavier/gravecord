//! Streamer Mode (module 351906 `StreamerModeStore`): the stored settings of the signed-in user and what they hide.
//! `hidePersonalInformation` takes the connected accounts block off the Connections page (`TJ` in module 158935
//! renders nothing then), `hideInstantInvites` replaces the invite link of the invite dialog with "Hidden; Use copy
//! button" (module 753190), `enableContentProtection` calls `setContentProtection` (module 926935), and
//! `RUNNING_STREAMER_TOOLS_CHANGE` turns Streamer Mode on or off while `autoToggle` is set.

use super::Root;
use crate::notifications;

pub(in crate::ui) const HIDDEN_LINK: &str = "Hidden; Use copy button";

/// `RUNNING_STREAMER_TOOLS_CHANGE`: the new `enabled`, or None when the user turned the automatic rule off.
pub(in crate::ui) fn auto_toggle(auto: bool, tools_running: usize) -> Option<bool> {
    auto.then_some(tools_running > 0)
}

/// `get hidePersonalInformation` and friends: the option only counts while Streamer Mode is enabled.
pub(in crate::ui) fn applies(enabled: bool, option: bool) -> bool {
    enabled && option
}

impl Root {
    fn streamer_option(&self, key: &str) -> bool {
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        applies(
            notifications::switch_value("streaming_enable_streamer_mode", user).unwrap_or(false),
            notifications::switch_value(key, user).unwrap_or(false),
        )
    }

    pub(in crate::ui) fn streamer_hides_personal(&self) -> bool {
        self.streamer_option("streamer_mode_hide_personal_information")
    }

    pub(in crate::ui) fn streamer_hides_invites(&self) -> bool {
        self.streamer_option("streamer_mode_hide_invite_links")
    }

    /// One scan of the running processes: the count dispatches `RUNNING_STREAMER_TOOLS_CHANGE` when it changed.
    pub(in crate::ui) fn streamer_scanned(&mut self, count: usize) {
        if std::mem::replace(&mut self.settings_extra.streamer_tools, count) != count {
            self.streamer_tools_changed(count);
        }
    }

    /// The tools list changed (`RUNNING_STREAMER_TOOLS_CHANGE`): Streamer Mode follows while the rule is on.
    fn streamer_tools_changed(&mut self, count: usize) {
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let auto =
            notifications::switch_value("streaming_auto_streamer_mode", user).unwrap_or(true);
        if let Some(on) = auto_toggle(auto, count) {
            notifications::set_switch("streaming_enable_streamer_mode", on, user);
        }
    }

    /// `setContentProtection(enableContentProtection)`: the window is left out of screen captures (`WDA_EXCLUDEFROMCAPTURE`
    /// on Windows) while Streamer Mode and the option are on. A harness run never changes the real window.
    pub(in crate::ui) fn streamer_apply(&mut self, window: &gpui::Window) {
        let wanted = !crate::storage::is_capture_run()
            && self.streamer_option("streamer_mode_hide_discord_window_from_screen_capture");
        if wanted == self.settings_extra.content_protected {
            return;
        }
        self.settings_extra.content_protected = wanted;
        set_content_protection(window, wanted);
    }
}

#[cfg(windows)]
fn set_content_protection(window: &gpui::Window, on: bool) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    if let Err(error) = unsafe { SetWindowDisplayAffinity(hwnd, affinity) } {
        tracing::warn!("display affinity not set: {error}");
    }
}

#[cfg(not(windows))]
fn set_content_protection(_: &gpui::Window, _: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_count_only_while_streamer_mode_is_on() {
        assert!(applies(true, true));
        assert!(!applies(false, true));
        assert!(!applies(true, false));
    }

    #[test]
    fn the_automatic_rule_follows_the_running_tools() {
        assert_eq!(auto_toggle(true, 1), Some(true));
        assert_eq!(auto_toggle(true, 0), Some(false));
        assert_eq!(auto_toggle(false, 3), None);
    }
}
