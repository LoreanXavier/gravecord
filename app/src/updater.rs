//! Automatic updates. The states and timing follow the official client: the desktop updater reports checking,
//! downloading (`UPDATE_AVAILABLE`), ready (`UPDATE_DOWNLOADED`) and "no update" (`UPDATE_NOT_AVAILABLE`) to the
//! `AutoUpdateStore` (module 596339); the renderer's update manager (module 578152) checks when the connection opens and
//! then every hour (`setInterval(checkForUpdates, HOUR)`); an error (`UPDATE_ERROR`) shows nothing. The title bar's
//! button is in `ui.rs`; its click is `AUTO_UPDATER_QUIT_AND_INSTALL`.
//!
//! gravecord has no Squirrel and puts no file of its own in a release. As P.T.'s updater does, it asks GitHub's REST API
//! for the release (`/repos/{owner}/{repo}/releases/latest`; the Nightly channel that tester builds follow reads the
//! rolling `nightly` pre-release at `/releases/tags/nightly`) and takes its `gravecord-setup.exe` asset with GitHub's
//! upload digest. The repository is the workspace's unless the build names another (`GRAVECORD_UPDATE_REPOSITORY`;
//! testers cannot read a private one). The setup is downloaded into the app's data folder, checked against the digest
//! and then against the Ed25519-signed update manifest `cargo xtask package` appends to it (`verify_signed`,
//! `dc_release`, key in `release_key.rs`), and run silently with the installer's own arguments (`docs/installer.md`).
//! Settings > Updates shows the state, progress, failures and the channel (`ui/settings/tester.rs`).
//!
//! Safety rules, each with a test: HTTPS only; the setup URL and every redirect stay on the feed's host (a GitHub feed
//! may also use GitHub's download host); the hash is mandatory; a version that is not newer is ignored (after a
//! channel switch, any other version is taken, since a nightly sorts below its stable release); a missing feed
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

/// The stable channel's feed for the workspace's repository, as [`Channel::feed`] builds it: `update.json` of the
/// latest release. `GRAVECORD_UPDATE_FEED` or the `update.feed` storage key replace the channel's feed.
#[cfg(test)]
pub const DEFAULT_FEED: &str =
    "https://api.github.com/repos/LoreanXavier/gravecord/releases/latest";
/// The renderer checks again every hour (`HOUR` in module 578152).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// A release answer larger than this is not read (release notes and assets make it some kilobytes).
const MAX_FEED_BYTES: usize = 1024 * 1024;
/// The last release answer with its ETag, for a conditional request next time.
static LAST_FEED: Mutex<Option<(Url, String, Vec<u8>)>> = Mutex::new(None);
/// A setup file larger than this is refused whatever the feed says.
pub const MAX_SETUP_BYTES: u64 = 1 << 30;
const MAX_REDIRECTS: usize = 5;

/// This build's version: the package's, or the stamped one `cargo xtask package --channel nightly` gave a tester
/// build (`0.0.0-nightly.202611171830`).
pub fn current_version() -> Version {
    let text = option_env!("GRAVECORD_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    Version::parse(text).expect("the build version is a valid version")
}

/// Whether this is a tester build (`cargo xtask package --channel nightly`): it follows the nightly feed by default and
/// always keeps a log file (`crate::logging::start_tester_file`).
pub fn tester_build() -> bool {
    option_env!("GRAVECORD_CHANNEL") == Some("nightly")
}

/// The repository whose releases carry the feeds: `GRAVECORD_UPDATE_REPOSITORY` at build time (testers cannot read a
/// private repository's releases, so a tester build may name a public one), else the workspace's.
pub fn repository() -> &'static str {
    option_env!("GRAVECORD_UPDATE_REPOSITORY").unwrap_or(env!("CARGO_PKG_REPOSITORY"))
}

/// Which builds an install follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// The latest release.
    Stable,
    /// Every tester build: the rolling `nightly` release, replaced on each publish.
    Nightly,
}

impl Channel {
    fn key(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }

    /// The release of this channel in `repository` (`https://github.com/<owner>/<repo>`), asked of GitHub's REST API as
    /// P.T.'s updater does: no file of our own sits in the release. `latest` never resolves to a pre-release, so the
    /// nightly channel names its fixed tag.
    pub fn feed(self, repository: &str) -> String {
        let repository = repository.trim_end_matches('/');
        let api = repository.strip_prefix("https://github.com/").map_or_else(
            || repository.to_owned(),
            |path| format!("https://api.github.com/repos/{path}"),
        );
        match self {
            Self::Stable => format!("{api}/releases/latest"),
            Self::Nightly => format!("{api}/releases/tags/nightly"),
        }
    }
}

