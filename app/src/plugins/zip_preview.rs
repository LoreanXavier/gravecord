//! ZipPreview's ZIP side (Equicord `src/equicordplugins/zipPreview/utils.ts` and `native.ts`, GPL-3.0-or-later): the
//! archive is read as `fflate`'s `unzipSync` reads it (stored and deflated entries; anything else is an error), the
//! folders' own entries dropped, sorted by path, cut at 1000 entries, each with its preview kind. The parsed archives
//! wait here by URL (`getCachedZip`), filled off the UI thread by `net.rs` (its `FetchZip` request).

use std::collections::HashMap;
use std::io::Read as _;
use std::sync::{Arc, Mutex, OnceLock};

/// `MAX_ZIP_BYTES`.
pub(crate) const MAX_ZIP_BYTES: usize = 50 * 1024 * 1024;
/// `MAX_ENTRIES`.
pub(crate) const MAX_ENTRIES: usize = 1000;
/// `MAX_PREVIEW_BYTES`.
const MAX_PREVIEW_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const CANCELLED: &str = "ZIP preview was cancelled.";
pub(crate) const TOO_LARGE: &str = "ZIP is too large to preview.";
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp"];
const TEXT_EXTENSIONS: &[&str] = &[
    "c", "cpp", "cs", "css", "csv", "go", "h", "html", "java", "js", "json", "jsx", "log", "lua",
    "md", "php", "py", "rs", "scss", "sh", "svg", "toml", "ts", "tsx", "txt", "xml", "yaml", "yml",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Image,
    Text,
    Unsupported,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub path: String,
    pub name: String,
    pub size: usize,
    pub data: Arc<[u8]>,
    pub kind: Kind,
    pub extension: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Archive {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum State {
    Pending,
    Resolved(Arc<Archive>),
    Rejected(String),
}

fn store() -> &'static Mutex<HashMap<String, State>> {
    static STORE: OnceLock<Mutex<HashMap<String, State>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn state(url: &str) -> Option<State> {
    store().lock().ok()?.get(url).cloned()
}

pub(crate) fn set_state(url: &str, state: State) {
    if let Ok(mut store) = store().lock() {
        store.insert(url.to_owned(), state);
    }
}

/// A failed load: kept as the answer, except a cancelled one, which is forgotten so the next open asks again.
pub(crate) fn reject(url: &str, message: String) {
    if let Ok(mut store) = store().lock() {
        if message == CANCELLED {
            store.remove(url);
        } else {
            store.insert(url.to_owned(), State::Rejected(message));
        }
    }
}

/// `isZipFile`.
pub(crate) fn is_zip(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".zip")
}

/// `getDiscordAttachmentPath`: the `<channel>/<attachment>/<name>?<query>` of an https Discord attachment URL, which
/// the plugin fetches through its native helper (after asking).
pub(crate) fn attachment_path(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    if !matches!(
        parsed.host_str(),
        Some("cdn.discordapp.com" | "media.discordapp.net")
    ) {
        return None;
    }
    let path = parsed.path().strip_prefix("/attachments/")?;
    if path.contains('\\') || path.contains("..") || path.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if parts.len() < 3
        || !digits(parts[0])
        || !digits(parts[1])
        || parts[2..].iter().any(|part| part.is_empty())
    {
        return None;
    }
    Some(match parsed.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    })
}

