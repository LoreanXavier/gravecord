//! Game detection at run time: the process scan loop, the detectable lists kept on disk, and the presence update a
//! detected game makes.
//!
//! The official client does the scanning in a native process observer that reports to `RunningGameStore` (module
//! 397369); the lists are `GameStore` (module 760751, `GET /games/detectable`, `/games/detectable/exclusions`,
//! `GET /applications/non-games/detectable`), cached in local storage and refetched once a day
//! (`detectableGamesTtl`). The presence update is `ConnectionStore`'s `presenceUpdate` (module 225123): status,
//! idle-since, activities and afk, at most five in twenty seconds.
//!
//! Nothing here runs in a harness run: the scan thread is never started and [`presence_refused`] stops every
//! presence update, so a capture can neither detect a game nor change the test account's status.

use dc_detect::{Blocklist, Candidate, DetectableGame, Detectables, Detected};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
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

/// The three lists and their routes, as the `GamesActionCreators` fetchers (module 273626) name them.
pub const GAMES_KEY: &str = "games-detectable";
pub const NON_GAMES_KEY: &str = "games-non-games";
pub const BLOCKLIST_KEY: &str = "games-blocklist";
pub const GAMES_ROUTE: &str = "/games/detectable";
pub const NON_GAMES_ROUTE: &str = "/applications/non-games/detectable";
pub const BLOCKLIST_ROUTE: &str = "/games/detectable/exclusions";

/// What one scan found.
#[derive(Clone, Debug, Default)]
pub struct GameScan {
    pub games: Vec<Detected>,
    pub candidates: Vec<Candidate>,
    pub foreground: Option<u32>,
}

#[derive(Default)]
struct Shared {
    lists: Detectables,
}

/// The scan thread's handle and the raw lists as they were last read or fetched.
pub struct Runtime {
    shared: Arc<Mutex<Shared>>,
    updates: UnboundedSender<crate::net::Update>,
    started: bool,
    cache: Cache,
    requested: Vec<&'static str>,
}

/// The lists as stored on disk: each body as the server sent it, with the time it was fetched.
#[derive(Default, Clone)]
struct Cache {
    games: Option<(u64, Value)>,
    non_games: Option<(u64, Value)>,
    blocklist: Option<(u64, Value)>,
}

fn cache_path() -> Option<std::path::PathBuf> {
    crate::storage::data_dir().map(|dir| dir.join("detectables.json"))
}

impl Cache {
    fn read() -> Self {
        let value = cache_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let part = |name: &str| {
            let entry = value.get(name)?;
            Some((entry["at"].as_u64()?, entry["body"].clone()))
        };
        Self {
            games: part("games"),
            non_games: part("nonGames"),
            blocklist: part("blocklist"),
        }
    }

    /// Writes beside and renames, so a crash mid-write never leaves half a file.
    fn write(&self) {
        // Unit tests never touch the owner's files.
        if cfg!(test) {
            return;
        }
        let Some(path) = cache_path() else { return };
        let entry = |part: &Option<(u64, Value)>| {
            part.as_ref()
                .map_or(Value::Null, |(at, body)| json!({"at": at, "body": body}))
        };
        let value = json!({
            "games": entry(&self.games),
            "nonGames": entry(&self.non_games),
            "blocklist": entry(&self.blocklist),
        });
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let temporary = path.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec(&value)
            && std::fs::write(&temporary, bytes).is_ok()
        {
            let _ = std::fs::rename(&temporary, &path);
        }
    }

    fn lists(&self) -> Detectables {
        let games = self
            .games
            .as_ref()
            .map(|(_, body)| DetectableGame::list_from_json(body))
            .unwrap_or_default();
        let non_games = self
            .non_games
            .as_ref()
            .map(|(_, body)| DetectableGame::list_from_json(body))
            .unwrap_or_default();
        let blocklist = self
            .blocklist
            .as_ref()
            .map(|(_, body)| Blocklist::from_json(body))
            .unwrap_or_default();
        Detectables {
            games,
            non_games,
            blocklist,
            overrides: Vec::new(),
        }
    }
}

/// Whether a list fetched at `at` is stale (`canFetchDetectableGames` and its siblings).
fn stale(at: Option<u64>, now_ms: u64) -> bool {
    at.is_none_or(|at| now_ms >= at.saturating_add(LIST_TTL_MS))
}

impl Runtime {
    pub fn new(updates: UnboundedSender<crate::net::Update>) -> Self {
        let cache = Cache::read();
        let overrides = Vec::new();
        let mut lists = cache.lists();
        lists.overrides = overrides;
        Self {
            shared: Arc::new(Mutex::new(Shared { lists })),
            updates,
            started: false,
            cache,
            requested: Vec::new(),
        }
    }

