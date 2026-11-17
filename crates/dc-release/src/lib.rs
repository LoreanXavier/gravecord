//! The signed update manifest appended after the payload, as P.T.'s setups carry theirs
//! (`pt-pc-release/src/engine/platform/signed_trailer.h`):
//! `[setup body][manifest JSON][Ed25519 signature, 64 bytes][footer, 48 bytes]`.
//!
//! The footer is `MAGIC`, the format version (u32 LE), a reserved zero (u32), then the manifest's offset and length and
//! the signature's offset and length (u64 LE each). The body is everything before the manifest: the stub and its payload,
//! exactly as `payload` wrote them, so the installer reads its payload as if no trailer were there.
//!
//! Only the format lives here (the installer reads its payload through a trailer: `dc-setup` tests it). Signing is `cargo xtask package`'s, verifying the app updater's; neither belongs in the
//! installer, which only has to see past the trailer.

use anyhow::{Result, ensure};

pub const MAGIC: &[u8; 8] = b"GRVCSIG1";
pub const VERSION: u32 = 1;
pub const FOOTER_LEN: usize = 48;
pub const SIGNATURE_LEN: usize = 64;
/// A manifest larger than this is not read.
pub const MAX_MANIFEST: usize = 64 * 1024;

/// Where the parts of a signed file lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footer {
    pub manifest_offset: u64,
    pub manifest_len: u64,
    pub signature_offset: u64,
}

impl Footer {
    /// The length of the body: the bytes the manifest describes.
    pub fn body_len(&self) -> u64 {
        self.manifest_offset
    }
}

/// Reads the footer from the last bytes of a file (at least [`FOOTER_LEN`] of them) and the file's total length.
/// `None` when the file carries no trailer or a malformed one.
pub fn parse(last: &[u8], total: u64) -> Option<Footer> {
    let footer = last.get(last.len().checked_sub(FOOTER_LEN)?..)?;
    if &footer[..8] != MAGIC {
        return None;
    }
    let u32_at = |at: usize| u32::from_le_bytes(footer[at..at + 4].try_into().unwrap_or_default());
    let u64_at = |at: usize| u64::from_le_bytes(footer[at..at + 8].try_into().unwrap_or_default());
    if u32_at(8) != VERSION || u32_at(12) != 0 {
        return None;
    }
    let (manifest_offset, manifest_len) = (u64_at(16), u64_at(24));
    let (signature_offset, signature_len) = (u64_at(32), u64_at(40));
    let fits = signature_len == SIGNATURE_LEN as u64
        && manifest_len > 0
        && manifest_len <= MAX_MANIFEST as u64
        && manifest_offset.checked_add(manifest_len) == Some(signature_offset)
        && signature_offset
            .checked_add(signature_len)
            .and_then(|end| end.checked_add(FOOTER_LEN as u64))
            == Some(total);
    fits.then_some(Footer {
        manifest_offset,
        manifest_len,
        signature_offset,
    })
}

/// The parts of a signed file held in memory: body, manifest and signature.
pub fn split(file: &[u8]) -> Option<(&[u8], &[u8], &[u8; SIGNATURE_LEN])> {
    let footer = parse(file, file.len() as u64)?;
    let body = &file[..footer.manifest_offset as usize];
    let manifest = &file[footer.manifest_offset as usize..footer.signature_offset as usize];
    let signature = file
        [footer.signature_offset as usize..footer.signature_offset as usize + SIGNATURE_LEN]
        .try_into()
        .ok()?;
    Some((body, manifest, signature))
}

/// `body` with `manifest` and its `signature` appended.
pub fn append(body: &[u8], manifest: &[u8], signature: &[u8; SIGNATURE_LEN]) -> Result<Vec<u8>> {
    ensure!(
        !manifest.is_empty() && manifest.len() <= MAX_MANIFEST,
        "the update manifest is empty or too large"
    );
    ensure!(
        parse(body, body.len() as u64).is_none(),
        "the setup is signed already"
    );
    let manifest_offset = body.len() as u64;
    let signature_offset = manifest_offset + manifest.len() as u64;
    let mut out = Vec::with_capacity(body.len() + manifest.len() + SIGNATURE_LEN + FOOTER_LEN);
    out.extend_from_slice(body);
    out.extend_from_slice(manifest);
    out.extend_from_slice(signature);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for value in [
        manifest_offset,
        manifest.len() as u64,
        signature_offset,
        SIGNATURE_LEN as u64,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_file_splits_back_into_its_parts_and_damage_is_refused() {
        let body = b"stub and payload".to_vec();
        let signed = append(&body, b"{\"format\":1}", &[7; 64]).unwrap();
        let (b, m, s) = split(&signed).unwrap();
        assert_eq!(b, body.as_slice());
        assert_eq!(m, b"{\"format\":1}");
        assert_eq!(s, &[7; 64]);
        assert_eq!(
            parse(&signed, signed.len() as u64).unwrap().body_len(),
            body.len() as u64
        );
        // Unsigned, cut short, or with a byte more at the end: no trailer.
        assert!(split(&body).is_none());
        assert!(split(&signed[..signed.len() - 1]).is_none());
        let mut longer = signed.clone();
        longer.push(0);
        assert!(split(&longer).is_none());
        // Signing twice is refused.
        assert!(append(&signed, b"{}", &[0; 64]).is_err());
    }
}
