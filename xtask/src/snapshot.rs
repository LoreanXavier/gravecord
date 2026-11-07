//! `cargo xtask snapshot pin [client_build_number] [--install <dir>]`
//!
//! Reads the installed desktop package and writes `design/snapshot/BUILD`, `desktop/` and `desktop-manifest.tsv`.
//! The install is only read. The web bundle is a separate step and is not downloaded here.

use crate::asar;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `app-1.0.9267` -> `[1, 0, 9267]`, so versions compare numerically.
fn app_version(name: &str) -> Option<Vec<u32>> {
    name.strip_prefix("app-")?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

pub fn newest_app_dir(install: &Path) -> Result<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in fs::read_dir(install).with_context(|| format!("reading {}", install.display()))? {
        let entry = entry?;
        let Some(version) = entry.file_name().to_str().and_then(app_version) else {
            continue;
        };
        if best.as_ref().is_none_or(|(have, _)| version > *have) {
            best = Some((version, entry.path()));
        }
    }
    match best {
        Some((_, path)) => Ok(path),
        None => bail!("no app-* directory in {}", install.display()),
    }
}

pub fn default_install() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(local).join("Discord"))
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            files_under(&entry.path(), out)?;
        } else {
            out.push(entry.path());
        }
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let size = io::copy(&mut file.by_ref(), &mut hasher)?;
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(hex, "{byte:02x}")?;
    }
    Ok((hex, size))
}

struct Args<'a> {
    build: Option<&'a str>,
    install: Option<PathBuf>,
}

fn parse_args<'a>(args: &[&'a str]) -> Result<Args<'a>> {
    let mut parsed = Args {
        build: None,
        install: None,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--install" => {
                let dir = args.next().context("--install needs a directory")?;
                parsed.install = Some(PathBuf::from(dir));
            }
            build if build.chars().all(|c| c.is_ascii_digit()) => parsed.build = Some(build),
            other => bail!("unknown argument {other:?}"),
        }
    }
    Ok(parsed)
}

pub fn pin(root: &Path, args: &[&str]) -> Result<()> {
    let args = parse_args(args)?;
    let install = args.install.map_or_else(default_install, Ok)?;
    let app = newest_app_dir(&install)?;
    let resources = app.join("resources");

    let info: Value = serde_json::from_slice(&fs::read(resources.join("build_info.json"))?)?;
    let field = |key: &str| info.get(key).and_then(Value::as_str).unwrap_or("unknown");

    // With a client mod installed, `app.asar` is the mod's directory and the official bootstrap is `_app.asar`.
    let injected = resources.join("app.asar").is_dir();
    let bootstrap = if injected { "_app.asar" } else { "app.asar" };

    let snapshot = root.join("design").join("snapshot");
    let desktop = snapshot.join("desktop");
    if desktop.exists() {
        fs::remove_dir_all(&desktop)?;
    }
    fs::create_dir_all(&desktop)?;

    let mut report = asar::extract(&resources.join(bootstrap), &desktop.join("bootstrap"))?;
    println!("bootstrap {bootstrap}: {} files", report.files);

    copy_tree(&app.join("modules"), &desktop.join("modules"))?;
    let mut copied = Vec::new();
    files_under(&desktop.join("modules"), &mut copied)?;

    let mut modules = Vec::new();
    for entry in fs::read_dir(desktop.join("modules"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some((module, version)) = name.rsplit_once('-') {
            modules.push(format!("module.{module}={version}"));
        }
    }
    modules.sort();

    for archive in copied
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "asar"))
    {
        let out = archive.with_extension("extracted");
        let extracted = asar::extract(archive, &out)?;
        println!("{}: {} files", archive.display(), extracted.files);
        report.missing_unpacked.extend(extracted.missing_unpacked);
    }

    let mut all = Vec::new();
    files_under(&desktop, &mut all)?;
    all.sort();
    let mut manifest = String::from("sha256\tsize\tpath\n");
    for path in &all {
        let (hash, size) = sha256(path)?;
        let rel = path
            .strip_prefix(&desktop)?
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(manifest, "{hash}\t{size}\t{rel}")?;
    }
    fs::write(snapshot.join("desktop-manifest.tsv"), manifest)?;

    let (exe_hash, _) = sha256(&app.join("Discord.exe"))?;
    let captured = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut build = String::new();
    writeln!(build, "captured_unix={captured}")?;
    writeln!(build, "release_channel={}", field("releaseChannel"))?;
    writeln!(build, "desktop_version={}", field("version"))?;
    writeln!(build, "sentry_release={}", field("sentryRelease"))?;
    writeln!(build, "sentry_dist={}", field("sentryDist"))?;
    writeln!(
        build,
        "client_build_number={}",
        args.build.unwrap_or("pending")
    )?;
    writeln!(build, "desktop_exe_sha256={exe_hash}")?;
    writeln!(build, "client_modified={injected}")?;
    writeln!(build, "bootstrap_source={bootstrap}")?;
    for module in &modules {
        writeln!(build, "{module}")?;
    }
    fs::write(snapshot.join("BUILD"), build)?;

    println!(
        "{} modules, {} files in the manifest",
        modules.len(),
        all.len()
    );
    if !report.missing_unpacked.is_empty() {
        println!(
            "unpacked files not found: {}",
            report.missing_unpacked.join(", ")
        );
    }
    println!("wrote {}", snapshot.join("BUILD").display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_versions_compare_numerically() {
        assert!(app_version("app-1.0.10000") > app_version("app-1.0.9267"));
        assert_eq!(app_version("packages"), None);
        assert_eq!(app_version("app-1.0.x"), None);
    }

    #[test]
    fn parses_build_number_and_install_in_any_order() {
        let args = parse_args(&["--install", "D:/x", "512345"]).unwrap();
        assert_eq!(args.build, Some("512345"));
        assert_eq!(args.install, Some(PathBuf::from("D:/x")));
        assert!(parse_args(&["--nope"]).is_err());
        assert!(parse_args(&["--install"]).is_err());
    }
}
