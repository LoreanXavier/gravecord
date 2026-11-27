//! Game detection at run time: the process scan loop, the detectable lists kept on disk, and the presence update a
//! detected game makes.
//!
//! The official client does the scanning in a native process observer that reports to `RunningGameStore` (module
//! 397369); the lists are `GameStore` (module 760751, CDN `detectables/games-v1.json` and
//! `detectables/non-games-v1.json` with ETag revalidation, and `GET /games/detectable/exclusions`), cached on disk
//! and refetched once a day (`detectableGamesTtl`). The presence update is `ConnectionStore`'s `presenceUpdate`
//! (module 225123): status, idle-since, activities and afk, at most five in twenty seconds.
//!
//! Nothing here runs in a harness run: the scan thread is never started and [`presence_refused`] stops every
//! presence update, so a capture can neither detect a game nor change the test account's status.

use dc_detect::{Blocklist, Candidate, DetectableGame, Detectables, Detected};
use serde_json::{Value, json};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// How often the process table is read. The native observer's interval is not in the snapshot; Serein scans every
/// ten seconds (`SCAN_INTERVAL` in `apps/desktop/src/game_activity.rs`).
const SCAN_INTERVAL: Duration = Duration::from_secs(10);

/// `detectableGamesTtl` (module 760751): the lists are refetched after a day.
const LIST_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// `ConnectionStore`'s presence rate limit (module 225123): five updates in twenty seconds.
const PRESENCE_BURST: usize = 5;
const PRESENCE_WINDOW_MS: u64 = 20_000;

/// The three lists and their endpoints.
pub const GAMES_KEY: &str = "games-detectable";
pub const NON_GAMES_KEY: &str = "games-non-games";
pub const BLOCKLIST_KEY: &str = "games-blocklist";
pub const GAMES_CDN_URL: &str = "https://cdn.discordapp.com/detectables/games-v1.json";
pub const NON_GAMES_CDN_URL: &str = "https://cdn.discordapp.com/detectables/non-games-v1.json";
pub const BLOCKLIST_ROUTE: &str = "/games/detectable/exclusions";

/// What one scan found.
#[derive(Clone, Debug, Default)]
pub struct GameScan {
    pub spotify_running: bool,
    pub games: Vec<Detected>,
    pub candidates: Vec<Candidate>,
    pub foreground: Option<u32>,
    /// Running OBS, XSplit and the other streamer tools (`RUNNING_STREAMER_TOOLS_CHANGE`'s count).
    pub streamer_tools: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DueFetch {
    Cdn {
        key: &'static str,
        url: &'static str,
        etag: Option<String>,
    },
    Api {
        key: &'static str,
        route: &'static str,
    },
}

#[derive(Default)]
struct Shared {
    lists: Detectables,
    generation: u64,
    games_from_net: bool,
    non_games_from_net: bool,
    blocklist_from_net: bool,
}

type WakeSignal = Arc<(Mutex<u64>, Condvar)>;

fn notify_wake(wake: &WakeSignal) {
    if let Ok(mut seq) = wake.0.lock() {
        *seq = seq.wrapping_add(1);
        wake.1.notify_all();
    }
}

/// The scan thread's handle and metadata of the cached lists on disk.
pub struct Runtime {
    shared: Arc<Mutex<Shared>>,
    wake: WakeSignal,
    updates: UnboundedSender<crate::net::Update>,
    started: bool,
    meta: CacheMeta,
    requested: Vec<&'static str>,
}

/// Small per-list metadata stored alongside `detectables.json` so startup reads timestamps and ETags without
/// parsing multi-megabyte JSON on the UI thread.
#[derive(Clone, Default)]
struct MetaEntry {
    at: u64,
    etag: Option<String>,
    cdn: bool,
}

#[derive(Clone, Default)]
struct CacheMeta {
    games: Option<MetaEntry>,
    non_games: Option<MetaEntry>,
    blocklist: Option<MetaEntry>,
}

static CACHE_IO: Mutex<()> = Mutex::new(());

fn cache_path() -> Option<std::path::PathBuf> {
    crate::storage::data_dir().map(|dir| dir.join("detectables.json"))
}

fn meta_path() -> Option<std::path::PathBuf> {
    crate::storage::data_dir().map(|dir| dir.join("detectables.meta.json"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn atomic_write_json(path: &std::path::Path, value: &Value) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(value)
        && std::fs::write(&temporary, bytes).is_ok()
    {
        let _ = std::fs::rename(&temporary, path);
    }
}

impl CacheMeta {
    fn from_value(value: &Value) -> Self {
        let part = |name: &str| -> Option<MetaEntry> {
            let entry = value.get(name)?;
            Some(MetaEntry {
                at: entry["at"].as_u64()?,
                etag: entry["etag"].as_str().map(str::to_owned),
                cdn: entry["cdn"].as_bool().unwrap_or(false),
            })
        };
        Self {
            games: part("games"),
            non_games: part("nonGames"),
            blocklist: part("blocklist"),
        }
    }

    fn to_value(&self) -> Value {
        let entry = |part: &Option<MetaEntry>| {
            part.as_ref().map_or(Value::Null, |meta| {
                json!({
                    "at": meta.at,
                    "etag": meta.etag,
                    "cdn": meta.cdn,
                })
            })
        };
        json!({
            "games": entry(&self.games),
            "nonGames": entry(&self.non_games),
            "blocklist": entry(&self.blocklist),
        })
    }

    fn read_if_cache_exists() -> Self {
        if cache_path().is_none_or(|path| !path.exists()) {
            return Self::default();
        }
        let Some(value) = meta_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            return Self::default();
        };
        Self::from_value(&value)
    }

    fn write(&self) {
        if cfg!(test) {
            return;
        }
        let _guard = CACHE_IO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(path) = meta_path() {
            atomic_write_json(&path, &self.to_value());
        }
    }
}

fn cache_field(key: &str) -> Option<&'static str> {
    match key {
        GAMES_KEY => Some("games"),
        NON_GAMES_KEY => Some("nonGames"),
        BLOCKLIST_KEY => Some("blocklist"),
        _ => None,
    }
}

/// Writes one updated list (`games`, `nonGames`, or `blocklist`) into `detectables.json` and `detectables.meta.json`.
/// Intended for background workers (`tokio::task::spawn_blocking`), never the UI thread.
pub(crate) fn persist_cache_part(
    key: &str,
    now_ms: u64,
    etag: Option<&str>,
    cdn: bool,
    body: Option<&Value>,
) {
    if cfg!(test) {
        return;
    }
    let Some(field) = cache_field(key) else {
        return;
    };
    let _guard = CACHE_IO.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = cache_path() else {
        return;
    };
    let mut doc = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({}));
    if let Some(body) = body {
        doc[field] = json!({
            "at": now_ms,
            "etag": etag,
            "cdn": cdn,
            "body": body,
        });
        atomic_write_json(&path, &doc);
    } else if let Some(entry) = doc.get_mut(field).and_then(Value::as_object_mut) {
        entry.insert("at".to_owned(), json!(now_ms));
        if let Some(etag) = etag {
            entry.insert("etag".to_owned(), json!(etag));
        }
        entry.insert("cdn".to_owned(), json!(cdn));
        atomic_write_json(&path, &doc);
    }
    let meta = CacheMeta::from_value(&doc);
    if let Some(meta_file) = meta_path() {
        atomic_write_json(&meta_file, &meta.to_value());
    }
}

