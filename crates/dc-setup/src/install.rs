//! Installing and removing. Everything the installer creates is recorded in `install.manifest`, and the uninstaller
//! removes exactly that, so a folder chosen by hand never loses files that were not ours.

use crate::paths::{self, MANIFEST, UNINSTALLER};
use crate::payload::Archive;
use crate::winsys;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub struct Plan {
    pub dir: PathBuf,
    pub desktop_shortcut: bool,
    /// False in tests: no shortcuts, no registry key.
    pub register: bool,
    pub reg_key: String,
    pub shortcut_name: String,
}

impl Plan {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            desktop_shortcut: false,
            register: true,
            reg_key: paths::DEFAULT_KEY.into(),
            shortcut_name: paths::DEFAULT_SHORTCUT.into(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<String>,
    pub reg_keys: Vec<String>,
    pub shortcuts: Vec<PathBuf>,
}

impl Manifest {
    pub fn render(&self) -> String {
        let mut out = format!("version {}\n", self.version);
        for file in &self.files {
            out.push_str(&format!("file {file}\n"));
        }
        for key in &self.reg_keys {
            out.push_str(&format!("reg {key}\n"));
        }
        for link in &self.shortcuts {
            out.push_str(&format!("lnk {}\n", link.display()));
        }
        out
    }

    /// Lines that do not parse, or name an unsafe file, are skipped: a damaged record must never widen the removal.
    pub fn parse(text: &str) -> Self {
        let mut manifest = Self::default();
        for line in text.lines() {
            let Some((kind, rest)) = line.split_once(' ') else {
                continue;
            };
            match kind {
                "version" => manifest.version = rest.to_owned(),
                "file" if paths::safe_relative(rest).is_some() => {
                    manifest.files.push(rest.to_owned())
                }
                "reg" if paths::plain_name(rest) => manifest.reg_keys.push(rest.to_owned()),
                "lnk" if rest.ends_with(".lnk") => manifest.shortcuts.push(rest.into()),
                _ => {}
            }
        }
        manifest
    }
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    std::fs::read_to_string(dir.join(MANIFEST))
        .ok()
        .map(|text| Manifest::parse(&text))
}

/// Total size of the payload in KiB, for Apps & features.
pub fn size_kib(archive: &Archive) -> u32 {
    (archive
        .entries
        .iter()
        .map(|e| e.data.len() as u64)
        .sum::<u64>()
        / 1024) as u32
}

/// Writes the files, the uninstaller, the record, the shortcuts and the registry key. `progress` gets 0..=1000.
pub fn install(
    archive: &Archive,
    stub: &[u8],
    plan: &Plan,
    progress: &mut dyn FnMut(u32, &str),
) -> Result<()> {
    paths::check_install_dir(&plan.dir).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(&plan.dir)
        .with_context(|| format!("Could not create {}", plan.dir.display()))?;

    let total: u64 = archive
        .entries
        .iter()
        .map(|e| e.data.len() as u64)
        .sum::<u64>()
        + stub.len() as u64;
    let mut done = 0u64;
    let mut tick = |amount: u64, label: &str, progress: &mut dyn FnMut(u32, &str)| {
        done += amount;
        progress((done * 900 / total.max(1)) as u32, label);
    };

    let old = read_manifest(&plan.dir);
    let mut manifest = Manifest {
        version: archive.version.clone(),
        ..Manifest::default()
    };
    for entry in &archive.entries {
        let target = plan
            .dir
            .join(paths::safe_relative(&entry.path).context("unsafe path")?);
        write_atomic(&target, &entry.data)?;
        manifest.files.push(entry.path.clone());
        tick(entry.data.len() as u64, "Copying files", progress);
    }
    write_atomic(&plan.dir.join(UNINSTALLER), stub)?;
    manifest.files.push(UNINSTALLER.into());
    tick(stub.len() as u64, "Copying files", progress);

    // Files of an earlier version that this one no longer ships.
    if let Some(old) = old {
        for file in old.files.iter().filter(|f| !manifest.files.contains(f)) {
            if let Some(relative) = paths::safe_relative(file) {
                let _ = std::fs::remove_file(plan.dir.join(relative));
            }
        }
        prune_empty(&plan.dir, &old.files);
    }

    if plan.register {
        progress(920, "Adding shortcuts");
        let exe = plan.dir.join("gravecord.exe");
        let start_menu = winsys::start_menu_dir()?.join(format!("{}.lnk", plan.shortcut_name));
        winsys::create_shortcut(&start_menu, &exe, &plan.dir)?;
        manifest.shortcuts.push(start_menu);
        if plan.desktop_shortcut {
            let desktop = winsys::desktop_dir()?.join(format!("{}.lnk", plan.shortcut_name));
            winsys::create_shortcut(&desktop, &exe, &plan.dir)?;
            manifest.shortcuts.push(desktop);
        }
        progress(960, "Registering the app");
        winsys::register_uninstall(
            &plan.reg_key,
            &winsys::UninstallEntry {
                version: &archive.version,
                dir: &plan.dir,
                size_kib: size_kib(archive),
            },
        )?;
        manifest.reg_keys.push(plan.reg_key.clone());
    }
    write_atomic(&plan.dir.join(MANIFEST), manifest.render().as_bytes())?;
    progress(1000, "Done");
    Ok(())
}

fn write_atomic(target: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = target.file_name().context("no file name")?.to_owned();
    name.push(".gc-new");
    let temp = target.with_file_name(name);
    std::fs::write(&temp, data).with_context(|| format!("Could not write {}", target.display()))?;
    std::fs::rename(&temp, target).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        anyhow::anyhow!("Could not replace {}: {error}", target.display())
    })
}

