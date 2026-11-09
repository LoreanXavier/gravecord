//! gg sans and gg mono, fetched at runtime from the official asset host with the pinned hashes, as
//! the official client loads them. Decoded fonts are cached on disk; nothing is bundled.

use anyhow::{Context, Result};
use dc_proto::pin;
use std::path::PathBuf;

/// `design/tokens/font-faces.json`, the faces the M0 screens use.
const FACES: [&str; 13] = [
    "66d715454104d24e", // gg sans 400
    "dd24010f3cf7def7", // gg sans 400 italic
    "b272b33815319bae", // gg sans 500
    "6a1346ad3821ff3c", // gg sans 500 italic
    "2df2c3ff74408972", // gg sans 600
    "d5d789aeb6282532", // gg sans 600 italic
    "189422196a4f8b53", // gg sans 700
    "ce3b8055f5114434", // gg sans 700 italic
    "b2fdbe507d6ce9ef", // gg sans 800
    "03dcf979852e8b8e", // gg sans 800 italic
    "249d0a057895c668", // gg mono 400
    "45efa6936fdfb918", // gg mono 700
    "404896f5b47121de", // ABC Ginto Discord Nord 700 italic (the Nitro page headings)
];

fn cache_dir() -> Result<PathBuf> {
    let dir = crate::storage::data_dir()
        .context("LOCALAPPDATA is not set")?
        .join("fonts");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

async fn face(client: &dc_http::Client, hash: &str) -> Result<Vec<u8>> {
    let cached = cache_dir()?.join(format!("{hash}.ttf"));
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok(bytes);
    }
    let url = format!("{}/assets/{hash}.woff2", pin::WEBAPP_ORIGIN);
    let compressed = client.fetch_public(&url).await?;
    let bytes = wuff::decompress_woff2(&compressed)
        .map_err(|error| anyhow::anyhow!("{hash}: {error:?}"))?;
    std::fs::write(&cached, &bytes)?;
    Ok(bytes)
}

pub async fn load(client: &dc_http::Client) -> Result<Vec<Vec<u8>>> {
    let mut faces = Vec::with_capacity(FACES.len());
    for hash in FACES {
        faces.push(face(client, hash).await?);
    }
    Ok(faces)
}
