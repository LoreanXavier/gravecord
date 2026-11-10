//! `cargo xtask package`: builds the release app and the setup stub, appends the payload and writes
//! `gravecord-dist/gravecord-setup.exe`, then refreshes the portable folder next to it.
//!
//! Two environment variables serve tests of the packaging itself: `GRAVECORD_DIST_DIR` writes into another folder
//! instead of `gravecord-dist`, and `GRAVECORD_PACKAGE_SKIP_BUILD=1` reuses the release build already in
//! `target/dist`.
//!
//! Next to the setup it writes `update.json`, the feed the app's updater reads (`app/src/updater.rs`): version, setup
//! file name, size and SHA-256 of the exact file just written. Publishing a release is copying those two files.
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

fn build(repo: &Path) -> Result<()> {
    let status = Command::new("cargo")
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

/// The update feed for `setup`: the version from the app's manifest, the size and SHA-256 of the file's bytes. The setup
/// address is relative, so it resolves next to the feed wherever the two files are published.
fn update_feed(version: &str, setup_name: &str, setup: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(setup)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let feed = serde_json::json!({
        "version": version,
        "url": setup_name,
        "size": setup.len(),
        "sha256": hash,
    });
    serde_json::to_string_pretty(&feed).expect("a feed is valid JSON")
        + "
"
}

/// Writes the feed next to the setup, through a temporary file like the setup itself.
fn write_update_feed(dist: &Path, version: &str, setup: &[u8]) -> Result<PathBuf> {
    let path = dist.join("update.json");
    let temp = dist.join("update.json.tmp");
    std::fs::write(&temp, update_feed(version, "gravecord-setup.exe", setup))?;
    std::fs::rename(&temp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(path)
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

pub fn run(repo: &Path) -> Result<()> {
    if std::env::var_os("GRAVECORD_PACKAGE_SKIP_BUILD").is_none() {
        build(repo)?;
    }
    let release = repo.join(TARGET_DIR).join("release");
    let app_exe = release.join("gravecord.exe");
    let stub_path = release.join("gravecord-setup.exe");
    let assets = repo.join("app/assets");
    let dist = dist_dir(repo);
    std::fs::create_dir_all(&dist)?;

    let version = app_version(repo)?;
    let archive = Archive {
        version: version.clone(),
        entries: payload::collect(&app_exe, &assets)?,
    };
    let image = payload::append(&std::fs::read(&stub_path)?, &archive)?;
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

    let feed = write_update_feed(&dist, &version, &image)?;
    println!(
        "update feed: {} (publish it with the setup)",
        feed.display()
    );

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
    fn the_update_feed_describes_the_exact_setup_bytes() {
        use sha2::{Digest, Sha256};
        let base = scratch("feed");
        let setup = b"setup image bytes";
        let path = write_update_feed(&base, "1.2.3", setup).unwrap();
        assert_eq!(path, base.join("update.json"));
        let feed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(feed["version"], "1.2.3");
        assert_eq!(feed["url"], "gravecord-setup.exe");
        assert_eq!(feed["size"], setup.len());
        let expected: String = Sha256::digest(setup)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(feed["sha256"], expected.as_str());
        assert_eq!(feed["sha256"].as_str().unwrap().len(), 64);
        assert!(!base.join("update.json.tmp").exists());
        // A second write replaces the first.
        write_update_feed(&base, "1.2.4", b"other").unwrap();
        let feed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(feed["version"], "1.2.4");
        let _ = std::fs::remove_dir_all(&base);
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
