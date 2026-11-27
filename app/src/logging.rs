//! The log file behind Debug Logging (Voice & Video > Advanced): "Saves debug logs to voice module folder that you can
//! upload to Discord Support". The official switch lives in the native voice module; here it switches a file logger
//! on and off while the app runs, writing the debug events of gravecord's own crates into a file of the voice folder.
//! `--log <file>` (runs driven from outside) keeps writing every debug event to its file, as before.
//!
//! Debug events may name user ids, so the files stay on this machine unless the user uploads them (Upload Logs).
//! Addresses are written as scheme, host and path only: every line passes `dc_http::redact::text` on its way into a
//! file, so no query string, fragment or user name and password of an address reaches one. gravecord's own log lines
//! print no request or response header.

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
/// A tester log file is closed and a new one started past this size, so a long session cannot fill the disk; with
/// [`KEEP_FILES`] the folder stays under 160 MiB.
pub const TESTER_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The always-on log of a tester build (`updater::tester_build`): its folder and the open file.
struct TesterSink {
    folder: PathBuf,
    file: File,
    written: u64,
}

struct Sinks {
    command_line: Option<File>,
    debug: Option<File>,
    tester: Option<TesterSink>,
}

static SINKS: Mutex<Sinks> = Mutex::new(Sinks {
    command_line: None,
    debug: None,
    tester: None,
});
static COMMAND_LINE: AtomicBool = AtomicBool::new(false);
static DEBUG: AtomicBool = AtomicBool::new(false);
static TESTER: AtomicBool = AtomicBool::new(false);

struct Writer;

/// One formatted event as it goes into a file: every address in it cut to scheme, host and path
/// (`dc_http::redact::text`), whichever crate wrote the line. The fmt layer hands over a whole event per write.
fn redacted(buffer: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match std::str::from_utf8(buffer) {
        Ok(text) => match dc_http::redact::text(text) {
            std::borrow::Cow::Borrowed(_) => std::borrow::Cow::Borrowed(buffer),
            std::borrow::Cow::Owned(safe) => std::borrow::Cow::Owned(safe.into_bytes()),
        },
        Err(_) => std::borrow::Cow::Owned(
            dc_http::redact::text(&String::from_utf8_lossy(buffer))
                .into_owned()
                .into_bytes(),
        ),
    }
}

impl Write for Writer {
    fn write(&mut self, written: &[u8]) -> std::io::Result<usize> {
        let taken = written.len();
        let safe = redacted(written);
        let buffer: &[u8] = &safe;
        if let Ok(mut sinks) = SINKS.lock() {
            let Sinks {
                command_line,
                debug,
                tester,
            } = &mut *sinks;
            for file in [command_line, debug].into_iter().flatten() {
                let _ = file.write_all(buffer);
            }
            if let Some(sink) = tester {
                if sink.written + buffer.len() as u64 > TESTER_FILE_BYTES
                    && let Some((file, _)) = new_file(&sink.folder, "gravecord")
                {
                    sink.file = file;
                    sink.written = 0;
                }
                if sink.file.write_all(buffer).is_ok() {
                    sink.written += buffer.len() as u64;
                }
            }
        }
        Ok(taken)
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
                || ((DEBUG.load(Ordering::Acquire) || TESTER.load(Ordering::Acquire))
                    && ours(meta.target())))
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

/// A new `<prefix>-<seconds>.log` in `folder`; the files beyond the newest [`KEEP_FILES`] are removed. The stamp gets
/// a counter when a file of that second exists already (a tester file rotating fast).
fn new_file(folder: &Path, prefix: &str) -> Option<(File, PathBuf)> {
    std::fs::create_dir_all(folder).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let path = (0..100)
        .map(|count| match count {
            0 => folder.join(format!("{prefix}-{stamp}.log")),
            _ => folder.join(format!("{prefix}-{stamp}-{count}.log")),
        })
        .find(|path| !path.exists())?;
    let file = File::create(&path).ok()?;
    for old in log_files(folder).into_iter().skip(KEEP_FILES) {
        let _ = std::fs::remove_file(old);
    }
    Some((file, path))
}

/// Switches the debug log file on: a new file in `folder`, the older ones beyond [`KEEP_FILES`] removed.
pub fn start_file(folder: &Path) -> Option<PathBuf> {
    let (file, path) = new_file(folder, "gravecord-voice")?;
    let mut sinks = SINKS.lock().ok()?;
    sinks.debug = Some(file);
    DEBUG.store(true, Ordering::Release);
    drop(sinks);
    tracing::callsite::rebuild_interest_cache();
    Some(path)
}

/// A tester build's log, on from start to exit: the debug events of gravecord's own crates in a new
/// `gravecord-<seconds>.log` of `folder`, a new file every [`TESTER_FILE_BYTES`], the newest [`KEEP_FILES`] kept.
/// Addresses in it have no queries (`dc_http::redact`) and tokens are never logged. Lines can still name user ids
/// and the paths of requests (a channel or message id), so read a file before posting it publicly.
pub fn start_tester_file(folder: &Path) -> Option<PathBuf> {
    let (file, path) = new_file(folder, "gravecord")?;
    let mut sinks = SINKS.lock().ok()?;
    sinks.tester = Some(TesterSink {
        folder: folder.to_owned(),
        file,
        written: 0,
    });
    TESTER.store(true, Ordering::Release);
    drop(sinks);
    tracing::callsite::rebuild_interest_cache();
    Some(path)
}

/// Switches the tester log off (Report a Bug's "Keep a log file" in a build that is not a tester build).
pub fn stop_tester_file() {
    TESTER.store(false, Ordering::Release);
    if let Ok(mut sinks) = SINKS.lock() {
        sinks.tester = None;
    }
    tracing::callsite::rebuild_interest_cache();
}

/// Whether this start keeps a log file: always in a tester build, otherwise when Report a Bug's switch (the
/// `logging.keep` storage key) is on.
pub fn keep_log() -> bool {
    crate::updater::tester_build()
        || crate::storage::get("logging.keep").is_some_and(|value| value.as_bool() == Some(true))
}

/// The folder of the tester log, when it is on.
pub fn tester_folder() -> Option<PathBuf> {
    SINKS
        .lock()
        .ok()?
        .tester
        .as_ref()
        .map(|sink| sink.folder.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A log line reaches a file without the query, fragment or credentials of any address in it, and a line without
    /// an address is written as it is.
    #[test]
    fn a_log_line_is_written_without_queries() {
        let line = b"WARN dc_http: cdn fetch failed: error sending request for url \
                     (https://media.example.com/a.gif?sig=secret#frag) /guilds/1/search\n";
        let safe = String::from_utf8(redacted(line).into_owned()).unwrap();
        assert_eq!(
            safe,
            "WARN dc_http: cdn fetch failed: error sending request for url \
             (https://media.example.com/a.gif) /guilds/1/search\n"
        );
        let plain = b"INFO app: voice connected\n";
        assert!(matches!(redacted(plain), std::borrow::Cow::Borrowed(_)));
    }
}
