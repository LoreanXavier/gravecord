//! Non-blocking Windows file open/save dialogs that never cross-thread disable the main window.
//!
//! `gpui_windows`'s built-in `prompt_for_paths` / `prompt_for_new_path` passes the main thread's `HWND` to
//! `IFileOpenDialog::Show` / `IFileSaveDialog::Show` from a worker thread. On borderless DirectComposition windows,
//! cross-thread modal ownership calls `EnableWindow(main_hwnd, FALSE)` while the COM dialog can appear behind or
//! hidden from the disabled main window, locking the entire app until the hidden dialog is closed from Alt-Tab.
//! Showing the dialog ownerless (`Show(None)`) after `AllowSetForegroundWindow(ASFW_ANY)` on a dedicated STA thread
//! keeps the dialog visible in the foreground and taskbar without ever locking the main window.

use gpui::PathPromptOptions;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

/// One entry in the native file dialog's file-type dropdown (e.g. `"JSON files"`, `"*.json"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileFilter {
    pub name: &'static str,
    pub spec: &'static str,
}

pub(super) const JSON_FILTERS: &[FileFilter] = &[
    FileFilter {
        name: "JSON files",
        spec: "*.json",
    },
    FileFilter {
        name: "All files",
        spec: "*.*",
    },
];

pub(super) const ALL_FILTERS: &[FileFilter] = &[FileFilter {
    name: "All files",
    spec: "*.*",
}];

/// Strips the `\\?\` extended-length prefix Windows `canonicalize()` produces so Shell APIs accept the path.
fn strip_unc_prefix(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        raw.into_owned()
    }
}

/// Extracts a clean default extension without leading dot from a suggested filename or first filter spec.
fn default_extension(suggested_name: Option<&str>, filters: &[FileFilter]) -> Option<String> {
    if let Some(name) = suggested_name
        && let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str())
        && !ext.is_empty()
        && ext != "*"
    {
        return Some(ext.to_owned());
    }
    filters.first().and_then(|filter| {
        let first = filter.spec.split(';').next()?.trim();
        let ext = first.strip_prefix("*.")?;
        (!ext.is_empty() && ext != "*").then(|| ext.to_owned())
    })
}

/// Opens a native file/folder picker without blocking the UI thread or locking the main window.
pub(super) fn open_paths(
    options: PathPromptOptions,
    filters: &'static [FileFilter],
    cx: &gpui::App,
) -> oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>> {
    #[cfg(windows)]
    {
        let _ = cx;
        let (tx, rx) = oneshot::channel();
        let spawn = std::thread::Builder::new()
            .name("gravecord-open-dialog".into())
            .spawn(move || {
                let result = win::open_paths(options, filters);
                let _ = tx.send(result);
            });
        if let Err(error) = spawn {
            let (err_tx, err_rx) = oneshot::channel();
            let _ = err_tx.send(Err(error.into()));
            return err_rx;
        }
        rx
    }
    #[cfg(not(windows))]
    {
        let _ = filters;
        let inner = cx.prompt_for_paths(options);
        let (tx, rx) = oneshot::channel();
        cx.background_executor()
            .spawn(async move {
                let result = match inner.await {
                    Ok(res) => res,
                    Err(_) => Ok(None),
                };
                let _ = tx.send(result);
            })
            .detach();
        rx
    }
}

/// Opens a native Save As dialog without blocking the UI thread or locking the main window.
pub(super) fn save_path(
    directory: &Path,
    suggested_name: Option<&str>,
    filters: &'static [FileFilter],
    cx: &gpui::App,
) -> oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
    #[cfg(windows)]
    {
        let _ = cx;
        let directory = directory.to_path_buf();
        let suggested_name = suggested_name.map(str::to_owned);
        let (tx, rx) = oneshot::channel();
        let spawn = std::thread::Builder::new()
            .name("gravecord-save-dialog".into())
            .spawn(move || {
                let result = win::save_path(&directory, suggested_name.as_deref(), filters);
                let _ = tx.send(result);
            });
        if let Err(error) = spawn {
            let (err_tx, err_rx) = oneshot::channel();
            let _ = err_tx.send(Err(error.into()));
            return err_rx;
        }
        rx
    }
    #[cfg(not(windows))]
    {
        let _ = filters;
        let inner = cx.prompt_for_new_path(directory, suggested_name);
        let (tx, rx) = oneshot::channel();
        cx.background_executor()
            .spawn(async move {
                let result = match inner.await {
                    Ok(res) => res,
                    Err(_) => Ok(None),
                };
                let _ = tx.send(result);
            })
            .detach();
        rx
    }
}

