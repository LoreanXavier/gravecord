//! The pure matcher: a process list and the detectable lists in, detected games out. Follows the web side of the
//! official client: `RunningGameStore` (module 397369: `eU` observed apps, the callback of `setObservedGamesCallback`
//! with its launcher, non-game, observed-app and blocklist outcomes), `GameStore` (module 760751: `shouldBlock`,
//! `findGame`), `NonGameStore` (module 953384) and the executable shape of `lg` in module 327891.

use crate::processes::Process;
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The OS name the detectable list uses for Windows (`os: "win32"`).
pub const WINDOWS: &str = "win32";

/// One `executables` entry of a detectable game (`lg` in module 327891). `name` is lower-cased on read, as the web
/// side lower-cases it for every comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    pub os: String,
    pub name: String,
    pub is_launcher: bool,
    pub arguments: Option<String>,
}

/// An entry of `GET /games/detectable` (or of the non-games list, which has the same shape).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectableGame {
    pub id: String,
    pub name: String,
    pub executables: Vec<Executable>,
    pub aliases: Vec<String>,
}

impl DetectableGame {
    /// Reads one server entry; `None` without an id and a name.
    pub fn from_json(value: &Value) -> Option<Self> {
        let id = value["id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| value["id"].as_u64().map(|id| id.to_string()))?;
        let name = value["name"].as_str()?.to_owned();
        let executables = value["executables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some(Executable {
                    os: entry["os"].as_str()?.to_owned(),
                    name: normalize_exe_name(entry["name"].as_str()?),
                    is_launcher: entry["is_launcher"]
                        .as_bool()
                        .or_else(|| entry["isLauncher"].as_bool())
                        .unwrap_or(false),
                    arguments: entry["arguments"]
                        .as_str()
                        .filter(|arguments| !arguments.is_empty())
                        .map(str::to_owned),
                })
            })
            .collect();
        let aliases = value["aliases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|alias| alias.as_str().map(str::to_owned))
            .collect();
        Some(Self {
            id,
            name,
            executables,
            aliases,
        })
    }

    /// A list body read entry by entry; entries that do not parse are skipped.
    pub fn list_from_json(body: &Value) -> Vec<Self> {
        body.as_array()
            .into_iter()
            .flatten()
            .filter_map(Self::from_json)
            .collect()
    }
}

/// `GET /games/detectable/exclusions`: executables that are never games, and patterns for the same.
#[derive(Clone, Debug, Default)]
pub struct Blocklist {
    /// Lower-cased fragments; a path that contains one is blocked (`L.find(e=>t.includes(e))`).
    pub executables: Vec<String>,
    patterns: Vec<(String, Regex)>,
}

impl Blocklist {
    pub fn new(executables: &[String], patterns: &[String]) -> Self {
        Self {
            executables: executables
                .iter()
                .map(|e| normalize_exe_name(e))
                .filter(|e| !e.is_empty())
                .collect(),
            patterns: patterns
                .iter()
                .filter_map(|source| {
                    // `RegExp(e, "i")` in `GAMES_BLOCKLIST_UPDATE`.
                    let regex = RegexBuilder::new(source)
                        .case_insensitive(true)
                        .build()
                        .ok()?;
                    Some((source.clone(), regex))
                })
                .collect(),
        }
    }

    pub fn from_json(body: &Value) -> Self {
        let strings = |key: &str| -> Vec<String> {
            body[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        };
        Self::new(&strings("executables"), &strings("patterns"))
    }

    /// The sources of the patterns, for keeping the list on disk.
    pub fn pattern_sources(&self) -> Vec<String> {
        self.patterns
            .iter()
            .map(|(source, _)| source.clone())
            .collect()
    }
}

/// `eb`/`eD` in module 397369 (`Q`): the streamer tools and Spotify that are watched but never shown as games.
struct ObservedApp {
    name: &'static str,
    streamer_tool: bool,
    executables: &'static [&'static str],
}

