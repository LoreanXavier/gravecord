//! RunningGameStore (module 397369): the games the process observer reports as running, the games seen (kept in
//! local storage under `RunningGameStore` with their last-played times), the games the user added by hand
//! (`gameOverrides`), the per-game detection switch (`enableDetection`) and the activity a visible game makes
//! (`LocalActivityStore`, module 491009). The store is pure: the scan loop feeds it process matches and the app
//! persists [`RunningGames::to_json`].

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

/// A game as the official store holds it (the native observer's process object, `j` in module 253400).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Game {
    /// The detectable game's id, when the process matched one.
    pub id: Option<String>,
    pub name: Option<String>,
    /// Lower-case, `/` separators.
    pub exe_path: String,
    pub cmd_line: String,
    /// Unix seconds.
    pub last_focused: u64,
    /// Unix seconds; set when the game was first seen running (`eM` in 397369).
    pub last_launched: Option<u64>,
    pub pid: Option<u32>,
    /// Unix milliseconds the game was first seen running this session (`start`).
    pub start: Option<u64>,
    pub hidden: bool,
    pub is_launcher: bool,
    /// Added by hand through "Add it!".
    pub add: bool,
}

/// `eF`: the key a game is stored under, `<exePath>:<name>`.
pub fn key(game: &Game) -> String {
    format!("{}:{}", game.exe_path, game.name.as_deref().unwrap_or(""))
}

impl Game {
    fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("exePath".into(), json!(self.exe_path));
        map.insert("cmdLine".into(), json!(self.cmd_line));
        map.insert("lastFocused".into(), json!(self.last_focused));
        if let Some(id) = &self.id {
            map.insert("id".into(), json!(id));
        }
        if let Some(name) = &self.name {
            map.insert("name".into(), json!(name));
        }
        if self.add {
            map.insert("add".into(), json!(true));
        }
        if let Some(launched) = self.last_launched {
            map.insert("lastLaunched".into(), json!(launched));
        }
        Value::Object(map)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let exe_path = value["exePath"].as_str()?.to_owned();
        Some(Self {
            // A number is an old native observer id (`nativeProcessObserverId` migration in `initialize`): not a game id.
            id: value["id"].as_str().map(str::to_owned),
            name: value["name"].as_str().map(str::to_owned),
            exe_path,
            cmd_line: value["cmdLine"].as_str().unwrap_or("").to_owned(),
            last_focused: value["lastFocused"].as_u64().unwrap_or(0),
            last_launched: value["lastLaunched"].as_u64(),
            add: value["add"].as_bool().unwrap_or(false),
            ..Self::default()
        })
    }
}

/// What a scan changed in the running list (`added` and `removed` of `RUNNING_GAMES_CHANGE`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: Vec<Game>,
    pub removed: Vec<Game>,
}

#[derive(Debug, Default)]
pub struct RunningGames {
    /// `gamesSeen`, newest first.
    games_seen: Vec<Game>,
    /// `gameOverrides`: the games added by hand, by [`key`].
    overrides: BTreeMap<String, Game>,
    /// `enableDetection`: false hides a game from the status.
    enable_detection: BTreeMap<String, bool>,
    /// The other keys of the stored object (`enableOverlay`, `enableOverlayV3`), kept as they were.
    other: Map<String, Value>,
    /// `er`.
    running: Vec<Game>,
    /// `ei`: running windows that are not known games ("Add it!").
    candidates: Vec<Game>,
    /// `eS`: running games by pid, to tell what was added and removed.
    by_pid: HashMap<u32, Game>,
    /// `eo`: the game shown as the status.
    visible: Option<Game>,
    /// `ec`: pid to the key of the entry first added for it.
    provisional: HashMap<u32, String>,
    /// `eA`: overrides that were hidden when they started running.
    unhidden: HashMap<String, bool>,
}

impl RunningGames {
    /// `initialize`: the stored `RunningGameStore` object, tolerant of what it lacks.
    pub fn from_json(stored: &Value) -> Self {
        let mut store = Self::default();
        for game in stored["gamesSeen"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Game::from_json)
        {
            store.games_seen.push(game);
        }
        for game in stored["gameOverrides"]
            .as_object()
            .into_iter()
            .flat_map(Map::values)
            .filter_map(Game::from_json)
        {
            store.overrides.insert(key(&game), game);
        }
        for (name, on) in stored["enableDetection"].as_object().into_iter().flatten() {
            if let Some(on) = on.as_bool() {
                store.enable_detection.insert(name.clone(), on);
            }
        }
        for name in ["enableOverlay", "enableOverlayV3"] {
            if let Some(value) = stored.get(name) {
                store.other.insert(name.to_owned(), value.clone());
            }
        }
        store
    }

