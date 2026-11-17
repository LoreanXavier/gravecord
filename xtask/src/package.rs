//! `cargo xtask package`: builds the release app and the setup stub, appends the payload and writes
//! `gravecord-dist/gravecord-setup.exe`, then refreshes the portable folder next to it.
//!
//! Two environment variables serve tests of the packaging itself: `GRAVECORD_DIST_DIR` writes into another folder
//! instead of `gravecord-dist`, and `GRAVECORD_PACKAGE_SKIP_BUILD=1` reuses the release build already in
//! `target/dist`.
//!
//! The setup is signed as P.T.'s are: an update manifest (version, size and SHA-256 of the setup before it) and its
//! Ed25519 signature are appended after the payload (`dc_release`). The app updater reads the release from GitHub's
//! REST API and checks that signature against the public key built into it (`app/src/release_key.rs`); the release
//! carries no file but the setup. The private key is a seed in `%USERPROFILE%\gravecord-release-keys` (or
//! `GRAVECORD_RELEASE_KEYS`), outside every repository; it is made once, never printed and never replaced.
//!
//! The release build uses the static C runtime, so neither exe imports VCRUNTIME140. It goes to its own target folder
//! so the flag change never invalidates the normal release cache.

use anyhow::{Context, Result, bail};
use dc_setup::payload::{self, Archive};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET_DIR: &str = "target/dist";

fn dist_dir(repo: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("GRAVECORD_DIST_DIR") {
        return PathBuf::from(dir);
    }
    repo.parent()
        .expect("repo has a parent")
        .join("gravecord-dist")
}

fn app_version(repo: &Path) -> Result<String> {
    let manifest = std::fs::read_to_string(repo.join("app/Cargo.toml"))?;
    manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .map(str::to_owned)
        .context("no version in app/Cargo.toml")
}

/// What the build is told about itself: a tester (nightly) build names its channel and its stamped version, which the
/// app reads at compile time (`app/src/updater.rs`); a stable build has neither, so a nightly setting never leaks
/// into it from the shell.
fn build(repo: &Path, nightly: Option<&str>) -> Result<()> {
    let mut command = Command::new("cargo");
    match nightly {
        Some(version) => command
            .env("GRAVECORD_CHANNEL", "nightly")
            .env("GRAVECORD_BUILD_VERSION", version),
        None => command
            .env_remove("GRAVECORD_CHANNEL")
            .env_remove("GRAVECORD_BUILD_VERSION"),
    };
    let status = command
        .current_dir(repo)
        .env("RUSTFLAGS", "-C target-feature=+crt-static")
        .env("CARGO_BUILD_JOBS", "4")
        .args(["build", "--release", "-j", "4", "--target-dir", TARGET_DIR])
        .args(["-p", "gravecord", "-p", "dc-setup"])
        .status()
        .context("could not run cargo")?;
    if !status.success() {
        bail!("release build failed");
    }
    Ok(())
}

