//! Open on startup (Windows). Follows the official client's `autoStart` module (desktop bootstrap, module 43019 and its
//! Windows implementation): one per-user value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, named after the
//! executable, holding the quoted path and the launch arguments. Installing first deletes the value Windows' own Startup
//! Apps page keeps under `...\Explorer\StartupApproved\Run`, so a switch turned on again works after the user turned the
//! entry off there. "Installed" is the Run value being present and not switched off in StartupApproved.
//!
//! The official value launches `Update.exe --processStart Discord.exe --process-start-args "--start-inactive
//! [--start-minimized]"` because Squirrel owns its install; gravecord has no launcher, so its value runs the exe with
//! the same two arguments directly. The value name is gravecord's own, never Discord's.
//!
//! A capture run never touches the registry (`allowed`), and the tests work on a scratch key and value name.

use std::path::Path;

/// The Run value's name: the exe's file name without `.exe`, as the official `appName` is.
pub const VALUE_NAME: &str = "gravecord";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Launch without taking the foreground (`--start-inactive`).
pub const ARG_INACTIVE: &str = "--start-inactive";
/// Launch to the tray with no window (`--start-minimized`).
pub const ARG_MINIMIZED: &str = "--start-minimized";

/// Where the value lives. `real` is the Run key; tests use a scratch key so nothing real is touched.
#[derive(Clone, Debug)]
pub struct Entry {
    run_key: String,
    approved_key: String,
    name: String,
}

impl Entry {
    pub fn real() -> Self {
        Self {
            run_key: RUN_KEY.to_owned(),
            approved_key: APPROVED_KEY.to_owned(),
            name: VALUE_NAME.to_owned(),
        }
    }

    /// A key tree of its own under `HKCU\Software`, for tests.
    #[cfg(test)]
    fn scratch(tag: &str) -> Self {
        let root = format!(r"Software\gravecord-test-{tag}-{}", std::process::id());
        Self {
            run_key: format!(r"{root}\Run"),
            approved_key: format!(r"{root}\Approved"),
            name: "gravecord-test-value".to_owned(),
        }
    }
}

/// Whether this process may write the real Run value: never from a capture run.
pub fn allowed(capture: bool) -> bool {
    !capture
}

/// The command the Run value holds: the quoted exe, then the official arguments.
pub fn command_line(exe: &Path, start_minimized: bool) -> String {
    let mut line = format!("\"{}\" {ARG_INACTIVE}", exe.display());
    if start_minimized {
        line.push(' ');
        line.push_str(ARG_MINIMIZED);
    }
    line
}

/// Whether the StartupApproved record marks the entry as enabled. Windows writes 02 or 06 for enabled and 03 for
/// disabled (the low bit of the first byte); no record at all means enabled. The official check reads the first byte
/// of the same record.
pub fn approved_enabled(record: Option<&[u8]>) -> bool {
    record
        .and_then(|bytes| bytes.first())
        .is_none_or(|first| first & 1 == 0)
}

#[cfg(windows)]
mod registry {
    use super::Entry;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows::core::PCWSTR;