    /// `eX`: the object that goes back to storage.
    pub fn to_json(&self) -> Value {
        let mut map = self.other.clone();
        map.insert(
            "gamesSeen".into(),
            Value::Array(self.games_seen.iter().map(Game::to_json).collect()),
        );
        map.insert(
            "gameOverrides".into(),
            Value::Object(
                self.overrides
                    .iter()
                    .map(|(name, game)| (name.clone(), game.to_json()))
                    .collect(),
            ),
        );
        map.insert(
            "enableDetection".into(),
            Value::Object(
                self.enable_detection
                    .iter()
                    .map(|(name, on)| (name.clone(), json!(on)))
                    .collect(),
            ),
        );
        for name in ["enableOverlay", "enableOverlayV3"] {
            map.entry(name.to_owned()).or_insert_with(|| json!({}));
        }
        Value::Object(map)
    }

    /// `eK`: a game is detected unless its switch was turned off.
    pub fn is_detection_enabled(&self, game: &Game) -> bool {
        self.enable_detection
            .get(&key(game))
            .copied()
            .unwrap_or(true)
    }

    /// `e$`: shown as the status when it is not hidden and detection is on for it.
    fn shows(&self, game: &Game) -> bool {
        !game.hidden && self.is_detection_enabled(game)
    }

    pub fn running_games(&self) -> &[Game] {
        &self.running
    }

    /// `getVisibleRunningGames`.
    pub fn visible_running_games(&self) -> Vec<&Game> {
        self.running
            .iter()
            .filter(|game| self.shows(game))
            .collect()
    }

    /// `getVisibleGame`: the first running game that shows, with the time it started.
    pub fn visible_game(&self) -> Option<&Game> {
        self.visible.as_ref()
    }

    /// `getCandidateGames` (module 397369): windows that are not known games and were not added by hand.
    pub fn candidate_games(&self) -> Vec<&Game> {
        self.candidates
            .iter()
            .filter(|game| game.hidden || game.id.is_none())
            .filter(|game| !self.overrides.contains_key(&key(game)))
            .collect()
    }

    pub fn set_candidates(&mut self, games: Vec<Game>) {
        self.candidates = games;
    }

    /// `e0`: the games seen that were not added by hand, then the added ones.
    fn all_seen(&self) -> Vec<Game> {
        self.games_seen
            .iter()
            .filter(|game| !self.overrides.contains_key(&key(game)))
            .cloned()
            .chain(self.overrides.values().cloned())
            .collect()
    }

    /// `getGamesSeen(excludeVisible)`, newest first: what the Added Games list shows.
    pub fn games_seen(&self, exclude_visible: bool) -> Vec<Game> {
        let mut games = self.all_seen();
        if exclude_visible && let Some(visible) = &self.visible {
            let visible = key(visible);
            games.retain(|game| key(game) != visible);
        }
        games.sort_by_key(|game| std::cmp::Reverse(game.last_focused));
        games
    }

    pub fn is_override(&self, game: &Game) -> bool {
        self.overrides.contains_key(&key(game))
    }

    /// The hand-added games, for the scanner (`setGameCandidateOverrides`).
    pub fn overrides(&self) -> impl Iterator<Item = &Game> {
        self.overrides.values()
    }

    /// `eG`: the visible game is the first that shows; it keeps its start while its pid stays.
    fn refresh_visible(&mut self, now_ms: u64) {
        let previous = self.visible.take();
        self.visible = self.running.iter().find(|game| self.shows(game)).cloned();
        if let Some(visible) = &mut self.visible {
            visible.start = match previous {
                Some(previous) if previous.pid == visible.pid => previous.start,
                _ => Some(now_ms),
            };
        }
    }

