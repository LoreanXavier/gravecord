#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod branding;
mod editor;
mod fonts;
#[cfg(feature = "capture")]
mod harness;
mod inline;
mod logging;
mod login_art;
mod net;
mod notifications;
mod palette;
mod parity;
mod shell;
mod sounds;
mod speech;
mod spinner;
mod storage;
mod text_buffer;
mod tray;
mod ui;
mod video_clip;

use anyhow::{Context as _, Result};
use dc_proto::identity::Identity;
use dc_proto::secrets::Token;
use gpui::{
    App, AssetSource, Bounds, SharedString, TextRenderingMode, TitlebarOptions, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Icons live in `app/assets/icons`, geometry extracted from the pinned build.
struct Assets {
    root: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(std::fs::read(self.root.join(path)).ok().map(Cow::Owned))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(std::fs::read_dir(self.root.join(path))?
            .filter_map(|entry| {
                Some(
                    entry
                        .ok()?
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                )
            })
            .collect())
    }
}

fn find_upwards(name: &str) -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|path| path.exists())
}

/// The icons: next to an installed executable, or up from a build folder or the working directory.
fn find_assets() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    exe.iter()
        .flat_map(|exe| exe.ancestors())
        .map(|dir| dir.join("app/assets"))
        .find(|path| path.exists())
        .or_else(|| find_upwards("app/assets"))
}

struct Capture {
    path: PathBuf,
    settle_ms: u64,
    /// Typed and sent through the composer once the channel has loaded, for end-to-end tests.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    send: Option<String>,
    /// Scripted input, see `harness.rs`.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    script: Option<String>,
    /// `--follow <file>`: stay open and run each line added to the file as a script, for live tests driven from
    /// outside (a voice call, say). `shot <png>` saves a frame, `quit` ends the run.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    follow: Option<PathBuf>,
}

fn capture_args() -> Option<Capture> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|arg| arg == "--capture")?;
    Some(Capture {
        path: PathBuf::from(args.get(at + 1)?),
        settle_ms: args
            .get(at + 2)
            .and_then(|value| value.parse().ok())
            .unwrap_or(2500),
        send: args
            .get(at + 3)
            .filter(|value| !value.starts_with("--"))
            .cloned(),
        script: args
            .iter()
            .position(|arg| arg == "--script")
            .and_then(|at| args.get(at + 1))
            .cloned(),
        follow: args
            .iter()
            .position(|arg| arg == "--follow")
            .and_then(|at| args.get(at + 1))
            .map(PathBuf::from),
    })
}

#[cfg(feature = "capture")]
fn save_frame(image: &image::RgbaImage, path: &Path) -> Result<()> {
    image.save(path).context("writing the capture")
}

/// Calibrated against the official client with `tools/calibrate-text.py`. 2.2 is the top of gpui's table.
const TEXT_GAMMA: &str = "2.2";

/// `--log <file>`: debug events go to that file, for live tests (a voice call) driven from outside. Off otherwise:
/// nothing is logged. Debug events may name user ids, so the file belongs in a scratch folder, never in the repo.
fn record_log() {
    let args: Vec<String> = std::env::args().collect();
    let file = args
        .iter()
        .position(|arg| arg == "--log")
        .and_then(|at| args.get(at + 1))
        .and_then(|path| std::fs::File::create(path).ok());
    logging::init(file);
}

/// A panic on the UI thread aborts the process with nothing on screen, so it is written down first:
/// The active profile's `panic.log`, newest last. Capture failures stay outside the owner's profile.
fn record_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = storage::data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                use std::io::Write as _;
                let backtrace = std::backtrace::Backtrace::force_capture();
                let _ = writeln!(
                    file,
                    "{info}
{backtrace}
"
                );
            }
        }
        restore_audio_in_time();
        previous(info);
    }));
}

/// A panic aborts the process: other applications' volumes lowered by Global Attenuation are put back first. On
/// another thread and with a deadline, so a panic that happened while the attenuation lock was held cannot hang it.
fn restore_audio_in_time() {
    let (done, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        dc_voice::shutdown();
        let _ = done.send(());
    });
    let _ = wait.recv_timeout(std::time::Duration::from_secs(2));
}

/// Where the app starts: the token in the OS credential store, else the login screen. Capture builds read
/// `secrets.toml` instead (the harness and the parity runs need a signed in client), and `--login <state>` shows one
/// login state with no network. The normal build never looks for `secrets.toml`.
fn initial_start() -> shell::Start {
    #[cfg(feature = "capture")]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(state) = args
            .iter()
            .position(|arg| arg == "--login")
            .and_then(|at| args.get(at + 1))
        {
            return shell::Start::Preview(state.clone());
        }
        if let Some(path) = find_upwards("secrets.toml")
            && let Ok(token) = dc_proto::secrets::load(&path)
        {
            return shell::Start::Token(token);
        }
        shell::Start::Login
    }
    #[cfg(not(feature = "capture"))]
    {
        use dc_auth::credentials::{ACTIVE, Store};
        match Store::app().load(ACTIVE) {
            Ok(Some(token)) => shell::Start::Token(token),
            _ => shell::Start::Login,
        }
    }
}

