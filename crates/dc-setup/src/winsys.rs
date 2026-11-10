//! The Windows calls: registry, shortcuts, processes, the folder picker and message boxes. The only `unsafe` in the crate.
#![allow(unsafe_code)]

use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, TRUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FileOpenDialog, IFileOpenDialog, IShellItem, IShellLinkW,
    KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderPath, SIGDN_FILESYSPATH,
    ShellExecuteW, ShellLink,
};
use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IDYES, MB_ICONERROR, MB_ICONINFORMATION,
    MB_ICONQUESTION, MB_OK, MB_RETRYCANCEL, MB_YESNO, MESSAGEBOX_STYLE, MessageBoxW, PostMessageW,
    RegisterWindowMessageW, SW_SHOWNORMAL, WM_CLOSE,
};
use windows::core::{PCWSTR, PWSTR, w};

pub const WEBVIEW2_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
const PUBLISHER: &str = "LoreanXavier";
const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

pub fn local_app_data() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .context("LOCALAPPDATA is not set")
}

fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let text = OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0 as *const c_void));
        Ok(text.into())
    }
}

pub fn start_menu_dir() -> Result<PathBuf> {
    known_folder(&FOLDERID_Programs)
}

pub fn desktop_dir() -> Result<PathBuf> {
    known_folder(&FOLDERID_Desktop)
}

pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

pub fn create_shortcut(link: &Path, target: &Path, work_dir: &Path) -> Result<()> {
    com_init();
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell.SetPath(PCWSTR(wide(target).as_ptr()))?;
        shell.SetWorkingDirectory(PCWSTR(wide(work_dir).as_ptr()))?;
        shell.SetDescription(PCWSTR(wide("gravecord").as_ptr()))?;
        shell.SetIconLocation(PCWSTR(wide(target).as_ptr()), 0)?;
        // The toast notifier and the installed shortcut share this application identity.
        let properties: windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore =
            windows::core::Interface::cast(&shell)?;
        let mut app_id = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();
        windows::Win32::System::Com::StructuredStorage::PropVariantChangeType(
            &mut app_id,
            &windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from(
                crate::app_id::APP_ID,
            ),
            windows::Win32::System::Com::StructuredStorage::PVCHF_DEFAULT,
            windows::Win32::System::Variant::VT_LPWSTR,
        )?;
        properties.SetValue(
            &windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID,
            &app_id,
        )?;
        properties.Commit()?;
        let file: IPersistFile = windows::core::Interface::cast(&shell)?;
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)?;
        }
        file.Save(PCWSTR(wide(link).as_ptr()), true)
            .with_context(|| format!("Could not create the shortcut {}", link.display()))?;
    }
    Ok(())
}

pub struct UninstallEntry<'a> {
    pub version: &'a str,
    pub dir: &'a Path,
    pub size_kib: u32,
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn set_string(key: &Key, name: &str, value: &str) -> Result<()> {
    let data = wide(value);
    let bytes: Vec<u8> = data.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(wide(name).as_ptr()),
            None,
            REG_SZ,
            Some(&bytes),
        )
        .ok()?
    };
    Ok(())
}

fn set_dword(key: &Key, name: &str, value: u32) -> Result<()> {
    unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(wide(name).as_ptr()),
            None,
            REG_DWORD,
            Some(&value.to_le_bytes()),
        )
        .ok()?
    };
    Ok(())
}

/// The Apps & features entry (per user, no administrator rights).
pub fn register_uninstall(key_name: &str, entry: &UninstallEntry) -> Result<()> {
    let exe = entry.dir.join("gravecord.exe");
    let uninstaller = entry.dir.join(crate::paths::UNINSTALLER);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(format!(r"{UNINSTALL_ROOT}\{key_name}")).as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()
        .context("Could not write the Apps & features entry")?;
    }
    let key = Key(key);
    set_string(&key, "DisplayName", "gravecord")?;
    set_string(&key, "DisplayVersion", entry.version)?;
    set_string(&key, "Publisher", PUBLISHER)?;
    set_string(&key, "InstallLocation", &entry.dir.display().to_string())?;
    set_string(&key, "DisplayIcon", &format!("{},0", exe.display()))?;
    set_string(
        &key,
        "UninstallString",
        &format!("\"{}\"", uninstaller.display()),
    )?;
    set_string(
        &key,
        "QuietUninstallString",
        &format!("\"{}\" --silent", uninstaller.display()),
    )?;
    set_dword(&key, "EstimatedSize", entry.size_kib)?;
    set_dword(&key, "NoModify", 1)?;
    set_dword(&key, "NoRepair", 1)?;
    Ok(())
}

