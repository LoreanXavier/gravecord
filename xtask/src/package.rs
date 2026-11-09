//! `cargo xtask package`: builds the release app and the setup stub, appends the payload and writes
//! `gravecord-dist/gravecord-setup.exe`, then refreshes the portable folder next to it.
//!
//! The release build uses the static C runtime, so neither exe imports VCRUNTIME140. It goes to its own target folder
//! so the flag change never invalidates the normal release cache.

use anyhow::{Context, Result, bail};
use dc_setup::payload::{self, Archive};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET_DIR: &str = "target/dist";

fn dist_dir(repo: &Path) -> PathBuf {
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

pub fn run(repo: &Path) -> Result<()> {
    build(repo)?;
    let release = repo.join(TARGET_DIR).join("release");
    let app_exe = release.join("gravecord.exe");
    let stub_path = release.join("gravecord-setup.exe");
    let assets = repo.join("app/assets");
    let dist = dist_dir(repo);
    std::fs::create_dir_all(&dist)?;

    let archive = Archive {
        version: app_version(repo)?,
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

    // The portable folder. The exe may be locked because the owner has it open; the setup is written either way.
    match std::fs::copy(&app_exe, dist.join("gravecord.exe")) {
        Ok(_) => println!("portable: {}", dist.join("gravecord.exe").display()),
        Err(error) => {
            let alternate = dist.join("gravecord-next.exe");
            write_replacing(&alternate, &std::fs::read(&app_exe)?)?;
            println!(
                "portable gravecord.exe NOT refreshed ({error}); replacement written to {}",
                alternate.display()
            );
        }
    }
    mirror(&assets, &dist.join("app").join("assets"))?;
    Ok(())
}
