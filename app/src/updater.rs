//! Automatic updates. The states and timing follow the official client: the desktop updater reports checking,
//! downloading (`UPDATE_AVAILABLE`), ready (`UPDATE_DOWNLOADED`) and "no update" (`UPDATE_NOT_AVAILABLE`) to the
//! `AutoUpdateStore` (module 596339); the renderer's update manager (module 578152) checks when the connection opens and
//! then every hour (`setInterval(checkForUpdates, HOUR)`); an error (`UPDATE_ERROR`) shows nothing. The title bar's
//! button is in `ui.rs`; its click is `AUTO_UPDATER_QUIT_AND_INSTALL`.
//!
//! gravecord has no Squirrel, so the feed is a small JSON file (`update.json`: version, setup file, size, SHA-256)
//! next to the setup file of the latest GitHub release of the repository named in the workspace metadata. The setup
//! file is downloaded into the app's data folder, verified against the feed's size and hash, and run silently with the
//! installer's own arguments (`docs/installer.md`).
//!
//! Safety rules, each with a test: HTTPS only; the setup URL and every redirect stay on the feed's host (a GitHub feed
//! may also use GitHub's download host); the hash is mandatory; a version that is not newer is ignored; a missing feed
//! (404) means "no update" with no error; a file that fails verification is deleted; nothing is run from a capture run.

use crate::net::Update;
use anyhow::{Context, Result, bail};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// The default feed: `update.json` of the latest release of the repository in the workspace metadata. The single
/// place that names it; `GRAVECORD_UPDATE_FEED` or the `update.feed` storage key replace it.
pub const DEFAULT_FEED: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/latest/download/update.json"
);
/// The renderer checks again every hour (`HOUR` in module 578152).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// A feed larger than this is not read.
const MAX_FEED_BYTES: usize = 64 * 1024;
/// A setup file larger than this is refused whatever the feed says.
pub const MAX_SETUP_BYTES: u64 = 1 << 30;
const MAX_REDIRECTS: usize = 5;

/// This build's version.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is a valid version")
}

// ---- versions ----

/// `major.minor.patch` with an optional `-pre.release` part, compared as semantic versions are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else {
            bail!("version {text:?} is not major.minor.patch");
        };
        let number = |part: &str| -> Result<u64> {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                bail!("version {text:?} has a part that is not a number");
            }
            part.parse().context("version number out of range")
        };
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let identifiers: Vec<String> = pre.split('.').map(str::to_owned).collect();
                if identifiers.iter().any(|part| {
                    part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
                }) {
                    bail!("version {text:?} has a bad pre-release part");
                }
                identifiers
            }
        };
        Ok(Self {
            core: [number(major)?, number(minor)?, number(patch)?],
            pre,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.core[0], self.core[1], self.core[2])?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                // A release is newer than any pre-release of the same numbers.
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            })
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_pre(left: &[String], right: &[String]) -> Ordering {
    for (a, b) in left.iter().zip(right) {
        let ordering = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

// ---- policy ----

/// What a feed and its files may be. The app always uses `STRICT`; the loopback form exists for the tests that serve
/// a feed from `127.0.0.1`.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    allow_loopback_http: bool,
}

impl Policy {
    pub const STRICT: Self = Self {
        allow_loopback_http: false,
    };

    #[cfg(test)]
    pub fn loopback_for_tests() -> Self {
        Self {
            allow_loopback_http: true,
        }
    }

    fn scheme_allowed(self, url: &Url) -> bool {
        match url.scheme() {
            "https" => true,
            "http" => {
                self.allow_loopback_http
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            }
            _ => false,
        }
    }

    /// Reads the feed address: absolute, allowed scheme, a host, no credentials.
    pub fn feed_url(self, text: &str) -> Result<Url> {
        let url = Url::parse(text.trim()).context("the update feed address is not a URL")?;
        if !self.scheme_allowed(&url) {
            bail!("the update feed must use https");
        }
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            bail!("the update feed address is not allowed");
        }
        Ok(url)
    }
}

/// Whether `candidate` is the feed's own host, or (for a feed on github.com) a GitHub download host.
fn host_allowed(feed_host: &str, candidate: &str) -> bool {
    let (feed_host, candidate) = (
        feed_host.to_ascii_lowercase(),
        candidate.to_ascii_lowercase(),
    );
    candidate == feed_host
        || (feed_host == "github.com" && candidate.ends_with(".githubusercontent.com"))
}

fn redirect_allowed(feed: &Url, target: &Url, policy: Policy) -> bool {
    policy.scheme_allowed(target)
        && target.username().is_empty()
        && match (feed.host_str(), target.host_str()) {
            (Some(feed), Some(target)) => host_allowed(feed, target),
            _ => false,
        }
}

// ---- the feed ----

/// An update the feed offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub url: Url,
    pub size: u64,
    pub sha256: [u8; 32],
}