/// Writes through a temporary file so a half-written setup never replaces a good one.
fn write_replacing(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("exe.tmp");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

const SEED_NAME: &str = "gravecord-release-ed25519.seed";
const PUBLIC_NAME: &str = "gravecord-release-ed25519.pub";
const KEY_SOURCE: &str = "app/src/release_key.rs";

fn keys_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("GRAVECORD_RELEASE_KEYS") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("USERPROFILE").context("no USERPROFILE for the release keys")?;
    Ok(PathBuf::from(home).join("gravecord-release-keys"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The release signing key: read from `dir`, or made there once. An existing seed is never replaced, and the seed is
/// never printed.
fn signing_key(dir: &Path) -> Result<ed25519_dalek::SigningKey> {
    let seed_path = dir.join(SEED_NAME);
    if seed_path.exists() {
        let seed = unhex(&std::fs::read_to_string(&seed_path)?)
            .context("the release seed is not 64 hex digits")?;
        return Ok(ed25519_dalek::SigningKey::from_bytes(&seed));
    }
    std::fs::create_dir_all(dir)?;
    let mut seed = [0u8; 32];
    rand::Fill::fill(&mut seed, &mut rand::rng());
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    std::fs::write(&seed_path, hex(&seed))?;
    std::fs::write(dir.join(PUBLIC_NAME), hex(key.verifying_key().as_bytes()))?;
    println!(
        "release key made in {} (public key in {KEY_SOURCE})",
        dir.display()
    );
    Ok(key)
}

/// The Rust source holding the public key the app checks setups against.
fn key_source(public: &[u8; 32]) -> String {
    let bytes: Vec<String> = public.iter().map(|byte| format!("0x{byte:02x}")).collect();
    let rows: Vec<String> = bytes
        .chunks(16)
        .map(|row| format!("    {},", row.join(", ")))
        .collect();
    format!(
        "//! The release public key: the Ed25519 key `cargo xtask package` signs every setup with (`xtask/src/package.rs`).\n\
         //! Written by that command from the key in `%USERPROFILE%\\gravecord-release-keys`; the updater refuses any setup\n\
         //! whose signed update manifest this key does not verify (`updater::verify_signed`).\n\n\
         pub const PUBLIC_KEY: [u8; 32] = [\n{}\n];\n",
        rows.join("\n")
    )
}

/// Makes sure the app is built with this key's public half: rewrites the key source when it differs (a first key).
fn ensure_key_source(repo: &Path, key: &ed25519_dalek::SigningKey) -> Result<()> {
    let path = repo.join(KEY_SOURCE);
    let wanted = key_source(key.verifying_key().as_bytes());
    // Compared without line ending differences: a checkout may turn the file into CRLF.
    let current = std::fs::read_to_string(&path).map(|text| text.replace("\r\n", "\n"));
    if current.ok().as_deref() != Some(wanted.as_str()) {
        std::fs::write(&path, wanted)?;
        println!("{KEY_SOURCE} written: commit it with this build");
    }
    Ok(())
}

/// The update manifest for a setup body, as the app reads it (`updater::verify_signed`).
fn update_manifest(version: &str, channel: &str, body: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let manifest = serde_json::json!({
        "format": 1,
        "version": version,
        "channel": channel,
        "package": {
            "name": "gravecord-setup.exe",
            "size": body.len(),
            "sha256": hex(&Sha256::digest(body)),
        },
    });
    serde_json::to_vec(&manifest).expect("a manifest is valid JSON")
}

/// `body` with its update manifest and the manifest's signature after it.
fn sign(
    body: &[u8],
    version: &str,
    channel: &str,
    key: &ed25519_dalek::SigningKey,
) -> Result<Vec<u8>> {
    use ed25519_dalek::Signer;
    let manifest = update_manifest(version, channel, body);
    let signature = key.sign(&manifest).to_bytes();
    dc_release::append(body, &manifest, &signature)
}

/// Makes `to` hold exactly the files of `from`.
fn mirror(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    let mut wanted = Vec::new();
    for item in std::fs::read_dir(from)? {
        let item = item?;
        let target = to.join(item.file_name());
        wanted.push(item.file_name());
        if item.path().is_dir() {
            mirror(&item.path(), &target)?;
        } else {
            std::fs::copy(item.path(), &target)?;
        }
    }
    for item in std::fs::read_dir(to)? {
        let item = item?;
        if !wanted.contains(&item.file_name()) {
            if item.path().is_dir() {
                std::fs::remove_dir_all(item.path())?;
            } else {
                std::fs::remove_file(item.path())?;
            }
        }
    }
    Ok(())
}

/// What happened to the portable folder.
#[derive(Debug, PartialEq, Eq)]
enum Portable {
    /// `gravecord.exe` and `app\assets` in the dist folder now match the new build.
    Refreshed,
    /// The exe was locked. The dist folder is untouched; the complete new build is in `gravecord-next\`.
    Beside(PathBuf),
}

/// Refreshes the portable build in `dist`. The exe is replaced first; only when that works are the assets mirrored, so
/// an old exe is never left with new assets. When the exe is locked (the owner has it open), the existing folder is
/// left exactly as it was and the whole new build (exe plus assets) goes to `dist\gravecord-next\`.
fn refresh_portable(dist: &Path, app_exe: &Path, assets: &Path) -> Result<Portable> {
    let exe_data = std::fs::read(app_exe)?;
    let target = dist.join("gravecord.exe");
    match write_replacing(&target, &exe_data) {
        Ok(()) => {
            mirror(assets, &dist.join("app").join("assets"))?;
            Ok(Portable::Refreshed)
        }
        Err(error) => {
            let next = dist.join("gravecord-next");
            std::fs::create_dir_all(&next)?;
            write_replacing(&next.join("gravecord.exe"), &exe_data)?;
            mirror(assets, &next.join("app").join("assets"))?;
            println!(
                "portable gravecord.exe is locked ({error}); {} left untouched",
                dist.display()
            );
            Ok(Portable::Beside(next))
        }
    }
}

/// `YYYYMMDDHHMM` in UTC: the nightly build number. Later builds sort higher as numbers, which is how the updater
/// orders `0.0.0-nightly.N` versions.
fn nightly_stamp(since_epoch: u64) -> String {
    let days = (since_epoch / 86_400) as i64;
    let minutes = since_epoch % 86_400 / 60;
    // Civil date from days since 1970-01-01 (Howard Hinnant's days_from_civil, inverted).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}",
        minutes / 60,
        minutes % 60
    )
}

/// `cargo xtask package` (stable) or `cargo xtask package --channel nightly` (a tester build: version
/// `<app version>-nightly.<UTC stamp>`, written to `gravecord-dist/nightly`). Publishing the nightly is copying its
/// `gravecord-setup.exe` and `update.json` to the `nightly` release of the update repository.
pub fn run(repo: &Path, nightly: bool) -> Result<()> {
    let base = app_version(repo)?;
    let version = if nightly {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("the clock is before 1970")?;
        format!("{base}-nightly.{}", nightly_stamp(now.as_secs()))
    } else {
        base
    };
    let key = signing_key(&keys_dir()?)?;
    ensure_key_source(repo, &key)?;
    if std::env::var_os("GRAVECORD_PACKAGE_SKIP_BUILD").is_none() {
        build(repo, nightly.then_some(version.as_str()))?;
    }
    let release = repo.join(TARGET_DIR).join("release");
    let app_exe = release.join("gravecord.exe");
    let stub_path = release.join("gravecord-setup.exe");
    let assets = repo.join("app/assets");
    let dist = if nightly {
        dist_dir(repo).join("nightly")
    } else {
        dist_dir(repo)
    };
    std::fs::create_dir_all(&dist)?;
    println!("version: {version}");

    let archive = Archive {
        version: version.clone(),
        entries: payload::collect(&app_exe, &assets)?,
    };
    let image = payload::append(&std::fs::read(&stub_path)?, &archive)?;
    let image = sign(
        &image,
        &version,
        if nightly { "nightly" } else { "stable" },
        &key,
    )?;
    let setup = dist.join("gravecord-setup.exe");
    write_replacing(&setup, &image)
        .with_context(|| format!("could not write {}", setup.display()))?;
    println!(
        "setup: {} ({:.1} MiB; app {:.1} MiB, stub {:.1} MiB)",
        setup.display(),
        image.len() as f64 / 1048576.0,
        std::fs::metadata(&app_exe)?.len() as f64 / 1048576.0,
        std::fs::metadata(&stub_path)?.len() as f64 / 1048576.0,
    );

    if nightly {
        println!(
            "publish: the `nightly` pre-release of the update repository, its name `{version}`, gravecord-setup.exe as \
             its only file"
        );
    } else {
        println!("publish: a release tagged v{version} with gravecord-setup.exe as its only file");
    }

    // The portable folder. The exe may be locked because the owner has it open; the setup is written either way.
    match refresh_portable(&dist, &app_exe, &assets)? {
        Portable::Refreshed => println!("portable: {}", dist.join("gravecord.exe").display()),
        Portable::Beside(next) => println!(
            "portable NOT refreshed: complete new build (exe and app\\assets) written to {}",
            next.display()
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn nightly_stamps_are_utc_dates_that_sort_as_numbers() {
        assert_eq!(nightly_stamp(0), "197001010000");
        // 2026-11-17 18:30 UTC.
        assert_eq!(nightly_stamp(1_794_940_200), "202611171830");
        // 2024-02-29 23:59 UTC, a leap day.
        assert_eq!(nightly_stamp(1_709_251_140), "202402292359");
        let a: u64 = nightly_stamp(1_794_940_200).parse().unwrap();
        let b: u64 = nightly_stamp(1_794_940_260).parse().unwrap();
        assert!(b > a);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gc-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn source(base: &Path, version: &str) -> (PathBuf, PathBuf) {
        let exe = base.join("new.exe");
        std::fs::write(&exe, format!("exe-{version}")).unwrap();
        let assets = base.join("src-assets");
        std::fs::create_dir_all(assets.join("icons")).unwrap();
        std::fs::write(assets.join("icons/a.svg"), format!("asset-{version}")).unwrap();
        (exe, assets)
    }

    #[test]
    fn a_setup_is_signed_with_a_manifest_of_its_own_bytes_and_the_key_is_kept() {
        use ed25519_dalek::{Signature, Verifier};
        let base = scratch("sign");
        let key = signing_key(&base.join("keys")).unwrap();
        // The same seed comes back; it is never replaced.
        let again = signing_key(&base.join("keys")).unwrap();
        assert_eq!(key.to_bytes(), again.to_bytes());
        let body = b"stub and payload".to_vec();
        let signed = sign(&body, "0.0.0-nightly.202611171830", "nightly", &key).unwrap();
        let (b, manifest, signature) = dc_release::split(&signed).unwrap();
        assert_eq!(b, body.as_slice());
        key.verifying_key()
            .verify(manifest, &Signature::from_bytes(signature))
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(manifest).unwrap();
        assert_eq!(manifest["format"], 1);
        assert_eq!(manifest["version"], "0.0.0-nightly.202611171830");
        assert_eq!(manifest["package"]["size"], body.len());
        assert_eq!(manifest["package"]["sha256"].as_str().unwrap().len(), 64);
        // The key source holds the public half, byte for byte.
        let source = key_source(key.verifying_key().as_bytes());
        let first = format!("0x{:02x}", key.verifying_key().as_bytes()[0]);
        assert!(source.contains("pub const PUBLIC_KEY: [u8; 32] = ["));
        assert!(source.contains(&first));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The committed key source is exactly what `cargo xtask package` writes for its key, so a package run never
    /// rewrites it (and rustfmt leaves it as it is).
    #[test]
    fn the_committed_key_source_is_the_generated_one() {
        let text = include_str!("../../app/src/release_key.rs").replace("\r\n", "\n");
        let bytes: Vec<u8> = text
            .split("PUBLIC_KEY")
            .nth(1)
            .unwrap()
            .split("0x")
            .skip(1)
            .map(|hex| u8::from_str_radix(&hex[..2], 16).unwrap())
            .collect();
        let key: [u8; 32] = bytes.try_into().unwrap();
        assert_eq!(key_source(&key), text);
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&key).is_ok());
    }

    #[test]
    fn an_unlocked_portable_folder_is_refreshed_as_a_whole() {
        let base = scratch("ok");
        let dist = base.join("dist");
        std::fs::create_dir_all(dist.join("app/assets")).unwrap();
        std::fs::write(dist.join("gravecord.exe"), "exe-old").unwrap();
        std::fs::write(dist.join("app/assets/stale.svg"), "x").unwrap();
        let (exe, assets) = source(&base, "new");
        assert_eq!(
            refresh_portable(&dist, &exe, &assets).unwrap(),
            Portable::Refreshed
        );
        assert_eq!(
            std::fs::read(dist.join("gravecord.exe")).unwrap(),
            b"exe-new"
        );
        assert_eq!(
            std::fs::read(dist.join("app/assets/icons/a.svg")).unwrap(),
            b"asset-new"
        );
        assert!(!dist.join("app/assets/stale.svg").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_locked_exe_leaves_the_folder_alone_and_writes_a_complete_next_folder() {
        let base = scratch("locked");
        let dist = base.join("dist");
        std::fs::create_dir_all(dist.join("app/assets/icons")).unwrap();
        std::fs::write(dist.join("gravecord.exe"), "exe-old").unwrap();
        std::fs::write(dist.join("app/assets/icons/a.svg"), "asset-old").unwrap();
        let (exe, assets) = source(&base, "new");
        // Held open without sharing: replacing it fails, the way a running exe does.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dist.join("gravecord.exe"))
            .unwrap();
        let outcome = refresh_portable(&dist, &exe, &assets).unwrap();
        drop(held);
        let next = dist.join("gravecord-next");
        assert_eq!(outcome, Portable::Beside(next.clone()));
        // The old folder is exactly as it was: old exe, old assets.
        assert_eq!(
            std::fs::read(dist.join("gravecord.exe")).unwrap(),
            b"exe-old"
        );
        assert_eq!(
            std::fs::read(dist.join("app/assets/icons/a.svg")).unwrap(),
            b"asset-old"
        );
        // The new folder is complete and consistent.
        assert_eq!(
            std::fs::read(next.join("gravecord.exe")).unwrap(),
            b"exe-new"
        );
        assert_eq!(
            std::fs::read(next.join("app/assets/icons/a.svg")).unwrap(),
            b"asset-new"
        );
        assert!(!dist.join("gravecord.exe.tmp").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