/// The attachment's name as the native dialog shows it (the last path part, URL-decoded).
pub(crate) fn dialog_name(path_and_query: &str) -> String {
    let path = path_and_query.split('?').next().unwrap_or_default();
    match path.rsplit('/').next().filter(|name| !name.is_empty()) {
        Some(name) => percent_decode(name),
        None => "Unknown attachment".to_owned(),
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn extension(path: &str) -> String {
    let name = file_name(path);
    name.rfind('.')
        .map_or(String::new(), |dot| name[dot + 1..].to_ascii_lowercase())
}

fn kind(extension: &str, size: usize) -> Kind {
    if size > MAX_PREVIEW_BYTES {
        Kind::Unsupported
    } else if IMAGE_EXTENSIONS.contains(&extension) {
        Kind::Image
    } else if TEXT_EXTENSIONS.contains(&extension) {
        Kind::Text
    } else {
        Kind::Unsupported
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// `fflate`'s message for a malformed archive.
const INVALID: &str = "invalid zip data";

/// `unzipSync`: every entry by name (a later one of the same name wins), inflated.
fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let invalid = || INVALID.to_owned();
    // The end of central directory record, searched from the end (its comment is at most 65535 bytes).
    let floor = bytes.len().saturating_sub(22 + 65_535);
    let end = (floor..=bytes.len().saturating_sub(22))
        .rev()
        .find(|at| bytes[*at..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(invalid)?;
    let count = u16_at(bytes, end + 10).ok_or_else(invalid)?;
    let mut at = u32_at(bytes, end + 16).ok_or_else(invalid)?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(count);
    for _ in 0..count {
        if !bytes
            .get(at..)
            .is_some_and(|rest| rest.starts_with(&[0x50, 0x4b, 0x01, 0x02]))
        {
            return Err(invalid());
        }
        let flags = u16_at(bytes, at + 8).ok_or_else(invalid)?;
        let method = u16_at(bytes, at + 10).ok_or_else(invalid)?;
        let compressed = u32_at(bytes, at + 20).ok_or_else(invalid)?;
        let size = u32_at(bytes, at + 24).ok_or_else(invalid)?;
        let name_length = u16_at(bytes, at + 28).ok_or_else(invalid)?;
        let extra_length = u16_at(bytes, at + 30).ok_or_else(invalid)?;
        let comment_length = u16_at(bytes, at + 32).ok_or_else(invalid)?;
        let local = u32_at(bytes, at + 42).ok_or_else(invalid)?;
        let raw_name = bytes
            .get(at + 46..at + 46 + name_length)
            .ok_or_else(invalid)?;
        // `strFromU8(…, !(flag & 2048))`: UTF-8 with the language flag, else one character per byte.
        let name = if flags & 0x800 != 0 {
            String::from_utf8_lossy(raw_name).into_owned()
        } else {
            raw_name.iter().map(|&b| char::from(b)).collect()
        };
        at += 46 + name_length + extra_length + comment_length;
        let local_name = u16_at(bytes, local + 26).ok_or_else(invalid)?;
        let local_extra = u16_at(bytes, local + 28).ok_or_else(invalid)?;
        let start = local + 30 + local_name + local_extra;
        let data = bytes.get(start..start + compressed).ok_or_else(invalid)?;
        let content = match method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size);
                flate2::read::DeflateDecoder::new(data)
                    .read_to_end(&mut out)
                    .map_err(|_| "invalid block type".to_owned())?;
                out
            }
            other => return Err(format!("unknown compression type {other}")),
        };
        match files.iter_mut().find(|(known, _)| *known == name) {
            Some(slot) => slot.1 = content,
            None => files.push((name, content)),
        }
    }
    Ok(files)
}

/// `parseZipBuffer`.
pub(crate) fn parse(bytes: &[u8]) -> Result<Archive, String> {
    let mut files: Vec<(String, Vec<u8>)> = unzip(bytes)?
        .into_iter()
        .filter(|(path, _)| !path.ends_with('/'))
        .collect();
    // `localeCompare`: letters before case, then the exact text.
    files.sort_by(|(a, _), (b, _)| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let truncated = files.len() > MAX_ENTRIES;
    let entries = files
        .into_iter()
        .take(MAX_ENTRIES)
        .map(|(path, data)| {
            let path = path.trim_start_matches('/').replace('\\', "/");
            let extension = extension(&path);
            let size = data.len();
            Entry {
                name: file_name(&path),
                kind: kind(&extension, size),
                path,
                size,
                data: data.into(),
                extension,
            }
        })
        .collect();
    Ok(Archive { entries, truncated })
}

/// `getVisibleEntries`: the folders (sorted) and the files right under `current`.
pub(crate) fn visible<'a>(entries: &'a [Entry], current: &str) -> (Vec<String>, Vec<&'a Entry>) {
    let prefix = if current.is_empty() {
        String::new()
    } else {
        format!("{current}/")
    };
    let mut folders: Vec<String> = Vec::new();
    let mut files = Vec::new();
    for entry in entries {
        let Some(rest) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        match rest.find('/') {
            None => files.push(entry),
            Some(slash) => {
                let folder = rest[..slash].to_owned();
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
            }
        }
    }
    folders.sort();
    (folders, files)
}

/// `formatBytes`.
pub(crate) fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.2} MB", bytes as f64 / 1024. / 1024.)
    }
}

/// `getCodeLanguage`.
pub(crate) fn code_language(extension: &str) -> String {
    match extension {
        "js" => "javascript",
        "md" => "markdown",
        "py" => "python",
        "rs" => "rust",
        "sh" => "bash",
        "ts" => "typescript",
        "yml" => "yaml",
        other => other,
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A two-entry archive: `b/x.txt` stored, `a.png` deflated, and a folder entry.
    fn archive() -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        let mut add = |out: &mut Vec<u8>, name: &str, data: &[u8], deflate: bool| {
            let stored = if deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(data).unwrap();
                encoder.finish().unwrap()
            } else {
                data.to_vec()
            };
            let offset = out.len() as u32;
            let method: u16 = if deflate { 8 } else { 0 };
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&stored);
            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 8]);
            central.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        };
        add(&mut out, "b/x.txt", b"hello", false);
        add(&mut out, "a.png", &[7; 100], true);
        add(&mut out, "b/", b"", false);
        let start = out.len() as u32;
        let size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 3, 0, 3, 0]);
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn an_archive_reads_as_unzip_sync_does() {
        let parsed = parse(&archive()).unwrap();
        let paths: Vec<_> = parsed.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.png", "b/x.txt"]);
        assert_eq!(parsed.entries[0].kind, Kind::Image);
        assert_eq!(parsed.entries[0].size, 100);
        assert_eq!(&*parsed.entries[1].data, b"hello");
        assert_eq!(parsed.entries[1].name, "x.txt");
        assert!(!parsed.truncated);
        let (folders, files) = visible(&parsed.entries, "");
        assert_eq!(folders, ["b"]);
        assert_eq!(files.len(), 1);
        assert_eq!(visible(&parsed.entries, "b").1[0].name, "x.txt");
        assert_eq!(parse(b"not a zip").unwrap_err(), INVALID);
    }

    #[test]
    fn helpers_follow_the_original() {
        assert!(is_zip("A.ZIP") && !is_zip("a.zip.txt"));
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.00 MB");
        assert_eq!(
            attachment_path("https://cdn.discordapp.com/attachments/1/2/a%20b.zip?ex=1").as_deref(),
            Some("1/2/a%20b.zip?ex=1")
        );
        assert_eq!(
            attachment_path("https://cdn.discordapp.com/attachments/x/2/a.zip"),
            None
        );
        assert_eq!(
            attachment_path("https://example.com/attachments/1/2/a.zip"),
            None
        );
        assert_eq!(dialog_name("1/2/a%20b.zip?ex=1"), "a b.zip");
        assert_eq!(code_language("rs"), "rust");
        assert_eq!(kind("txt", MAX_PREVIEW_BYTES + 1), Kind::Unsupported);
    }
}