pub fn unregister_uninstall(key_name: &str) -> Result<()> {
    if !crate::paths::plain_name(key_name) {
        bail!("bad registry key name");
    }
    let mut root = HKEY::default();
    unsafe {
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(UNINSTALL_ROOT).as_ptr()),
            None,
            KEY_WRITE,
            &mut root,
        );
        if opened.is_err() {
            return Ok(());
        }
        let root = Key(root);
        // Missing is fine: the entry may have been removed by hand.
        let _ = RegDeleteTreeW(root.0, PCWSTR(wide(key_name).as_ptr()));
    }
    Ok(())
}

/// Whether the Microsoft Edge WebView2 Runtime is installed (machine-wide or per user).
pub fn webview2_present() -> bool {
    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let places = [
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_CURRENT_USER,
            format!(r"Software\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
    ];
    places.iter().any(|(root, path)| {
        let mut key = HKEY::default();
        unsafe {
            if RegOpenKeyExW(*root, PCWSTR(wide(path).as_ptr()), None, KEY_READ, &mut key).is_err()
            {
                return false;
            }
            let key = Key(key);
            let mut buffer = [0u16; 64];
            let mut length = std::mem::size_of_val(&buffer) as u32;
            let read = RegQueryValueExW(
                key.0,
                PCWSTR(wide("pv").as_ptr()),
                None,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut length),
            );
            let units = (length as usize / 2).min(buffer.len());
            let version = String::from_utf16_lossy(&buffer[..units]);
            let version = version.trim_end_matches('\0');
            read.is_ok() && !version.is_empty() && version != "0.0.0.0"
        }
    })
}

/// A running process whose image name matches the one being looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    /// False when the image path could not be read, so it is not known to be the wanted exe.
    pub known: bool,
}

/// How one process relates to the exe being looked for.
#[derive(Debug, PartialEq, Eq)]
enum Match {
    Yes,
    No,
    /// The image path could not be read. Only a process named `gravecord.exe` counts as possibly ours.
    Unknown,
}

/// `image` is the process's full image path when it could be read; `name` its file name from the process list.
fn classify(image: Option<&Path>, name: &str, wanted: &Path) -> Match {
    match image {
        Some(image) if crate::paths::same_path(image, wanted) => Match::Yes,
        Some(_) => Match::No,
        None if name.eq_ignore_ascii_case("gravecord.exe") => Match::Unknown,
        None => Match::No,
    }
}

/// Processes running `exe`, by image path (case, slashes, `\\?\`, junctions and 8.3 names do not matter).
pub fn running(exe: &Path) -> Vec<Running> {
    let wanted_name = exe.file_name().unwrap_or_default().to_string_lossy();
    let mut found = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return found;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)],
            );
            if name.eq_ignore_ascii_case(&wanted_name) || name.eq_ignore_ascii_case("gravecord.exe")
            {
                let image = image_path(entry.th32ProcessID);
                match classify(image.as_deref(), &name, exe) {
                    Match::Yes => found.push(Running {
                        pid: entry.th32ProcessID,
                        known: true,
                    }),
                    Match::Unknown => found.push(Running {
                        pid: entry.th32ProcessID,
                        known: false,
                    }),
                    Match::No => {}
                }
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found
}

fn image_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let read = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        read.ok()?;
        Some(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    }
}

/// Process ids that are running `exe`, or might be (see `Match::Unknown`).
pub fn running_pids(exe: &Path) -> Vec<u32> {
    running(exe)
        .into_iter()
        .map(|process| process.pid)
        .collect()
}

