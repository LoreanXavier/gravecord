//! A ZIP archive with every file stored as it is (`zipSync(files, { level: 0 })` of fflate, which DownloadAllAttachments
//! uses), and the file names it gives (`uniqueFilenames`). Names are UTF-8 (general purpose bit 11). The archive is
//! never larger than 1 GiB (the plugin's `MAX_ZIP_BYTES`), so no ZIP64 records are needed.

use std::collections::HashSet;

/// `uniqueFilenames`: a name already used (any case) gets `_1`, `_2`… before its extension.
pub(crate) fn unique_filenames(originals: &[String]) -> Vec<String> {
    let mut used = HashSet::new();
    originals
        .iter()
        .map(|original| {
            let (base, ext) = match original.rfind('.') {
                Some(dot) if dot > 0 => original.split_at(dot),
                _ => (original.as_str(), ""),
            };
            let mut name = original.clone();
            let mut i = 1;
            while used.contains(&name.to_lowercase()) {
                name = format!("{base}_{i}{ext}");
                i += 1;
            }
            used.insert(name.to_lowercase());
            name
        })
        .collect()
}

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The archive of `files` (name, bytes), each stored.
pub(crate) fn stored(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    // 1980-01-01 00:00, the earliest DOS time (fflate's default without `mtime`).
    const DOS_TIME: u16 = 0;
    const DOS_DATE: u16 = (1 << 5) | 1;
    const UTF8: u16 = 1 << 11;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, bytes) in files {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(bytes);
        let size = bytes.len() as u32;
        let header = |out: &mut Vec<u8>, central: bool| {
            u32le(out, if central { 0x0201_4b50 } else { 0x0403_4b50 });
            if central {
                u16le(out, 20);
            }
            u16le(out, 20);
            u16le(out, UTF8);
            u16le(out, 0);
            u16le(out, DOS_TIME);
            u16le(out, DOS_DATE);
            u32le(out, crc);
            u32le(out, size);
            u32le(out, size);
            u16le(out, name.len() as u16);
            u16le(out, 0);
            if central {
                u16le(out, 0);
                u16le(out, 0);
                u16le(out, 0);
                u32le(out, 0);
                u32le(out, offset);
            }
            out.extend_from_slice(name.as_bytes());
        };
        header(&mut out, false);
        out.extend_from_slice(bytes);
        header(&mut central, true);
    }
    let start = out.len() as u32;
    let count = files.len() as u16;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, count);
    u16le(&mut out, count);
    u32le(&mut out, central.len() as u32);
    u32le(&mut out, start);
    u16le(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_unique_as_the_original_does() {
        let names = ["a.png", "A.png", "a.png", "b", "b", ".env", ".env"].map(str::to_owned);
        assert_eq!(
            unique_filenames(&names),
            ["a.png", "A_1.png", "a_2.png", "b", "b_1", ".env", ".env_1"]
        );
    }

    #[test]
    fn the_archive_holds_each_file_stored_with_its_crc() {
        let files = vec![
            ("one.txt".to_owned(), b"hello".to_vec()),
            ("ğüş.bin".to_owned(), vec![0, 1, 2, 3]),
        ];
        let zip = stored(&files);
        // Local header, name, data for each; then the central directory and its end record.
        assert_eq!(&zip[..4], b"PK\x03\x04");
        assert_eq!(&zip[30..37], b"one.txt");
        assert_eq!(&zip[37..42], b"hello");
        assert_eq!(
            u32::from_le_bytes(zip[14..18].try_into().unwrap()),
            crc32fast::hash(b"hello")
        );
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end + 4], b"PK\x05\x06");
        assert_eq!(
            u16::from_le_bytes(zip[end + 10..end + 12].try_into().unwrap()),
            2
        );
        let directory = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
        assert_eq!(&zip[directory..directory + 4], b"PK\x01\x02");
        // The second entry's local header sits where the central directory says.
        let second = u32::from_le_bytes(
            zip[directory + 46 + 7 + 42..directory + 46 + 7 + 46]
                .try_into()
                .unwrap(),
        );
        assert_eq!(&zip[second as usize..second as usize + 4], b"PK\x03\x04");
    }
}
