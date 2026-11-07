//! The client's local key-value storage: what the official client keeps in `localStorage` through its `Storage`
//! wrapper (`y.w` in module 889558, e.g. `MaskedLinkStore`). One JSON object at `%LOCALAPPDATA%\gravecord\storage.json`,
//! read once and written through on every change. Never holds tokens.

use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("gravecord").join("storage.json"))
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