const OBSERVED_APPS: &[ObservedApp] = &[
    ObservedApp {
        name: "OBS",
        streamer_tool: true,
        executables: &["obs/obs.exe", "obs32.exe", "obs64.exe", "obs.app", "obs"],
    },
    ObservedApp {
        name: "XSplit",
        streamer_tool: true,
        executables: &["xsplit.gamecaster.exe", "xsplit.core.exe", "gamecaster.exe"],
    },
    ObservedApp {
        name: "Bebo",
        streamer_tool: true,
        executables: &["bebo.exe"],
    },
    ObservedApp {
        name: "Streamlabs Desktop",
        streamer_tool: true,
        executables: &[
            "streamlabs obs.exe",
            "streamlabs desktop.exe",
            "streamlabs desktop.app",
        ],
    },
    ObservedApp {
        name: "Twitch Studio",
        streamer_tool: true,
        executables: &["twitchstudio.exe", "twitch studio.app"],
    },
    ObservedApp {
        name: "Spotify",
        streamer_tool: false,
        executables: &["spotify.exe", "spotify.app", "spotify"],
    },
];

/// What the matcher made of one process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Game,
    /// An entry of the non-games list.
    NonGame,
    /// A streamer tool or Spotify (`observed_app`).
    Observed {
        streamer_tool: bool,
    },
    Blocked,
}

/// A process the matcher recognised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    pub pid: u32,
    /// Lower-case with `/` separators, as the native side reports `exePath`.
    pub exe_path: String,
    pub exe_name: String,
    /// The detectable game's id, when the process matched one.
    pub id: Option<String>,
    pub name: String,
    pub is_launcher: bool,
}

/// A running window that is not a known game: what "Add it!" offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    pub exe_name: String,
}

/// The sorted outcome of one scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Matched games (launchers included, flagged), in process order.
    pub games: Vec<Detected>,
    /// Non-games and observed apps, for the streamer-tool count and `getRunningNonGames`.
    pub observed: Vec<(Detected, Kind)>,
}

/// Lower-case with `/` separators (`s` in module 973522 without its trailing-slash rule, which paths never have).
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// Normalizes a detectable list or blocklist executable pattern (`>` flags and `\` separators).
fn normalize_exe_name(name: &str) -> String {
    normalize_path(name.trim().trim_start_matches('>'))
}

/// The last path segment of a normalized path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `path` ends with `name` at a path boundary: the whole path, or after a `/` (the rule of `eU`).
fn ends_with_name(path: &str, name: &str) -> bool {
    if name.is_empty() || !path.ends_with(name) {
        return false;
    }
    let start = path.len() - name.len();
    start == 0 || path.as_bytes()[start - 1] == b'/'
}

/// `eU`: the observed app for a process, by its name, then its file name, then a path suffix.
fn observed_app(name: &str, exe_name: &str, exe_path: &str) -> Option<&'static ObservedApp> {
    let lowered = name.to_lowercase();
    if let Some(app) = OBSERVED_APPS
        .iter()
        .find(|app| app.name.to_lowercase() == lowered)
    {
        return Some(app);
    }
    let file = if exe_name.is_empty() {
        file_name(exe_path)
    } else {
        exe_name
    }
    .to_lowercase();
    let by_name = |wanted: &str| {
        OBSERVED_APPS
            .iter()
            .find(|app| app.executables.contains(&wanted))
    };
    if let Some(app) = by_name(&file) {
        return Some(app);
    }
    let path = exe_path.to_lowercase();
    OBSERVED_APPS.iter().find(|app| {
        app.executables
            .iter()
            .any(|executable| ends_with_name(&path, executable))
    })
}

/// The detectable lists with the blocklist: everything the matcher needs.
#[derive(Clone, Debug, Default)]
pub struct Detectables {
    pub games: Vec<DetectableGame>,
    pub non_games: Vec<DetectableGame>,
    pub blocklist: Blocklist,
    /// Games the user added by hand: a normalized executable path and the name given to it
    /// (`setGameCandidateOverrides`, module 397369 `eQ`).
    pub overrides: Vec<(String, String)>,
}

struct Match<'a> {
    game: &'a DetectableGame,
    launcher: bool,
    length: usize,
    non_game: bool,
}

