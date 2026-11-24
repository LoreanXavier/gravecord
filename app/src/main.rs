#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod branding;
mod clipboard_image;
#[cfg(windows)]
mod clips_capture;
mod deep_link;
mod editor;
mod fonts;
mod games;
#[cfg(feature = "capture")]
mod harness;
mod idle;
mod inert;
mod inline;
mod instance;
mod intl;
mod logging;
mod login_art;
mod lottie;
mod markup_preview;
mod mp4_info;
mod net;
mod notifications;
mod palette;
mod parity;
#[cfg(feature = "capture")]
mod perf;
mod plugins;
mod release_key;
mod rive;
mod rpc;
mod send_policy;
mod shell;
mod sounds;
mod speech;
mod spellcheck;
mod spinner;
mod spotify;
mod start_trace;
mod storage;
mod system_events;
mod text_buffer;
mod tray;
mod ui;
mod updater;
mod upload_flow;
mod video_clip;
mod video_vod;
mod webauthn;
#[cfg(feature = "capture")]
mod webhook_demo;

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

#[cfg_attr(not(feature = "capture"), allow(dead_code))]
fn find_upwards(name: &str) -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|path| path.exists())
}

/// Whether this is a development run, which may find the assets up from the working directory: a debug build, a
/// capture build, or a `--capture` run. An installed or portable release looks beside its exe and nowhere else, so a
/// stray `C:\app\assets` further up is never picked up.
fn development_run() -> bool {
    cfg!(debug_assertions) || storage::is_capture_run()
}

/// `app/assets` beside the exe, or for development runs up from the working directory.
fn assets_in(exe: Option<&Path>, cwd: Option<&Path>, development: bool) -> Option<PathBuf> {
    let beside = exe
        .and_then(Path::parent)
        .map(|dir| dir.join("app/assets"))
        .filter(|path| path.is_dir());
    beside.or_else(|| {
        if !development {
            return None;
        }
        cwd?.ancestors()
            .map(|dir| dir.join("app/assets"))
            .find(|path| path.is_dir())
    })
}

fn find_assets() -> Option<PathBuf> {
    assets_in(
        std::env::current_exe().ok().as_deref(),
        std::env::current_dir().ok().as_deref(),
        development_run(),
    )
}

/// A message box that names a problem the user cannot otherwise see (the release build has no console).
#[cfg(windows)]
pub(crate) fn native_message(title: &str, text: &str) {
    use ::windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
        core::HSTRING,
    };
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub(crate) fn native_message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
}

/// Appends `text` to `panic.log` in `dir`, newest last.
fn append_log(dir: &Path, text: &str) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("panic.log"))
    {
        use std::io::Write as _;
        let _ = writeln!(file, "{text}");
    }
}

/// What the user is told when startup fails: the whole error chain, plainly.
fn fatal_text(error: &anyhow::Error) -> String {
    format!("gravecord could not start.\n\n{error:#}")
}

/// A fatal error returned from `main`: written to the active profile's `panic.log`, and shown in a message box
/// unless this is a capture run, where it stays a logged error and a non-zero exit.
fn report_fatal(error: &anyhow::Error) {
    let text = fatal_text(error);
    tracing::error!(error = %format!("{error:#}"), "fatal startup error");
    eprintln!("{text}");
    if let Some(dir) = storage::data_dir() {
        append_log(&dir, &format!("fatal startup error: {error:#}\n"));
    }
    if !storage::is_capture_run() {
        native_message("gravecord", &text);
    }
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
            .inspect(|script| {
                #[cfg(feature = "capture")]
                perf::set_measuring(script);
                #[cfg(not(feature = "capture"))]
                let _ = script;
            })
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
        .cloned()
        // A harness run is started by a tool that passes no such argument: it names the file here.
        .or_else(|| std::env::var("GRAVECORD_LOG_FILE").ok())
        .and_then(|path| std::fs::File::create(path).ok());
    logging::init(file);
}

