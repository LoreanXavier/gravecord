//! Path handling and the command line. Pure functions, tested without touching the system.

use std::path::{Component, Path, PathBuf};

/// Name of the install record written next to the program: the files, shortcuts and registry key it created.
pub const MANIFEST: &str = "install.manifest";
pub const UNINSTALLER: &str = "uninstall.exe";
pub const DEFAULT_KEY: &str = "gravecord";
pub const DEFAULT_SHORTCUT: &str = "gravecord";

/// A payload path: relative, `/` separated, with only plain names in it.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains(['\\', ':', '\0']) || path.starts_with('/') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.']) {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

pub fn default_install_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("gravecord")
}

/// The folder the files actually go to. A folder picked by hand that already holds other things gets a `gravecord`
/// folder inside it, so choosing Documents never scatters the program there. `has_content` tells whether the folder
/// exists with entries in it; `has_manifest` whether it is an earlier install of ours.
pub fn resolve_install_dir(chosen: &Path, has_content: bool, has_manifest: bool) -> PathBuf {
    if has_content && !has_manifest {
        chosen.join("gravecord")
    } else {
        chosen.to_owned()
    }
}

/// A path in a spelling-independent form for comparison: no `\\?\` prefix, `\` separators, no trailing separator,
/// lower case (Windows file names are case-insensitive).
pub fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned());
    let trimmed = text.trim_end_matches('\\');
    // "C:\" must stay a root, not become "C:".
    if trimmed.ends_with(':') {
        format!("{}\\", trimmed.to_lowercase())
    } else {
        trimmed.to_lowercase()
    }
}

/// `canonicalize` for a path that may not exist yet: the deepest existing ancestor is resolved (junctions, 8.3 names,
/// the real case) and the missing rest is appended unchanged.
pub fn canonical_lenient(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if let Ok(mut real) = current.canonicalize() {
            for part in tail.iter().rev() {
                real.push(part);
            }
            return real;
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_owned());
                current = parent.to_owned();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether two spellings name the same place.
pub fn same_path(a: &Path, b: &Path) -> bool {
    comparable(&canonical_lenient(a)) == comparable(&canonical_lenient(b))
}

/// Whether `path` is `root` or below it, by `comparable` spelling.
pub fn is_within(path: &Path, root: &Path) -> bool {
    let (path, root) = (comparable(path), comparable(root));
    path == root || path.starts_with(&format!("{}\\", root.trim_end_matches('\\')))
}

/// Places the installer must never write into besides the system's own folders.
#[derive(Debug, Default)]
pub struct Protected {
    /// `%LOCALAPPDATA%\gravecord`, where settings and sign-in live; the uninstaller may delete it.
    pub data_dirs: Vec<PathBuf>,
    /// The user-profile root, and the folder holding all profiles.
    pub profile: Option<PathBuf>,
    /// `%SystemRoot%`, `%ProgramFiles%`, ... as the environment names them.
    pub system: Vec<PathBuf>,
}

impl Protected {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            data_dirs: var("LOCALAPPDATA")
                .map(|dir| dir.join("gravecord"))
                .into_iter()
                .collect(),
            profile: var("USERPROFILE"),
            system: ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|name| var(name))
                .collect(),
        }
    }
}

/// Folders the installer refuses to write into: roots, the system's own, the profile root and the data folder.
pub fn check_install_dir(dir: &Path) -> Result<(), String> {
    check_install_dir_against(dir, &Protected::from_env())
}

pub fn check_install_dir_against(dir: &Path, protected: &Protected) -> Result<(), String> {
    if !dir.is_absolute() {
        return Err("The install location must be a full path.".into());
    }
    if dir
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("The install location must not contain \"..\".".into());
    }
    // Junctions, 8.3 names and other spellings are checked as what they resolve to.
    for candidate in [dir.to_path_buf(), canonical_lenient(dir)] {
        check_spelling(&candidate, protected)?;
    }
    Ok(())
}

fn check_spelling(dir: &Path, protected: &Protected) -> Result<(), String> {
    let normals = dir
        .components()
        .filter(|part| matches!(part, Component::Normal(_)))
        .count();
    let text = comparable(dir);
    let rest = text.split_once(':').map_or("", |(_, rest)| rest);
    if normals == 0 || rest.trim_matches('\\').is_empty() {
        return Err("Pick a folder, not a drive.".into());
    }
    for system in ["\\windows", "\\program files", "\\program files (x86)"] {
        if rest == system || rest.starts_with(&format!("{system}\\")) {
            return Err("Pick a folder you own; this one needs administrator rights.".into());
        }
    }
    for system in &protected.system {
        if is_within(&canonical_lenient(dir), &canonical_lenient(system)) {
            return Err("Pick a folder you own; this one needs administrator rights.".into());
        }
    }
    if rest == "\\users" || rest == "\\programdata" {
        return Err("Pick a folder inside your own user folder.".into());
    }
    if let Some(profile) = &protected.profile {
        let profile = canonical_lenient(profile);
        let same = comparable(dir) == comparable(&profile);
        let parent = profile
            .parent()
            .is_some_and(|parent| comparable(dir) == comparable(parent));
        if same || parent {
            return Err(
                "Pick a folder inside your user folder, not the user folder itself.".into(),
            );
        }
    }
    for data in &protected.data_dirs {
        if is_within(dir, &canonical_lenient(data)) {
            return Err(format!(
                "{} holds your gravecord settings and sign-in; pick another folder.",
                data.display()
            ));
        }
    }
    Ok(())
}

