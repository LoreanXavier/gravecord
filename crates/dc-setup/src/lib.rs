//! gravecord's Windows installer. See `docs/installer.md`.
#![deny(unsafe_code)]

pub mod app_id;
pub mod icon_gen;
pub mod install;
pub mod paths;
pub mod payload;
mod ui;
mod winsys;

use paths::Options;
use std::path::{Path, PathBuf};

/// The process exit code: 0 done, 1 failed, 2 bad command line.
pub fn run() -> i32 {
    let options = match paths::parse_args(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            winsys::message(None, "gravecord setup", &message, winsys::Kind::Error);
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
                winsys::message(None, "gravecord setup", &message, winsys::Kind::Error);
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
        let mut plan = install::Plan::new(dir);
        plan.desktop_shortcut = options.desktop;
        plan.register = !options.no_register;
        if let Some(name) = &options.reg_key {
            plan.reg_key = name.clone();
        }
        if let Some(name) = &options.shortcut_name {
            plan.shortcut_name = name.clone();
        }
        close_running(&plan.dir.join("gravecord.exe"), true)?;
        install::install(&archive, &stub, &plan, &mut |_, _| {})?;
        if options.launch {
            winsys::launch(&plan.dir.join("gravecord.exe"), &plan.dir);
        }
        return Ok(0);
    }

    Ok(ui::run(ui::Start {
        dir: options.dir.unwrap_or(default_dir),
        archive,
        stub,
        screenshot: options.screenshot,
        state: options.state,
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
        let ask = format!("Remove gravecord from {}?", dir.display());
        if !winsys::message(None, "Uninstall gravecord", &ask, winsys::Kind::AskYes) {
            return Ok(0);
        }
        let keep = winsys::message(
            None,
            "Uninstall gravecord",
            &format!(
                "Keep your settings and sign-in data?\n\n{}\n\nYes keeps it, so a later install picks up where you left off. No deletes it.",
                data_dir.display()
            ),
            winsys::Kind::AskYes,
        );
        purge = !keep;
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
        winsys::message(
            None,
            "Uninstall gravecord",
            "gravecord was removed.",
            winsys::Kind::Info,
        );
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