    pub fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
        text.as_ref().encode_wide().chain([0]).collect()
    }

    pub struct Key(pub HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(path: &str, write: bool) -> Option<Key> {
        let mut key = HKEY::default();
        let access = if write { KEY_WRITE } else { KEY_READ };
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide(path).as_ptr()),
                None,
                access,
                &mut key,
            )
        }
        .is_ok()
        .then_some(Key(key))
    }

    pub fn create(path: &str) -> std::io::Result<Key> {
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide(path).as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .map_err(std::io::Error::other)?;
        Ok(Key(key))
    }

    /// The raw bytes of a value, or `None` when the key or the value is missing.
    fn read(path: &str, name: &str) -> Option<Vec<u8>> {
        let key = open(path, false)?;
        let name = wide(name);
        let mut length = 0u32;
        unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                None,
                Some(&mut length),
            )
        }
        .ok()
        .ok()?;
        let mut data = vec![0u8; length as usize];
        unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut length),
            )
        }
        .ok()
        .ok()?;
        data.truncate(length as usize);
        Some(data)
    }

    pub fn run_value(entry: &Entry) -> Option<String> {
        let bytes = read(&entry.run_key, &entry.name)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        Some(
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_owned(),
        )
    }

    pub fn approved_record(entry: &Entry) -> Option<Vec<u8>> {
        read(&entry.approved_key, &entry.name)
    }

    pub fn write_run(entry: &Entry, command: &str) -> std::io::Result<()> {
        // Windows' own "turned off" record would keep a fresh Run value from starting.
        delete_approved(entry);
        let key = create(&entry.run_key)?;
        let bytes: Vec<u8> = wide(command)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        unsafe {
            RegSetValueExW(
                key.0,
                PCWSTR(wide(&entry.name).as_ptr()),
                None,
                REG_SZ,
                Some(&bytes),
            )
        }
        .ok()
        .map_err(std::io::Error::other)
    }

    fn delete_approved(entry: &Entry) {
        if let Some(key) = open(&entry.approved_key, true) {
            unsafe {
                let _ = RegDeleteValueW(key.0, PCWSTR(wide(&entry.name).as_ptr()));
            }
        }
    }

    pub fn delete_run(entry: &Entry) {
        if let Some(key) = open(&entry.run_key, true) {
            unsafe {
                let _ = RegDeleteValueW(key.0, PCWSTR(wide(&entry.name).as_ptr()));
            }
        }
        delete_approved(entry);
    }

    /// Removes a whole scratch tree (tests only).
    #[cfg(test)]
    pub fn delete_tree(root: &str) {
        use windows::Win32::System::Registry::RegDeleteTreeW;
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide(root).as_ptr()));
        }
    }
}

#[cfg(not(windows))]
mod registry {
    use super::Entry;
    pub fn run_value(_: &Entry) -> Option<String> {
        None
    }
    pub fn approved_record(_: &Entry) -> Option<Vec<u8>> {
        None
    }
    pub fn write_run(_: &Entry, _: &str) -> std::io::Result<()> {
        Err(std::io::Error::other("open on startup is Windows only"))
    }
    pub fn delete_run(_: &Entry) {}
    #[cfg(test)]
    pub fn delete_tree(_: &str) {}
}

/// `autoStart.isInstalled`: the value is there and Windows has not switched it off.
pub fn is_installed(entry: &Entry) -> bool {
    registry::run_value(entry).is_some()
        && approved_enabled(registry::approved_record(entry).as_deref())
}

/// `autoStart.install`: writes the value for `exe`.
pub fn install(entry: &Entry, exe: &Path, start_minimized: bool) -> std::io::Result<()> {
    registry::write_run(entry, &command_line(exe, start_minimized))
}

/// `autoStart.uninstall`.
pub fn uninstall(entry: &Entry) {
    registry::delete_run(entry);
}

/// `autoStart.update`, run at launch: an entry that exists is written again for the exe that is running now, so a
/// moved or updated install keeps starting with Windows. Nothing is created when there is none.
pub fn refresh(entry: &Entry, exe: &Path, start_minimized: bool) {
    if !is_installed(entry) {
        return;
    }
    let wanted = command_line(exe, start_minimized);
    if registry::run_value(entry).as_deref() != Some(wanted.as_str()) {
        let _ = registry::write_run(entry, &wanted);
    }
}

// What the settings page and the launch use. Each does nothing in a capture run.

static CACHE: std::sync::Mutex<Option<(std::time::Instant, bool)>> = std::sync::Mutex::new(None);

/// The page's switch value: what the registry says now, read at most once a second. `None` in a capture run.
pub fn switch_value() -> Option<bool> {
    use std::time::{Duration, Instant};
    if !allowed(crate::storage::is_capture_run()) {
        return None;
    }
    let mut cache = CACHE.lock().ok()?;
    if let Some((at, value)) = *cache
        && at.elapsed() < Duration::from_secs(1)
    {
        return Some(value);
    }
    let value = is_installed(&Entry::real());
    *cache = Some((Instant::now(), value));
    Some(value)
}