impl Detectables {
    /// `shouldBlock` (module 760751): the process is blocked unless the game it matched lists this very path for
    /// this OS; otherwise a blocklisted fragment or pattern blocks it.
    pub fn should_block(&self, id: Option<&str>, exe_path: &str, os: &str) -> bool {
        if exe_path.is_empty() {
            return false;
        }
        let path = normalize_path(exe_path);
        if let Some(id) = id
            && let Some(game) = self.games.iter().find(|game| game.id == id)
            && game
                .executables
                .iter()
                .any(|executable| executable.os == os && ends_with_name(&path, &executable.name))
        {
            return false;
        }
        self.blocklist
            .executables
            .iter()
            .any(|fragment| path.contains(fragment.as_str()))
            || self
                .blocklist
                .patterns
                .iter()
                .any(|(_, pattern)| pattern.is_match(exe_path))
    }

    /// Games whose executables match this path on `os`. Entries that need command-line arguments cannot be told
    /// apart without the command line, so they are skipped. Longest matched name first, then list order.
    /// When `exe_path` has no path separators (only bare `szExeFile` from toolhelp), falls back to filename match.
    fn find<'a>(
        &'a self,
        exe_path: &str,
        bare_name: &str,
        os: &str,
        index: &Index,
    ) -> Option<Match<'a>> {
        let mut best: Option<Match<'a>> = None;
        let file = if exe_path.is_empty() {
            bare_name
        } else {
            file_name(exe_path)
        };
        let allow_bare_fallback = !exe_path.contains('/');
        for &(non_game, position) in index.by_file.get(file).into_iter().flatten() {
            let game = if non_game {
                &self.non_games[position]
            } else {
                &self.games[position]
            };
            for executable in &game.executables {
                if executable.os != os || executable.arguments.is_some() {
                    continue;
                }
                let matched = ends_with_name(exe_path, &executable.name)
                    || (allow_bare_fallback && file_name(&executable.name) == file);
                if !matched {
                    continue;
                }
                let length = executable.name.len();
                if best.as_ref().is_none_or(|known| length > known.length) {
                    best = Some(Match {
                        game,
                        launcher: executable.is_launcher,
                        length,
                        non_game,
                    });
                }
            }
        }
        best
    }

    /// Sorts the process list using a prebuilt [`Index`] (the hot path for repeated scans).
    pub fn scan_with_index(&self, processes: &[Process], os: &str, index: &Index) -> Scan {
        let mut scan = Scan::default();
        for process in processes {
            let exe_path = normalize_path(&process.path);
            let bare_name = normalize_path(&process.name);
            let exe_name = if exe_path.is_empty() {
                bare_name.clone()
            } else {
                file_name(&exe_path).to_owned()
            };
            let found = self.find(&exe_path, &bare_name, os, index);
            let id = found.as_ref().map(|found| found.game.id.clone());
            let name = found
                .as_ref()
                .map_or_else(|| process.name.clone(), |found| found.game.name.clone());
            let detected = Detected {
                pid: process.pid,
                exe_path: exe_path.clone(),
                exe_name: exe_name.clone(),
                id,
                name,
                is_launcher: found.as_ref().is_some_and(|found| found.launcher)
                    || index.launchers.contains(exe_name.as_str()),
            };
            if found.as_ref().is_some_and(|found| found.non_game) {
                scan.observed.push((detected, Kind::NonGame));
                continue;
            }
            if let Some(app) = observed_app(&detected.name, &exe_name, &exe_path) {
                scan.observed.push((
                    detected,
                    Kind::Observed {
                        streamer_tool: app.streamer_tool,
                    },
                ));
                continue;
            }
            // Only a recognised game is a running game here: the official list of running games is what the
            // native observer matched; unknown processes are candidates, not games.
            if found.is_none() {
                if let Some((_, name)) = self.overrides.iter().find(|(path, _)| *path == exe_path) {
                    let mut detected = detected;
                    detected.name = name.clone();
                    scan.games.push(detected);
                }
                continue;
            }
            if self.should_block(detected.id.as_deref(), &exe_path, os) {
                continue;
            }
            scan.games.push(detected);
        }
        scan
    }

    /// Sorts the process list the way the observed-games callback does (module 397369): non-games and launchers
    /// are flagged, observed apps leave the games, blocklisted paths drop out.
    pub fn scan(&self, processes: &[Process], os: &str) -> Scan {
        let index = Index::build(self, os);
        self.scan_with_index(processes, os, &index)
    }

    /// Processes with a window that matched nothing and are not blocked: the "Add it!" picker's list. System
    /// folders are left out. The name is the file name without its extension.
    pub fn candidates(
        &self,
        processes: &[Process],
        window_pids: &HashSet<u32>,
        scan: &Scan,
        os: &str,
    ) -> Vec<Candidate> {
        let known: HashSet<u32> = scan
            .games
            .iter()
            .map(|game| game.pid)
            .chain(scan.observed.iter().map(|(game, _)| game.pid))
            .collect();
        let mut seen_paths = HashSet::new();
        let mut out = Vec::new();
        for process in processes {
            if !window_pids.contains(&process.pid) || known.contains(&process.pid) {
                continue;
            }
            let exe_path = normalize_path(&process.path);
            if exe_path.contains('/') && is_system_path(&exe_path) {
                continue;
            }
            if self.should_block(None, &exe_path, os) || !seen_paths.insert(exe_path.clone()) {
                continue;
            }
            let exe_name = file_name(&exe_path).to_owned();
            let name = process
                .name
                .rsplit_once('.')
                .map_or(process.name.as_str(), |(stem, _)| stem)
                .to_owned();
            out.push(Candidate {
                pid: process.pid,
                name,
                exe_path,
                exe_name,
            });
        }
        out.sort_by(|a, b| {
            (a.name.to_lowercase(), &a.exe_path).cmp(&(b.name.to_lowercase(), &b.exe_path))
        });
        out
    }
}

