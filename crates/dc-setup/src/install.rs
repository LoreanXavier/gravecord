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

    /// The plan for `dir` that keeps what the install already there chose: its registry key, its shortcut name,
    /// its desktop shortcut and whether it was registered at all. With no record it is [`Plan::new`].
    /// `desktop_dir` is the Desktop folder, where a shortcut counts as the desktop one.
    ///
    /// The app's updater runs setup with the folder alone (`--silent --launch --dir`), so an update must not move
    /// an install made with `--reg-key`, `--shortcut-name`, `--desktop` or `--no-register` onto the defaults: that
    /// would leave its desktop shortcut out of the record, or point the default Apps & features entry and Start
    /// Menu shortcut at another install.
    pub fn keeping(dir: PathBuf, old: Option<&Manifest>, desktop_dir: Option<&Path>) -> Self {
        let mut plan = Self::new(dir);
        let Some(old) = old else {
            return plan;
        };
        let on_desktop = |link: &Path| {
            desktop_dir.is_some_and(|desktop| {
                link.parent()
                    .is_some_and(|parent| paths::same_path(parent, desktop))
            })
        };
        plan.register = !old.reg_keys.is_empty() || !old.shortcuts.is_empty();
        if let Some(key) = old.reg_keys.first() {
            plan.reg_key = key.clone();
        }
        plan.desktop_shortcut = old.shortcuts.iter().any(|link| on_desktop(link));
        // The Start Menu shortcut names the install; a record with only a desktop one still gives its name.
        let named = old
            .shortcuts
            .iter()
            .find(|link| !on_desktop(link))
            .or(old.shortcuts.first())
            .and_then(|link| link.file_stem())
            .and_then(|stem| stem.to_str())
            .filter(|stem| paths::plain_name(stem));
        if let Some(name) = named {
            plan.shortcut_name = name.to_owned();
        }
        plan
    }

    /// The plan of a silent run: [`Plan::keeping`], then what the command line says. `--no-register` turns the
    /// registration off; `--reg-key`, `--shortcut-name` and `--desktop` name what to create, so they turn it on.
    pub fn silent(
        dir: PathBuf,
        old: Option<&Manifest>,
        desktop_dir: Option<&Path>,
        options: &paths::Options,
    ) -> Self {
        let mut plan = Self::keeping(dir, old, desktop_dir);
        if let Some(name) = &options.reg_key {
            plan.reg_key = name.clone();
            plan.register = true;
        }
        if let Some(name) = &options.shortcut_name {
            plan.shortcut_name = name.clone();
            plan.register = true;
        }
        if options.desktop {
            plan.desktop_shortcut = true;
            plan.register = true;
        }
        if options.no_register {
            plan.register = false;
        }
        plan
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

const STAGED: &str = ".gc-new";
const BACKUP: &str = ".gc-old";

fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    target.with_file_name(name)
}

/// One file of the update: written next to its target as `<name>.gc-new`, then swapped in.
struct Staged {
    target: PathBuf,
    new: PathBuf,
    backup: PathBuf,
}

/// Writes every file next to its target and reads each one back. Nothing existing is touched; on any failure all the
/// staged files are removed again.
fn stage(files: &[(PathBuf, &[u8])], on_written: &mut dyn FnMut(u64)) -> Result<Vec<Staged>> {
    let mut staged: Vec<Staged> = Vec::new();
    let result = (|| -> Result<()> {
        for (target, data) in files {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let item = Staged {
                target: target.clone(),
                new: sibling(target, STAGED),
                backup: sibling(target, BACKUP),
            };
            // Listed before it is written, so a failed write is cleaned up as well.
            let new = item.new.clone();
            staged.push(item);
            std::fs::write(&new, data)
                .with_context(|| format!("Could not write {}", target.display()))?;
            let back = std::fs::read(&new)
                .with_context(|| format!("Could not verify {}", target.display()))?;
            anyhow::ensure!(
                back == *data,
                "{} was not written correctly (disk full?)",
                target.display()
            );
            on_written(data.len() as u64);
        }
        Ok(())
    })();
    if let Err(error) = result {
        discard(&staged);
        return Err(error);
    }
    Ok(staged)
}