    /// `ek`: the running list changed. Returns what was added and removed.
    fn running_changed(&mut self, now_ms: u64) -> Change {
        self.refresh_visible(now_ms);
        let mut change = Change::default();
        for game in &self.running {
            if let Some(pid) = game.pid
                && !self.by_pid.contains_key(&pid)
            {
                self.by_pid.insert(pid, game.clone());
                change.added.push(game.clone());
            }
        }
        let gone: Vec<u32> = self
            .by_pid
            .keys()
            .copied()
            .filter(|pid| !self.running.iter().any(|game| game.pid == Some(*pid)))
            .collect();
        for pid in gone {
            if let Some(game) = self.by_pid.remove(&pid) {
                change.removed.push(game);
            }
        }
        change
    }

    /// The scan result: `games` are the processes the matcher reported, with their pids. A game whose
    /// `last_focused` is zero was not focused since it started: it gets the current time when new (`Date.now()`).
    /// Runs the `RUNNING_GAMES_CHANGE` handler (`eJ`) and keeps the games seen up to date.
    pub fn set_running(&mut self, mut games: Vec<Game>, now_ms: u64) -> Change {
        let now = now_ms / 1000;
        for game in &mut games {
            if game.last_focused == 0 {
                game.last_focused = self
                    .by_pid
                    .get(&game.pid.unwrap_or(0))
                    .map(|known| known.last_focused)
                    .filter(|focused| *focused != 0)
                    .unwrap_or(now);
            }
            // A hand-added game that was hidden starts showing (`gameOverrides[..].add`).
            if self
                .overrides
                .get(&key(game))
                .is_some_and(|known| known.add)
                && game.hidden
            {
                self.unhidden.insert(key(game), true);
                game.hidden = false;
            }
        }
        self.running = games;
        let change = self.running_changed(now_ms);
        let running = self.running.clone();
        self.update_seen(&running);
        change
    }

    /// `eJ`: records the running games in the games seen.
    fn update_seen(&mut self, games: &[Game]) {
        if games.is_empty() {
            return;
        }
        for game in games {
            // A pid that was recorded under a provisional entry replaces it.
            if let Some(pid) = game.pid
                && let Some(entry) = self.provisional.remove(&pid)
            {
                self.games_seen.retain(|seen| key(seen) != entry);
            }
            let found = self.games_seen.iter().position(|seen| {
                seen.name == game.name || (seen.id.is_some() && seen.id == game.id)
            });
            if let Some(position) = found {
                let (old_key, new_key) = (key(&self.games_seen[position]), key(game));
                let seen = &mut self.games_seen[position];
                if game.last_focused != 0 {
                    seen.last_focused = game.last_focused;
                    if let Some(over) = self.overrides.get_mut(&key(game)) {
                        over.last_focused = game.last_focused;
                    }
                }
                if seen.id.is_some() && seen.id == game.id && old_key != new_key {
                    seen.exe_path = game.exe_path.clone();
                    self.rename_key(&old_key, &new_key);
                }
                let seen = &mut self.games_seen[position];
                match (game.last_launched, game.start) {
                    (Some(launched), _) => seen.last_launched = Some(launched),
                    (None, Some(start)) => seen.last_launched = Some(start),
                    _ => {}
                }
            } else {
                if game.hidden {
                    continue;
                }
                let mut entry = Game {
                    id: game.id.clone(),
                    name: game.name.clone(),
                    exe_path: game.exe_path.clone(),
                    cmd_line: game.cmd_line.clone(),
                    last_focused: game.last_focused,
                    add: game.add,
                    ..Game::default()
                };
                entry.last_launched = game.last_launched.or(game.start);
                if let Some(pid) = game.pid {
                    self.provisional.insert(pid, key(game));
                }
                self.games_seen.insert(0, entry);
            }
        }
        self.games_seen
            .sort_by_key(|game| std::cmp::Reverse(game.last_focused));
    }

    /// `eZ`: a game's settings follow it to a new key.
    fn rename_key(&mut self, old: &str, new: &str) {
        if let Some(game) = self.overrides.remove(old) {
            self.overrides.insert(new.to_owned(), game);
        }
        if let Some(on) = self.enable_detection.remove(old) {
            self.enable_detection.insert(new.to_owned(), on);
        }
    }

