//! The payload appended to the end of the setup exe:
//! `[stub exe][zstd archive][SHA-256 of the zstd archive][archive length u64 LE][MAGIC]`.
//!
//! The archive (before compression) is `ARCHIVE_MAGIC`, the version (u16 length, utf-8), the file count (u32), then per
//! file the relative path (u16 length, utf-8, `/` separators), the size (u64) and the bytes.
//!
//! The hash covers the compressed archive and is checked before a single byte is decompressed or written. Tails of the
//! first format (`GRVCDSET`, no hash) are refused with a message instead of being half-read.

use anyhow::{Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"GRVCDST2";
/// The first format, which carried no hash. Only recognised so it can be refused.
const LEGACY_MAGIC: &[u8; 8] = b"GRVCDSET";
const ARCHIVE_MAGIC: &[u8; 4] = b"GCA1";
const LEVEL: i32 = 10;
const HASH_LEN: usize = 32;
/// Hash, length and magic.
const TAIL_LEN: usize = HASH_LEN + 8 + 8;

pub const DAMAGED: &str = "The setup file is damaged: its contents do not match the checksum stored in it. Download gravecord-setup.exe again.";
pub const OLD_FORMAT: &str = "This setup file was packaged by an older version and cannot be verified. Download gravecord-setup.exe again.";

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Archive {
    pub version: String,
    pub entries: Vec<Entry>,
}

/// What the end of an exe says about its payload.
#[derive(Debug, PartialEq, Eq)]
enum Tail {
    None,
    Legacy,
    /// Byte range of the compressed archive and the hash stored for it.
    Current {
        start: u64,
        end: u64,
        hash: [u8; HASH_LEN],
    },
}

/// Reads the tail from the last bytes of an exe (at most `TAIL_LEN` of them) and the exe's total length.
fn parse_tail(last: &[u8], total: u64) -> Result<Tail> {
    let Some(magic) = last.len().checked_sub(8).map(|at| &last[at..]) else {
        return Ok(Tail::None);
    };
    if magic == LEGACY_MAGIC {
        return Ok(Tail::Legacy);
    }
    if magic != MAGIC {
        return Ok(Tail::None);
    }
    ensure!(last.len() >= TAIL_LEN, "payload is truncated");
    let tail = &last[last.len() - TAIL_LEN..];
    let hash: [u8; HASH_LEN] = tail[..HASH_LEN].try_into()?;
    let length = u64::from_le_bytes(tail[HASH_LEN..HASH_LEN + 8].try_into()?);
    let end = total
        .checked_sub(TAIL_LEN as u64)
        .ok_or_else(|| anyhow::anyhow!("payload is truncated"))?;
    let start = end
        .checked_sub(length)
        .ok_or_else(|| anyhow::anyhow!("payload is truncated"))?;
    Ok(Tail::Current { start, end, hash })
}

/// The exe without a signed update manifest after it (`trailer`): what the payload format describes.
fn unsigned(exe: &[u8]) -> &[u8] {
    match dc_release::parse(exe, exe.len() as u64) {
        Some(footer) => &exe[..footer.body_len() as usize],
        None => exe,
    }
}

fn tail_of(exe: &[u8]) -> Result<Tail> {
    let exe = unsigned(exe);
    let from = exe.len().saturating_sub(TAIL_LEN);
    parse_tail(&exe[from..], exe.len() as u64)
}

/// The length of the stub in front of an appended payload, or the whole length when there is none.
pub fn stub_len(exe: &[u8]) -> usize {
    match tail_of(exe) {
        Ok(Tail::Current { start, .. }) => start as usize,
        // An older payload has no hash; its length sits in the 8 bytes before the magic.
        Ok(Tail::Legacy) => exe
            .len()
            .checked_sub(16)
            .and_then(|at| {
                let length = u64::from_le_bytes(exe[at..at + 8].try_into().ok()?) as usize;
                at.checked_sub(length)
            })
            .unwrap_or(exe.len()),
        _ => exe.len(),
    }
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
    out.extend(Sha256::digest(&packed));
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

/// Checks the hash of the compressed archive, and only then decompresses and parses it.
fn decode(packed: &[u8], hash: &[u8; HASH_LEN]) -> Result<Archive> {
    if Sha256::digest(packed).as_slice() != hash {
        bail!(DAMAGED);
    }
    let raw = zstd::stream::decode_all(packed)?;
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
    Ok(Archive { version, entries })
}

/// Decodes the payload of an exe image. `Ok(None)` when the image has no payload.
pub fn read(exe: &[u8]) -> Result<Option<Archive>> {
    let exe = unsigned(exe);
    match tail_of(exe)? {
        Tail::None => Ok(None),
        Tail::Legacy => bail!(OLD_FORMAT),
        Tail::Current { start, end, hash } => {
            Ok(Some(decode(&exe[start as usize..end as usize], &hash)?))
        }
    }
}

/// An exe read from disk: its payload (verified) and the stub in front of it.
pub struct Loaded {
    pub archive: Option<Archive>,
    pub stub: Vec<u8>,
}

/// Reads and verifies the payload of the exe at `path`, reading each byte once: the stub, then the archive.
pub fn load(path: &Path) -> Result<Loaded> {
    let mut file = std::fs::File::open(path)?;
    let mut total = file.metadata()?.len();
    // A signed setup (`trailer`): the payload ends where the update manifest starts.
    let mut footer = vec![0u8; dc_release::FOOTER_LEN.min(total as usize)];
    file.seek(SeekFrom::Start(total - footer.len() as u64))?;
    file.read_exact(&mut footer)?;
    if let Some(signed) = dc_release::parse(&footer, total) {
        total = signed.body_len();
    }
    let from = total.saturating_sub(TAIL_LEN as u64);
    file.seek(SeekFrom::Start(from))?;
    let mut last = vec![0u8; (total - from) as usize];
    file.read_exact(&mut last)?;
    match parse_tail(&last, total)? {
        Tail::None => {
            let mut stub = vec![0u8; total as usize];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut stub)?;
            Ok(Loaded {
                archive: None,
                stub,
            })
        }
        Tail::Legacy => bail!(OLD_FORMAT),
        Tail::Current { start, end, hash } => {
            file.seek(SeekFrom::Start(0))?;
            let mut stub = vec![0u8; start as usize];
            file.read_exact(&mut stub)?;
            let mut packed = vec![0u8; (end - start) as usize];
            file.read_exact(&mut packed)?;
            Ok(Loaded {
                archive: Some(decode(&packed, &hash)?),
                stub,
            })
        }
    }
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
        assert!(image.len() > stub.len() + TAIL_LEN);
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
        // A length that points before the start of the file.
        let mut bogus = vec![0u8; 4];
        bogus.extend([0u8; HASH_LEN]);
        bogus.extend(u64::MAX.to_le_bytes());
        bogus.extend(MAGIC);
        assert!(read(&bogus).is_err());
        // A tail shorter than its own fields.
        assert!(read(MAGIC).is_err());
    }

    #[test]
    fn a_flipped_byte_anywhere_in_the_archive_is_refused_by_the_hash() {
        let image = append(b"MZ", &sample()).unwrap();
        let packed = 2..image.len() - TAIL_LEN;
        for at in [
            packed.start,
            (packed.start + packed.end) / 2,
            packed.end - 1,
        ] {
            let mut damaged = image.clone();
            damaged[at] ^= 0x01;
            let error = read(&damaged).unwrap_err().to_string();
            assert_eq!(error, DAMAGED, "byte {at}");
        }
        // A damaged stored hash is just as fatal.
        let mut damaged = image.clone();
        let at = image.len() - TAIL_LEN;
        damaged[at] ^= 0x80;
        assert_eq!(read(&damaged).unwrap_err().to_string(), DAMAGED);
        // The intact image still reads.
        assert_eq!(read(&image).unwrap().unwrap(), sample());
    }

    #[test]
    fn the_first_format_is_refused_not_half_read() {
        let mut old = b"MZ".to_vec();
        old.extend(b"some zstd bytes");
        old.extend(15u64.to_le_bytes());
        old.extend(LEGACY_MAGIC);
        assert_eq!(read(&old).unwrap_err().to_string(), OLD_FORMAT);
        assert_eq!(stub_len(&old), 2);
    }

    #[test]
    fn load_reads_the_same_archive_and_stub_from_a_file() {
        let dir = std::env::temp_dir().join(format!("gc-payload-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stub = b"MZ-stub".to_vec();
        let good = dir.join("good.exe");
        std::fs::write(&good, append(&stub, &sample()).unwrap()).unwrap();
        let loaded = load(&good).unwrap();
        assert_eq!(loaded.archive.unwrap(), sample());
        assert_eq!(loaded.stub, stub);

        let plain = dir.join("plain.exe");
        std::fs::write(&plain, b"no payload here at all, just bytes").unwrap();
        let loaded = load(&plain).unwrap();
        assert!(loaded.archive.is_none());
        assert_eq!(loaded.stub, b"no payload here at all, just bytes");

        let mut image = append(&stub, &sample()).unwrap();
        let at = image.len() - TAIL_LEN - 3;
        image[at] ^= 0xff;
        let bad = dir.join("bad.exe");
        std::fs::write(&bad, image).unwrap();
        assert_eq!(load(&bad).err().unwrap().to_string(), DAMAGED);
        let _ = std::fs::remove_dir_all(&dir);
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

    /// A setup signed by `cargo xtask package` (`dc_release`): the installer reads its payload as before.
    #[test]
    fn the_installer_reads_its_payload_through_a_signature() {
        let archive = Archive {
            version: "0.0.0-nightly.202611171830".into(),
            entries: vec![Entry {
                path: "gravecord.exe".into(),
                data: b"app".to_vec(),
            }],
        };
        let image = append(b"MZ stub", &archive).unwrap();
        let signed = dc_release::append(&image, b"{\"format\":1}", &[1; 64]).unwrap();
        assert_eq!(read(&signed).unwrap(), Some(archive.clone()));
        assert_eq!(stub_len(&signed), 7);
        let dir = std::env::temp_dir().join(format!("gc-signed-setup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gravecord-setup.exe");
        std::fs::write(&path, &signed).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.archive, Some(archive));
        assert_eq!(loaded.stub, b"MZ stub");
        let _ = std::fs::remove_dir_all(dir);
    }
}
