//! The log file behind Debug Logging (Voice & Video > Advanced): "Saves debug logs to voice module folder that you can
//! upload to Discord Support". The official switch lives in the native voice module; here it switches a file logger
//! on and off while the app runs, writing the debug events of gravecord's own crates into a file of the voice folder.
//! `--log <file>` (live tests driven from outside) keeps writing every debug event to its file, as before.
//!
//! Debug events may name user ids, so the files stay on this machine unless the user uploads them (Upload Logs).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, filter::filter_fn, fmt};

/// How many log files the voice folder keeps (the newest).
pub const KEEP_FILES: usize = 5;

struct Sinks {
    command_line: Option<File>,
    debug: Option<File>,
}

static SINKS: Mutex<Sinks> = Mutex::new(Sinks {
    command_line: None,
    debug: None,
});
static COMMAND_LINE: AtomicBool = AtomicBool::new(false);
static DEBUG: AtomicBool = AtomicBool::new(false);

struct Writer;

impl Write for Writer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut sinks) = SINKS.lock() {
            let Sinks {
                command_line,
                debug,
            } = &mut *sinks;
            for file in [command_line, debug].into_iter().flatten() {
                let _ = file.write_all(buffer);
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Writer {
    type Writer = Writer;

    fn make_writer(&'a self) -> Writer {
        Writer
    }
}

/// Whether a debug log file takes an event of this target: the app and its own crates, not the HTTP stack's chatter.
fn ours(target: &str) -> bool {
    target.starts_with("gravecord") || target.starts_with("dc_") || target.starts_with("app")
}

/// Installs the global subscriber. Nothing is written until a file is open.
pub fn init(command_line: Option<File>) {
    if let Ok(mut sinks) = SINKS.lock() {
        COMMAND_LINE.store(command_line.is_some(), Ordering::Release);
        sinks.command_line = command_line;
    }
    let filter = filter_fn(|meta| {
        *meta.level() <= Level::DEBUG
            && (COMMAND_LINE.load(Ordering::Acquire)
                || (DEBUG.load(Ordering::Acquire) && ours(meta.target())))
    });
    let _ = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(Writer)
                .with_ansi(false)
                .with_filter(filter),
        )
        .try_init();
}

/// The `.log` files of a folder, newest first.
pub fn log_files(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found.into_iter().map(|(_, path)| path).collect()
}

/// Switches the debug log file on: a new file in `folder`, the older ones beyond [`KEEP_FILES`] removed.
pub fn start_file(folder: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(folder).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let path = folder.join(format!("gravecord-voice-{stamp}.log"));
    let file = File::create(&path).ok()?;
    let mut sinks = SINKS.lock().ok()?;
    sinks.debug = Some(file);
    DEBUG.store(true, Ordering::Release);
    drop(sinks);
    for old in log_files(folder).into_iter().skip(KEEP_FILES) {
        let _ = std::fs::remove_file(old);
    }
    tracing::callsite::rebuild_interest_cache();
    Some(path)
}

/// Switches it off and closes the file.
pub fn stop_file() {
    DEBUG.store(false, Ordering::Release);
    if let Ok(mut sinks) = SINKS.lock() {
        sinks.debug = None;
    }
    tracing::callsite::rebuild_interest_cache();
}

pub fn file_open() -> bool {
    DEBUG.load(Ordering::Acquire)
}
