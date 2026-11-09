//! The login screen's artwork. Static files of the pinned build's CDN, fetched on first use and kept on disk next to the
//! fonts (`fonts.rs` does the same); nothing is bundled (`DECISIONS.md`).

use anyhow::{Context as _, Result};
use dc_proto::pin;
use std::path::PathBuf;

/// The page background, a 1440 x 900 SVG with its own blurred glows.
const ARTWORK: &str = "d8680b1c1576ecc8.svg";
/// The wordmark top left, 124 x 24.
const LOGO: &str = "131c318dd45b7aa4.svg";
/// The Discord mark over the middle of the QR code, 50 x 50.
const QR_LOGO: &str = "dd05fd1ea37e7747.png";
/// The default avatar of a scanned account that has none, 120 x 120.
const DEFAULT_AVATAR: &str = "18e336a74a159cfd.png";
/// The sunglasses mascot of the captcha dialog.
const MASCOT: &str = "a1c385fb82c39bab.svg";

/// What was fetched; a missing file shows as nothing, the screen is usable without it.
#[derive(Default)]
pub struct ArtBytes {
    pub artwork: Option<Vec<u8>>,
    pub logo: Option<Vec<u8>>,
    pub qr_logo: Option<Vec<u8>>,
    pub default_avatar: Option<Vec<u8>>,
    pub mascot: Option<Vec<u8>>,
}

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(base).join("gravecord").join("login-art");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

async fn file(client: &dc_http::Client, name: &str) -> Option<Vec<u8>> {
    let cached = cache_dir().ok()?.join(name);
    if let Ok(bytes) = std::fs::read(&cached)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }
    let url = format!("{}/assets/{name}", pin::WEBAPP_ORIGIN);
    let bytes = client.fetch_public(&url).await.ok()?;
    let _ = std::fs::write(&cached, &bytes);
    Some(bytes)
}

pub async fn load(client: &dc_http::Client) -> ArtBytes {
    ArtBytes {
        artwork: file(client, ARTWORK).await,
        logo: file(client, LOGO).await,
        qr_logo: file(client, QR_LOGO).await,
        default_avatar: file(client, DEFAULT_AVATAR).await,
        mascot: file(client, MASCOT).await,
    }
}

/// A scanned account's avatar from the public CDN.
pub async fn avatar(client: &dc_http::Client, user: &str, hash: &str) -> Option<Vec<u8>> {
    let url = format!("https://cdn.discordapp.com/avatars/{user}/{hash}.png?size=240");
    client.fetch_public(&url).await.ok()
}