fn decode_hash(text: &str) -> Result<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        bail!("the feed's sha256 is not 64 hex digits");
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .context("the feed's sha256 is not hex")?;
    }
    Ok(out)
}

impl Manifest {
    /// Parses the feed. `feed` is where it came from: a relative `url` resolves against it, and the setup file must
    /// be on the same host and port.
    pub fn parse(bytes: &[u8], feed: &Url, policy: Policy) -> Result<Self> {
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).context("the update feed is not valid")?;
        let text = |key: &str| {
            raw.get(key)
                .and_then(serde_json::Value::as_str)
                .with_context(|| format!("the update feed has no {key}"))
        };
        let version = Version::parse(text("version")?)?;
        let size = raw
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .context("the update feed has no size")?;
        if size == 0 || size > MAX_SETUP_BYTES {
            bail!("the feed's size is out of range");
        }
        let sha256 = decode_hash(text("sha256")?)?;
        let url = feed
            .join(text("url")?.trim())
            .context("the feed's setup address is not a URL")?;
        if !policy.scheme_allowed(&url) {
            bail!("the setup file must be fetched over https");
        }
        if !url.username().is_empty() || url.password().is_some() {
            bail!("the setup address is not allowed");
        }
        if url.host_str().map(str::to_ascii_lowercase)
            != feed.host_str().map(str::to_ascii_lowercase)
            || url.port_or_known_default() != feed.port_or_known_default()
        {
            bail!("the setup file is not on the feed's host");
        }
        Ok(Self {
            version,
            url,
            size,
            sha256,
        })
    }
}

/// The outcome of one feed check.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// No feed (404), or nothing newer than this build: a normal outcome, never an error.
    NoUpdate,
    Available(Manifest),
}

/// Whether the feed's version is an update for `current`. An equal or older one (a downgrade) is refused.
pub fn is_update(current: &Version, offered: &Version) -> bool {
    offered > current
}

fn build_client(feed: &Url, policy: Policy) -> Result<reqwest::Client> {
    let feed = feed.clone();
    reqwest::Client::builder()
        .user_agent(concat!("gravecord/", env!("CARGO_PKG_VERSION")))
        .https_only(!policy.allow_loopback_http)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if redirect_allowed(&feed, attempt.url(), policy) {
                attempt.follow()
            } else {
                attempt.error("redirect to another host refused")
            }
        }))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .context("could not build the update client")
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            bail!("the update feed is too large");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Fetches the feed. 404 and 410 are "no update"; any other failure is an error the caller only logs.
pub async fn check(
    client: &reqwest::Client,
    feed: &Url,
    policy: Policy,
    current: &Version,
) -> Result<Check> {
    let response = client.get(feed.clone()).send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Ok(Check::NoUpdate);
    }
    if !status.is_success() {
        bail!("the update feed answered {status}");
    }
    let body = read_limited(response, MAX_FEED_BYTES).await?;
    let manifest = Manifest::parse(&body, feed, policy)?;
    Ok(if is_update(current, &manifest.version) {
        Check::Available(manifest)
    } else {
        Check::NoUpdate
    })
}

// ---- the file ----

/// Checks size and SHA-256 of a file against the feed.
pub fn verify_file(path: &Path, size: u64, sha256: &[u8; 32]) -> Result<()> {
    let mut file = std::fs::File::open(path).context("could not open the downloaded file")?;
    if file.metadata()?.len() != size {
        bail!("the downloaded file has the wrong size");
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if hasher.finalize().as_slice() != sha256 {
        bail!("the downloaded file does not match the feed's SHA-256");
    }
    Ok(())
}

fn final_name(version: &Version) -> String {
    format!("gravecord-setup-{version}.exe")
}

/// Downloads the setup file into `dir`, verified. A file left by an earlier run is reused when it verifies. An
/// interrupted download is kept as `<name>.part` and resumed with a range request; when the server does not honour it,
/// or the finished file fails verification, the partial file is deleted and (for the first) started again.
pub async fn download(
    client: &reqwest::Client,
    manifest: &Manifest,
    dir: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let done = dir.join(final_name(&manifest.version));
    if done.exists() {
        if verify_file(&done, manifest.size, &manifest.sha256).is_ok() {
            return Ok(done);
        }
        let _ = std::fs::remove_file(&done);
    }
    let part = done.with_extension("exe.part");
    for attempt in 0..2 {
        match fetch_into(client, manifest, &part, progress).await {
            Ok(()) => break,
            Err(Fetch::Restart) if attempt == 0 => {
                let _ = std::fs::remove_file(&part);
            }
            Err(Fetch::Restart) => {
                let _ = std::fs::remove_file(&part);
                bail!("the server would not serve the setup file");
            }
            Err(Fetch::Failed(error)) => return Err(error),
        }
    }
    if let Err(error) = verify_file(&part, manifest.size, &manifest.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(error);
    }
    std::fs::rename(&part, &done).context("could not finish the downloaded file")?;
    Ok(done)
}

enum Fetch {
    /// The partial file cannot be continued: delete it and ask again from the start.
    Restart,
    /// A network or file failure. A partial file stays for the next try.
    Failed(anyhow::Error),
}
impl From<anyhow::Error> for Fetch {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}
impl From<reqwest::Error> for Fetch {
    fn from(error: reqwest::Error) -> Self {
        Self::Failed(error.into())
    }
}
impl From<std::io::Error> for Fetch {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.into())
    }
}