/// Removes the (now empty) folders that held `files`, deepest first. Never touches `root` itself.
fn prune_empty(root: &Path, files: &[String]) {
    let mut dirs: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| paths::safe_relative(file))
        .flat_map(|relative| {
            relative
                .ancestors()
                .skip(1)
                .filter(|a| !a.as_os_str().is_empty())
                .map(|a| root.join(a))
                .collect::<Vec<_>>()
        })
        .collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(&dir);
    }
}

pub struct Removal {
    pub purge_data: bool,
    pub data_dir: PathBuf,
}

/// Removes what the record lists. The running uninstaller itself is left for `winsys::delete_self_later`.
pub fn uninstall(dir: &Path, removal: &Removal) -> Result<()> {
    let Some(manifest) = read_manifest(dir) else {
        bail!(
            "{} is not a gravecord install (no {MANIFEST}).",
            dir.display()
        );
    };
    let me = std::env::current_exe().ok();
    let mut leftover = Vec::new();
    for file in &manifest.files {
        let Some(relative) = paths::safe_relative(file) else {
            continue;
        };
        let path = dir.join(relative);
        if me.as_deref() == Some(path.as_path()) {
            continue;
        }
        if path.exists() && std::fs::remove_file(&path).is_err() {
            leftover.push(file.clone());
        }
    }
    if !leftover.is_empty() {
        bail!(
            "Could not remove: {} (is gravecord still running?)",
            leftover.join(", ")
        );
    }
    prune_empty(dir, &manifest.files);
    for link in &manifest.shortcuts {
        let _ = std::fs::remove_file(link);
    }
    for key in &manifest.reg_keys {
        winsys::unregister_uninstall(key)?;
    }
    if removal.purge_data {
        remove_data(&removal.data_dir)?;
    }
    let _ = std::fs::remove_file(dir.join(MANIFEST));
    Ok(())
}

fn remove_data(dir: &Path) -> Result<()> {
    if dir.file_name().is_some_and(|name| name == "gravecord") && dir.is_dir() {
        std::fs::remove_dir_all(dir)
            .with_context(|| format!("Could not remove {}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::Entry;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gc-setup-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn archive(files: &[&str]) -> Archive {
        Archive {
            version: "1.0.0".into(),
            entries: files
                .iter()
                .map(|path| Entry {
                    path: (*path).into(),
                    data: path.as_bytes().to_vec(),
                })
                .collect(),
        }
    }

    #[test]
    fn manifest_round_trips_and_skips_unsafe_lines() {
        let manifest = Manifest {
            version: "2".into(),
            files: vec!["gravecord.exe".into(), "app/assets/a.svg".into()],
            reg_keys: vec!["gravecord".into()],
            shortcuts: vec![PathBuf::from(r"C:\x\gravecord.lnk")],
        };
        assert_eq!(Manifest::parse(&manifest.render()), manifest);
        let hostile = Manifest::parse(
            "file ../../Windows/x\nfile C:/a\nreg a\\b\nlnk C:\\a.exe\nnonsense\nfile ok/y",
        );
        assert_eq!(hostile.files, ["ok/y"]);
        assert!(hostile.reg_keys.is_empty() && hostile.shortcuts.is_empty());
    }

    #[test]
    fn install_update_and_removal_of_files() {
        let dir = scratch("flow").join("gravecord");
        let mut plan = Plan::new(dir.clone());
        plan.register = false;
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep-me.txt"), "mine").unwrap();

        let mut last = 0;
        install(
            &archive(&[
                "gravecord.exe",
                "app/assets/icons/a.svg",
                "app/assets/old.svg",
            ]),
            b"stub",
            &plan,
            &mut |permille, _| last = permille,
        )
        .unwrap();
        assert_eq!(last, 1000);
        assert_eq!(
            std::fs::read(dir.join("app/assets/icons/a.svg")).unwrap(),
            b"app/assets/icons/a.svg"
        );
        assert_eq!(std::fs::read(dir.join(UNINSTALLER)).unwrap(), b"stub");

        // An update drops files the new version no longer ships, and leaves foreign files alone.
        install(
            &archive(&["gravecord.exe", "app/assets/icons/a.svg"]),
            b"stub",
            &plan,
            &mut |_, _| {},
        )
        .unwrap();
        assert!(!dir.join("app/assets/old.svg").exists());
        assert!(dir.join("keep-me.txt").exists());

        let removal = Removal {
            purge_data: false,
            data_dir: scratch("data").join("gravecord"),
        };
        uninstall(&dir, &removal).unwrap();
        // The test binary is not in the folder, so even the uninstaller copy is removed here.
        assert!(!dir.join("gravecord.exe").exists() && !dir.join("app").exists());
        assert!(dir.join("keep-me.txt").exists(), "foreign file survives");
        assert!(!dir.join(MANIFEST).exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn uninstall_refuses_a_folder_without_a_record() {
        let dir = scratch("norecord");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let removal = Removal {
            purge_data: true,
            data_dir: dir.join("gravecord"),
        };
        assert!(uninstall(&dir, &removal).is_err());
        assert!(dir.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_is_only_removed_when_asked_and_named_gravecord() {
        let base = scratch("purge");
        let data = base.join("gravecord");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("storage.json"), "{}").unwrap();
        remove_data(&base.join("other")).unwrap();
        assert!(data.exists());
        remove_data(&base).unwrap();
        assert!(base.exists(), "a folder not named gravecord is left alone");
        remove_data(&data).unwrap();
        assert!(!data.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_system_folders() {
        let mut plan = Plan::new(PathBuf::from(r"C:\Windows\gravecord"));
        plan.register = false;
        assert!(install(&archive(&["gravecord.exe"]), b"s", &plan, &mut |_, _| {}).is_err());
    }
}