    /// `RUNNING_GAME_ADD_OVERRIDE`: "Add it!" with the chosen window's pid. Returns whether anything was added.
    pub fn add_override(&mut self, pid: u32, now_ms: u64) -> bool {
        if self
            .running
            .iter()
            .any(|game| game.pid == Some(pid) && game.id.is_some())
        {
            return false;
        }
        let name;
        match self.running.iter_mut().find(|game| game.pid == Some(pid)) {
            Some(game) => {
                if game.hidden {
                    self.unhidden.insert(key(game), true);
                }
                game.hidden = false;
                if game.last_focused == 0 {
                    game.last_focused = now_ms / 1000;
                }
                name = key(game);
                let mut stored = game.clone();
                stored.add = true;
                self.overrides.insert(name, stored);
            }
            None => {
                let Some(candidate) = self
                    .candidates
                    .iter()
                    .find(|game| game.pid == Some(pid))
                    .cloned()
                else {
                    return false;
                };
                let mut game = candidate;
                game.hidden = false;
                if game.last_focused == 0 {
                    game.last_focused = now_ms / 1000;
                }
                self.unhidden.insert(key(&game), true);
                let mut stored = game.clone();
                stored.add = true;
                self.overrides.insert(key(&game), stored);
                self.running.push(game);
            }
        }
        let running = self.running.clone();
        self.update_seen(&running);
        self.running_changed(now_ms);
        true
    }

    /// `RUNNING_GAME_TOGGLE_DETECTION`: flips the game's switch.
    pub fn toggle_detection(&mut self, game: &Game, now_ms: u64) {
        let on = self.is_detection_enabled(game);
        self.enable_detection.insert(key(game), !on);
        self.refresh_visible(now_ms);
    }

    /// The user's own overlay switches for a game: `enableOverlay[key]` (legacy) and `enableOverlayV3[key]`.
    pub fn overlay_override(&self, game: &Game) -> (Option<bool>, Option<bool>) {
        let read = |name: &str| {
            self.other
                .get(name)
                .and_then(|map| map.get(key(game)))
                .and_then(Value::as_bool)
        };
        (read("enableOverlay"), read("enableOverlayV3"))
    }

    /// `RUNNING_GAME_TOGGLE_OVERLAY`: the legacy value is always written, the v3 one when given.
    pub fn toggle_overlay(&mut self, game: &Game, legacy: bool, v3: Option<bool>) {
        let name = key(game);
        let mut write = |store: &str, on: bool| {
            let entry = self
                .other
                .entry(store.to_owned())
                .or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            if let Some(map) = entry.as_object_mut() {
                map.insert(name.clone(), json!(on));
            }
        };
        write("enableOverlay", legacy);
        if let Some(on) = v3 {
            write("enableOverlayV3", on);
        }
    }

    /// `RUNNING_GAME_EDIT_NAME`: only for games added by hand or not in the database; the settings follow the
    /// new key.
    pub fn edit_name(&mut self, game: &Game, new_name: &str, now_ms: u64) {
        let old = key(game);
        let mut over = self.overrides.remove(&old).unwrap_or_else(|| Game {
            name: game.name.clone(),
            exe_path: game.exe_path.clone(),
            cmd_line: game.cmd_line.clone(),
            last_focused: game.last_focused,
            add: true,
            ..Game::default()
        });
        over.name = Some(new_name.to_owned());
        let new = key(&over);
        self.overrides.insert(new.clone(), over);
        if let Some(on) = self.enable_detection.remove(&old) {
            self.enable_detection.insert(new.clone(), on);
        }
        if let Some(flag) = self.unhidden.remove(&old) {
            self.unhidden.insert(new.clone(), flag);
        }
        for seen in &mut self.games_seen {
            if key(seen) == old {
                seen.name = Some(new_name.to_owned());
            }
        }
        let mut renamed = false;
        for running in &mut self.running {
            if key(running) == old {
                running.name = Some(new_name.to_owned());
                renamed = true;
            }
        }
        if renamed {
            self.running_changed(now_ms);
        }
    }

    /// `RUNNING_GAME_DELETE_ENTRY`: forgets a game and its switches.
    pub fn delete_entry(&mut self, game: &Game, now_ms: u64) {
        let target = key(game);
        self.overrides.remove(&target);
        self.enable_detection.remove(&target);
        self.games_seen.retain(|seen| key(seen) != target);
        if self.unhidden.remove(&target).is_some() {
            for running in &mut self.running {
                if key(running) == target {
                    running.hidden = true;
                }
            }
        }
        if self.running.iter().any(|running| key(running) == target) {
            self.running_changed(now_ms);
        }
    }

