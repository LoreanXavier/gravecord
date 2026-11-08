//! The client's local key-value storage: what the official client keeps in `localStorage` through its `Storage`
//! wrapper (`y.w` in module 889558, e.g. `MaskedLinkStore`). One JSON object at `%LOCALAPPDATA%\gravecord\storage.json`,
//! read once and written through on every change. Never holds tokens.
//!
//! Capture runs (`--capture`, or a build with the `capture` feature: the parity harness) keep theirs at
//! `%LOCALAPPDATA%\gravecord-capture\storage.json`, so tests never read or change the owner's settings.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The folder under `%LOCALAPPDATA%` that holds the storage file.
fn folder(capture: bool) -> &'static str {
    if capture {
        "gravecord-capture"
    } else {
        "gravecord"
    }
}

fn file_in(local_app_data: &Path, capture: bool) -> PathBuf {
    local_app_data.join(folder(capture)).join("storage.json")
}

/// This run is a harness run: the `capture` feature, or `--capture <png>` on the command line.
pub fn is_capture_run() -> bool {
    cfg!(feature = "capture") || std::env::args().any(|arg| arg == "--capture")
}

/// `%LOCALAPPDATA%\gravecord` (or `gravecord-capture`): where this run keeps its files.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(folder(is_capture_run())))
}

fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| file_in(Path::new(&dir), is_capture_run()))
}

fn cell() -> &'static Mutex<Map<String, Value>> {
    static CELL: OnceLock<Mutex<Map<String, Value>>> = OnceLock::new();
    CELL.get_or_init(|| {
        let loaded = path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        Mutex::new(loaded)
    })
}

pub fn get(key: &str) -> Option<Value> {
    cell().lock().ok()?.get(key).cloned()
}

pub fn set(key: &str, value: Value) {
    let Ok(mut map) = cell().lock() else {
        return;
    };
    map.insert(key.to_owned(), value);
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&*map) {
        // Write beside and rename, so a crash mid-write never leaves half a file.
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_runs_never_share_the_owners_file() {
        let local = Path::new("C:\\Users\\someone\\AppData\\Local");
        let owner = file_in(local, false);
        let harness = file_in(local, true);
        assert_eq!(owner, local.join("gravecord").join("storage.json"));
        assert_eq!(
            harness,
            local.join("gravecord-capture").join("storage.json")
        );
        assert_ne!(owner, harness);
        assert!(
            !harness.starts_with(owner.parent().unwrap()),
            "the capture folder is not inside the owner's"
        );
    }

    #[test]
    fn the_capture_feature_selects_the_capture_file() {
        // With the feature on, every process of that build is a harness run, whatever its arguments.
        if cfg!(feature = "capture") {
            assert!(is_capture_run());
            let path = path().expect("LOCALAPPDATA is set");
            assert!(path.to_string_lossy().contains("gravecord-capture"));
        } else {
            assert!(
                path().is_none_or(|path| !path.to_string_lossy().contains("gravecord-capture"))
                    || std::env::args().any(|arg| arg == "--capture")
            );
        }
    }
}
