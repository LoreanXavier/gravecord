//! gravecord's Windows installer. See `docs/installer.md`.
#![deny(unsafe_code)]

pub mod app_id;
mod dialog;
mod draw;
pub mod elide;
pub mod icon_gen;
pub mod install;
pub mod paths;
pub mod payload;
mod ui;
mod winsys;
pub mod wording;

use paths::Options;
use std::path::{Path, PathBuf};

/// The process exit code: 0 done, 1 failed, 2 bad command line.
pub fn run() -> i32 {
    let options = match paths::parse_args(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            dialog::error(None, "gravecord setup", &message);
            return 2;
        }
    };
    for name in [&options.reg_key, &options.shortcut_name]
        .into_iter()
        .flatten()
    {
        if !paths::plain_name(name) {
            return 2;
        }
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return 1,
    };
    let silent = options.silent;
    let is_uninstaller = options.uninstall
        || exe
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(paths::UNINSTALLER));
    let outcome = if is_uninstaller {
        uninstall(&options, &exe)
    } else {
        setup(options, &exe)
    };
    match outcome {
        Ok(code) => code,
        Err(error) => {
            let message = format!("{error:#}");
            log_failure(&message);
            if silent {
                eprintln!("{message}");
            } else {
                dialog::error(None, "gravecord setup", &message);
            }
            1
        }
    }
}

fn log_failure(message: &str) {
    let path = std::env::temp_dir().join("gravecord-setup.log");
    let _ = std::fs::write(path, message);
}

fn setup(options: Options, exe: &Path) -> anyhow::Result<i32> {
    // Verified before anything else: a damaged setup refuses here, in the window and in silent mode alike.
    let payload::Loaded { archive, stub } = payload::load(exe)?;
    let default_dir = paths::default_install_dir(&winsys::local_app_data()?);

    if options.silent {
        let archive = archive.ok_or_else(|| anyhow::anyhow!("this setup has no payload"))?;
        // A folder named on the command line gets the same checks and the same `gravecord` subfolder rule as the Change
        // button in the window.
        let dir = match &options.dir {
            Some(chosen) => paths::resolve_from_disk(chosen).map_err(anyhow::Error::msg)?,
            None => default_dir,
        };
        // An update over an install keeps its key, shortcut names and desktop shortcut: the app's updater names
        // only the folder.
        let old = install::read_manifest(&dir);
        let desktop = winsys::desktop_dir().ok();
        let plan = install::Plan::silent(dir, old.as_ref(), desktop.as_deref(), &options);
        close_running(&plan.dir.join("gravecord.exe"), true)?;
        install::install(&archive, &stub, &plan, &mut |_, _| {})?;
        if options.launch {
            winsys::launch(&plan.dir.join("gravecord.exe"), &plan.dir);
        }
        return Ok(0);
    }

    let shot = options.screenshot.map(|path| ui::Shot {
        path,
        page: options.screenshot_page.unwrap_or(paths::ShotPage::Welcome),
        progress: options.screenshot_progress,
        dpi: options.screenshot_dpi,
        version: options.screenshot_version,
        button: options.screenshot_button,
    });
    let data_dir = match &options.data_dir {
        Some(dir) => dir.clone(),
        None => winsys::local_app_data()?.join("gravecord"),
    };
    Ok(ui::run(ui::Start {
        dir: options.dir.unwrap_or(default_dir),
        data_dir,
        archive,
        stub,
        shot,
    }))
}

fn uninstall(options: &Options, exe: &Path) -> anyhow::Result<i32> {
    let dir: PathBuf = match &options.dir {
        Some(dir) => dir.clone(),
        None => exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("no folder"))?
            .to_owned(),
    };
    let data_dir = match &options.data_dir {
        Some(dir) => dir.clone(),
        None => winsys::local_app_data()?.join("gravecord"),
    };
    let mut purge = options.purge_data;
    if !options.silent {
        if !dialog::ask_removal(None, &dir) {
            return Ok(0);
        }
        purge = !dialog::ask_keep_data(None, &data_dir);
    }
    close_running(&dir.join("gravecord.exe"), options.silent)?;
    install::uninstall(
        &dir,
        &install::Removal {
            purge_data: purge,
            data_dir,
        },
    )?;
    if !options.silent {
        dialog::info(None, "Uninstall gravecord", "gravecord was removed.");
    }
    // This exe is the last file in the folder; it can only be deleted once it has exited. The helper is started
    // only now, after the dialog is gone, and waits for this process by id.
    if exe
        .parent()
        .is_some_and(|parent| paths::same_path(parent, &dir))
    {
        winsys::delete_self_later(exe, &dir);
    }
    Ok(0)
}

fn close_running(exe: &Path, silent: bool) -> anyhow::Result<()> {
    if silent {
        anyhow::ensure!(
            winsys::ask_to_close(exe, std::time::Duration::from_secs(8)),
            "gravecord is still running; close it and run setup again."
        );
        Ok(())
    } else {
        ui::close_running(exe, None)
    }
}