#[cfg(windows)]
mod win {
    use super::{FileFilter, default_extension, strip_unc_prefix};
    use anyhow::Result;
    use gpui::PathPromptOptions;
    use std::path::{Path, PathBuf};
    use windows::{
        Win32::{
            Foundation::ERROR_CANCELLED,
            System::Com::{
                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            UI::{
                Shell::{
                    Common::COMDLG_FILTERSPEC, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST,
                    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PICKFOLDERS, FileOpenDialog,
                    FileSaveDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
                    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
                },
                WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
            },
        },
        core::{HRESULT, HSTRING, PCWSTR},
    };

    struct StaGuard;

    impl StaGuard {
        fn enter() -> Result<Self> {
            // SAFETY: Initializes COM in single-threaded apartment mode on this dedicated dialog thread.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
            Ok(Self)
        }
    }

    impl Drop for StaGuard {
        fn drop(&mut self) {
            // SAFETY: Balances the matching `CoInitializeEx` call on the same thread.
            unsafe { CoUninitialize() };
        }
    }

    fn build_filters(filters: &[FileFilter]) -> (Vec<(HSTRING, HSTRING)>, Vec<COMDLG_FILTERSPEC>) {
        let storage: Vec<(HSTRING, HSTRING)> = filters
            .iter()
            .map(|f| (HSTRING::from(f.name), HSTRING::from(f.spec)))
            .collect();
        let specs: Vec<COMDLG_FILTERSPEC> = storage
            .iter()
            .map(|(name, spec)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(spec.as_ptr()),
            })
            .collect();
        (storage, specs)
    }

    fn item_path(item: &IShellItem) -> Result<PathBuf> {
        // SAFETY: `GetDisplayName` allocates a CoTaskMem string freed right after copying.
        unsafe {
            let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let text = pwstr.to_string();
            CoTaskMemFree(Some(pwstr.0 as _));
            Ok(PathBuf::from(text?))
        }
    }

    pub(super) fn open_paths(
        options: PathPromptOptions,
        filters: &[FileFilter],
    ) -> Result<Option<Vec<PathBuf>>> {
        let _sta = StaGuard::enter()?;
        // SAFETY: Allow the ownerless shell dialog to take foreground focus cleanly.
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        // SAFETY: Valid COM creation on initialized STA thread.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)? };
        let mut flags = FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST;
        if options.multiple {
            flags |= FOS_ALLOWMULTISELECT;
        }
        if options.directories {
            flags |= FOS_PICKFOLDERS;
        }
        // SAFETY: All COM pointers and UTF-16 buffers stay alive through the synchronous `Show(None)` call.
        unsafe {
            dialog.SetOptions(flags)?;
            if !options.directories && !filters.is_empty() {
                let (_storage, specs) = build_filters(filters);
                let _ = dialog.SetFileTypes(&specs);
            }
            if let Some(prompt) = options.prompt {
                let _ = dialog.SetOkButtonLabel(&HSTRING::from(prompt.as_ref()));
            }
            if let Err(error) = dialog.Show(None) {
                if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                    return Ok(None);
                }
                return Err(error.into());
            }
            let results = dialog.GetResults()?;
            let count = results.GetCount()?;
            if count == 0 {
                return Ok(None);
            }
            let mut paths = Vec::with_capacity(count as usize);
            for index in 0..count {
                let item = results.GetItemAt(index)?;
                paths.push(item_path(&item)?);
            }
            Ok(Some(paths))
        }
    }

    pub(super) fn save_path(
        directory: &Path,
        suggested_name: Option<&str>,
        filters: &[FileFilter],
    ) -> Result<Option<PathBuf>> {
        let _sta = StaGuard::enter()?;
        // SAFETY: Allow the ownerless shell dialog to take foreground focus cleanly.
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        // SAFETY: Valid COM creation on initialized STA thread.
        let dialog: IFileSaveDialog =
            unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)? };
        // SAFETY: All COM pointers and UTF-16 buffers stay alive through the synchronous `Show(None)` call.
        unsafe {
            let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT);
            if !directory.as_os_str().is_empty()
                && let Ok(canon) = directory.canonicalize()
            {
                let folder = strip_unc_prefix(&canon);
                if let Ok(item) =
                    SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder), None)
                {
                    let _ = dialog.SetFolder(&item);
                }
            }
            if let Some(name) = suggested_name {
                let _ = dialog.SetFileName(&HSTRING::from(name));
            }
            let active_filters = if filters.is_empty() {
                super::ALL_FILTERS
            } else {
                filters
            };
            let (_storage, specs) = build_filters(active_filters);
            let _ = dialog.SetFileTypes(&specs);
            if let Some(ext) = default_extension(suggested_name, active_filters) {
                let _ = dialog.SetDefaultExtension(&HSTRING::from(ext));
            }
            if let Err(error) = dialog.Show(None) {
                if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                    return Ok(None);
                }
                return Err(error.into());
            }
            let item = dialog.GetResult()?;
            Ok(Some(item_path(&item)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_unc_prefix_handles_drive_and_network_paths() {
        assert_eq!(
            strip_unc_prefix(Path::new(r"\\?\C:\Users\demo\backup.json")),
            r"C:\Users\demo\backup.json"
        );
        assert_eq!(
            strip_unc_prefix(Path::new(r"\\?\UNC\server\share\file.json")),
            r"\\server\share\file.json"
        );
        assert_eq!(
            strip_unc_prefix(Path::new(r"C:\Projects\gravecord")),
            r"C:\Projects\gravecord"
        );
    }

    #[test]
    fn default_extension_prefers_suggested_name_then_first_filter() {
        assert_eq!(
            default_extension(Some("gravecord-plugins-2026-11-20.json"), JSON_FILTERS).as_deref(),
            Some("json")
        );
        assert_eq!(
            default_extension(Some("backup"), JSON_FILTERS).as_deref(),
            Some("json")
        );
        assert_eq!(default_extension(None, ALL_FILTERS), None);
    }
}