    /// The route and key of each list that is missing or stale and not already being fetched.
    pub fn due_fetches(&mut self, now_ms: u64) -> Vec<(&'static str, &'static str)> {
        let wanted = [
            (
                GAMES_KEY,
                GAMES_ROUTE,
                self.cache.games.as_ref().map(|part| part.0),
            ),
            (
                NON_GAMES_KEY,
                NON_GAMES_ROUTE,
                self.cache.non_games.as_ref().map(|part| part.0),
            ),
            (
                BLOCKLIST_KEY,
                BLOCKLIST_ROUTE,
                self.cache.blocklist.as_ref().map(|part| part.0),
            ),
        ];
        let mut due = Vec::new();
        for (key, route, at) in wanted {
            if stale(at, now_ms) && !self.requested.contains(&key) {
                self.requested.push(key);
                due.push((key, route));
            }
        }
        due
    }

    /// A list's reply. Returns whether the key was one of the lists. A failed fetch (not an array or object of the
    /// right shape) keeps what was there and is asked for again at the next connection.
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
        let part = Some((now_ms, body.clone()));
        match known {
            GAMES_KEY => self.cache.games = part,
            NON_GAMES_KEY => self.cache.non_games = part,
            _ => self.cache.blocklist = part,
        }
        self.cache.write();
        let mut lists = self.cache.lists();
        if let Ok(mut shared) = self.shared.lock() {
            lists.overrides = std::mem::take(&mut shared.lists.overrides);
            shared.lists = lists;
        }
        true
    }

    /// `setGameCandidateOverrides`: the games added by hand, as `(path, name)`.
    pub fn set_overrides(&self, overrides: Vec<(String, String)>) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.lists.overrides = overrides;
        }
    }

    /// Starts the scan thread, once. A harness run never scans.
    pub fn start(&mut self) {
        if self.started || (crate::storage::is_capture_run() && !live_presence_allowed()) {
            return;
        }
        self.started = true;
        let shared = self.shared.clone();
        let updates = self.updates.clone();
        let spawned = std::thread::Builder::new()
            .name("game-scan".into())
            .spawn(move || {
                loop {
                    let scan = scan_once(&shared);
                    if updates
                        .send(crate::net::Update::Games(Box::new(scan)))
                        .is_err()
                    {
                        return;
                    }
                    std::thread::sleep(SCAN_INTERVAL);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("game scan thread failed to start: {error}");
        }
    }
}

fn scan_once(shared: &Mutex<Shared>) -> GameScan {
    let Ok(processes) = dc_detect::running_processes() else {
        return GameScan::default();
    };
    let windows = dc_detect::visible_window_pids();
    let lists = match shared.lock() {
        Ok(shared) => shared.lists.clone(),
        Err(_) => return GameScan::default(),
    };
    let scan = lists.scan(&processes, dc_detect::matcher::WINDOWS);
    let candidates = lists.candidates(&processes, &windows, &scan, dc_detect::matcher::WINDOWS);
    GameScan {
        games: scan.games,
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

/// A harness run sends a game or scans for one only when it says so with `GRAVECORD_ALLOW_PRESENCE=1` (a live test
/// the owner asked for).
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

/// A harness run never sends a presence update, whatever it carries: the test account's visible status must not
/// change (`account_write_refused` in `net.rs` is the same guard for REST writes).
pub fn presence_refused(capture: bool) -> bool {
    capture && !live_presence_allowed()
}

/// Five updates in twenty seconds (module 225123): past that, the update waits for the next scan.
#[derive(Default)]
pub struct SendLimiter {
    sent: Vec<u64>,
}

impl SendLimiter {
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
        assert!(!limiter.allow(19_999));
        assert!(limiter.allow(20_000));
    }

    #[test]
    fn lists_are_due_when_missing_and_again_after_a_day() {
        let (updates, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = Runtime {
            shared: Arc::default(),
            updates,
            started: false,
            cache: Cache::default(),
            requested: Vec::new(),
        };
        let now = 10 * LIST_TTL_MS;
        assert_eq!(runtime.due_fetches(now).len(), 3);
        // Asked once; not again while the reply is awaited.
        assert!(runtime.due_fetches(now).is_empty());
        let games =
            json!([{"id": "1", "name": "A", "executables": [{"os": "win32", "name": "a.exe"}]}]);
        assert!(runtime.list_fetched(GAMES_KEY, &games, now));
        assert!(!runtime.list_fetched("other", &games, now));
        // A failed reply keeps the old list and asks again at the next connection.
        assert!(runtime.list_fetched(NON_GAMES_KEY, &json!({"fetch_failed": true}), now));
        assert!(runtime.cache.non_games.is_none());
        let due = runtime.due_fetches(now + 1);
        // Only the failed list is asked again; the blocklist reply is still awaited.
        assert_eq!(due.len(), 1);
        assert!(stale(Some(now), now + LIST_TTL_MS));
        assert!(!stale(Some(now), now + LIST_TTL_MS - 1));
        // The stored games reach the scanner's lists.
        let lists = runtime.shared.lock().unwrap().lists.clone();
        assert_eq!(lists.games.len(), 1);
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