/// Whether a list fetched at `at` is stale (`canFetchDetectableGames` and its siblings).
fn stale(at: Option<u64>, now_ms: u64) -> bool {
    at.is_none_or(|at| now_ms >= at.saturating_add(LIST_TTL_MS))
}

impl Runtime {
    pub fn new(updates: UnboundedSender<crate::net::Update>) -> Self {
        let meta = if cfg!(test) {
            CacheMeta::default()
        } else {
            CacheMeta::read_if_cache_exists()
        };
        let shared = Arc::new(Mutex::new(Shared::default()));
        let wake: WakeSignal = Arc::default();
        if !cfg!(test) && (!crate::storage::is_capture_run() || live_presence_allowed()) {
            let shared_load = shared.clone();
            let wake_load = wake.clone();
            let _ = std::thread::Builder::new()
                .name("detectables-load".into())
                .spawn(move || {
                    load_disk_cache_into(&shared_load, &wake_load);
                });
        }
        Self {
            shared,
            wake,
            updates,
            started: false,
            meta,
            requested: Vec::new(),
        }
    }

    /// The target and key of each list that is missing, non-CDN legacy, or stale and not already being fetched.
    pub fn due_fetches_all(&mut self, now_ms: u64) -> Vec<DueFetch> {
        let mut due = Vec::new();
        let cdn_lists = [
            (GAMES_KEY, GAMES_CDN_URL, self.meta.games.as_ref()),
            (
                NON_GAMES_KEY,
                NON_GAMES_CDN_URL,
                self.meta.non_games.as_ref(),
            ),
        ];
        for (key, url, meta) in cdn_lists {
            let valid_cdn = meta.filter(|entry| entry.cdn);
            let at = valid_cdn.map(|entry| entry.at);
            let etag = valid_cdn.and_then(|entry| entry.etag.clone());
            if stale(at, now_ms) && !self.requested.contains(&key) {
                self.requested.push(key);
                due.push(DueFetch::Cdn { key, url, etag });
            }
        }
        let blocklist_at = self.meta.blocklist.as_ref().map(|entry| entry.at);
        if stale(blocklist_at, now_ms) && !self.requested.contains(&BLOCKLIST_KEY) {
            self.requested.push(BLOCKLIST_KEY);
            due.push(DueFetch::Api {
                key: BLOCKLIST_KEY,
                route: BLOCKLIST_ROUTE,
            });
        }
        due
    }