unsafe extern "system" fn close_window(window: HWND, param: LPARAM) -> windows::core::BOOL {
    let pids = unsafe { &*(param.0 as *const Vec<u32>) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pids.contains(&pid) {
        unsafe {
            // Newer clients distinguish setup shutdown from the caption's close-to-tray action.
            // WM_CLOSE remains for older clients and for windows without the tray hook.
            let quit = RegisterWindowMessageW(w!("gravecord.QuitForSetup"));
            if quit != 0 {
                let _ = PostMessageW(Some(window), quit, Default::default(), Default::default());
            }
            let _ = PostMessageW(
                Some(window),
                WM_CLOSE,
                Default::default(),
                Default::default(),
            );
        }
    }
    TRUE
}

/// Asks the running copies of `exe` to close and waits up to `wait` for them to be gone. True when none is left.
pub fn ask_to_close(exe: &Path, wait: Duration) -> bool {
    let processes = running(exe);
    if processes.is_empty() {
        return true;
    }
    // Windows are only closed for processes known to run this exe; an unreadable one is waited for, never poked.
    let pids: Vec<u32> = processes
        .iter()
        .filter(|process| process.known)
        .map(|process| process.pid)
        .collect();
    unsafe {
        let _ = EnumWindows(
            Some(close_window),
            LPARAM(&pids as *const Vec<u32> as isize),
        );
    }
    let start = Instant::now();
    while start.elapsed() < wait {
        if running_pids(exe).is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    running_pids(exe).is_empty()
}

/// The helper that deletes the uninstaller after the process that started it has exited. Paths and the pid come
/// through environment variables, never through the script text, so no character of a folder name is interpreted.
const SELF_DELETE_SCRIPT: &str = "$ErrorActionPreference = 'SilentlyContinue'; \
    try { Wait-Process -Id ([int]$env:GRAVECORD_WAIT_PID) -Timeout 300 } catch {}; \
    for ($i = 0; $i -lt 40; $i++) { \
        if (-not (Test-Path -LiteralPath $env:GRAVECORD_REMOVE_EXE)) { break }; \
        Remove-Item -LiteralPath $env:GRAVECORD_REMOVE_EXE -Force -ErrorAction SilentlyContinue; \
        if (Test-Path -LiteralPath $env:GRAVECORD_REMOVE_EXE) { Start-Sleep -Milliseconds 500 } }; \
    for ($i = 0; $i -lt 10; $i++) { \
        if (-not (Test-Path -LiteralPath $env:GRAVECORD_REMOVE_DIR)) { break }; \
        if ([System.IO.Directory]::GetFileSystemEntries($env:GRAVECORD_REMOVE_DIR).Length -gt 0) { break }; \
        try { [System.IO.Directory]::Delete($env:GRAVECORD_REMOVE_DIR, $false); break } catch { Start-Sleep -Milliseconds 500 } }";

/// Starts the helper that deletes the running uninstaller and then its folder (when empty). Call it as the last act,
/// after any dialog has been answered: the helper waits for this process to exit, so the order no longer races a
/// slow answer. Retries cover a scanner that still holds the file.
pub fn delete_self_later(exe: &Path, dir: &Path) {
    if !exe.is_absolute()
        || !crate::paths::same_path(exe.parent().unwrap_or(exe), dir)
        || !exe
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(crate::paths::UNINSTALLER))
    {
        return;
    }
    let _ = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            SELF_DELETE_SCRIPT,
        ])
        .env("GRAVECORD_WAIT_PID", std::process::id().to_string())
        .env("GRAVECORD_REMOVE_EXE", exe)
        .env("GRAVECORD_REMOVE_DIR", dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub fn launch(exe: &Path, dir: &Path) {
    let _ = std::process::Command::new(exe).current_dir(dir).spawn();
}

pub fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(wide("open").as_ptr()),
            PCWSTR(wide(url).as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub enum Kind {
    Info,
    Error,
    /// Yes/No, yes is the default. Returns true for yes.
    AskYes,
    /// Retry/Cancel. Returns true for retry.
    AskRetry,
}

pub fn message(owner: Option<HWND>, title: &str, text: &str, kind: Kind) -> bool {
    let style: MESSAGEBOX_STYLE = match kind {
        Kind::Info => MB_OK | MB_ICONINFORMATION,
        Kind::Error => MB_OK | MB_ICONERROR,
        Kind::AskYes => MB_YESNO | MB_ICONQUESTION,
        Kind::AskRetry => MB_RETRYCANCEL | MB_ICONINFORMATION,
    };
    let answer = unsafe {
        MessageBoxW(
            owner,
            PCWSTR(wide(text).as_ptr()),
            PCWSTR(wide(title).as_ptr()),
            style,
        )
    };
    match kind {
        Kind::AskYes => answer == IDYES,
        Kind::AskRetry => answer.0 == 4,
        _ => true,
    }
}

pub fn pick_folder(owner: HWND, start: &Path) -> Option<PathBuf> {
    com_init();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        dialog
            .SetTitle(PCWSTR(wide("Choose where to install gravecord").as_ptr()))
            .ok()?;
        let existing = start.ancestors().find(|dir| dir.is_dir());
        if let Some(existing) = existing
            && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                PCWSTR(wide(existing).as_ptr()),
                None,
            )
        {
            let _ = dialog.SetFolder(&item);
        }
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.0 as *const c_void));
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_carries_the_toast_application_identity() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "gravecord-shortcut-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&scratch).unwrap();
        let link = scratch.join("test.lnk");
        create_shortcut(&link, &std::env::current_exe().unwrap(), &scratch).unwrap();
        unsafe {
            let shell: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let file: IPersistFile = windows::core::Interface::cast(&shell).unwrap();
            file.Load(
                PCWSTR(wide(&link).as_ptr()),
                windows::Win32::System::Com::STGM_READ,
            )
            .unwrap();
            let properties: windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore =
                windows::core::Interface::cast(&shell).unwrap();
            let value = properties
                .GetValue(&windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID)
                .unwrap();
            assert_eq!(value.vt(), windows::Win32::System::Variant::VT_LPWSTR);
            assert_eq!(
                windows::core::BSTR::try_from(&value).unwrap().to_string(),
                crate::app_id::APP_ID
            );
        }
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir(scratch).unwrap();
    }

    #[test]
    fn the_shortcut_and_the_app_share_one_identity_constant() {
        assert_eq!(crate::app_id::APP_ID, "gravecord.desktop");
    }

    #[test]
    fn the_self_delete_helper_never_interpolates_paths() {
        for name in [
            "GRAVECORD_WAIT_PID",
            "GRAVECORD_REMOVE_EXE",
            "GRAVECORD_REMOVE_DIR",
        ] {
            assert!(SELF_DELETE_SCRIPT.contains(&format!("$env:{name}")));
        }
        assert!(SELF_DELETE_SCRIPT.contains("Wait-Process -Id"));
        assert!(!SELF_DELETE_SCRIPT.contains("C:\\"));
    }

    #[test]
    fn image_paths_are_matched_by_place_and_unreadable_ones_only_by_name() {
        let me = std::env::current_exe().unwrap();
        let upper = PathBuf::from(me.to_string_lossy().to_uppercase());
        let slashes = PathBuf::from(me.to_string_lossy().replace('\\', "/"));
        let verbatim = PathBuf::from(format!(r"\\?\{}", me.display()));
        for image in [&upper, &slashes, &verbatim] {
            assert_eq!(classify(Some(image), "x.exe", &me), Match::Yes);
        }
        assert_eq!(
            classify(Some(Path::new(r"C:\elsewhere\x.exe")), "x.exe", &me),
            Match::No
        );
        assert_eq!(classify(None, "GraveCord.EXE", &me), Match::Unknown);
        assert_eq!(classify(None, "other.exe", &me), Match::No);
        // This very process is found through the real process list, whatever the spelling asked for.
        for spelling in [&me, &upper, &slashes] {
            assert!(
                running(spelling)
                    .iter()
                    .any(|process| process.pid == std::process::id() && process.known),
                "{}",
                spelling.display()
            );
        }
    }
}