/// A panic on the UI thread aborts the process with nothing on screen, so it is written down first:
/// The active profile's `panic.log`, newest last. Capture failures stay outside the owner's profile.
fn record_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = storage::data_dir() {
            let backtrace = std::backtrace::Backtrace::force_capture();
            append_log(&dir, &format!("{info}\n{backtrace}\n"));
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
        // A replay run (`cargo xtask perf --replay`) signs in to the local server: no token is needed or read.
        if dc_proto::replay::endpoint().is_some() {
            return shell::Start::Token(replay_token());
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

/// Capture builds only: `GRAVECORD_REPLAY=<url>` plays a recording of Discord back from the local server at `url`
/// (`xtask/src/replay.rs`), `GRAVECORD_RECORD=<folder>` records what the real servers answer (`dc_proto::replay`).
#[cfg(feature = "capture")]
fn replay_switches() {
    if let Ok(url) = std::env::var("GRAVECORD_REPLAY") {
        dc_proto::replay::set_endpoint(&url);
        tracing::info!("replaying Discord from the local server");
    }
    // Both at once records what the replay answered: the recorder's own test.
    if let Some(dir) = std::env::var_os("GRAVECORD_RECORD") {
        match dc_proto::replay::start_recording(PathBuf::from(dir)) {
            Ok(()) => tracing::info!("recording what Discord answers"),
            Err(error) => tracing::warn!("the recording did not start: {error}"),
        }
    }
}

/// The login of a replay run: shaped like a token of the recorded account (`GRAVECORD_REPLAY_ACCOUNT`), so the start
/// cache's key matches as it does live; it only ever reaches the local server.
#[cfg(feature = "capture")]
fn replay_token() -> Token {
    use base64::Engine as _;
    let account = std::env::var("GRAVECORD_REPLAY_ACCOUNT").unwrap_or_else(|_| "0".to_owned());
    let first = base64::engine::general_purpose::STANDARD_NO_PAD.encode(account.as_bytes());
    Token::new(format!("{first}.replay.replay"))
}

/// Opens a link of the app in the main view. The view may still be starting (or showing the login), so this waits for
/// it, a minute at most.
async fn open_link_when_ready(
    shell: gpui::Entity<shell::Shell>,
    url: String,
    cx: &mut gpui::AsyncApp,
) {
    for _ in 0..300 {
        if shell.update(cx, |shell, cx| shell.open_link(&url, cx)) {
            return;
        }
        cx.background_executor()
            .timer(std::time::Duration::from_millis(200))
            .await;
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_fatal(&error);
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    start_trace::init();
    dc_proto::replay::set_stage_hook(start_trace::mark_ago);
    #[cfg(feature = "capture")]
    replay_switches();
    // The en-US strings, which every frame reads and which take about 40 ms to decompress and parse, load on their own
    // thread while the window opens, not on the UI thread inside the first frame.
    let _ = std::thread::Builder::new()
        .name("preload".to_owned())
        .spawn(|| {
            dc_intl::preload();
            start_trace::mark("intl_loaded");
        });
    inert::init();
    #[cfg(feature = "capture")]
    perf::start_heap_profile();
    #[cfg(feature = "capture")]
    perf::checkpoint("main");
    // Read once by gpui's renderer. Set before any other thread exists.
    if std::env::var_os("GRAVECORD_TEXT_GAMMA").is_none() {
        unsafe { std::env::set_var("GRAVECORD_TEXT_GAMMA", TEXT_GAMMA) };
    }
    // The installer's call: register `discord://` for this executable and exit. A capture run never does.
    if std::env::args().any(|arg| arg == deep_link::ARG_REGISTER) {
        if !storage::is_capture_run() {
            deep_link::register_protocol()
                .context("the discord:// protocol could not be registered")?;
        }
        return Ok(());
    }
    let (mut instance, instance_error) = match instance::claim() {
        Ok(instance::Launch::Forwarded) => return Ok(()),
        Ok(instance::Launch::Unresponsive) => {
            native_message("gravecord", instance::NOT_RESPONDING);
            return Ok(());
        }
        Ok(instance::Launch::Primary(instance)) => (instance, None),
        Err(error) => (None, Some(error)),
    };
    let acknowledger = instance.as_ref().map(instance::Instance::acknowledger);
    let restore_requests = instance
        .as_mut()
        .and_then(instance::Instance::take_requests);
    record_panics();
    record_log();
    // A tester build always keeps a log for bug reports (Settings > Report a Bug), other builds when it is switched on
    // there; a capture run never writes one.
    if logging::keep_log()
        && !storage::is_capture_run()
        && let Some(dir) = storage::data_dir()
    {
        logging::start_tester_file(&dir.join("logs"));
        tracing::info!(version = %updater::current_version(), "tester build");
    }
    if let Some(error) = instance_error {
        tracing::warn!(%error, "single-instance claim unavailable");
    }
    if storage::is_capture_run() {
        tracing::info!("single instance (capture no-op)");
        // A capture run plays voice messages without a speaker: nothing is audible.
        dc_voice::voice_player::set_silent(true);
    }
    notifications::configure_process();
    let assets_root = find_assets().context(
        "The program files are incomplete: the app/assets folder was not found next to gravecord.exe. Reinstall gravecord.",
    )?;
    let capture = capture_args();

    #[cfg(feature = "capture")]
    perf::checkpoint("pre_runtime");
    let start = initial_start();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    // With a stored login the network starts now, beside the fonts and the window: the gateway's connection, its
    // READY and the open channel's page do not wait for the first frame.
    let session = match &start {
        shell::Start::Token(token) => Some(net::Session::start(runtime.handle(), token.clone())),
        _ => None,
    };
    let saved_channel = ui::start_cache::saved_channel();
    let cache_task = match &start {
        shell::Start::Token(token) => {
            let account = ui::start_cache::token_account(token.expose());
            let requests = session.as_ref().map(|session| session.requests.clone());
            Some(std::thread::spawn(move || {
                let cached = ui::start_cache::load(account);
                // The page of the channel the app opens on is asked for as soon as that channel is known: the
                // one the cache kept (it is the place last shown), else the saved place's.
                let channel = cached
                    .as_ref()
                    .and_then(|cached| cached.messages.as_ref().map(|(channel, _)| *channel))
                    .or(saved_channel);
                if let (Some(channel), Some(requests)) = (channel, requests) {
                    let _ = requests.send(net::Request::PrefetchMessages { channel });
                }
                cached
            }))
        }
        _ => None,
    };
    let faces = match fonts::load_cached() {
        Some(faces) => faces,
        None => {
            // The fonts are public files: no login is needed (or known) yet.
            let font_client = dc_http::Client::new(Token::new(""), Identity::new())?;
            runtime.block_on(fonts::load(&font_client))?
        }
    };
    start_trace::mark("fonts_read");
    #[cfg(feature = "capture")]
    perf::mark(&perf::FONTS_MS);
    #[cfg(feature = "capture")]
    perf::checkpoint("fonts_read");
    let handle = runtime.handle().clone();
    // The Run value's arguments (`--start-minimized`, `--start-inactive`). Minimized only means anything with a signed
    // in session: the tray icon that brings the window back exists from then on, and the login screen has none.
    let flags = autostart::LaunchFlags::parse(std::env::args());
    let hidden =
        flags.minimized && !storage::is_capture_run() && matches!(start, shell::Start::Token(_));
    autostart::refresh_at_launch();

    application()
        .with_assets(Assets { root: assets_root })
        .run(move |cx: &mut App| {
            // Chromium draws this UI with grayscale antialiasing.
            cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
            // Keeps the decoded pictures within their budget (`ui/image_cache.rs`).
            ui::image_cache::start(cx);
            // Registering the faces takes about 170 ms. It runs beside the window and the shell's start (the gateway
            // connection begins in `Shell::new`), and is finished before this closure returns, so before any frame.
            let text_system = cx.text_system().clone();
            let fonts_added = std::thread::spawn(move || {
                text_system
                    .add_fonts(faces.into_iter().map(Cow::Owned).collect())
                    .expect("gg sans must load");
                #[cfg(feature = "capture")]
                perf::checkpoint("fonts_added");
            });
            let bounds = Bounds::centered(None, size(px(1280.), px(720.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(940.), px(500.))),
                        titlebar: Some(TitlebarOptions {
                            title: Some("gravecord".into()),
                            appears_transparent: true,
                            ..Default::default()
                        }),
                        // No window and no flash when started to the tray; no focus theft when started by Windows.
                        show: !hidden,
                        focus: !hidden && !flags.inactive,
                        ..Default::default()
                    },
                    move |window, cx| {
                        notifications::observe_window(window);
                        start_trace::mark("window_created");
                        let cached = cache_task.and_then(|task| task.join().ok()).flatten();
                        start_trace::mark("cache_joined");
                        cx.new(|cx| shell::Shell::new(handle, start, cached, session, window, cx))
                    },
                )
                .expect("the window must open");
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                // The desktop main-window close handler quits even when popouts remain.
                // Close-to-tray intercepts the native close before the window is removed.
                let main_window = window.window_id();
                cx.on_window_closed(move |cx, closed| {
                    if closed == main_window {
                        cx.quit();
                    }
                })
                .detach();
            }
            start_trace::mark("shell_built");
            #[cfg(feature = "capture")]
            perf::mark(&perf::WINDOW_MS);
            #[cfg(feature = "capture")]
            perf::checkpoint("window");
            if fonts_added.join().is_err() {
                panic!("gg sans must load");
            }
            start_trace::mark("fonts_added");
            // Capture runs only: `GRAVECORD_START_FRAMES=<folder>` saves the window at fixed moments after the
            // process started (`start-<ms>.png`), to see what the first seconds show.
            #[cfg(feature = "capture")]
            if let Some(folder) = std::env::var_os("GRAVECORD_START_FRAMES").map(PathBuf::from) {
                cx.spawn(async move |cx| {
                    for at in [0u64, 100, 250, 500, 1000, 1500, 2000, 3000] {
                        let now = start_trace::now_ms();
                        if at > now {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(at - now))
                                .await;
                        }
                        let taken = start_trace::now_ms();
                        cx.update(|cx| {
                            write_capture(
                                cx,
                                window.into(),
                                &folder.join(format!("start-{at}.png")),
                            )
                        });
                        tracing::debug!(target: "gravecord::start", at, taken, "start frame");
                    }
                })
                .detach();
            }
            let shell = window.entity(cx).expect("the shell exists");
            if hidden {
                let _ = window.update(cx, |_, window, _| autostart::place_hidden(window, bounds));
            } else if !flags.inactive {
                cx.activate(true);
            }

            // A `discord://` link this launch was started with opens once the main view is ready.
            if !storage::is_capture_run()
                && let Some(url) = deep_link::url_from_args(std::env::args())
                && deep_link::parse(&url).is_some()
            {
                let target = shell.clone();
                cx.spawn(async move |cx| open_link_when_ready(target, url, cx).await)
                    .detach();
            }
            if let Some(mut requests) = restore_requests {
                let link_target = shell.clone();
                cx.spawn(async move |cx| {
                    while requests.recv().await.is_some() {
                        if window
                            .update(cx, |_, window, _| notifications::restore_window(window))
                            .is_err()
                        {
                            break;
                        }
                        // A second launch that carried a link left it in a file before signalling.
                        if let Some(dir) = storage::data_dir()
                            && let Some(url) = deep_link::take_pending(&dir)
                        {
                            let target = link_target.clone();
                            cx.spawn(async move |cx| open_link_when_ready(target, url, cx).await)
                                .detach();
                        }
                        // Tells the launch that asked that this instance is alive and handled it.
                        if let Some(acknowledger) = &acknowledger {
                            acknowledger.acknowledge();
                        }
                    }
                })
                .detach();
            }

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
                        gpui::INERT.store(false, std::sync::atomic::Ordering::Relaxed);
                        cx.quit();
                    });
                })
                .detach();
            }
        });
    // Release the claim before media teardown, which can wait for a device driver.
    drop(instance);
    // The window closed: whatever Global Attenuation lowered goes back, and the call's recordings are written.
    dc_voice::shutdown();
    // A call still open leaves with the gateway's close.
    net::close_gateway_at_exit();
    Ok(())
}

