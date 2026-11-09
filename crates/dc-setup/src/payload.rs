//! The payload appended to the end of the setup exe: `[stub exe][zstd archive][archive length u64 LE][MAGIC]`.
//!
//! The archive (before compression) is `ARCHIVE_MAGIC`, the version (u16 length, utf-8), the file count (u32), then per
//! file the relative path (u16 length, utf-8, `/` separators), the size (u64) and the bytes.

use anyhow::{Result, bail, ensure};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"GRVCDSET";
const ARCHIVE_MAGIC: &[u8; 4] = b"GCA1";
const LEVEL: i32 = 10;

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Archive {
    pub version: String,
    pub entries: Vec<Entry>,
}

/// The length of the stub in front of an appended payload, or the whole length when there is none.
pub fn stub_len(exe: &[u8]) -> usize {
    match payload_range(exe) {
        Some((start, _)) => start,
        None => exe.len(),
    }
}

/// Byte range of the compressed archive inside an exe image, if the image carries one.
fn payload_range(exe: &[u8]) -> Option<(usize, usize)> {
    let tail = exe.len().checked_sub(16)?;
    if &exe[tail + 8..] != MAGIC {
        return None;
    }
    let length = u64::from_le_bytes(exe[tail..tail + 8].try_into().ok()?) as usize;
    let start = tail.checked_sub(length)?;
    Some((start, tail))
}

/// Encodes the archive and appends it to `stub` (which may already carry an older payload).
pub fn append(stub: &[u8], archive: &Archive) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    raw.extend(ARCHIVE_MAGIC);
    ensure!(
        archive.version.len() <= u16::MAX as usize,
        "version too long"
    );
    raw.extend((archive.version.len() as u16).to_le_bytes());
    raw.extend(archive.version.as_bytes());
    raw.extend((archive.entries.len() as u32).to_le_bytes());
    for entry in &archive.entries {
        ensure!(
            crate::paths::safe_relative(&entry.path).is_some(),
            "unsafe payload path {}",
            entry.path
        );
        raw.extend((entry.path.len() as u16).to_le_bytes());
        raw.extend(entry.path.as_bytes());
        raw.extend((entry.data.len() as u64).to_le_bytes());
        raw.extend(&entry.data);
    }
    let packed = zstd::bulk::compress(&raw, LEVEL)?;
    let mut out = stub[..stub_len(stub)].to_vec();
    out.extend(&packed);
    out.extend((packed.len() as u64).to_le_bytes());
    out.extend(MAGIC);
    Ok(out)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        ensure!(self.0.len() >= count, "payload is truncated");
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(head)
    }
    fn u16(&mut self) -> Result<usize> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?) as usize)
    }
    fn u32(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?) as usize)
    }
    fn u64(&mut self) -> Result<usize> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?) as usize)
    }
}

/// Decodes the payload of an exe image. `Ok(None)` when the image has no payload.
pub fn read(exe: &[u8]) -> Result<Option<Archive>> {
    let Some((start, end)) = payload_range(exe) else {
        return Ok(None);
    };
    let raw = zstd::stream::decode_all(&exe[start..end])?;
    let mut reader = Reader(&raw);
    if reader.take(4)? != ARCHIVE_MAGIC {
        bail!("payload has an unknown format");
    }
    let length = reader.u16()?;
    let version = String::from_utf8(reader.take(length)?.to_vec())?;
    let count = reader.u32()?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let length = reader.u16()?;
        let path = String::from_utf8(reader.take(length)?.to_vec())?;
        ensure!(
            crate::paths::safe_relative(&path).is_some(),
            "unsafe payload path {path}"
        );
        let size = reader.u64()?;
        entries.push(Entry {
            path,
            data: reader.take(size)?.to_vec(),
        });
    }
    Ok(Some(Archive { version, entries }))
}

/// Reads the payload of the running executable.
pub fn read_own() -> Result<Option<Archive>> {
    let exe = std::fs::read(std::env::current_exe()?)?;
    read(&exe)
}

/// Collects `gravecord.exe` and every file under `app/assets` for the payload.
pub fn collect(exe: &Path, assets: &Path) -> Result<Vec<Entry>> {
    let mut entries = vec![Entry {
        path: "gravecord.exe".into(),
        data: std::fs::read(exe)?,
    }];
    let mut files = Vec::new();
    walk(assets, &mut files)?;
    files.sort();
    for file in files {
        let relative = file
            .strip_prefix(assets)?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(Entry {
            path: format!("app/assets/{relative}"),
            data: std::fs::read(&file)?,
        });
    }
    Ok(entries)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for item in std::fs::read_dir(dir)? {
        let path = item?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Archive {
        Archive {
            version: "1.2.3".into(),
            entries: vec![
                Entry {
                    path: "gravecord.exe".into(),
                    data: vec![7; 5000],
                },
                Entry {
                    path: "app/assets/icons/a.svg".into(),
                    data: b"<svg/>".to_vec(),
                },
                Entry {
                    path: "app/assets/empty".into(),
                    data: vec![],
                },
            ],
        }
    }

    #[test]
    fn round_trips_behind_a_stub() {
        let stub = b"MZ-stub-bytes".to_vec();
        let image = append(&stub, &sample()).unwrap();
        assert!(image.starts_with(&stub));
        assert_eq!(&image[image.len() - 8..], MAGIC);
        assert_eq!(stub_len(&image), stub.len());
        assert_eq!(read(&image).unwrap().unwrap(), sample());
    }

    #[test]
    fn appending_again_replaces_the_old_payload() {
        let stub = b"MZ".to_vec();
        let once = append(&stub, &sample()).unwrap();
        let mut other = sample();
        other.version = "9".into();
        let twice = append(&once, &other).unwrap();
        assert_eq!(stub_len(&twice), 2);
        assert_eq!(read(&twice).unwrap().unwrap().version, "9");
    }

    #[test]
    fn an_exe_without_payload_reads_as_none() {
        assert!(read(b"just an exe, no tail at all").unwrap().is_none());
        assert!(read(b"").unwrap().is_none());
        assert_eq!(stub_len(b"abc"), 3);
    }

    #[test]
    fn damaged_payloads_are_errors_not_panics() {
        let mut image = append(b"MZ", &sample()).unwrap();
        let at = image.len() - 20;
        image[at] ^= 0xff;
        assert!(read(&image).is_err());
        // A length that points before the start of the file.
        let mut bogus = vec![0u8; 4];
        bogus.extend(u64::MAX.to_le_bytes());
        bogus.extend(MAGIC);
        assert!(read(&bogus).unwrap().is_none());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for bad in ["../x", "/abs", "a/../b", "C:/x", "a\\b", ""] {
            let archive = Archive {
                version: "1".into(),
                entries: vec![Entry {
                    path: bad.into(),
                    data: vec![],
                }],
            };
            assert!(append(b"MZ", &archive).is_err(), "{bad}");
        }
    }
}