async fn fetch_into(
    client: &reqwest::Client,
    manifest: &Manifest,
    part: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<(), Fetch> {
    let have = std::fs::metadata(part).map_or(0, |meta| meta.len());
    if have > manifest.size {
        return Err(Fetch::Restart);
    }
    if have == manifest.size {
        // A complete partial file: nothing to fetch, the caller verifies it.
        return Ok(());
    }
    let mut request = client.get(manifest.url.clone());
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request.send().await?;
    let status = response.status();
    if have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        let starts_here = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with(&format!("bytes {have}-")));
        if !starts_here {
            return Err(Fetch::Restart);
        }
        let file = std::fs::OpenOptions::new().append(true).open(part)?;
        return stream(response, file, manifest, have, part, progress).await;
    }
    if status == reqwest::StatusCode::OK {
        // Everything is coming (no range asked, or the server ignored it): take it from the start.
        let file = std::fs::File::create(part)?;
        return stream(response, file, manifest, 0, part, progress).await;
    }
    if have > 0 {
        // 416 or anything else to a range request: the partial file cannot be continued.
        return Err(Fetch::Restart);
    }
    Err(Fetch::Failed(anyhow::anyhow!(
        "the setup file answered {status}"
    )))
}

async fn stream(
    mut response: reqwest::Response,
    mut file: std::fs::File,
    manifest: &Manifest,
    mut have: u64,
    part: &Path,
    progress: &dyn Fn(u64, u64),
) -> Result<(), Fetch> {
    progress(have, manifest.size);
    while let Some(chunk) = response.chunk().await? {
        have += chunk.len() as u64;
        if have > manifest.size {
            drop(file);
            let _ = std::fs::remove_file(part);
            return Err(Fetch::Failed(anyhow::anyhow!(
                "the setup file is larger than the feed says"
            )));
        }
        file.write_all(&chunk)?;
        progress(have, manifest.size);
    }
    file.flush()?;
    Ok(())
}

/// Removes everything in `dir` except `keep` (a finished file and its partial one).
fn sweep(dir: &Path, keep: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !keep.contains(&entry.path()) && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---- applying ----

/// A verified setup file waiting to be run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub manifest: Manifest,
    pub path: PathBuf,
}

/// The installer's arguments for a silent update in place that starts the app again (`docs/installer.md`).
pub fn setup_args(install_dir: &Path) -> Vec<OsString> {
    vec![
        "--silent".into(),
        "--launch".into(),
        "--dir".into(),
        install_dir.as_os_str().to_owned(),
    ]
}

/// Runs the setup file, after checking it once more. Refused in a capture run. `run` starts the process; the tests
/// give it a recorder so nothing is ever executed.
pub fn apply_with(
    ready: &Ready,
    install_dir: &Path,
    capture: bool,
    run: impl FnOnce(&Path, &[OsString]) -> std::io::Result<()>,
) -> Result<()> {
    if capture {
        bail!("updates are never applied from a capture run");
    }
    verify_file(&ready.path, ready.manifest.size, &ready.manifest.sha256)?;
    run(&ready.path, &setup_args(install_dir)).context("could not start the setup file")
}

#[cfg(windows)]
fn spawn_detached(path: &Path, args: &[OsString]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(path)
        .args(args)
        .creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
}

#[cfg(not(windows))]
fn spawn_detached(path: &Path, args: &[OsString]) -> std::io::Result<()> {
    std::process::Command::new(path)
        .args(args)
        .spawn()
        .map(drop)
}

/// The folder of the running exe when it is a gravecord install (the installer's record is beside it). Updates apply
/// to installs only: a portable folder or a development build is never replaced.
pub fn installed_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    dir.join("install.manifest")
        .is_file()
        .then(|| dir.to_owned())
}

// ---- state shared with the window ----

/// What the title bar button shows. `NotAvailable` also covers checking and errors: the official client shows
/// nothing for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    NotAvailable,
    /// `UPDATE_AVAILABLE`: the download runs; the button is shown and disabled, "Downloading Update".
    Downloading {
        version: String,
    },
    /// `UPDATE_DOWNLOADED`: "Update Ready!", the button is live.
    Ready {
        version: String,
    },
}