/// The channel in force: the `update.channel` storage key, else nightly for a tester build and stable otherwise.
pub fn channel() -> Channel {
    match crate::storage::get("update.channel")
        .and_then(|value| value.as_str().map(str::to_owned))
        .as_deref()
    {
        Some("nightly") => Channel::Nightly,
        Some("stable") => Channel::Stable,
        _ if tester_build() => Channel::Nightly,
        _ => Channel::Stable,
    }
}

/// Switches channel and checks again at once. A download already finished for the other channel is forgotten.
pub fn set_channel(channel: Channel) {
    crate::storage::set("update.channel", serde_json::json!(channel.key()));
    if let Ok(mut ready) = READY.lock() {
        *ready = None;
    }
    set_state(State::NotAvailable);
    request_check();
}

// ---- versions ----

/// `major.minor.patch` with an optional `-pre.release` part, compared as semantic versions are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    /// A tester build's version: `x.y.z-nightly.N`.
    pub fn is_nightly(&self) -> bool {
        self.pre.first().is_some_and(|part| part == "nightly")
    }

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

/// Whether `candidate` is the feed's own host, or for a feed on GitHub (the REST API at api.github.com, or github.com)
/// one of GitHub's: release files are served from github.com and redirected to its download hosts.
fn host_allowed(feed_host: &str, candidate: &str) -> bool {
    let (feed_host, candidate) = (
        feed_host.to_ascii_lowercase(),
        candidate.to_ascii_lowercase(),
    );
    let github = feed_host == "github.com" || feed_host == "api.github.com";
    candidate == feed_host
        || (github && (candidate == "github.com" || candidate.ends_with(".githubusercontent.com")))
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

/// The release asset that is the setup file.
pub const SETUP_ASSET: &str = "gravecord-setup.exe";

/// An update the feed offers: a GitHub release as the REST API describes it (`GET /repos/{owner}/{repo}/releases/latest`
/// or `/releases/tags/nightly`), reduced to its version and its setup asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub url: Url,
    pub size: u64,
    /// The asset's `digest` as GitHub computed it on upload: the whole signed setup file.
    pub sha256: [u8; 32],
    pub prerelease: bool,
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
    /// Parses a release. `feed` is where it came from: a relative address resolves against it, and the setup file must
    /// be on the feed's host (GitHub's file host for the API). The version is the tag (`v0.2.0`), or the release's name
    /// for the rolling `nightly` tag. A draft is refused; nothing here is trusted beyond choosing what to download: the
    /// file is checked against its digest and then against the signature it carries (`verify_signed`).
    pub fn parse(bytes: &[u8], feed: &Url, policy: Policy) -> Result<Self> {
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).context("the release answer is not valid")?;
        if !raw.is_object() {
            bail!("the release answer is not an object");
        }
        if raw["draft"].as_bool() == Some(true) {
            bail!("the release is a draft");
        }
        let prerelease = raw["prerelease"].as_bool().unwrap_or(false);
        let version = raw["tag_name"]
            .as_str()
            .and_then(|tag| Version::parse(tag).ok())
            .or_else(|| {
                raw["name"]
                    .as_str()
                    .and_then(|name| Version::parse(name).ok())
            })
            .context("the release names no version")?;
        let asset = raw["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|asset| asset["name"].as_str() == Some(SETUP_ASSET))
            .context("the release has no setup file")?;
        let text = |key: &str| {
            asset
                .get(key)
                .and_then(serde_json::Value::as_str)
                .with_context(|| format!("the setup file has no {key}"))
        };
        let size = asset
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .context("the setup file has no size")?;
        if size == 0 || size > MAX_SETUP_BYTES {
            bail!("the setup file's size is out of range");
        }
        let sha256 = decode_hash(
            text("digest")?
                .strip_prefix("sha256:")
                .context("the setup file's digest is not SHA-256")?,
        )?;
        let url = feed
            .join(text("browser_download_url")?.trim())
            .context("the setup address is not a URL")?;
        if !policy.scheme_allowed(&url) {
            bail!("the setup file must be fetched over https");
        }
        if !url.username().is_empty() || url.password().is_some() {
            bail!("the setup address is not allowed");
        }
        let feed_host = feed.host_str().unwrap_or_default().to_ascii_lowercase();
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let on_feed =
            host == feed_host && url.port_or_known_default() == feed.port_or_known_default();
        let on_github = feed_host == "api.github.com" && host == "github.com";
        if !(on_feed || on_github) {
            bail!("the setup file is not on the feed's host");
        }
        Ok(Self {
            version,
            url,
            size,
            sha256,
            prerelease,
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

/// What a channel accepts: a newer version, or after a channel switch any other version, since a nightly
/// (`0.0.0-nightly.N`) sorts below the stable release it leads to and the stable feed's build must still replace it.
pub fn is_offer(current: &Version, offered: &Version, channel: Channel) -> bool {
    let running_nightly = current.is_nightly();
    let switching = running_nightly != (channel == Channel::Nightly);
    if switching {
        offered != current
    } else {
        is_update(current, offered)
    }
}

fn build_client(feed: &Url, policy: Policy) -> Result<reqwest::Client> {
    let feed = feed.clone();
    // The plain client (`dc_http::plain`): a browser user agent and nothing of the Discord identity, no referer.
    dc_http::plain::builder()
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
#[cfg(test)]
pub async fn check(
    client: &reqwest::Client,
    feed: &Url,
    policy: Policy,
    current: &Version,
) -> Result<Check> {
    check_offer(client, feed, policy, &|offered| is_update(current, offered)).await
}

/// [`check`] with the channel's own rule for what counts as an offer ([`is_offer`]).
pub async fn check_offer(
    client: &reqwest::Client,
    feed: &Url,
    policy: Policy,
    accept: &(dyn Fn(&Version) -> bool + Sync),
) -> Result<Check> {
    // GitHub answers an unchanged release with 304, which does not count against the 60 requests an hour an address
    // may make without signing in.
    let cached = LAST_FEED
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(|(url, _, _)| url == feed);
    let mut request = client
        .get(feed.clone())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some((_, etag, _)) = &cached {
        request = request.header("If-None-Match", etag.as_str());
    }
    let response = request.send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Ok(Check::NoUpdate);
    }
    let body = if status == reqwest::StatusCode::NOT_MODIFIED
        && let Some((_, _, body)) = cached
    {
        body
    } else {
        if !status.is_success() {
            bail!("the update feed answered {status}");
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = read_limited(response, MAX_FEED_BYTES).await?;
        if let (Some(etag), Ok(mut last)) = (etag, LAST_FEED.lock()) {
            *last = Some((feed.clone(), etag, body.clone()));
        }
        body
    };
    let manifest = Manifest::parse(&body, feed, policy)?;
    Ok(if accept(&manifest.version) {
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

/// What the Updates settings page shows beyond the title bar's states, which the official client keeps silent about:
/// a check running, its failure, the download's progress, the last check's time and what the feed offered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub checking: bool,
    /// Seconds since 1970 of the last check that finished.
    pub last_checked: Option<u64>,
    /// The last check's failure, in words, until the next check succeeds.
    pub error: Option<String>,
    /// Bytes downloaded of the total.
    pub progress: Option<(u64, u64)>,
    /// This copy was not started from an install, so nothing here could apply an update.
    pub not_installed: bool,
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    checking: false,
    last_checked: None,
    error: None,
    progress: None,
    not_installed: false,
});

pub fn status() -> Status {
    STATUS
        .lock()
        .map_or_else(|_| Status::default(), |status| status.clone())
}

fn set_status(change: impl FnOnce(&mut Status)) {
    let changed = STATUS.lock().is_ok_and(|mut status| {
        let before = status.clone();
        change(&mut status);
        *status != before
    });
    if changed {
        notify_window();
    }
}

fn notify_window() {
    if let Ok(sender) = SENDER.lock()
        && let Some(sender) = sender.as_ref()
    {
        let _ = sender.send(Update::Updater);
    }
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
    notify_window();
}

/// The feed address in force: the environment, then the `update.feed` storage key, then the channel's feed.
pub fn configured_feed() -> Result<Url> {
    let text = std::env::var("GRAVECORD_UPDATE_FEED")
        .ok()
        .or_else(|| {
            crate::storage::get("update.feed").and_then(|value| value.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| channel().feed(repository()));
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
                            set_status(|status| status.checking = true);
                            let outcome = cycle().await;
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |elapsed| elapsed.as_secs());
                            let error = outcome.as_ref().err().map(|error| format!("{error:#}"));
                            if let Some(error) = &error {
                                // The official client shows nothing for an update error; the Updates page does.
                                tracing::debug!(%error, "update check failed");
                                set_state(State::NotAvailable);
                            }
                            set_status(|status| {
                                status.checking = false;
                                status.last_checked = Some(now);
                                status.error = error;
                                status.progress = None;
                            });
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
    let not_installed = installed_dir(&exe).is_none();
    set_status(|status| status.not_installed = not_installed);
    if not_installed {
        // Not an install: nothing here could apply an update.
        return Ok(());
    }
    let dir = crate::storage::data_dir()
        .context("no data folder")?
        .join("update");
    let feed = configured_feed()?;
    let client = build_client(&feed, Policy::STRICT)?;
    let current = current_version();
    let channel = channel();
    let accept = move |offered: &Version| is_offer(&current, offered, channel);
    let found = check_offer(&client, &feed, Policy::STRICT, &accept).await?;
    tracing::debug!(
        channel = channel.key(),
        current = %current_version(),
        offered = %match &found {
            Check::Available(manifest) => manifest.version.to_string(),
            Check::NoUpdate => "nothing newer".to_owned(),
        },
        "update check"
    );
    let found = match found {
        // The stable channel takes releases only, whatever the address answers.
        Check::Available(manifest) if channel == Channel::Stable && manifest.prerelease => {
            Check::NoUpdate
        }
        found => found,
    };
    let Check::Available(manifest) = found else {
        sweep(&dir, &[]);
        return Ok(());
    };
    let version = manifest.version.to_string();
    let wanted = dir.join(final_name(&manifest.version));
    sweep(&dir, &[wanted.clone(), wanted.with_extension("exe.part")]);
    set_state(State::Downloading {
        version: version.clone(),
    });
    let path = download(&client, &manifest, &dir, &|done, total| {
        // A step per percent is enough for the page; every chunk would wake the window thousands of times.
        let percent = |bytes: u64| bytes.saturating_mul(100) / total.max(1);
        let step = status().progress.map(|(before, _)| percent(before));
        if step != Some(percent(done)) || done == total {
            set_status(|status| status.progress = Some((done, total)));
        }
    })
    .await?;
    // The digest came from the same answer as the address; the signature did not. A setup whose signed manifest does
    // not name this version and these bytes is deleted and never run.
    if let Err(error) = verify_signed(&path, &manifest.version, &crate::release_key::PUBLIC_KEY) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    if let Ok(mut ready) = READY.lock() {
        *ready = Some(Ready { manifest, path });
    }
    set_state(State::Ready { version });
    Ok(())
}

/// Checks the update manifest a setup carries after its payload (`dc_release`), written and signed by
/// `cargo xtask package` with the release key: the Ed25519 signature against `public_key`, then that the manifest
/// names `version` and the SHA-256 and size of the bytes before it. As P.T.'s updater does, the release itself carries
/// no file of ours; a release swapped for another build, or one an outsider uploads, fails here.
pub fn verify_signed(path: &Path, version: &Version, public_key: &[u8; 32]) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let file = std::fs::read(path).context("could not read the downloaded file")?;
    let (body, manifest, signature) =
        dc_release::split(&file).context("the setup carries no signed update manifest")?;
    let key = VerifyingKey::from_bytes(public_key).context("the release key is not valid")?;
    key.verify(manifest, &Signature::from_bytes(signature))
        .map_err(|_| {
            anyhow::anyhow!("the setup's update manifest is not signed with the release key")
        })?;
    let manifest: serde_json::Value =
        serde_json::from_slice(manifest).context("the signed update manifest is not valid")?;
    if manifest["format"].as_u64() != Some(1) {
        bail!("the signed update manifest has an unknown format");
    }
    let named = manifest["version"]
        .as_str()
        .and_then(|text| Version::parse(text).ok())
        .context("the signed update manifest names no version")?;
    if &named != version {
        bail!("the setup is version {named}, not the {version} the release names");
    }
    let package = &manifest["package"];
    if package["size"].as_u64() != Some(body.len() as u64) {
        bail!("the setup is not the size its signed manifest names");
    }
    let sha256 = decode_hash(package["sha256"].as_str().unwrap_or_default())?;
    if Sha256::digest(body).as_slice() != sha256 {
        bail!("the setup does not match its signed manifest");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