/// The switch was changed: writes or removes the real value. Ignored in a capture run.
pub fn set(on: bool, start_minimized: bool) {
    if !allowed(crate::storage::is_capture_run()) {
        return;
    }
    let entry = Entry::real();
    if on {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(error) = install(&entry, &exe, start_minimized) {
                    tracing::warn!(%error, "could not register open on startup");
                }
            }
            Err(error) => tracing::warn!(%error, "no executable path for open on startup"),
        }
    } else {
        uninstall(&entry);
    }
    // Read back at once: the switch shows what was really written.
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

/// `TOGGLE_START_MINIMIZED`: the argument lives in the value, so an installed entry is written again.
pub fn start_minimized_changed(start_minimized: bool) {
    if !allowed(crate::storage::is_capture_run()) {
        return;
    }
    let entry = Entry::real();
    if is_installed(&entry)
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = install(&entry, &exe, start_minimized);
    }
}

/// The "Start Discord minimized" switch as the settings page saved it (`settings.prefs` in `storage.json`; the key is
/// `ui/settings/prefs.rs`'s).
pub fn start_minimized_setting() -> bool {
    crate::storage::get("settings.prefs")
        .and_then(|saved| saved.get("os_start_minimized")?.as_bool())
        .unwrap_or(false)
}

/// At launch (`autoStart.update`): repairs the value's path, never creates it.
pub fn refresh_at_launch() {
    if !allowed(crate::storage::is_capture_run()) {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        refresh(&Entry::real(), &exe, start_minimized_setting());
    }
}

/// A window opened hidden (`--start-minimized`) has no position yet: gpui applies its bounds when it first shows the
/// window itself, and the tray shows it with a plain `ShowWindow`. This puts the hidden window where it would have
/// opened, so the tray's first show lands there.
#[cfg(windows)]
pub fn place_hidden(window: &gpui::Window, bounds: gpui::Bounds<gpui::Pixels>) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOREDRAW, SWP_NOZORDER, SetWindowPos,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let scale = window.scale_factor();
    let device = |value: gpui::Pixels| (f32::from(value) * scale).round() as i32;
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            device(bounds.origin.x),
            device(bounds.origin.y),
            device(bounds.size.width),
            device(bounds.size.height),
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW,
        )
    };
}

#[cfg(not(windows))]
pub fn place_hidden(_: &gpui::Window, _: gpui::Bounds<gpui::Pixels>) {}

/// Keeps a window above the others, or lets it go back among them ("Stay On Top" of the popped-out call).
#[cfg(windows)]
pub fn set_topmost(window: &gpui::Window, on: bool) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
}

#[cfg(not(windows))]
pub fn set_topmost(_: &gpui::Window, _: bool) {}

/// The launch arguments: `--start-minimized` and `--start-inactive`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaunchFlags {
    pub minimized: bool,
    pub inactive: bool,
}