fn discard(staged: &[Staged]) {
    for item in staged {
        let _ = std::fs::remove_file(&item.new);
    }
}

/// Swaps the staged files in, in order (the exe first). Each old file is renamed to `<name>.gc-old` before the new one
/// takes its place; if any step fails, the steps already done are undone and the old install is what remains. A crash
/// in between leaves a mix that running setup again completes, because every step starts from whatever is there.
fn commit(staged: &[Staged]) -> Result<()> {
    commit_with(staged, &mut |_| Ok(()))
}

fn commit_with(
    staged: &[Staged],
    before_each: &mut dyn FnMut(usize) -> std::io::Result<()>,
) -> Result<()> {
    let mut done: Vec<(&Staged, bool)> = Vec::new();
    for (index, item) in staged.iter().enumerate() {
        let step = (|| -> std::io::Result<bool> {
            before_each(index)?;
            let had_old = item.target.exists();
            if had_old {
                let _ = std::fs::remove_file(&item.backup);
                std::fs::rename(&item.target, &item.backup)?;
            }
            if let Err(error) = std::fs::rename(&item.new, &item.target) {
                if had_old {
                    let _ = std::fs::rename(&item.backup, &item.target);
                }
                return Err(error);
            }
            Ok(had_old)
        })();
        match step {
            Ok(had_old) => done.push((item, had_old)),
            Err(error) => {
                for (undo, had_old) in done.into_iter().rev() {
                    if had_old {
                        let _ = std::fs::rename(&undo.backup, &undo.target);
                    } else {
                        let _ = std::fs::remove_file(&undo.target);
                    }
                }
                discard(staged);
                return Err(anyhow::anyhow!(
                    "Could not replace {}: {error}. Nothing was changed; close gravecord and run setup again.",
                    item.target.display()
                ));
            }
        }
    }
    for item in staged {
        let _ = std::fs::remove_file(&item.backup);
    }
    Ok(())
}

/// Removes `.gc-new` and `.gc-old` files an interrupted update left beside the files of `names`.
fn sweep_leftovers(dir: &Path, names: &[String]) {
    for name in names {
        if let Some(relative) = paths::safe_relative(name) {
            let target = dir.join(relative);
            let _ = std::fs::remove_file(sibling(&target, STAGED));
            let _ = std::fs::remove_file(sibling(&target, BACKUP));
        }
    }
}