    /// A CDN list's reply (`200` with parsed `Vec<DetectableGame>`, `304` keeping cache, or `0`/error to retry later).
    pub fn cdn_fetched(
        &mut self,
        key: &str,
        status: u16,
        etag: Option<String>,
        games: Option<Arc<Vec<DetectableGame>>>,
        now_ms: u64,
    ) -> bool {
        let Some(known) = [GAMES_KEY, NON_GAMES_KEY]
            .into_iter()
            .find(|known| *known == key)
        else {
            return false;
        };
        self.requested.retain(|asked| *asked != known);
        if status == 304 {
            let next_etag = {
                let slot = match known {
                    GAMES_KEY => &mut self.meta.games,
                    _ => &mut self.meta.non_games,
                };
                let next_etag = etag.or_else(|| slot.as_ref().and_then(|entry| entry.etag.clone()));
                *slot = Some(MetaEntry {
                    at: now_ms,
                    etag: next_etag.clone(),
                    cdn: true,
                });
                next_etag
            };
            let meta = self.meta.clone();
            if !cfg!(test) {
                let key_owned = known.to_owned();
                let _ = std::thread::Builder::new()
                    .name("detectables-meta".into())
                    .spawn(move || {
                        persist_cache_part(&key_owned, now_ms, next_etag.as_deref(), true, None);
                        meta.write();
                    });
            }
            return true;
        }
        if status != 200 {
            return true;
        }
        let Some(games) = games else {
            return true;
        };
        let entry = Some(MetaEntry {
            at: now_ms,
            etag,
            cdn: true,
        });
        match known {
            GAMES_KEY => self.meta.games = entry,
            _ => self.meta.non_games = entry,
        }
        if let Ok(mut shared) = self.shared.lock() {
            match known {
                GAMES_KEY => {
                    shared.lists.games = (*games).clone();
                    shared.games_from_net = true;
                }
                _ => {
                    shared.lists.non_games = (*games).clone();
                    shared.non_games_from_net = true;
                }
            }
            shared.generation = shared.generation.wrapping_add(1);
        }
        notify_wake(&self.wake);
        true
    }

    /// A list's reply from `Request::Fetch`. Returns whether the key was one of the lists. A failed fetch keeps what
    /// was there and asks again at the next connection.
    pub fn list_fetched(&mut self, key: &str, body: &Value, now_ms: u64) -> bool {
        let Some(known) = [GAMES_KEY, NON_GAMES_KEY, BLOCKLIST_KEY]
            .into_iter()
            .find(|known| *known == key)
        else {
            return false;
        };
        self.requested.retain(|asked| *asked != known);
        let good = if known == BLOCKLIST_KEY {
            body.is_object() && body["fetch_failed"] != true
        } else {
            body.is_array()
        };
        if !good {
            return true;
        }
        let entry = Some(MetaEntry {
            at: now_ms,
            etag: None,
            cdn: known != BLOCKLIST_KEY,
        });
        match known {
            GAMES_KEY => self.meta.games = entry,
            NON_GAMES_KEY => self.meta.non_games = entry,
            _ => self.meta.blocklist = entry,
        }
        if let Ok(mut shared) = self.shared.lock() {
            match known {
                GAMES_KEY => {
                    shared.lists.games = DetectableGame::list_from_json(body);
                    shared.games_from_net = true;
                }
                NON_GAMES_KEY => {
                    shared.lists.non_games = DetectableGame::list_from_json(body);
                    shared.non_games_from_net = true;
                }
                _ => {
                    shared.lists.blocklist = Blocklist::from_json(body);
                    shared.blocklist_from_net = true;
                }
            }
            shared.generation = shared.generation.wrapping_add(1);
        }
        notify_wake(&self.wake);
        if !cfg!(test) {
            let key_owned = known.to_owned();
            let body_owned = body.clone();
            let cdn = known != BLOCKLIST_KEY;
            let _ = std::thread::Builder::new()
                .name("detectables-save".into())
                .spawn(move || {
                    persist_cache_part(&key_owned, now_ms, None, cdn, Some(&body_owned));
                });
        }
        true
    }