fn main() -> Result<()> {
    // Read once by gpui's renderer. Set before any other thread exists.
    if std::env::var_os("GRAVECORD_TEXT_GAMMA").is_none() {
        unsafe { std::env::set_var("GRAVECORD_TEXT_GAMMA", TEXT_GAMMA) };
    }
    record_panics();
    record_log();
    notifications::configure_process();
    let assets_root = find_assets().context("app/assets not found")?;
    let capture = capture_args();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    // The fonts are public files: no login is needed (or known) yet.
    let font_client = dc_http::Client::new(Token::new(""), Identity::new())?;
    let faces = runtime.block_on(fonts::load(&font_client))?;
    let start = initial_start();
    let handle = runtime.handle().clone();

    application()
        .with_assets(Assets { root: assets_root })
        .run(move |cx: &mut App| {
            // Chromium draws this UI with grayscale antialiasing.
            cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
            cx.text_system()
                .add_fonts(faces.into_iter().map(Cow::Owned).collect())
                .expect("gg sans must load");
            let bounds = Bounds::centered(None, size(px(1280.), px(720.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(TitlebarOptions {
                            title: Some("gravecord".into()),
                            appears_transparent: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    move |window, cx| {
                        notifications::observe_window(window);
                        cx.new(|cx| shell::Shell::new(handle, start, window, cx))
                    },
                )
                .expect("the window must open");
            let shell = window.entity(cx).expect("the shell exists");
            cx.activate(true);

            if let Some(capture) = capture {
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(100))
                            .await;
                        if shell.read_with(cx, |shell, cx| shell.ready(cx)) {
                            break;
                        }
                    }
                    // The harness drives the main view; a login capture has none.
                    #[cfg(feature = "capture")]
                    let root = shell.read_with(cx, |shell, _| shell.root());
                    #[cfg(feature = "capture")]
                    if let Some(text) = &capture.send {
                        let script = format!("click 700 683; type {text}; key enter");
                        let log = capture.path.with_extension("log");
                        if let Some(root) = root.clone() {
                            harness::run(&script, window.into(), root, log, cx).await;
                        }
                    }
                    #[cfg(feature = "capture")]
                    if let Some(script) = &capture.script {
                        let log = capture.path.with_extension("log");
                        if let Some(root) = root.clone() {
                            harness::run(script, window.into(), root, log, cx).await;
                        }
                    }
                    #[cfg(feature = "capture")]
                    if let Some(follow) = &capture.follow {
                        let log = capture.path.with_extension("log");
                        let mut done = 0;
                        'follow: loop {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(300))
                                .await;
                            let text = std::fs::read_to_string(follow).unwrap_or_default();
                            let lines: Vec<String> = text.lines().map(str::to_owned).collect();
                            for line in lines.iter().skip(done) {
                                done += 1;
                                let line = line.trim();
                                if line == "quit" {
                                    break 'follow;
                                }
                                if let Some(shot) = line.strip_prefix("shot ") {
                                    let shot = PathBuf::from(shot.trim());
                                    cx.update(|cx| write_capture(cx, window.into(), &shot));
                                    continue;
                                }
                                if let Some(root) = root.clone() {
                                    harness::run(line, window.into(), root, log.clone(), cx).await;
                                }
                            }
                        }
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(capture.settle_ms))
                        .await;
                    cx.update(|cx| {
                        write_capture(cx, window.into(), &capture.path);
                        cx.quit();
                    });
                })
                .detach();
            }
        });
    // The window closed: whatever Global Attenuation lowered goes back, and the call's recordings are written.
    dc_voice::shutdown();
    Ok(())
}

#[cfg(feature = "capture")]
fn write_capture(cx: &mut App, window: gpui::AnyWindowHandle, path: &Path) {
    let result = window.update(cx, |_, window, cx| {
        parity::clear();
        window.refresh();
        window.draw(cx).clear(cx);
        parity::write(&path.with_extension("layout.json"));
        window.render_to_image()
    });
    match result {
        Ok(Ok(image)) => {
            if let Err(error) = save_frame(&image, path) {
                eprintln!("{error}");
            }
        }
        Ok(Err(error)) => eprintln!("capture failed: {error}"),
        Err(error) => eprintln!("capture failed: {error}"),
    }
}

#[cfg(not(feature = "capture"))]
fn write_capture(_: &mut App, _: gpui::AnyWindowHandle, _: &Path) {
    eprintln!("this build has no capture support, rebuild with --features capture");
}