    /// `LocalActivityStore` `B` for the visible game: the PLAYING activity (type 0, name, application id, start).
    /// A launcher is not shown as a game.
    pub fn activity(&self) -> Option<Value> {
        let game = self.visible.as_ref()?;
        let name = game.name.as_ref()?;
        if game.is_launcher {
            return None;
        }
        let mut activity = json!({
            "type": 0,
            "name": name,
            "timestamps": {"start": game.start},
        });
        if let Some(id) = &game.id {
            activity["application_id"] = json!(id);
        }
        Some(activity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(pid: u32, id: Option<&str>, name: &str, path: &str) -> Game {
        Game {
            id: id.map(str::to_owned),
            name: Some(name.to_owned()),
            exe_path: path.to_owned(),
            pid: Some(pid),
            ..Game::default()
        }
    }

    const T0: u64 = 1_700_000_000_000;

    #[test]
    fn a_new_game_becomes_visible_with_its_start_and_is_seen() {
        let mut store = RunningGames::default();
        let change = store.set_running(
            vec![running(7, Some("1"), "Minecraft", "c:/g/minecraft.exe")],
            T0,
        );
        assert_eq!(change.added.len(), 1);
        assert!(change.removed.is_empty());
        let visible = store.visible_game().unwrap();
        assert_eq!(visible.start, Some(T0));
        assert_eq!(store.games_seen(false).len(), 1);
        assert_eq!(store.games_seen(true).len(), 0);
        assert_eq!(store.games_seen(false)[0].last_focused, T0 / 1000);
        // The same game on the next scan keeps its start.
        let change = store.set_running(
            vec![running(7, Some("1"), "Minecraft", "c:/g/minecraft.exe")],
            T0 + 10_000,
        );
        assert_eq!(change, Change::default());
        assert_eq!(store.visible_game().unwrap().start, Some(T0));
    }

    #[test]
    fn a_game_that_stops_is_removed_and_stays_in_the_games_seen() {
        let mut store = RunningGames::default();
        store.set_running(vec![running(7, Some("1"), "A", "c:/a.exe")], T0);
        let change = store.set_running(Vec::new(), T0 + 5000);
        assert_eq!(change.removed.len(), 1);
        assert!(store.visible_game().is_none());
        assert!(store.activity().is_none());
        assert_eq!(store.games_seen(true).len(), 1);
    }

    #[test]
    fn the_same_game_under_a_new_pid_is_one_entry() {
        let mut store = RunningGames::default();
        store.set_running(vec![running(7, Some("1"), "A", "c:/a.exe")], T0);
        store.set_running(Vec::new(), T0 + 1000);
        store.set_running(vec![running(9, Some("1"), "A", "c:/a.exe")], T0 + 2000);
        assert_eq!(store.games_seen(false).len(), 1);
        assert_eq!(store.visible_game().unwrap().start, Some(T0 + 2000));
    }

    #[test]
    fn the_switch_hides_a_game_from_the_status_only() {
        let mut store = RunningGames::default();
        store.set_running(vec![running(7, Some("1"), "A", "c:/a.exe")], T0);
        let game = store.visible_game().cloned().unwrap();
        store.toggle_detection(&game, T0);
        assert!(store.visible_game().is_none());
        assert!(store.activity().is_none());
        assert_eq!(store.running_games().len(), 1);
        store.toggle_detection(&game, T0 + 1);
        assert!(store.visible_game().is_some());
    }

    #[test]
    fn the_activity_is_a_playing_activity_with_the_application_id() {
        let mut store = RunningGames::default();
        store.set_running(vec![running(7, Some("356"), "Minecraft", "c:/m.exe")], T0);
        let activity = store.activity().unwrap();
        assert_eq!(activity["type"], 0);
        assert_eq!(activity["name"], "Minecraft");
        assert_eq!(activity["application_id"], "356");
        assert_eq!(activity["timestamps"]["start"], T0);
        // A launcher is not a game on show.
        let mut launcher = running(8, Some("2"), "Client", "c:/c.exe");
        launcher.is_launcher = true;
        let mut other = RunningGames::default();
        other.set_running(vec![launcher], T0);
        assert!(other.activity().is_none());
    }

    #[test]
    fn add_it_stores_an_override_for_a_candidate_and_it_shows() {
        let mut store = RunningGames::default();
        store.set_candidates(vec![running(5, None, "mygame", "d:/mygame.exe")]);
        assert_eq!(store.candidate_games().len(), 1);
        assert!(store.add_override(5, T0));
        assert!(store.is_override(&running(5, None, "mygame", "d:/mygame.exe")));
        assert_eq!(
            store.visible_game().unwrap().name.as_deref(),
            Some("mygame")
        );
        assert!(store.candidate_games().is_empty());
        assert!(!store.add_override(99, T0));
        let activity = store.activity().unwrap();
        assert!(activity.get("application_id").is_none());
    }

    #[test]
    fn rename_moves_the_switch_and_the_entry_to_the_new_key() {
        let mut store = RunningGames::default();
        store.set_candidates(vec![running(5, None, "mygame", "d:/mygame.exe")]);
        store.add_override(5, T0);
        let game = store.visible_game().cloned().unwrap();
        store.toggle_detection(&game, T0);
        store.edit_name(&game, "My Game", T0);
        let renamed = Game {
            name: Some("My Game".into()),
            ..game.clone()
        };
        assert!(store.is_override(&renamed));
        assert!(!store.is_detection_enabled(&renamed));
        assert!(
            store
                .games_seen(false)
                .iter()
                .any(|g| g.name.as_deref() == Some("My Game"))
        );
    }

    #[test]
    fn delete_forgets_the_game_and_hides_a_running_override() {
        let mut store = RunningGames::default();
        store.set_candidates(vec![running(5, None, "mygame", "d:/mygame.exe")]);
        store.add_override(5, T0);
        let game = store.visible_game().cloned().unwrap();
        store.delete_entry(&game, T0 + 1);
        assert!(store.games_seen(false).is_empty());
        assert!(!store.is_override(&game));
        assert!(store.visible_game().is_none());
    }

    #[test]
    fn storage_round_trips_and_keeps_unknown_keys() {
        let mut store = RunningGames::from_json(&json!({
            "gamesSeen": [{"exePath": "c:/a.exe", "cmdLine": "", "lastFocused": 100, "id": "1", "name": "A"}],
            "gameOverrides": {"d:/b.exe:B": {"exePath": "d:/b.exe", "name": "B", "lastFocused": 50, "add": true}},
            "enableDetection": {"c:/a.exe:A": false},
            "enableOverlay": {"c:/a.exe:A": true},
        }));
        assert_eq!(store.games_seen(false).len(), 2);
        assert!(!store.is_detection_enabled(&running(1, Some("1"), "A", "c:/a.exe")));
        let saved = store.to_json();
        assert_eq!(saved["enableOverlay"]["c:/a.exe:A"], true);
        assert_eq!(saved["gameOverrides"]["d:/b.exe:B"]["add"], true);
        assert_eq!(saved["gamesSeen"][0]["id"], "1");
        let again = RunningGames::from_json(&saved);
        assert_eq!(again.to_json(), saved);
        store.set_running(Vec::new(), T0);
        assert_eq!(store.games_seen(false).len(), 2);
    }

    #[test]
    fn games_seen_are_newest_first_and_stored_ids_that_are_numbers_are_dropped() {
        let store = RunningGames::from_json(&json!({
            "gamesSeen": [
                {"exePath": "c:/old.exe", "lastFocused": 10, "name": "Old"},
                {"exePath": "c:/new.exe", "lastFocused": 90, "name": "New", "id": 4}
            ]
        }));
        let seen = store.games_seen(false);
        assert_eq!(seen[0].name.as_deref(), Some("New"));
        assert_eq!(seen[0].id, None);
    }

    #[test]
    fn overlay_switches_are_kept_per_game_and_survive_a_restart() {
        let mut store = RunningGames::default();
        let game = running(4, Some("1"), "Game", "c:/games/game.exe");
        assert_eq!(store.overlay_override(&game), (None, None));
        store.toggle_overlay(&game, true, Some(false));
        assert_eq!(store.overlay_override(&game), (Some(true), Some(false)));
        // Only the legacy value: the v3 one stays.
        store.toggle_overlay(&game, false, None);
        assert_eq!(store.overlay_override(&game), (Some(false), Some(false)));
        let stored = store.to_json();
        assert_eq!(stored["enableOverlayV3"]["c:/games/game.exe:Game"], false);
        let read = RunningGames::from_json(&stored);
        assert_eq!(read.overlay_override(&game), (Some(false), Some(false)));
    }
}