    /// `setGameCandidateOverrides`: the games added by hand, as `(path, name)`.
    pub fn set_overrides(&self, overrides: Vec<(String, String)>) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.lists.overrides = overrides;
            shared.generation = shared.generation.wrapping_add(1);
        }
        notify_wake(&self.wake);
    }

    /// Starts the scan thread, once. A harness run never scans.
    pub fn start(&mut self) {
        if self.started || (crate::storage::is_capture_run() && !live_presence_allowed()) {
            return;
        }
        self.started = true;
        let shared = self.shared.clone();
        let wake = self.wake.clone();
        let updates = self.updates.clone();
        let spawned = std::thread::Builder::new()
            .name("game-scan".into())
            .spawn(move || {
                let mut seen_generation = u64::MAX;
                let mut cached_lists = Detectables::default();
                let mut cached_index = dc_detect::Index::default();
                loop {
                    let seq = wake.0.lock().map_or(0, |s| *s);
                    if let Ok(guard) = shared.lock()
                        && guard.generation != seen_generation
                    {
                        cached_lists = guard.lists.clone();
                        seen_generation = guard.generation;
                        drop(guard);
                        cached_index =
                            dc_detect::Index::build(&cached_lists, dc_detect::matcher::WINDOWS);
                    }
                    let scan = scan_with(&cached_lists, &cached_index);
                    if updates
                        .send(crate::net::Update::Games(Box::new(scan)))
                        .is_err()
                    {
                        return;
                    }
                    if let Ok(guard) = wake.0.lock()
                        && *guard == seq
                    {
                        let _ = wake.1.wait_timeout(guard, SCAN_INTERVAL);
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("game scan thread failed to start: {error}");
        }
    }
}

fn load_disk_cache_into(shared: &Mutex<Shared>, wake: &WakeSignal) {
    let value = {
        let _guard = CACHE_IO.lock().unwrap_or_else(|e| e.into_inner());
        cache_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null)
    };
    if !value.is_object() {
        return;
    }
    let meta = CacheMeta::from_value(&value);
    if !meta_path().is_some_and(|p| p.exists()) {
        meta.write();
    }
    let games = value
        .get("games")
        .and_then(|e| e.get("body"))
        .map(DetectableGame::list_from_json);
    let non_games = value
        .get("nonGames")
        .and_then(|e| e.get("body"))
        .map(DetectableGame::list_from_json);
    let blocklist = value
        .get("blocklist")
        .and_then(|e| e.get("body"))
        .map(Blocklist::from_json);
    let mut updated = false;
    if let Ok(mut guard) = shared.lock() {
        if !guard.games_from_net
            && let Some(games) = games
        {
            guard.lists.games = games;
            updated = true;
        }
        if !guard.non_games_from_net
            && let Some(non_games) = non_games
        {
            guard.lists.non_games = non_games;
            updated = true;
        }
        if !guard.blocklist_from_net
            && let Some(blocklist) = blocklist
        {
            guard.lists.blocklist = blocklist;
            updated = true;
        }
        if updated {
            guard.generation = guard.generation.wrapping_add(1);
        }
    }
    if updated {
        notify_wake(wake);
    }
}

/// Converts a CDN detectables JSON response off the UI thread, persists it to disk, and returns the parsed list.
pub(crate) fn parse_and_cache_cdn(
    key: &str,
    etag: Option<&str>,
    bytes: &[u8],
) -> Option<Arc<Vec<DetectableGame>>> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if !value.is_array() {
        return None;
    }
    let games = Arc::new(DetectableGame::list_from_json(&value));
    persist_cache_part(key, now_ms(), etag, true, Some(&value));
    Some(games)
}

fn scan_with(lists: &Detectables, index: &dc_detect::Index) -> GameScan {
    let Ok(processes) = dc_detect::running_processes() else {
        return GameScan::default();
    };
    let windows = dc_detect::visible_window_pids();
    let scan = lists.scan_with_index(&processes, dc_detect::matcher::WINDOWS, index);
    let candidates = lists.candidates(&processes, &windows, &scan, dc_detect::matcher::WINDOWS);
    let streamer_tools = scan
        .observed
        .iter()
        .filter(|(_, kind)| {
            matches!(
                kind,
                dc_detect::matcher::Kind::Observed {
                    streamer_tool: true
                }
            )
        })
        .count();
    let mut games = scan.games;
    dc_detect::matcher::keep_windowed(&mut games, &windows);
    let spotify_running = scan.observed.iter().any(|(app, _)| app.name == "Spotify");
    // Prefer real game executables over launchers in visible-game selection.
    games.sort_by_key(|game| game.is_launcher);
    GameScan {
        spotify_running,
        streamer_tools,
        games,
        candidates,
        foreground: dc_detect::foreground_pid(),
    }
}

/// `SelfPresenceStore`'s filter for the activities (module 894768 `V`, `kv` in module 674378): nothing is shared
/// when "Share my detected activities" is off or the account is invisible.
///
/// The custom status is one of `LocalActivityStore`'s activities and is not a detected activity: it goes out
/// whatever that switch says. Left out, a presence update would take the account's custom status away.
/// Only the tests build a presence without RPC activities now: the app calls `shared_local_activities`.
#[cfg(test)]
pub fn shared_activities(
    custom: Option<Value>,
    activity: Option<Value>,
    show_current_game: bool,
    status: &str,
) -> Vec<Value> {
    if status == "invisible" {
        return Vec::new();
    }
    custom
        .into_iter()
        .chain(activity.filter(|_| show_current_game))
        .collect()
}