impl LaunchFlags {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut flags = Self::default();
        for arg in args {
            match arg.as_str() {
                ARG_MINIMIZED => flags.minimized = true,
                ARG_INACTIVE => flags.inactive = true,
                _ => {}
            }
        }
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Removes the scratch tree even when an assertion fails.
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            registry::delete_tree(&self.0);
        }
    }

    fn scratch(tag: &str) -> (Entry, Cleanup) {
        let entry = Entry::scratch(tag);
        let root = entry.run_key.rsplit_once('\\').unwrap().0.to_owned();
        (entry, Cleanup(root))
    }

    #[test]
    fn the_command_quotes_the_exe_and_carries_the_official_arguments() {
        let exe = Path::new(r"C:\Program Files\gravecord\gravecord.exe");
        assert_eq!(
            command_line(exe, false),
            r#""C:\Program Files\gravecord\gravecord.exe" --start-inactive"#
        );
        assert_eq!(
            command_line(exe, true),
            r#""C:\Program Files\gravecord\gravecord.exe" --start-inactive --start-minimized"#
        );
    }

    #[test]
    fn the_real_value_is_gravecords_own_and_never_discords() {
        assert_eq!(Entry::real().name, "gravecord");
        assert!(!Entry::real().name.to_lowercase().contains("discord"));
    }

    #[test]
    fn the_startup_approved_record_reads_like_windows_writes_it() {
        assert!(approved_enabled(None));
        assert!(approved_enabled(Some(&[0x02, 0, 0, 0])));
        assert!(approved_enabled(Some(&[0x06, 0, 0, 0])));
        assert!(!approved_enabled(Some(&[0x03, 0, 0, 0])));
        assert!(approved_enabled(Some(&[])));
    }

    #[test]
    fn a_capture_run_may_not_write_the_real_value() {
        assert!(!allowed(true));
        assert!(allowed(false));
    }

    #[test]
    fn launch_arguments_are_read_from_the_command_line() {
        let flags = |args: &[&str]| LaunchFlags::parse(args.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(flags(&[]), LaunchFlags::default());
        assert_eq!(
            flags(&["--start-inactive", "--start-minimized"]),
            LaunchFlags {
                minimized: true,
                inactive: true
            }
        );
        assert!(!flags(&["--start-inactive"]).minimized);
    }

    #[cfg(windows)]
    #[test]
    fn install_reads_back_and_uninstall_removes_on_a_scratch_key() {
        let (entry, _cleanup) = scratch("roundtrip");
        let exe = Path::new(r"C:\scratch\gravecord.exe");
        assert!(!is_installed(&entry));
        install(&entry, exe, false).unwrap();
        assert!(is_installed(&entry));
        assert_eq!(
            registry::run_value(&entry).as_deref(),
            Some(command_line(exe, false).as_str())
        );
        // Turning start minimized on rewrites the value with the argument.
        install(&entry, exe, true).unwrap();
        assert!(
            registry::run_value(&entry)
                .unwrap()
                .ends_with("--start-minimized")
        );
        uninstall(&entry);
        assert!(!is_installed(&entry));
    }

    #[cfg(windows)]
    #[test]
    fn a_value_removed_by_hand_reads_as_off_and_refresh_never_creates_one() {
        let (entry, _cleanup) = scratch("refresh");
        let exe = Path::new(r"C:\scratch\gravecord.exe");
        refresh(&entry, exe, false);
        assert!(!is_installed(&entry));
        install(&entry, exe, false).unwrap();
        let moved = Path::new(r"D:\moved\gravecord.exe");
        refresh(&entry, moved, true);
        assert_eq!(
            registry::run_value(&entry).as_deref(),
            Some(command_line(moved, true).as_str())
        );
        uninstall(&entry);
        refresh(&entry, moved, true);
        assert!(!is_installed(&entry));
    }

    #[cfg(windows)]
    #[test]
    fn an_entry_switched_off_in_startup_apps_reads_as_off_until_installed_again() {
        let (entry, _cleanup) = scratch("approved");
        let exe = Path::new(r"C:\scratch\gravecord.exe");
        install(&entry, exe, false).unwrap();
        // Windows' Startup Apps page writes 03 when the user turns an entry off.
        let key = registry::create(&entry.approved_key).unwrap();
        let record = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        unsafe {
            use windows::Win32::System::Registry::{REG_BINARY, RegSetValueExW};
            use windows::core::PCWSTR;
            RegSetValueExW(
                key.0,
                PCWSTR(registry::wide(&entry.name).as_ptr()),
                None,
                REG_BINARY,
                Some(&record),
            )
            .ok()
            .unwrap();
        }
        drop(key);
        assert!(!is_installed(&entry));
        install(&entry, exe, false).unwrap();
        assert!(is_installed(&entry));
    }
}
