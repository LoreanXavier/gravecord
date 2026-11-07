//! `cargo xtask client prepare|launch|check`
//!
//! A clean, frozen copy of the installed official client for captures, in `design/client/`. It has its own profile,
//! the official bootstrap instead of any client mod, no updater, and a CDP port on loopback.
//!
//! The copy shares Windows registry names with the real install (`Run\Discord`), so `prepare` records those values
//! and `check` fails if they changed. Run `check` after every capture session.

use crate::snapshot::{copy_tree, default_install, newest_app_dir};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const CDP_PORT: u16 = 9230;

const REGISTRY_VALUES: [(&str, Option<&str>); 4] = [
    (
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        Some("Discord"),
    ),
    (
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
        Some("Discord"),
    ),
    (r"HKCU\Software\Classes\discord\shell\open\command", None),
    (
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Discord",
        Some("DisplayVersion"),
    ),
];

struct Layout {
    root: PathBuf,
    app: PathBuf,
    modules: PathBuf,
    data: PathBuf,
    baseline: PathBuf,
}

fn layout(repo: &Path) -> Layout {
    let root = repo.join("design").join("client");
    let app = root.join("app");
    Layout {
        modules: app.join("local_modules"),
        data: root.join("data"),
        baseline: root.join("registry-baseline.txt"),
        app,
        root,
    }
}

fn registry_state() -> Result<String> {
    let mut state = String::new();
    for (key, value) in REGISTRY_VALUES {
        let mut query = Command::new("reg");
        query.args(["query", key]);
        match value {
            Some(name) => query.args(["/v", name]),
            None => query.arg("/ve"),
        };
        let output = query.output().context("running reg query")?;
        state.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    Ok(state.replace('\r', ""))
}

/// Merges the switches that keep the copy frozen into its `build_info.json`.
fn frozen_build_info(mut info: Value, modules: &Path) -> Result<Value> {
    let object = info
        .as_object_mut()
        .context("build_info is not an object")?;
    object.insert("disableUpdater".into(), json!(true));
    object.insert("localModulesRoot".into(), json!(modules));
    Ok(info)
}

fn prepare(repo: &Path) -> Result<()> {
    let paths = layout(repo);
    if is_running(&paths)? {
        bail!("the clean client is running, close it first");
    }
    let installed = newest_app_dir(&default_install()?)?;

    if paths.app.exists() {
        fs::remove_dir_all(&paths.app)?;
    }
    fs::create_dir_all(&paths.app)?;
    for entry in fs::read_dir(&installed)? {
        let entry = entry?;
        let name = entry.file_name();
        let target = paths.app.join(&name);
        if name == "modules" || name == "resources" {
            continue;
        } else if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }

    // Installed modules are `<name>-<version>/<name>`; `localModulesRoot` wants `<name>` directly.
    fs::create_dir_all(&paths.modules)?;
    for entry in fs::read_dir(installed.join("modules"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((module, _)) = name.rsplit_once('-') else {
            continue;
        };
        let inner = entry.path().join(module);
        if inner.is_dir() {
            copy_tree(&inner, &paths.modules.join(module))?;
        }
    }

    // `_app.asar` is the official bootstrap whenever a client mod has taken the `app.asar` name.
    let source = installed.join("resources");
    let resources = paths.app.join("resources");
    fs::create_dir_all(resources.join("bootstrap"))?;
    let official = ["_app.asar", "app.asar"]
        .map(|name| source.join(name))
        .into_iter()
        .find(|path| path.is_file())
        .context("no official bootstrap archive in the install")?;
    fs::copy(official, resources.join("app.asar"))?;
    fs::copy(
        source.join("bootstrap").join("manifest.json"),
        resources.join("bootstrap").join("manifest.json"),
    )?;
    let info: Value = serde_json::from_slice(&fs::read(source.join("build_info.json"))?)?;
    fs::write(
        resources.join("build_info.json"),
        serde_json::to_vec_pretty(&frozen_build_info(info, &paths.modules)?)?,
    )?;

    let profile = paths.data.join("discord");
    fs::create_dir_all(&profile)?;
    let settings = profile.join("settings.json");
    if !settings.exists() {
        let values = json!({
            "SKIP_HOST_UPDATE": true,
            "SKIP_MODULE_UPDATE": true,
            "DANGEROUS_ENABLE_DEVTOOLS_ONLY_ENABLE_IF_YOU_KNOW_WHAT_YOURE_DOING": true,
        });
        fs::write(settings, serde_json::to_vec_pretty(&values)?)?;
    }

    if !paths.baseline.exists() {
        fs::write(&paths.baseline, registry_state()?)?;
    }
    println!("prepared {}", paths.root.display());
    Ok(())
}

fn is_running(paths: &Layout) -> Result<bool> {
    let exe = paths.app.join("Discord.exe");
    let filter = exe.display().to_string().replace('\\', "\\\\");
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter \"ExecutablePath='{filter}'\" | Measure-Object).Count"),
        ])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim() != "0")
}

fn launch(repo: &Path) -> Result<()> {
    let paths = layout(repo);
    if !paths.baseline.exists() {
        bail!("run `cargo xtask client prepare` first");
    }
    if is_running(&paths)? {
        println!("already running, CDP on 127.0.0.1:{CDP_PORT}");
        return Ok(());
    }
    let mut command = Command::new(paths.app.join("Discord.exe"));
    command
        .env("DISCORD_USER_DATA_DIR", &paths.data)
        .arg(format!("--remote-debugging-port={CDP_PORT}"))
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--multi-instance")
        .current_dir(&paths.app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x8;
        command.creation_flags(DETACHED_PROCESS);
    }
    let child = command.spawn().context("starting the clean client")?;
    println!("started pid {}, CDP on 127.0.0.1:{CDP_PORT}", child.id());
    Ok(())
}

fn check(repo: &Path) -> Result<()> {
    let paths = layout(repo);
    let baseline = fs::read_to_string(&paths.baseline).context("no baseline, run prepare first")?;
    let current = registry_state()?;
    if baseline != current {
        bail!("real Discord registry values changed\n--- baseline\n{baseline}\n--- now\n{current}");
    }
    println!("registry values match the baseline");
    Ok(())
}

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    match args {
        ["prepare"] => prepare(repo),
        ["launch"] => launch(repo),
        ["check"] => check(repo),
        other => bail!("usage: cargo xtask client prepare|launch|check, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_build_info_keeps_the_official_fields() {
        let info = json!({"releaseChannel": "stable", "version": "1.0.9267"});
        let frozen = frozen_build_info(info, Path::new("D:/m")).unwrap();
        assert_eq!(frozen["releaseChannel"], "stable");
        assert_eq!(frozen["version"], "1.0.9267");
        assert_eq!(frozen["disableUpdater"], true);
        assert_eq!(frozen["localModulesRoot"], "D:/m");
    }

    #[test]
    fn frozen_build_info_refuses_a_non_object() {
        assert!(frozen_build_info(json!([]), Path::new("D:/m")).is_err());
    }
}