fn is_system_path(path: &str) -> bool {
    path.get(1..).is_some_and(|rest| {
        rest.starts_with(":/windows/")
            || rest.starts_with(":/program files/windowsapps/")
            || rest.starts_with(":/programdata/")
    })
}

/// Executable file name to the games listing it, so a scan does not compare every process with every game.
#[derive(Clone, Debug, Default)]
pub struct Index {
    by_file: HashMap<String, Vec<(bool, usize)>>,
    launchers: HashSet<String>,
}

impl Index {
    pub fn build(lists: &Detectables, os: &str) -> Self {
        let mut by_file: HashMap<String, Vec<(bool, usize)>> = HashMap::new();
        let mut launchers = HashSet::new();
        for (non_game, games) in [(false, &lists.games), (true, &lists.non_games)] {
            for (position, game) in games.iter().enumerate() {
                for executable in game.executables.iter().filter(|e| e.os == os) {
                    if !non_game && executable.is_launcher {
                        launchers.insert(executable.name.clone());
                    }
                    let file = file_name(&executable.name);
                    if file.is_empty() {
                        continue;
                    }
                    let bucket = by_file.entry(file.to_owned()).or_default();
                    if bucket.last() != Some(&(non_game, position)) {
                        bucket.push((non_game, position));
                    }
                }
            }
        }
        Self { by_file, launchers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn process(pid: u32, path: &str) -> Process {
        Process {
            pid,
            name: path.rsplit(['\\', '/']).next().unwrap().to_owned(),
            path: path.to_owned(),
        }
    }

    /// Entries in the shape of `GET /games/detectable`.
    fn lists() -> Detectables {
        let games = json!([
            {"id": "356875221078245376", "name": "Minecraft",
             "executables": [
                {"os": "win32", "name": "minecraft.exe", "is_launcher": false},
                {"os": "darwin", "name": "minecraft.app", "is_launcher": false}],
             "aliases": [], "third_party_skus": []},
            {"id": "356875988589740042", "name": "League of Legends",
             "executables": [
                {"os": "win32", "name": "league of legends.exe", "is_launcher": false},
                {"os": "win32", "name": "leagueclient.exe", "is_launcher": true},
                {"os": "win32", "name": "riot client/riotclientservices.exe", "is_launcher": true}],
             "aliases": ["LoL"]},
            {"id": "1", "name": "Dotted", "executables": [
                {"os": "win32", "name": "bin/game.exe", "is_launcher": false}]},
            {"id": "2", "name": "Argument Game", "executables": [
                {"os": "win32", "name": "launcher.exe", "arguments": "--play"}]},
            {"id": "3", "name": "Odd Game", "executables": [
                {"os": "win32", "name": "notepad.exe"}]}
        ]);
        let non_games = json!([
            {"id": "900", "name": "Some Editor", "executables": [
                {"os": "win32", "name": "editor.exe"}]}
        ]);
        Detectables {
            games: DetectableGame::list_from_json(&games),
            non_games: DetectableGame::list_from_json(&non_games),
            overrides: Vec::new(),
            blocklist: Blocklist::from_json(&json!({
                "executables": ["\\windows\\system32", "crashreporter.exe"],
                "patterns": ["^c:/windows/.*\\.exe$"]
            })),
        }
    }

    #[test]
    fn list_entries_parse_with_lowercased_executables() {
        let list = lists();
        assert_eq!(list.games.len(), 5);
        let league = &list.games[1];
        assert_eq!(league.aliases, ["LoL"]);
        assert!(league.executables[1].is_launcher);
        assert_eq!(league.executables[0].name, "league of legends.exe");
        assert!(DetectableGame::from_json(&json!({"name": "no id"})).is_none());
    }

    #[test]
    fn a_path_matches_by_file_name_at_a_boundary() {
        let list = lists();
        let scan = list.scan(
            &[
                process(10, r"C:\Games\Minecraft\Minecraft.exe"),
                process(11, r"C:\Games\notminecraft.exe"),
                process(12, r"C:\Windows\explorer.exe"),
            ],
            WINDOWS,
        );
        assert_eq!(scan.games.len(), 1);
        let game = &scan.games[0];
        assert_eq!(game.id.as_deref(), Some("356875221078245376"));
        assert_eq!(game.name, "Minecraft");
        assert_eq!(game.exe_path, "c:/games/minecraft/minecraft.exe");
        assert_eq!(game.exe_name, "minecraft.exe");
    }

    #[test]
    fn the_os_must_be_the_running_one() {
        let list = lists();
        let apps = [process(1, "/Applications/Minecraft.app")];
        assert!(list.scan(&apps, WINDOWS).games.is_empty());
        assert_eq!(list.scan(&apps, "darwin").games.len(), 1);
    }

    #[test]
    fn directory_entries_need_the_directory_in_the_path() {
        let list = lists();
        let scan = list.scan(
            &[
                process(1, r"D:\Dotted\bin\game.exe"),
                process(2, r"D:\Dotted\xbin\game.exe"),
                process(3, r"D:\game.exe"),
            ],
            WINDOWS,
        );
        let pids: Vec<u32> = scan.games.iter().map(|game| game.pid).collect();
        assert_eq!(pids, [1]);
    }

    #[test]
    fn launchers_are_flagged_not_dropped() {
        let list = lists();
        let scan = list.scan(
            &[
                process(1, r"C:\Riot Games\League of Legends\LeagueClient.exe"),
                process(2, r"C:\Riot Games\League of Legends\League of Legends.exe"),
                process(3, r"C:\Riot Games\Riot Client\RiotClientServices.exe"),
            ],
            WINDOWS,
        );
        assert_eq!(scan.games.len(), 3);
        assert!(scan.games[0].is_launcher);
        assert!(!scan.games[1].is_launcher);
        assert!(scan.games[2].is_launcher);
    }

    #[test]
    fn entries_with_arguments_are_not_matched_without_a_command_line() {
        let list = lists();
        assert!(
            list.scan(&[process(1, r"C:\x\launcher.exe")], WINDOWS)
                .games
                .is_empty()
        );
    }

    #[test]
    fn non_games_and_observed_apps_are_not_games() {
        let list = lists();
        let scan = list.scan(
            &[
                process(1, r"C:\Tools\editor.exe"),
                process(2, r"C:\Program Files\obs-studio\bin\64bit\obs64.exe"),
                process(3, r"C:\Users\x\AppData\Roaming\Spotify\Spotify.exe"),
            ],
            WINDOWS,
        );
        assert!(scan.games.is_empty());
        assert_eq!(scan.observed.len(), 3);
        assert_eq!(scan.observed[0].1, Kind::NonGame);
        assert_eq!(
            scan.observed[1].1,
            Kind::Observed {
                streamer_tool: true
            }
        );
        assert_eq!(
            scan.observed[2].1,
            Kind::Observed {
                streamer_tool: false
            }
        );
    }

    #[test]
    fn the_blocklist_blocks_unless_the_game_lists_the_path() {
        let list = lists();
        // `notepad.exe` is a listed game executable, so a fragment match does not block it.
        assert!(!list.should_block(Some("3"), r"c:/windows/system32/notepad.exe", WINDOWS));
        // The same path with no game behind it is blocked by the fragment and by the pattern.
        assert!(list.should_block(None, r"c:/windows/system32/notepad.exe", WINDOWS));
        assert!(list.should_block(None, r"c:/games/crashreporter.exe", WINDOWS));
        assert!(list.should_block(None, r"C:/Windows/foo.exe", WINDOWS));
        assert!(!list.should_block(None, r"c:/games/foo.exe", WINDOWS));
        assert!(!list.should_block(None, "", WINDOWS));
        let scan = list.scan(&[process(1, r"C:\Windows\System32\notepad.exe")], WINDOWS);
        assert_eq!(scan.games.len(), 1);
    }

    #[test]
    fn a_hand_added_path_is_a_game_with_the_given_name() {
        let mut list = lists();
        list.overrides = vec![("d:/stuff/mygame.exe".into(), "My Game".into())];
        let scan = list.scan(&[process(4, r"D:\Stuff\MyGame.exe")], WINDOWS);
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].name, "My Game");
        assert_eq!(scan.games[0].id, None);
    }