static STATE: Mutex<State> = Mutex::new(State::NotAvailable);
static READY: Mutex<Option<Ready>> = Mutex::new(None);
static SENDER: Mutex<Option<UnboundedSender<Update>>> = Mutex::new(None);

#[derive(Default)]
struct CheckRequests {
    busy: AtomicBool,
    wake: tokio::sync::Notify,
}

impl CheckRequests {
    fn begin(&self) -> bool {
        self.busy
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    }

    fn finish(&self) {
        self.busy.store(false, AtomicOrdering::Release);
    }

    fn request(&self, capture: bool, idle: bool) -> bool {
        // Desktop checkForUpdatesWithUpdater ignores another request while checking/downloading/ready.
        if capture || !idle || !self.begin() {
            return false;
        }
        self.wake.notify_one();
        true
    }
}

static CHECK_REQUESTS: LazyLock<CheckRequests> = LazyLock::new(CheckRequests::default);

/// The tray's manual check wakes the existing worker; it never starts a second download.
pub fn request_check() -> bool {
    let capture = crate::storage::is_capture_run();
    if capture {
        tracing::info!("update check (capture no-op)");
    }
    CHECK_REQUESTS.request(capture, matches!(state(), State::NotAvailable))
}

pub fn state() -> State {
    STATE
        .lock()
        .map_or(State::NotAvailable, |state| state.clone())
}

fn set_state(state: State) {
    if let Ok(mut current) = STATE.lock() {
        if *current == state {
            return;
        }
        *current = state;
    }
    if let Ok(sender) = SENDER.lock()
        && let Some(sender) = sender.as_ref()
    {
        let _ = sender.send(Update::Updater);
    }
}

/// The feed address in force: the environment, then the `update.feed` storage key, then the default.
fn configured_feed() -> Result<Url> {
    let text = std::env::var("GRAVECORD_UPDATE_FEED")
        .ok()
        .or_else(|| {
            crate::storage::get("update.feed").and_then(|value| value.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| DEFAULT_FEED.to_owned());
    Policy::STRICT.feed_url(&text)
}

/// The title bar button was clicked while "Update Ready!" showed: starts the verified setup file. The caller quits
/// the app afterwards; the setup waits for it to close and starts the new one (`--launch`).
pub fn quit_and_install() -> Result<()> {
    let ready = READY
        .lock()
        .ok()
        .and_then(|ready| ready.clone())
        .context("no update is ready")?;
    let exe = std::env::current_exe()?;
    let dir = installed_dir(&exe).context("this copy of gravecord is not installed")?;
    apply_with(
        &ready,
        &dir,
        crate::storage::is_capture_run(),
        spawn_detached,
    )
}

/// Starts the background updater, once, and lets it reach the window of the current session. Does nothing in a
/// capture run.
pub fn attach(sender: UnboundedSender<Update>) {
    if crate::storage::is_capture_run() {
        return;
    }
    if let Ok(mut slot) = SENDER.lock() {
        *slot = Some(sender);
    }
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("updater".into())
            .spawn(|| {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async {
                    let mut requested = false;
                    loop {
                        if requested || CHECK_REQUESTS.begin() {
                            if let Err(error) = cycle().await {
                                // The official client shows nothing for an update error.
                                tracing::debug!(error = %format!("{error:#}"), "update check failed");
                                set_state(State::NotAvailable);
                            }
                            CHECK_REQUESTS.finish();
                        }
                        requested = tokio::select! {
                            _ = tokio::time::sleep(CHECK_INTERVAL) => false,
                            _ = CHECK_REQUESTS.wake.notified() => true,
                        };
                    }
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "the updater thread did not start");
        }
    });
}

/// One check, and the download when something newer is offered.
async fn cycle() -> Result<()> {
    if matches!(state(), State::Ready { .. }) {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if installed_dir(&exe).is_none() {
        // Not an install: nothing here could apply an update.
        return Ok(());
    }
    let dir = crate::storage::data_dir()
        .context("no data folder")?
        .join("update");
    let feed = configured_feed()?;
    let client = build_client(&feed, Policy::STRICT)?;
    let Check::Available(manifest) =
        check(&client, &feed, Policy::STRICT, &current_version()).await?
    else {
        sweep(&dir, &[]);
        return Ok(());
    };
    let version = manifest.version.to_string();
    let wanted = dir.join(final_name(&manifest.version));
    sweep(&dir, &[wanted.clone(), wanted.with_extension("exe.part")]);
    set_state(State::Downloading {
        version: version.clone(),
    });
    let path = download(&client, &manifest, &dir, &|_, _| {}).await?;
    if let Ok(mut ready) = READY.lock() {
        *ready = Some(Ready { manifest, path });
    }
    set_state(State::Ready { version });
    Ok(())
}

#[cfg(test)]
mod tests;