/// `shared_activities` with the activities games set over RPC (`dc-rpc`): `LocalActivityStore.getActivities` lists
/// the custom status, the RPC activities in socket order, then the detected game unless an RPC activity stands for it
/// (module 491009 `B`); `SelfPresenceStore`'s filter (module 894768 `V`) then drops every one that names an
/// application unless "Share my detected activities" is on, and nothing is shared when invisible. Of several
/// `PLAYING` activities only the first goes out (`F`).
pub fn shared_local_activities(
    local: &dc_state::local_activity::LocalActivityStore,
    custom: Option<Value>,
    detected: Option<Value>,
    show_current_game: bool,
    status: &str,
) -> Vec<Value> {
    if status == "invisible" {
        return Vec::new();
    }
    let mut playing = false;
    local
        .activities(custom, detected)
        .into_iter()
        .filter(|activity| activity["type"] == 4 || show_current_game)
        .filter(|activity| {
            if activity["type"] != 0 {
                return true;
            }
            !std::mem::replace(&mut playing, true)
        })
        .collect()
}

/// The Spotify activity joins last (`LocalActivityStore`, module 491009 `B`: `{type: LISTENING, ...getActivity()}`
/// after the game). `SelfPresenceStore`'s filter (module 894768 `V`, via 90644) keeps it on `shouldShowActivity`
/// alone, so "Share my detected activities" does not apply to it; `spotify` is `None` when that is false. Invisible
/// shares nothing.
pub fn with_spotify(
    mut activities: Vec<Value>,
    spotify: Option<Value>,
    status: &str,
) -> Vec<Value> {
    if status != "invisible" {
        activities.extend(spotify);
    }
    activities
}

/// What the account's own row and profile show (`SelfPresenceStore` `x`, module 894768): this session's activities,
/// then another active session's (a game another client reports), without that session's custom status, one of each
/// `type:application_id:name`. Only this session's go into the presence update.
pub fn own_visible_activities(local: &[Value], remote: &[Value]) -> Vec<Value> {
    let key = |activity: &Value| {
        format!(
            "{}:{}:{}",
            activity["type"], activity["application_id"], activity["name"]
        )
    };
    let mut seen = std::collections::HashSet::new();
    local
        .iter()
        .chain(remote.iter().filter(|activity| activity["type"] != 4))
        .filter(|activity| seen.insert(key(activity)))
        .cloned()
        .collect()
}

/// A harness run sends a game or scans for one only when it says so with `GRAVECORD_ALLOW_PRESENCE=1`, so a
/// capture run never changes what the account shows others.
pub fn live_presence_allowed() -> bool {
    std::env::var("GRAVECORD_ALLOW_PRESENCE").as_deref() == Ok("1")
}

/// The emoji of a custom status as its activity carries it: a custom emoji the account may use, or a Unicode one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusEmoji {
    Custom {
        id: u64,
        name: String,
        animated: bool,
    },
    Unicode(String),
}

/// The account's custom status as an activity (module 685396 `c`): no `state` without text, an end time only when
/// the status lapses, `emoji` null without one. A status that has lapsed is no activity.
pub fn custom_status_activity(
    text: &str,
    expires_at_ms: u64,
    emoji: Option<StatusEmoji>,
    now_ms: u64,
) -> Option<Value> {
    if (expires_at_ms > 0 && expires_at_ms <= now_ms) || (text.is_empty() && emoji.is_none()) {
        return None;
    }
    let mut activity = json!({
        "name": "Custom Status",
        "type": 4,
        "emoji": match emoji {
            Some(StatusEmoji::Custom { id, name, animated }) => {
                json!({"id": id.to_string(), "name": name, "animated": animated})
            }
            Some(StatusEmoji::Unicode(name)) => json!({"id": null, "name": name, "animated": false}),
            None => Value::Null,
        },
        "metadata": {},
    });
    if !text.is_empty() {
        activity["state"] = json!(text);
    }
    if expires_at_ms > 0 {
        activity["timestamps"] = json!({"end": expires_at_ms});
    }
    Some(activity)
}

/// The status a presence update carries: one of the four the gateway takes.
pub fn presence_status(status: &str) -> &'static str {
    match status {
        "idle" => "idle",
        "dnd" => "dnd",
        "invisible" => "invisible",
        _ => "online",
    }
}

/// Whether a presence update is due: the activities differ from the last one sent, or the status does. A status
/// not seen yet (empty) is the one the connection's handshake carried, so it alone asks for nothing.
pub fn presence_changed(sent: &(String, Vec<Value>), status: &str, activities: &[Value]) -> bool {
    sent.1 != activities || (!sent.0.is_empty() && sent.0 != status)
}

/// Op 3's payload as `presenceUpdate` writes it (module 868): `{status, since, activities, afk}`.
pub fn presence_body(status: &str, activities: &[Value]) -> Value {
    json!({
        "status": presence_status(status),
        "since": 0,
        "activities": activities,
        "afk": false,
    })
}