/// Writes the files, the uninstaller, the record, the shortcuts and the registry key. `progress` gets 0..=1000.
///
/// The files are staged beside their targets and verified first, then swapped in with the exe first and the record
/// last, so a failure leaves the previous install intact (see `commit`).
pub fn install(
    archive: &Archive,
    stub: &[u8],
    plan: &Plan,
    progress: &mut dyn FnMut(u32, &str),
) -> Result<()> {
    paths::check_install_dir(&plan.dir).map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(&plan.dir)
        .with_context(|| format!("Could not create {}", plan.dir.display()))?;

    let old = read_manifest(&plan.dir);
    let mut manifest = Manifest {
        version: archive.version.clone(),
        ..Manifest::default()
    };
    // The exe goes first in the swap; the record is added last.
    let mut files: Vec<(PathBuf, &[u8])> = Vec::new();
    let mut ordered: Vec<&crate::payload::Entry> = archive.entries.iter().collect();
    ordered.sort_by_key(|entry| entry.path != "gravecord.exe");
    for entry in ordered {
        let target = plan
            .dir
            .join(paths::safe_relative(&entry.path).context("unsafe path")?);
        files.push((target, &entry.data));
        manifest.files.push(entry.path.clone());
    }
    files.push((plan.dir.join(UNINSTALLER), stub));
    manifest.files.push(UNINSTALLER.into());

    // The shortcut and key names are known before anything is written, so the record can ship with the files.
    let mut start_menu = None;
    let mut desktop = None;
    if plan.register {
        let link = winsys::start_menu_dir()?.join(format!("{}.lnk", plan.shortcut_name));
        manifest.shortcuts.push(link.clone());
        start_menu = Some(link);
        if plan.desktop_shortcut {
            let link = winsys::desktop_dir()?.join(format!("{}.lnk", plan.shortcut_name));
            manifest.shortcuts.push(link.clone());
            desktop = Some(link);
        }
        manifest.reg_keys.push(plan.reg_key.clone());
    }
    let record = manifest.render().into_bytes();
    files.push((plan.dir.join(MANIFEST), &record));

    let mut leftovers = manifest.files.clone();
    if let Some(old) = &old {
        leftovers.extend(old.files.iter().cloned());
    }
    sweep_leftovers(&plan.dir, &leftovers);

    let total: u64 = files.iter().map(|(_, data)| data.len() as u64).sum();
    let mut done = 0u64;
    let staged = stage(&files, &mut |amount| {
        done += amount;
        progress((done * 880 / total.max(1)) as u32, "Copying files");
    })?;
    progress(900, "Replacing files");
    commit(&staged)?;

    // Files of an earlier version that this one no longer ships, and shortcuts and keys the new record no longer
    // lists (a run that named others): the record is the whole of what the uninstaller will remove.
    if let Some(old) = &old {
        for file in old.files.iter().filter(|f| !manifest.files.contains(f)) {
            if let Some(relative) = paths::safe_relative(file) {
                let _ = std::fs::remove_file(plan.dir.join(relative));
            }
        }
        prune_empty(&plan.dir, &old.files);
        for link in old.shortcuts.iter().filter(|link| {
            !manifest
                .shortcuts
                .iter()
                .any(|kept| paths::same_path(kept, link))
        }) {
            let _ = std::fs::remove_file(link);
        }
        for key in old.reg_keys.iter().filter(|key| {
            !manifest
                .reg_keys
                .iter()
                .any(|kept| kept.eq_ignore_ascii_case(key))
        }) {
            let _ = winsys::unregister_uninstall(key);
        }
    }

    if plan.register {
        progress(920, "Adding shortcuts");
        let exe = plan.dir.join("gravecord.exe");
        for link in start_menu.iter().chain(desktop.iter()) {
            winsys::create_shortcut(link, &exe, &plan.dir)?;
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
    }
    progress(1000, "Done");
    Ok(())
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
    let me = std::env::current_exe().ok();
    uninstall_as(dir, removal, me.as_deref())
}

/// `me` is the running executable, which is skipped however its path is spelled (case, `/`, a junction, 8.3 names).
fn uninstall_as(dir: &Path, removal: &Removal, me: Option<&Path>) -> Result<()> {
    let Some(manifest) = read_manifest(dir) else {
        bail!(
            "{} is not a gravecord install (no {MANIFEST}).",
            dir.display()
        );
    };
    let mut leftover = Vec::new();
    for file in &manifest.files {
        let Some(relative) = paths::safe_relative(file) else {
            continue;
        };
        let path = dir.join(relative);
        let _ = std::fs::remove_file(sibling(&path, STAGED));
        let _ = std::fs::remove_file(sibling(&path, BACKUP));
        if me.is_some_and(|me| paths::same_path(me, &path)) {
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
    // A registered install may have an "open on startup" value; one that starts this folder's exe goes with it. An
    // install made with no registration (the tests) never touches the registry.
    if !manifest.reg_keys.is_empty() {
        winsys::remove_startup_entry(dir);
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

    fn options(args: &[&str]) -> paths::Options {
        paths::parse_args(args.iter().map(|arg| (*arg).to_owned())).unwrap()
    }

    fn record(reg: &[&str], links: &[&str]) -> Manifest {
        Manifest {
            version: "0.3.0".into(),
            files: vec!["gravecord.exe".into(), UNINSTALLER.into()],
            reg_keys: reg.iter().map(|key| (*key).to_owned()).collect(),
            shortcuts: links.iter().map(PathBuf::from).collect(),
        }
    }

    #[test]
    fn the_updaters_silent_run_keeps_a_default_install_with_its_desktop_shortcut() {
        let base = scratch("keep-default");
        let desktop = base.join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let start = base
            .join("Start Menu")
            .join("Programs")
            .join("gravecord.lnk");
        let old = record(
            &["gravecord"],
            &[
                start.to_str().unwrap(),
                desktop.join("gravecord.lnk").to_str().unwrap(),
            ],
        );
        // Exactly what `updater::setup_args` passes.
        let args = options(&["--silent", "--launch", "--dir", r"C:\x\gravecord"]);
        let plan = Plan::silent(base.join("gravecord"), Some(&old), Some(&desktop), &args);
        assert!(plan.register);
        assert!(
            plan.desktop_shortcut,
            "the desktop shortcut stays in the record"
        );
        assert_eq!(plan.reg_key, "gravecord");
        assert_eq!(plan.shortcut_name, "gravecord");
        // Without one, none is added.
        let old = record(&["gravecord"], &[start.to_str().unwrap()]);
        let plan = Plan::silent(base.join("gravecord"), Some(&old), Some(&desktop), &args);
        assert!(plan.register && !plan.desktop_shortcut);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_updaters_silent_run_keeps_a_custom_installs_names() {
        let base = scratch("keep-custom");
        let desktop = base.join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let old = record(
            &["gravecord-uptest"],
            &[
                desktop.join("gravecord uptest.lnk").to_str().unwrap(),
                base.join("Programs")
                    .join("gravecord uptest.lnk")
                    .to_str()
                    .unwrap(),
            ],
        );
        let args = options(&["--silent", "--launch", "--dir", r"C:\x\uptest"]);
        let plan = Plan::silent(base.join("uptest"), Some(&old), Some(&desktop), &args);
        assert!(plan.register && plan.desktop_shortcut);
        assert_eq!(plan.reg_key, "gravecord-uptest", "not the default key");
        assert_eq!(
            plan.shortcut_name, "gravecord uptest",
            "not the default shortcut"
        );
        // An install made with --no-register stays unregistered.
        let plan = Plan::silent(
            base.join("uptest"),
            Some(&record(&[], &[])),
            Some(&desktop),
            &args,
        );
        assert!(!plan.register);
        // No record: a fresh install with the defaults, as before.
        let plan = Plan::silent(base.join("uptest"), None, Some(&desktop), &args);
        assert!(plan.register && !plan.desktop_shortcut);
        assert_eq!(
            (plan.reg_key.as_str(), plan.shortcut_name.as_str()),
            ("gravecord", "gravecord")
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_command_line_still_overrides_what_the_record_chose() {
        let old = record(&["gravecord-a"], &[r"C:\nowhere\Programs\gravecord a.lnk"]);
        let args = options(&[
            "--silent",
            "--dir",
            r"C:\x\a",
            "--reg-key",
            "gravecord-b",
            "--shortcut-name",
            "gravecord b",
            "--desktop",
        ]);
        let plan = Plan::silent(PathBuf::from(r"C:\x\a"), Some(&old), None, &args);
        assert_eq!(plan.reg_key, "gravecord-b");
        assert_eq!(plan.shortcut_name, "gravecord b");
        assert!(plan.register && plan.desktop_shortcut);
        let args = options(&["--silent", "--dir", r"C:\x\a", "--no-register"]);
        assert!(!Plan::silent(PathBuf::from(r"C:\x\a"), Some(&old), None, &args).register);
        // A damaged record never names a shortcut outside the plain-name rule.
        let odd = record(&[], &[r"C:\nowhere\a\..\b:c.lnk"]);
        let plan = Plan::keeping(PathBuf::from(r"C:\x\a"), Some(&odd), None);
        assert_eq!(plan.shortcut_name, paths::DEFAULT_SHORTCUT);
    }

    #[test]
    fn shortcuts_the_new_record_drops_are_removed_with_it() {
        let base = scratch("drop-links");
        let dir = base.join("gravecord");
        std::fs::create_dir_all(&dir).unwrap();
        let link = base.join("old shortcut.lnk");
        std::fs::write(&link, "lnk").unwrap();
        let mut old = record(&[], &[link.to_str().unwrap()]);
        old.version = "1".into();
        std::fs::write(dir.join(MANIFEST), old.render()).unwrap();
        // An explicitly unregistered run: the new record lists no shortcut, so the old one goes.
        install(
            &archive(&["gravecord.exe"]),
            b"stub",
            &plan_in(&dir),
            &mut |_, _| {},
        )
        .unwrap();
        assert!(!link.exists());
        assert!(read_manifest(&dir).unwrap().shortcuts.is_empty());
        let _ = std::fs::remove_dir_all(&base);
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

    fn plan_in(dir: &Path) -> Plan {
        let mut plan = Plan::new(dir.to_path_buf());
        plan.register = false;
        plan
    }

    fn versioned(version: &str, files: &[&str]) -> Archive {
        Archive {
            version: version.into(),
            entries: files
                .iter()
                .map(|path| Entry {
                    path: (*path).into(),
                    data: format!("{version}:{path}").into_bytes(),
                })
                .collect(),
        }
    }

    fn strays(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for item in std::fs::read_dir(next).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.to_string_lossy().ends_with(STAGED)
                    || path.to_string_lossy().ends_with(BACKUP)
                {
                    found.push(path);
                }
            }
        }
        found
    }

    #[test]
    fn a_failed_staging_leaves_the_old_install_untouched_and_no_staged_files() {
        let dir = scratch("stagefail").join("gravecord");
        let plan = plan_in(&dir);
        let files = ["gravecord.exe", "app/assets/a.svg", "app/assets/b.svg"];
        install(&versioned("1", &files), b"stub1", &plan, &mut |_, _| {}).unwrap();
        // A non-empty folder where b.svg's staged file must go: the third write fails.
        let block = dir.join("app/assets/b.svg.gc-new");
        std::fs::create_dir_all(block.join("inner")).unwrap();
        std::fs::write(block.join("inner/x"), "x").unwrap();
        let error = install(&versioned("2", &files), b"stub2", &plan, &mut |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("b.svg"), "{error}");
        assert_eq!(
            std::fs::read(dir.join("gravecord.exe")).unwrap(),
            b"1:gravecord.exe"
        );
        assert_eq!(std::fs::read(dir.join(UNINSTALLER)).unwrap(), b"stub1");
        assert_eq!(read_manifest(&dir).unwrap().version, "1");
        assert!(!dir.join("app/assets/a.svg.gc-new").exists());
        assert!(!dir.join("gravecord.exe.gc-new").exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_locked_file_during_the_swap_rolls_the_exe_back() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch("swapfail").join("gravecord");
        let plan = plan_in(&dir);
        let files = ["gravecord.exe", "app/assets/a.svg"];
        install(&versioned("1", &files), b"stub1", &plan, &mut |_, _| {}).unwrap();
        // The exe swaps first and succeeds; a.svg is held open with no sharing, so its swap fails.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join("app/assets/a.svg"))
            .unwrap();
        let error = install(&versioned("2", &files), b"stub2", &plan, &mut |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("a.svg") && error.contains("Nothing was changed"),
            "{error}"
        );
        drop(held);
        assert_eq!(
            std::fs::read(dir.join("gravecord.exe")).unwrap(),
            b"1:gravecord.exe"
        );
        assert_eq!(
            std::fs::read(dir.join("app/assets/a.svg")).unwrap(),
            b"1:app/assets/a.svg"
        );
        assert_eq!(read_manifest(&dir).unwrap().version, "1");
        assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
        // With the lock gone the same update goes through.
        install(&versioned("2", &files), b"stub2", &plan, &mut |_, _| {}).unwrap();
        assert_eq!(
            std::fs::read(dir.join("gravecord.exe")).unwrap(),
            b"2:gravecord.exe"
        );
        assert_eq!(read_manifest(&dir).unwrap().version, "2");
        assert!(strays(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_failure_injected_mid_swap_restores_every_earlier_step() {
        let dir = scratch("inject").join("gravecord");
        std::fs::create_dir_all(&dir).unwrap();
        let names = ["gravecord.exe", "b", "c", "new-only"];
        for name in &names[..3] {
            std::fs::write(dir.join(name), format!("old-{name}")).unwrap();
        }
        let contents: Vec<Vec<u8>> = names
            .iter()
            .map(|name| format!("new-{name}").into_bytes())
            .collect();
        let files: Vec<(PathBuf, &[u8])> = names
            .iter()
            .zip(&contents)
            .map(|(name, data)| (dir.join(name), data.as_slice()))
            .collect();
        let staged = stage(&files, &mut |_| {}).unwrap();
        let result = commit_with(&staged, &mut |index| {
            if index == 3 {
                Err(std::io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        for name in &names[..3] {
            assert_eq!(
                std::fs::read_to_string(dir.join(name)).unwrap(),
                format!("old-{name}")
            );
        }
        assert!(!dir.join("new-only").exists());
        assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_crash_between_swaps_is_completed_by_running_setup_again() {
        let dir = scratch("crash").join("gravecord");
        let plan = plan_in(&dir);
        let files = ["gravecord.exe", "app/assets/a.svg"];
        install(&versioned("1", &files), b"stub1", &plan, &mut |_, _| {}).unwrap();
        // The state a crash leaves: exe swapped (old one parked), assets still old, staged files lying around.
        std::fs::rename(dir.join("gravecord.exe"), dir.join("gravecord.exe.gc-old")).unwrap();
        std::fs::write(dir.join("gravecord.exe"), "2:gravecord.exe").unwrap();
        std::fs::write(dir.join("app/assets/a.svg.gc-new"), "half").unwrap();
        install(&versioned("2", &files), b"stub2", &plan, &mut |_, _| {}).unwrap();
        assert_eq!(
            std::fs::read(dir.join("app/assets/a.svg")).unwrap(),
            b"2:app/assets/a.svg"
        );
        assert_eq!(read_manifest(&dir).unwrap().version, "2");
        assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn uninstall_skips_the_running_uninstaller_under_any_spelling() {
        let base = scratch("spelling");
        let dir = base.join("Gravecord Ünï");
        let plan = plan_in(&dir);
        install(
            &versioned("1", &["gravecord.exe", "app/assets/a.svg"]),
            b"stub",
            &plan,
            &mut |_, _| {},
        )
        .unwrap();
        let removal = || Removal {
            purge_data: false,
            data_dir: base.join("data").join("gravecord"),
        };
        let upper = PathBuf::from(dir.join(UNINSTALLER).to_string_lossy().to_uppercase());
        uninstall_as(&dir, &removal(), Some(&upper)).unwrap();
        assert!(dir.join(UNINSTALLER).exists(), "upper-case spelling");
        assert!(!dir.join("gravecord.exe").exists() && !dir.join(MANIFEST).exists());
        // Forward slashes and a dotted spelling, on a fresh install.
        for spelling in [
            PathBuf::from(dir.join(UNINSTALLER).to_string_lossy().replace('\\', "/")),
            dir.join("app").join("..").join(UNINSTALLER),
        ] {
            install(
                &versioned("1", &["gravecord.exe"]),
                b"stub",
                &plan,
                &mut |_, _| {},
            )
            .unwrap();
            uninstall_as(&dir, &removal(), Some(&spelling)).unwrap();
            assert!(dir.join(UNINSTALLER).exists(), "{}", spelling.display());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
