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

/// Folders the installer refuses to write into: roots and the system's own.
pub fn check_install_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() {
        return Err("The install location must be a full path.".into());
    }
    let normals = dir
        .components()
        .filter(|part| matches!(part, Component::Normal(_)))
        .count();
    if normals == 0 {
        return Err("Pick a folder, not a drive.".into());
    }
    if dir
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("The install location must not contain \"..\".".into());
    }
    let text = dir.to_string_lossy().to_lowercase();
    for system in ["\\windows", "\\program files", "\\program files (x86)"] {
        let rest = text.split_once(':').map_or("", |(_, rest)| rest);
        if rest == system || rest.starts_with(&format!("{system}\\")) {
            return Err("Pick a folder you own; this one needs administrator rights.".into());
        }
    }
    Ok(())
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
}