/// Op 3 with the idle rule (module 894768 `getLocalPresence`): `since` is when the idle began (zero while active),
/// `afk` whether the account is AFK; the status is already the wire one (`idle::wire_status`).
pub fn presence_body_idle(status: &str, activities: &[Value], since: u64, afk: bool) -> Value {
    let mut body = presence_body(status, activities);
    body["since"] = json!(since);
    body["afk"] = json!(afk);
    body
}

/// A harness run never sends a presence update, whatever it carries: the test account's visible status must not
/// change (`account_write_refused` in `net.rs` is the same guard for REST writes).
pub fn presence_refused(capture: bool) -> bool {
    capture && !live_presence_allowed()
}

/// Five updates in twenty seconds (module 225123 `didCommit`): past that, the latest update goes out when the oldest
/// of the five leaves the window (`setTimeout(…, n[0] - l)`, each newer call replacing the waiting one).
#[derive(Default)]
pub struct SendLimiter {
    sent: Vec<u64>,
}

impl SendLimiter {
    /// How long a refused update waits for a free slot.
    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        self.sent.iter().min().map_or(0, |first| {
            (first + PRESENCE_WINDOW_MS).saturating_sub(now_ms)
        })
    }

    pub fn allow(&mut self, now_ms: u64) -> bool {
        self.sent.retain(|at| *at + PRESENCE_WINDOW_MS > now_ms);
        if self.sent.len() >= PRESENCE_BURST {
            return false;
        }
        self.sent.push(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_activity() -> Value {
        json!({"type": 0, "name": "Minecraft", "application_id": "356",
               "timestamps": {"start": 1_700_000_000_000u64}})
    }

    #[test]
    fn the_presence_payload_is_op_3_shaped() {
        let body = presence_body("online", &[game_activity()]);
        assert_eq!(body["status"], "online");
        assert_eq!(body["since"], 0);
        assert_eq!(body["afk"], false);
        assert_eq!(body["activities"][0]["type"], 0);
        assert_eq!(body["activities"][0]["name"], "Minecraft");
        assert_eq!(body["activities"][0]["application_id"], "356");
        assert_eq!(
            body["activities"][0]["timestamps"]["start"],
            1_700_000_000_000u64
        );
        // Anything that is not one of the four statuses goes out as online.
        assert_eq!(presence_body("", &[])["status"], "online");
        assert_eq!(presence_body("dnd", &[])["status"], "dnd");
    }

    #[test]
    fn an_idle_account_sends_since_and_afk() {
        let body = presence_body_idle("idle", &[], 1_700_000_000_000, true);
        assert_eq!(body["status"], "idle");
        assert_eq!(body["since"], 1_700_000_000_000u64);
        assert_eq!(body["afk"], true);
        assert_eq!(presence_body_idle("online", &[], 0, false)["since"], 0);
    }

    #[test]
    fn nothing_is_shared_when_sharing_is_off_or_the_account_is_invisible() {
        let shared = |show, status| shared_activities(None, Some(game_activity()), show, status);
        assert_eq!(shared(true, "online").len(), 1);
        assert_eq!(shared(true, "dnd").len(), 1);
        assert!(shared(false, "online").is_empty());
        assert!(shared(true, "invisible").is_empty());
        assert!(shared_activities(None, None, true, "online").is_empty());
        // The custom status goes out with or without the game, and whatever the sharing switch says.
        let custom = custom_status_activity("brb", 0, None, 5).unwrap();
        let both = shared_activities(Some(custom.clone()), Some(game_activity()), true, "dnd");
        assert_eq!(both.len(), 2);
        assert_eq!(both[0]["type"], 4);
        assert_eq!(
            shared_activities(Some(custom.clone()), Some(game_activity()), false, "online"),
            vec![custom.clone()]
        );
        assert_eq!(
            shared_activities(Some(custom.clone()), None, true, "online"),
            vec![custom.clone()]
        );
        assert!(
            shared_activities(Some(custom), Some(game_activity()), true, "invisible").is_empty()
        );
    }

    #[test]
    fn the_own_row_shows_another_sessions_game_but_not_its_custom_status() {
        let custom = custom_status_activity("brb", 0, None, 5).unwrap();
        let rich = json!({"type": 0, "name": "P.T. Silent Hills", "application_id": "1559538474065993750",
            "details": "In the menus", "state": "New Game+ 4/4"});
        let other_custom = json!({"type": 4, "name": "Custom Status", "state": "elsewhere"});
        let shown = own_visible_activities(
            std::slice::from_ref(&custom),
            &[other_custom, rich.clone(), rich.clone()],
        );
        assert_eq!(shown, vec![custom, rich.clone()]);
        // The same game from both sessions shows once.
        assert_eq!(
            own_visible_activities(std::slice::from_ref(&rich), std::slice::from_ref(&rich)).len(),
            1
        );
    }

    #[test]
    fn a_presence_update_is_due_for_new_activities_or_a_new_status_only() {
        let fresh = (String::new(), Vec::new());
        assert!(!presence_changed(&fresh, "online", &[]));
        assert!(presence_changed(&fresh, "online", &[game_activity()]));
        let sent = ("online".to_owned(), vec![game_activity()]);
        assert!(!presence_changed(&sent, "online", &[game_activity()]));
        assert!(presence_changed(&sent, "dnd", &[game_activity()]));
        assert!(presence_changed(&sent, "online", &[]));
    }

    #[test]
    fn the_custom_status_becomes_the_activity_the_official_client_sends() {
        let plain = custom_status_activity("brb", 0, None, 5).unwrap();
        assert_eq!(plain["name"], "Custom Status");
        assert_eq!(plain["type"], 4);
        assert_eq!(plain["state"], "brb");
        assert!(plain["emoji"].is_null());
        assert!(plain.get("timestamps").is_none());
        let timed = custom_status_activity(
            "",
            90,
            Some(StatusEmoji::Custom {
                id: 7,
                name: "wave".into(),
                animated: true,
            }),
            5,
        )
        .unwrap();
        assert!(timed.get("state").is_none());
        assert_eq!(timed["timestamps"]["end"], 90);
        assert_eq!(
            timed["emoji"],
            json!({"id": "7", "name": "wave", "animated": true})
        );
        let unicode =
            custom_status_activity("", 0, Some(StatusEmoji::Unicode("\u{1f44b}".into())), 5)
                .unwrap();
        assert_eq!(unicode["emoji"]["id"], Value::Null);
        // Lapsed, or with nothing in it: no activity.
        assert!(custom_status_activity("brb", 4, None, 5).is_none());
        assert!(custom_status_activity("", 0, None, 5).is_none());
    }

    #[test]
    fn a_harness_run_never_sends_a_presence_update() {
        assert!(presence_refused(true));
        assert!(!presence_refused(false));
        // The capture build is a harness run in every process it starts.
        if cfg!(feature = "capture") {
            assert!(presence_refused(crate::storage::is_capture_run()));
        }
    }

    #[test]
    fn at_most_five_updates_in_twenty_seconds() {
        let mut limiter = SendLimiter::default();
        for step in 0..5 {
            assert!(limiter.allow(step * 1000));
        }
        assert!(!limiter.allow(5_000));
        // The refused update waits for the oldest slot (`n[0] - l`).
        assert_eq!(limiter.wait_ms(5_000), 15_000);
        assert!(!limiter.allow(19_999));
        assert!(limiter.allow(20_000));
        assert_eq!(SendLimiter::default().wait_ms(1), 0);
    }

    #[test]
    fn the_spotify_activity_joins_last_whatever_the_share_switch_says() {
        let custom = json!({"type": 4, "name": "Custom Status"});
        let listening = json!({"type": 2, "name": "Spotify", "party": {"id": "spotify:1"}});
        let local = dc_state::local_activity::LocalActivityStore::default();
        let shared = shared_local_activities(
            &local,
            Some(custom.clone()),
            Some(game_activity()),
            false,
            "online",
        );
        assert_eq!(
            with_spotify(shared, Some(listening.clone()), "online"),
            vec![custom.clone(), listening.clone()]
        );
        let shared = shared_local_activities(
            &local,
            Some(custom.clone()),
            Some(game_activity()),
            true,
            "online",
        );
        assert_eq!(
            with_spotify(shared, Some(listening.clone()), "online"),
            vec![custom, game_activity(), listening.clone()]
        );
        assert!(with_spotify(Vec::new(), Some(listening), "invisible").is_empty());
    }

    #[test]
    fn lists_are_due_when_missing_and_again_after_a_day() {
        let (updates, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = Runtime::new(updates);
        let now = 10 * LIST_TTL_MS;
        let first_due = runtime.due_fetches_all(now);
        assert_eq!(first_due.len(), 3);
        assert_eq!(
            first_due[0],
            DueFetch::Cdn {
                key: GAMES_KEY,
                url: GAMES_CDN_URL,
                etag: None,
            }
        );
        assert_eq!(
            first_due[1],
            DueFetch::Cdn {
                key: NON_GAMES_KEY,
                url: NON_GAMES_CDN_URL,
                etag: None,
            }
        );
        assert_eq!(
            first_due[2],
            DueFetch::Api {
                key: BLOCKLIST_KEY,
                route: BLOCKLIST_ROUTE,
            }
        );
        // Asked once; not again while the reply is awaited.
        assert!(runtime.due_fetches_all(now).is_empty());
        let games =
            json!([{"id": "1", "name": "A", "executables": [{"os": "win32", "name": "a.exe"}]}]);
        assert!(runtime.list_fetched(GAMES_KEY, &games, now));
        assert!(!runtime.list_fetched("other", &games, now));
        // A failed reply keeps the old list and asks again at the next connection.
        assert!(runtime.list_fetched(NON_GAMES_KEY, &json!({"fetch_failed": true}), now));
        assert!(runtime.meta.non_games.is_none());
        let due = runtime.due_fetches_all(now + 1);
        // Only the failed list is asked again; the blocklist reply is still awaited.
        assert_eq!(due.len(), 1);
        assert!(stale(Some(now), now + LIST_TTL_MS));
        assert!(!stale(Some(now), now + LIST_TTL_MS - 1));
        // The stored games reach the scanner's lists.
        let lists = runtime.shared.lock().unwrap().lists.clone();
        assert_eq!(lists.games.len(), 1);
    }

    #[test]
    fn cdn_etag_304_refreshes_timestamp_and_non_cdn_legacy_upgrades() {
        let (updates, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = Runtime::new(updates);
        let now = 10 * LIST_TTL_MS;
        let hytale = DetectableGame::list_from_json(&json!([{
            "id": "1458530944955973852",
            "name": "Hytale",
            "executables": [{"os": "win32", "name": ">hytaleclient.exe"}]
        }]));
        assert!(runtime.cdn_fetched(
            GAMES_KEY,
            200,
            Some("\"etag-v1\"".to_owned()),
            Some(Arc::new(hytale)),
            now,
        ));
        let immediate = runtime.due_fetches_all(now + 1000);
        assert_eq!(immediate.len(), 2);
        assert!(
            !immediate
                .iter()
                .any(|d| matches!(d, DueFetch::Cdn { key, .. } if *key == GAMES_KEY))
        );
        // After a day, games is due again with its saved ETag.
        let due_day_later = runtime.due_fetches_all(now + LIST_TTL_MS);
        assert_eq!(
            due_day_later
                .iter()
                .find(|d| matches!(d, DueFetch::Cdn { key, .. } if *key == GAMES_KEY)),
            Some(&DueFetch::Cdn {
                key: GAMES_KEY,
                url: GAMES_CDN_URL,
                etag: Some("\"etag-v1\"".to_owned()),
            })
        );
        // 304 bumps timestamp without clearing cached games.
        assert!(runtime.cdn_fetched(GAMES_KEY, 304, None, None, now + LIST_TTL_MS + 5));
        assert_eq!(runtime.shared.lock().unwrap().lists.games.len(), 1);
        assert_eq!(
            runtime.meta.games.as_ref().and_then(|m| m.etag.as_deref()),
            Some("\"etag-v1\"")
        );
        // Legacy non-CDN cache entry is treated as immediately stale with no ETag.
        runtime.meta.games = Some(MetaEntry {
            at: now + LIST_TTL_MS + 5,
            etag: Some("\"old\"".to_owned()),
            cdn: false,
        });
        let due_upgrade = runtime.due_fetches_all(now + LIST_TTL_MS + 6);
        assert!(due_upgrade.contains(&DueFetch::Cdn {
            key: GAMES_KEY,
            url: GAMES_CDN_URL,
            etag: None,
        }));
    }

    fn rpc_game(app: &str, name: &str) -> Value {
        json!({"type": 0, "name": name, "application_id": app, "platform": "desktop", "metadata": {}})
    }

    #[test]
    fn a_games_rpc_activity_joins_the_presence_under_the_same_sharing_rule() {
        let mut local = dc_state::local_activity::LocalActivityStore::default();
        local.update(4, Some(40), Some(rpc_game("1", "P.T. Silent Hills")), None);
        let custom = json!({"type": 4, "name": "Custom Status", "state": "hi"});
        // On: the custom status, then the game.
        let on = shared_local_activities(&local, Some(custom.clone()), None, true, "online");
        assert_eq!(on.len(), 2);
        assert_eq!(on[0]["type"], 4);
        assert_eq!(on[1]["name"], "P.T. Silent Hills");
        // Off: the game is a detected activity and is held back, the custom status is not.
        assert_eq!(
            shared_local_activities(&local, Some(custom.clone()), None, false, "online"),
            vec![custom]
        );
        // Invisible shares nothing.
        assert!(shared_local_activities(&local, None, None, true, "invisible").is_empty());
    }

    #[test]
    fn the_detected_copy_of_an_rpc_game_is_not_sent_twice_and_one_game_plays_at_a_time() {
        let mut local = dc_state::local_activity::LocalActivityStore::default();
        local.update(1, Some(10), Some(rpc_game("1", "P.T. Silent Hills")), None);
        let detected = json!({"type": 0, "name": "P.T. Silent Hills", "timestamps": {"start": 5}});
        let list = shared_local_activities(&local, None, Some(detected), true, "online");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["application_id"], "1");
        // Another, different game: only the first PLAYING activity goes out.
        local.update(2, Some(20), Some(rpc_game("2", "Other")), None);
        let other = json!({"type": 0, "name": "Third"});
        let list = shared_local_activities(&local, None, Some(other), true, "online");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "P.T. Silent Hills");
    }
}
