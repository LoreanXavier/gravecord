//! The Windows application identity (AppUserModelID) shared by the installed shortcut and the toast notifier.
//!
//! This one file is the single source: `dc-setup` uses it as `crate::app_id`, and the app includes the same file with
//! `#[path]` (the app cannot depend on the installer, and neither crate may gain a dependency on the other). The
//! test below therefore runs in both crates.

/// Set on the process (`SetCurrentProcessExplicitAppUserModelID`), on the notifier
/// (`CreateToastNotifierWithId`) and on the Start Menu shortcut (`PKEY_AppUserModel_ID`). Changing it detaches
/// existing shortcuts from the notification settings Windows keeps per identity.
pub const APP_ID: &str = "gravecord.desktop";

#[cfg(test)]
mod tests {
    #[test]
    fn the_application_identity_is_pinned() {
        assert_eq!(super::APP_ID, "gravecord.desktop");
    }
}