    #[test]
    fn candidates_are_unmatched_windows_outside_system_folders() {
        let list = lists();
        let processes = [
            process(1, r"C:\Games\Minecraft\Minecraft.exe"),
            process(2, r"D:\Stuff\MyGame.exe"),
            process(3, r"C:\Windows\explorer.exe"),
            process(4, r"D:\Stuff\NoWindow.exe"),
            process(5, r"D:\Other\mygame.exe"),
        ];
        let scan = list.scan(&processes, WINDOWS);
        let windows: HashSet<u32> = [1, 2, 3, 5].into_iter().collect();
        let candidates = list.candidates(&processes, &windows, &scan, WINDOWS);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].name, "mygame");
        assert_eq!(candidates[0].exe_path, "d:/other/mygame.exe");
    }

    #[test]
    fn hytale_and_prefixed_or_backslash_patterns_and_bare_fallback_match() {
        let games = json!([
            {
                "id": "1458530944955973852",
                "name": "Hytale",
                "executables": [
                    {"os": "win32", "name": ">HytaleClient.exe", "is_launcher": false},
                    {"os": "win32", "name": "win64\\HytaleLauncher.exe", "is_launcher": true}
                ],
                "aliases": []
            }
        ]);
        let list = Detectables {
            games: DetectableGame::list_from_json(&games),
            non_games: Vec::new(),
            overrides: Vec::new(),
            blocklist: Blocklist::from_json(&json!({
                "executables": [">CrashReport\\"],
                "patterns": []
            })),
        };
        assert_eq!(list.games[0].executables[0].name, "hytaleclient.exe");
        assert_eq!(list.games[0].executables[1].name, "win64/hytalelauncher.exe");

        // Full path, subdirectory with backslash normalized, and bare szExeFile fallback when OpenProcess failed.
        let scan = list.scan(
            &[
                process(10, r"C:\Program Files\Hypixel Studios\Hytale\Client\HytaleClient.exe"),
                process(11, r"C:\Program Files\Hypixel Studios\Hytale\win64\HytaleLauncher.exe"),
                Process {
                    pid: 12,
                    name: "HytaleLauncher.exe".to_owned(),
                    path: "HytaleLauncher.exe".to_owned(),
                },
            ],
            WINDOWS,
        );
        assert_eq!(scan.games.len(), 3);
        assert_eq!(scan.games[0].id.as_deref(), Some("1458530944955973852"));
        assert_eq!(scan.games[0].name, "Hytale");
        assert!(!scan.games[0].is_launcher);
        assert!(scan.games[1].is_launcher);
        assert_eq!(scan.games[2].id.as_deref(), Some("1458530944955973852"));

        // Blocklist normalization matches forward-slash paths.
        assert!(list.should_block(None, r"C:\Games\CrashReport\dump.exe", WINDOWS));
    }
}