/// The folder the files go to for a folder chosen by hand (the picker, or `--dir` in silent mode): checked, then
/// given a `gravecord` subfolder when it already holds other things, then checked again.
pub fn resolve_from_disk(chosen: &Path) -> Result<PathBuf, String> {
    resolve_against(chosen, &Protected::from_env())
}

pub fn resolve_against(chosen: &Path, protected: &Protected) -> Result<PathBuf, String> {
    check_install_dir_against(chosen, protected)?;
    let busy = std::fs::read_dir(chosen).is_ok_and(|mut entries| entries.next().is_some());
    let ours = chosen.join(MANIFEST).is_file();
    let dir = resolve_install_dir(chosen, busy, ours);
    check_install_dir_against(&dir, protected)?;
    Ok(dir)
}

/// What to do, from the command line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub silent: bool,
    pub uninstall: bool,
    pub dir: Option<PathBuf>,
    /// Test mode: write no shortcuts and no registry key.
    pub no_register: bool,
    pub desktop: bool,
    pub launch: bool,
    pub purge_data: bool,
    pub reg_key: Option<String>,
    pub shortcut_name: Option<String>,
    /// Overrides `%LOCALAPPDATA%\gravecord` as the folder the uninstaller may offer to delete.
    pub data_dir: Option<PathBuf>,
    /// Renders the window, saves a PNG and exits.
    pub screenshot: Option<PathBuf>,
    pub state: Option<String>,
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    let value = |flag: &str, args: &mut I::IntoIter| {
        args.next().ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--silent" => options.silent = true,
            "--uninstall" => options.uninstall = true,
            "--no-register" => options.no_register = true,
            "--desktop" => options.desktop = true,
            "--launch" => options.launch = true,
            "--purge-data" => options.purge_data = true,
            "--dir" => options.dir = Some(value(&flag, &mut args)?.into()),
            "--data-dir" => options.data_dir = Some(value(&flag, &mut args)?.into()),
            "--reg-key" => options.reg_key = Some(value(&flag, &mut args)?),
            "--shortcut-name" => options.shortcut_name = Some(value(&flag, &mut args)?),
            "--screenshot" => options.screenshot = Some(value(&flag, &mut args)?.into()),
            "--state" => options.state = Some(value(&flag, &mut args)?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// A name that is safe as a file name or registry key name (tests pass their own).
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn payload_paths_must_be_plain_relative_names() {
        assert_eq!(
            safe_relative("app/assets/icons/a.svg"),
            Some(
                PathBuf::from("app")
                    .join("assets")
                    .join("icons")
                    .join("a.svg")
            )
        );
        for bad in [
            "", "/a", "a//b", "a/./b", "../a", "a/..", "c:/a", "a\\b", "a/b/", "con.", "a /b",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn default_location_is_under_programs() {
        assert_eq!(
            default_install_dir(Path::new(r"C:\Users\x\AppData\Local")),
            PathBuf::from(r"C:\Users\x\AppData\Local\Programs\gravecord")
        );
    }

    #[test]
    fn a_busy_folder_gets_a_subfolder() {
        let docs = Path::new(r"C:\Users\x\Documents");
        assert_eq!(
            resolve_install_dir(docs, true, false),
            docs.join("gravecord")
        );
        assert_eq!(resolve_install_dir(docs, false, false), docs);
        assert_eq!(resolve_install_dir(docs, true, true), docs);
    }

    #[test]
    fn install_dir_checks() {
        assert!(check_install_dir(Path::new(r"C:\Users\x\Apps\gravecord")).is_ok());
        assert!(check_install_dir(Path::new(r"D:\games\gravecord")).is_ok());
        for bad in [
            r"gravecord",
            r"C:\",
            r"C:\Windows",
            r"c:\program files\gravecord",
            r"C:\Program Files (x86)",
            r"C:\a\..\b",
        ] {
            assert!(check_install_dir(Path::new(bad)).is_err(), "{bad}");
        }
        assert!(check_install_dir(Path::new(r"C:\Windows2")).is_ok());
    }

    #[test]
    fn command_line() {
        let options = parse_args(args(&[
            "--silent",
            "--dir",
            r"C:\t e st",
            "--no-register",
            "--reg-key",
            "k",
            "--desktop",
        ]))
        .unwrap();
        assert!(options.silent && options.no_register && options.desktop && !options.uninstall);
        assert_eq!(options.dir, Some(PathBuf::from(r"C:\t e st")));
        assert_eq!(options.reg_key.as_deref(), Some("k"));
        assert!(parse_args(args(&["--dir"])).is_err());
        assert!(parse_args(args(&["--nope"])).is_err());
        assert_eq!(parse_args(args(&[])).unwrap(), Options::default());
    }

    #[test]
    fn test_names_are_plain() {
        assert!(plain_name("gravecord-test"));
        assert!(!plain_name("a\\b"));
        assert!(!plain_name(""));
    }

    fn protected() -> Protected {
        Protected {
            data_dirs: vec![PathBuf::from(r"C:\Users\x\AppData\Local\gravecord")],
            profile: Some(PathBuf::from(r"C:\Users\x")),
            system: vec![PathBuf::from(r"C:\Windows")],
        }
    }

    #[test]
    fn data_folder_profile_root_drives_and_system_folders_are_refused() {
        let protected = protected();
        for bad in [
            r"C:\Users\x\AppData\Local\gravecord",
            r"c:/users/X/appdata/local/GRAVECORD/inner",
            r"C:\Users\x",
            r"c:\users\x\\",
            r"C:\Users",
            r"C:\ProgramData",
            r"C:\",
            r"D:\",
            r"C:\Windows\System32",
            r"C:\Program Files\gravecord",
        ] {
            assert!(
                check_install_dir_against(Path::new(bad), &protected).is_err(),
                "{bad}"
            );
        }
        for good in [
            r"C:\Users\x\Documents\gravecord",
            r"C:\Users\x\AppData\Local\Programs\gravecord",
            r"C:\Users\x\AppData\Local\gravecord2",
            r"D:\apps\ünï code",
        ] {
            assert!(
                check_install_dir_against(Path::new(good), &protected).is_ok(),
                "{good}"
            );
        }
    }

    #[test]
    fn silent_style_resolution_appends_gravecord_to_a_busy_folder_and_refuses_the_rest() {
        let base = std::env::temp_dir().join(format!("gc-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let docs = base.join("Documents");
        std::fs::create_dir_all(&docs).unwrap();
        let protected = Protected {
            data_dirs: vec![base.join("Local").join("gravecord")],
            profile: Some(base.clone()),
            system: vec![],
        };
        // Empty folder: used as is. A new folder: used as is.
        assert_eq!(resolve_against(&docs, &protected).unwrap(), docs);
        assert_eq!(
            resolve_against(&base.join("Fresh"), &protected).unwrap(),
            base.join("Fresh")
        );
        // Busy folder: gets a gravecord subfolder.
        std::fs::write(docs.join("report.txt"), "x").unwrap();
        assert_eq!(
            resolve_against(&docs, &protected).unwrap(),
            docs.join("gravecord")
        );
        // An earlier install of ours is reused.
        std::fs::write(docs.join(MANIFEST), "version 1\n").unwrap();
        assert_eq!(resolve_against(&docs, &protected).unwrap(), docs);
        // The folder holding the data folder resolves into the data folder and is refused; so is the profile root.
        let local = base.join("Local");
        std::fs::create_dir_all(local.join("gravecord")).unwrap();
        std::fs::write(local.join("other"), "x").unwrap();
        assert!(resolve_against(&local, &protected).is_err());
        assert!(resolve_against(&local.join("gravecord"), &protected).is_err());
        assert!(resolve_against(&base, &protected).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn paths_compare_by_place_not_spelling() {
        let base = std::env::temp_dir().join(format!("gc-same-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Sub")).unwrap();
        let file = base.join("Sub").join("uninstall.exe");
        std::fs::write(&file, "x").unwrap();
        let upper = PathBuf::from(file.to_string_lossy().to_uppercase());
        let slashes = PathBuf::from(file.to_string_lossy().replace('\\', "/"));
        let dotted = base
            .join("Sub")
            .join("..")
            .join("Sub")
            .join("uninstall.exe");
        assert!(same_path(&file, &upper));
        assert!(same_path(&file, &slashes));
        assert!(same_path(&file, &dotted));
        assert!(same_path(
            &file,
            &PathBuf::from(format!(r"\\?\{}", file.display()))
        ));
        assert!(!same_path(&file, &base.join("Sub").join("other.exe")));
        // A path that does not exist yet still compares by its existing part.
        assert!(same_path(
            &base.join("sub").join("new").join("a"),
            &base.join("Sub").join("NEW").join("a")
        ));
        // A junction names the same place.
        let junction = base.join("Link");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(base.join("Sub"))
            .output()
            .unwrap();
        if made.status.success() {
            assert!(same_path(&file, &junction.join("uninstall.exe")));
            let _ = std::process::Command::new("cmd")
                .args(["/C", "rmdir"])
                .arg(&junction)
                .output();
        }
        assert!(is_within(Path::new(r"C:\A\B\c"), Path::new("c:/a/b/")));
        assert!(!is_within(Path::new(r"C:\A\Bc"), Path::new(r"C:\A\B")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
