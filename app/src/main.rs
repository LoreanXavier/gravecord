#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod editor;
mod fonts;
#[cfg(feature = "capture")]
mod harness;
mod inline;
mod net;
mod palette;
mod parity;
mod speech;
mod storage;
mod text_buffer;
mod ui;

use anyhow::{Context as _, Result};
use dc_proto::identity::Identity;
use dc_proto::secrets;
use gpui::{
    App, AssetSource, Bounds, SharedString, TextRenderingMode, TitlebarOptions, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

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

struct Capture {
    path: PathBuf,
    settle_ms: u64,
    /// Typed and sent through the composer once the channel has loaded, for end-to-end tests.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    send: Option<String>,
    /// Scripted input, see `harness.rs`.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    script: Option<String>,
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
    })
}

#[cfg(feature = "capture")]
fn save_frame(image: &image::RgbaImage, path: &Path) -> Result<()> {
    image.save(path).context("writing the capture")
}

/// Calibrated against the official client with `tools/calibrate-text.py`. 2.2 is the top of gpui's table.
const TEXT_GAMMA: &str = "2.2";

/// A panic on the UI thread aborts the process with nothing on screen, so it is written down first:
/// `%LOCALAPPDATA%\gravecord\panic.log`, newest last. The message never holds tokens or message content.
fn record_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) =
            std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("gravecord"))
        {
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
        previous(info);
    }));
}

fn main() -> Result<()> {
    // Read once by gpui's renderer. Set before any other thread exists.
    if std::env::var_os("GRAVECORD_TEXT_GAMMA").is_none() {
        unsafe { std::env::set_var("GRAVECORD_TEXT_GAMMA", TEXT_GAMMA) };
    }
    record_panics();
    let secrets_path = find_upwards("secrets.toml").context("secrets.toml not found")?;
    let token = secrets::load(&secrets_path).context("loading secrets.toml")?;
    let assets_root = find_upwards("app/assets").context("app/assets not found")?;
    let capture = capture_args();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    let font_client = dc_http::Client::new(token.clone(), Identity::new())?;
    let faces = runtime.block_on(fonts::load(&font_client))?;

    let (update_tx, mut update_rx) = mpsc::unbounded_channel();
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    runtime.spawn(net::run(token, update_tx, request_rx));

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
                    |window, cx| {
                        let root = cx.new(|cx| ui::Root::new(request_tx, cx));
                        ui::bind_focus(&root, window, cx);
                        root
                    },
                )
                .expect("the window must open");
            let root = window.entity(cx).expect("the root view exists");
            cx.activate(true);

            cx.spawn({
                let root = root.clone();
                async move |cx| {
                    while let Some(update) = update_rx.recv().await {
                        root.update(cx, |root, cx| root.update(update, cx));
                    }
                }
            })
            .detach();

            if let Some(capture) = capture {
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(100))
                            .await;
                        if root.read_with(cx, |root, _| root.ready) {
                            break;
                        }
                    }
                    #[cfg(feature = "capture")]
                    if let Some(text) = &capture.send {
                        let script = format!("click 700 683; type {text}; key enter");
                        let log = capture.path.with_extension("log");
                        harness::run(&script, window.into(), root.clone(), log, cx).await;
                    }
                    #[cfg(feature = "capture")]
                    if let Some(script) = &capture.script {
                        let log = capture.path.with_extension("log");
                        harness::run(script, window.into(), root.clone(), log, cx).await;
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
    Ok(())
}

#[cfg(feature = "capture")]
fn write_capture(cx: &mut App, window: gpui::AnyWindowHandle, path: &Path) {
    parity::write(&path.with_extension("layout.json"));
    let result = window.update(cx, |_, window, _| window.render_to_image());
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
