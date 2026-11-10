//! The clips directory: every clip file in the storage folder with its embedded metadata (`loadClipsDirectory` in the
//! native layer, then `V` per clip in module 489496 `eh`). A file without a readable, valid metadata document is left
//! out silently, as the official loader does.
use crate::metadata::ClipMetadata;
use crate::mp4;
use std::fs;
use std::path::{Path, PathBuf};

/// A clip on disk: `filepath` and `sizeBytes` are added here, they are not in the file's document.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub meta: ClipMetadata,
    pub filepath: PathBuf,
    pub size_bytes: u64,
}

/// Reads one clip file's metadata.
pub fn load_file(path: &Path) -> Option<Clip> {
    let mut file = fs::File::open(path).ok()?;
    let info = mp4::read_info(&mut file).ok()?;
    let meta = ClipMetadata::parse(info.metadata.as_deref()?)?;
    Some(Clip {
        meta,
        filepath: path.to_path_buf(),
        size_bytes: info.size,
    })
}

/// Every clip in `dir` (the folder itself, not its subfolders: the auto-clip cache is one), oldest first. A missing
/// folder is an empty library.
pub fn load_directory(dir: &Path) -> Vec<Clip> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut clips: Vec<Clip> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
        })
        .filter_map(|path| load_file(&path))
        .collect();
    clips.sort_by(|a, b| (a.meta.created_at, &a.meta.id).cmp(&(b.meta.created_at, &b.meta.id)));
    clips
}
