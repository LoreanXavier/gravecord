//! Reader for Electron's asar archives.
//!
//! Layout: two Chromium pickles, then the file data. Bytes 4..8 hold the size of the header pickle, bytes 12..16 hold
//! the length of the JSON string that starts at byte 16. File offsets are relative to the end of the header pickle.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct Extracted {
    pub files: usize,
    pub links: usize,
    pub missing_unpacked: Vec<String>,
}

fn read_u32(bytes: &[u8], at: usize) -> Result<usize> {
    let raw = bytes.get(at..at + 4).context("asar header is truncated")?;
    Ok(u32::from_le_bytes(raw.try_into()?) as usize)
}

fn read_header(bytes: &[u8]) -> Result<(Value, usize)> {
    let header_size = read_u32(bytes, 4)?;
    let json_len = read_u32(bytes, 12)?;
    let json = bytes
        .get(16..16 + json_len)
        .context("asar header JSON is truncated")?;
    Ok((serde_json::from_slice(json)?, 8 + header_size))
}

/// Archive entry names come from the file, so anything that could leave the output directory is refused.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        bail!("unsafe asar entry name {name:?}");
    }
    Ok(())
}

struct Walk<'a> {
    bytes: &'a [u8],
    base: usize,
    unpacked_root: PathBuf,
    result: Extracted,
}

impl Walk<'_> {
    fn dir(&mut self, entries: &Map<String, Value>, out: &Path, rel: &Path) -> Result<()> {
        fs::create_dir_all(out)?;
        for (name, entry) in entries {
            check_name(name)?;
            let (out, rel) = (out.join(name), rel.join(name));
            if let Some(children) = entry.get("files").and_then(Value::as_object) {
                self.dir(children, &out, &rel)?;
            } else if entry.get("link").is_some() {
                self.result.links += 1;
            } else {
                self.file(entry, &out, &rel)?;
            }
        }
        Ok(())
    }

    fn file(&mut self, entry: &Value, out: &Path, rel: &Path) -> Result<()> {
        if entry.get("unpacked").and_then(Value::as_bool) == Some(true) {
            let source = self.unpacked_root.join(rel);
            if source.is_file() {
                fs::copy(&source, out)?;
                self.result.files += 1;
            } else {
                self.result.missing_unpacked.push(rel.display().to_string());
            }
            return Ok(());
        }
        let size = entry
            .get("size")
            .and_then(Value::as_u64)
            .with_context(|| format!("{} has no size", rel.display()))? as usize;
        let offset: usize = entry
            .get("offset")
            .and_then(Value::as_str)
            .with_context(|| format!("{} has no offset", rel.display()))?
            .parse()?;
        let start = self.base + offset;
        let data = self
            .bytes
            .get(start..start + size)
            .with_context(|| format!("{} is outside the archive", rel.display()))?;
        fs::write(out, data)?;
        self.result.files += 1;
        Ok(())
    }
}

pub fn extract(archive: &Path, out: &Path) -> Result<Extracted> {
    let bytes = fs::read(archive).with_context(|| format!("reading {}", archive.display()))?;
    let (header, base) = read_header(&bytes)?;
    let root = header
        .get("files")
        .and_then(Value::as_object)
        .context("asar header has no root files table")?;
    let mut unpacked_root = archive.as_os_str().to_owned();
    unpacked_root.push(".unpacked");
    let mut walk = Walk {
        bytes: &bytes,
        base,
        unpacked_root: unpacked_root.into(),
        result: Extracted::default(),
    };
    walk.dir(root, out, Path::new(""))?;
    Ok(walk.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn archive(header: &Value, data: &[u8]) -> Vec<u8> {
        let json = header.to_string().into_bytes();
        let padded = json.len().next_multiple_of(4);
        let mut bytes = Vec::new();
        bytes.extend(4u32.to_le_bytes());
        bytes.extend((padded as u32 + 8).to_le_bytes());
        bytes.extend((padded as u32 + 4).to_le_bytes());
        bytes.extend((json.len() as u32).to_le_bytes());
        bytes.extend(&json);
        bytes.resize(16 + padded, 0);
        bytes.extend(data);
        bytes
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-asar-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extracts_nested_files_at_their_offsets() {
        let dir = scratch("nested");
        let header = json!({"files": {
            "a.txt": {"size": 3, "offset": "0"},
            "sub": {"files": {"b.txt": {"size": 2, "offset": "3"}}},
            "link": {"link": "a.txt"},
        }});
        let path = dir.join("t.asar");
        fs::write(&path, archive(&header, b"onetw")).unwrap();

        let result = extract(&path, &dir.join("out")).unwrap();

        assert_eq!(result.files, 2);
        assert_eq!(result.links, 1);
        assert_eq!(fs::read(dir.join("out/a.txt")).unwrap(), b"one");
        assert_eq!(fs::read(dir.join("out/sub/b.txt")).unwrap(), b"tw");
    }

    #[test]
    fn reports_unpacked_files_it_cannot_find() {
        let dir = scratch("unpacked");
        let header = json!({"files": {"n.node": {"size": 9, "unpacked": true}}});
        let path = dir.join("t.asar");
        fs::write(&path, archive(&header, b"")).unwrap();

        let result = extract(&path, &dir.join("out")).unwrap();

        assert_eq!(result.files, 0);
        assert_eq!(result.missing_unpacked, ["n.node"]);
    }

    #[test]
    fn refuses_entries_that_leave_the_output_directory() {
        let dir = scratch("traversal");
        for name in ["..", "a/b", "a\\b", "C:"] {
            let header = json!({"files": {name: {"size": 0, "offset": "0"}}});
            let path = dir.join("t.asar");
            fs::write(&path, archive(&header, b"")).unwrap();
            assert!(extract(&path, &dir.join("out")).is_err(), "{name}");
        }
    }

    #[test]
    fn refuses_a_file_past_the_end_of_the_archive() {
        let dir = scratch("short");
        let header = json!({"files": {"a": {"size": 50, "offset": "0"}}});
        let path = dir.join("t.asar");
        fs::write(&path, archive(&header, b"x")).unwrap();
        assert!(extract(&path, &dir.join("out")).is_err());
    }
}