#[cfg(feature = "capture")]
fn write_capture(cx: &mut App, window: gpui::AnyWindowHandle, path: &Path) {
    let result = window.update(cx, |_, window, cx| {
        parity::clear();
        ui::regions::prepare_hand_draw(window, cx);
        window.draw(cx).clear(cx);
        parity::write(&path.with_extension("layout.json"));
        parity::write_scene(&path.with_extension("scene.json"), window);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gc-main-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn assets_are_found_beside_the_exe_only_in_a_release_run() {
        let base = scratch("assets");
        // A stray folder far above the exe, and the real one beside it.
        std::fs::create_dir_all(base.join("app/assets")).unwrap();
        let installed = base.join("Programs/gravecord");
        std::fs::create_dir_all(&installed).unwrap();
        let exe = installed.join("gravecord.exe");
        assert_eq!(assets_in(Some(&exe), Some(&installed), false), None);
        assert_eq!(
            assets_in(Some(&exe), Some(&installed), true),
            Some(base.join("app/assets"))
        );
        std::fs::create_dir_all(installed.join("app/assets")).unwrap();
        assert_eq!(
            assets_in(Some(&exe), Some(&installed), false),
            Some(installed.join("app/assets"))
        );
        // A development run falls back to the working directory's ancestors, not the exe's.
        let build = base.join("target/debug");
        std::fs::create_dir_all(&build).unwrap();
        assert_eq!(
            assets_in(
                Some(&build.join("gravecord.exe")),
                Some(&base.join("target")),
                false
            ),
            None
        );
        assert_eq!(
            assets_in(
                Some(&build.join("gravecord.exe")),
                Some(&base.join("target")),
                true
            ),
            Some(base.join("app/assets"))
        );
        assert_eq!(assets_in(None, None, true), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_fatal_error_is_logged_to_the_profile_and_names_the_problem() {
        let dir = scratch("fatal");
        let error = anyhow::anyhow!("disk gone").context("app/assets not found");
        let text = fatal_text(&error);
        assert!(
            text.contains("could not start") && text.contains("app/assets not found: disk gone")
        );
        append_log(&dir, &format!("fatal startup error: {error:#}\n"));
        append_log(&dir, "second");
        let log = std::fs::read_to_string(dir.join("panic.log")).unwrap();
        assert!(log.contains("fatal startup error: app/assets not found: disk gone"));
        assert!(log.trim_end().ends_with("second"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
