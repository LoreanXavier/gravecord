//! The Shop's pictures kept on disk between runs. The official client draws them with `<img>` and CSS backgrounds,
//! so Chromium's HTTP cache serves a second visit from disk (`Cache-Control: public, max-age=86400` on
//! `cdn.discordapp.com/media/v1/collectibles-shop/...`, with an `ETag` it revalidates after the day). Without a cache
//! every start fetched every picture again: about fifty files, half a megabyte to 1.7 MB each, which on the owner's
//! line took up to twenty seconds, and the CDN has no smaller variants of them (`size`, `width` and `format` are
//! ignored on that path).
//!
//! What is kept is the picture as the Shop draws it (already cut down to its box), under a name made of the URL's path
//! and the box. A copy younger than the CDN's `max-age` is used as it is; an older one is drawn at once and fetched
//! again behind it (the official revalidates first; a stale copy shown for that moment is the same picture for every
//! content-addressed URL). Copies not used for `KEEP_DAYS` are removed.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// `max-age` of the CDN's answers.
pub(crate) const FRESH: Duration = Duration::from_secs(86_400);
/// Copies untouched for this long are deleted.
const KEEP_DAYS: u64 = 14;

/// What the disk holds for a picture.
pub(crate) struct Cached {
    pub bytes: Vec<u8>,
    /// Older than `FRESH`: draw it, and fetch it again.
    pub stale: bool,
}

fn dir() -> Option<PathBuf> {
    let dir = crate::storage::data_dir()?.join("shop-art");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The file name for `url` drawn in a `box_` (logical width, height and the scale, when the Shop gave one).
pub(crate) fn file_name(url: &str, box_: Option<(f32, f32, f32)>) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let no_fragment = rest.split('#').next().unwrap_or_default();
    let (path, query) = no_fragment.split_once('?').unwrap_or((no_fragment, ""));
    let sanitize = |part: &str| -> String {
        part.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let mut name = if query.is_empty() {
        sanitize(path)
    } else {
        format!("{}_{}", sanitize(path), sanitize(query))
    };
    // Keep the end of long paths: the hash or the sku (plus query) is there.
    if name.len() > 150 {
        name = name[name.len() - 150..].to_owned();
    }
    match box_ {
        Some((width, height, scale)) => format!(
            "{name}@{}x{}x{}.bin",
            width.round() as u32,
            height.round() as u32,
            (scale * 100.).round() as u32
        ),
        None => format!("{name}.bin"),
    }
}

/// Whether a copy written at `modified` is still fresh at `now`.
pub(crate) fn is_fresh(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified).map_or(true, |age| age < FRESH)
}

pub(crate) fn read(url: &str, box_: Option<(f32, f32, f32)>) -> Option<Cached> {
    let path = dir()?.join(file_name(url, box_));
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    let bytes = std::fs::read(&path)
        .ok()
        .filter(|bytes| !bytes.is_empty())?;
    Some(Cached {
        bytes,
        stale: !is_fresh(modified, SystemTime::now()),
    })
}

/// Writes the copy through a temporary file, so a failed write never leaves half a picture.
pub(crate) fn write(url: &str, box_: Option<(f32, f32, f32)>, bytes: &[u8]) {
    let Some(dir) = dir() else { return };
    let name = file_name(url, box_);
    let temporary = dir.join(format!("{name}.part"));
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, dir.join(name));
    }
    prune_once(&dir);
}

/// Once a run: removes copies not written for `KEEP_DAYS`.
fn prune_once(dir: &std::path::Path) {
    static PRUNED: std::sync::Once = std::sync::Once::new();
    PRUNED.call_once(|| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let now = SystemTime::now();
        let keep = Duration::from_secs(KEEP_DAYS * 86_400);
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > keep);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_path_and_the_box() {
        let url = "https://cdn.discordapp.com/media/v1/collectibles-shop/bundle-fg-static/1476293164347424808";
        assert_eq!(
            file_name(url, Some((242., 242., 1.5))),
            "cdn_discordapp_com_media_v1_collectibles-shop_bundle-fg-static_1476293164347424808@242x242x150.bin"
        );
        assert_eq!(
            file_name("https://a.b/c.png?size=4", None),
            "a_b_c_png_size_4.bin"
        );
        assert_ne!(
            file_name("https://a.b/c.png?size=4", None),
            file_name("https://a.b/c.png?size=512", None)
        );
        let long = format!("https://x/{}", "a".repeat(400));
        assert!(file_name(&long, None).len() <= 155);
    }

    #[test]
    fn copies_are_fresh_for_the_cdn_max_age() {
        let now = SystemTime::now();
        assert!(is_fresh(now - Duration::from_secs(3_600), now));
        assert!(!is_fresh(now - Duration::from_secs(90_000), now));
    }
}
