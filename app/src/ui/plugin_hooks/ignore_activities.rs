//! IgnoreActivities (Vencord `src/plugins/ignoreActivities`, GPL-3.0-or-later): activities kept off your own status
//! only (`LocalActivityStore`'s list filtered by `isActivityNotIgnored` before it goes out). A kind can be ignored
//! wholesale (`ignorePlaying` 0, `ignoreStreaming` 1, `ignoreListening` 2, `ignoreWatching` 3, `ignoreCompeting` 5;
//! a custom status is never touched); `idsList` names application ids that, in `listMode` Blacklist, are ignored,
//! and in Whitelist are kept even when ignored otherwise; and a registered game's overflow menu carries an "Enable
//! Activity" check (`registered-game-overflow-menu`) that keeps that game off (by its id, else its exe path; checking
//! it again turns detection back on when it was off). The ignored games are kept apart from the settings (the
//! original's `ignoredActivities`, a custom setting).
//!
//! GAP: `importCustomRPC` (CustomRPC is not ported), the eye toggle on activities in the app launcher, and `start`'s
//! pruning of ignored games never seen. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use serde_json::{Value, json};
use std::cell::RefCell;

const ID: &str = "IgnoreActivities";
const STORAGE_KEY: &str = "plugin-ignore-activities";

/// `FilterMode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Whitelist,
    Blacklist,
}

/// The settings that decide.
#[derive(Clone, Debug, PartialEq)]
struct Rules {
    mode: Mode,
    ids: String,
    kinds: [bool; 6],
}

fn rules(state: &State) -> Option<Rules> {
    if !state.is_on(ID) {
        return None;
    }
    let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
    Some(Rules {
        mode: if state.value(ID, "listMode") == json!(1) {
            Mode::Blacklist
        } else {
            Mode::Whitelist
        },
        ids: state
            .value(ID, "idsList")
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        kinds: [
            flag("ignorePlaying"),
            flag("ignoreStreaming"),
            flag("ignoreListening"),
            flag("ignoreWatching"),
            false,
            flag("ignoreCompeting"),
        ],
    })
}

fn current() -> Option<Rules> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(rules)
}

thread_local! {
    /// The ignored activities (`{id, name, type}`), loaded once.
    static IGNORED: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

fn ignored_ids() -> Vec<String> {
    IGNORED.with(|ignored| {
        let mut ignored = ignored.borrow_mut();
        let list = ignored.get_or_insert_with(|| {
            if cfg!(test) {
                return Vec::new();
            }
            crate::storage::get(STORAGE_KEY)
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default()
        });
        list.iter()
            .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
            .collect()
    })
}

/// Adds or removes `id`. True when it is now ignored.
fn toggle_ignored(id: &str, name: &str) -> bool {
    let now = IGNORED.with(|ignored| {
        let mut ignored = ignored.borrow_mut();
        let list = ignored.get_or_insert_with(Vec::new);
        if let Some(at) = list.iter().position(|entry| entry["id"] == id) {
            list.remove(at);
            false
        } else {
            // `ActivitiesTypes.Game`.
            list.push(json!({"id": id, "name": name, "type": 0}));
            true
        }
    });
    if !cfg!(test) {
        let list = IGNORED.with(|ignored| ignored.borrow().clone().unwrap_or_default());
        crate::storage::set(STORAGE_KEY, Value::Array(list));
    }
    now
}

/// `isActivityTypeIgnored`.
fn kind_ignored(rules: &Rules, kind: u64, application: Option<&str>) -> bool {
    if let Some(id) = application
        && rules.ids.contains(id)
    {
        return rules.mode == Mode::Blacklist;
    }
    rules.kinds.get(kind as usize).copied().unwrap_or(false)
}

/// `isActivityNotIgnored`: `exe` is the running game's exe path for an activity without an application.
fn kept(rules: &Rules, ignored: &[String], activity: &Value, exe: Option<&str>) -> bool {
    let kind = activity["type"].as_u64().unwrap_or(0);
    let application = activity["application_id"].as_str();
    if kind_ignored(rules, kind, application) {
        return false;
    }
    match application {
        Some(id) => {
            !ignored.iter().any(|ignored| ignored == id)
                || (rules.mode == Mode::Whitelist && rules.ids.contains(id))
        }
        None => exe.is_none_or(|exe| !ignored.iter().any(|ignored| ignored == exe)),
    }
}

impl Root {
    /// Your own activities with the ignored ones left out; as they are while the plugin is off.
    pub(in crate::ui) fn ignore_activities_filter(&self, activities: Vec<Value>) -> Vec<Value> {
        let Some(rules) = current() else {
            return activities;
        };
        let ignored = ignored_ids();
        let running = self.detect.store.running_games();
        activities
            .into_iter()
            .filter(|activity| {
                let exe = activity["name"].as_str().and_then(|name| {
                    running
                        .iter()
                        .find(|game| game.name.as_deref() == Some(name))
                        .map(|game| game.exe_path.as_str())
                });
                kept(&rules, &ignored, activity, exe)
            })
            .collect()
    }

    /// The "Enable Activity" check of a registered game's menu: `Some(checked)` while the plugin is on.
    pub(in crate::ui) fn ignore_activities_check(
        &self,
        game: &dc_state::running_games::Game,
    ) -> Option<bool> {
        current()?;
        let id = game.id.clone().unwrap_or_else(|| game.exe_path.clone());
        Some(!ignored_ids().contains(&id))
    }

    /// The check pressed: the game goes on or off the list; checked again, its detection comes back on.
    pub(in crate::ui) fn ignore_activities_toggle(
        &mut self,
        game: &dc_state::running_games::Game,
        cx: &mut gpui::Context<Self>,
    ) {
        if current().is_none() {
            return;
        }
        let id = game.id.clone().unwrap_or_else(|| game.exe_path.clone());
        let ignored = toggle_ignored(&id, game.name.as_deref().unwrap_or_default());
        if !ignored && !self.detect.store.is_detection_enabled(game) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as u64);
            self.detect.store.toggle_detection(game, now);
            self.detect_changed(cx);
        }
        self.refresh_own_presence();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> State {
        let mut state = State::default();
        state.set_enabled(ID, true);
        state
    }

    #[test]
    fn kinds_are_ignored_by_their_switch_and_custom_status_never() {
        let mut state = on();
        state.set_value(ID, "ignoreListening", &json!(true));
        let rules = rules(&state).unwrap();
        let spotify = json!({"type": 2, "name": "Spotify"});
        let game = json!({"type": 0, "name": "Game", "application_id": "7"});
        let custom = json!({"type": 4, "name": "Custom Status"});
        assert!(!kept(&rules, &[], &spotify, None));
        assert!(kept(&rules, &[], &game, None));
        assert!(kept(&rules, &[], &custom, None));
    }

    #[test]
    fn the_ids_list_blacklists_or_whitelists() {
        let mut state = on();
        state.set_value(ID, "idsList", &json!("7, 8"));
        state.set_value(ID, "ignorePlaying", &json!(true));
        let game = json!({"type": 0, "name": "Game", "application_id": "7"});
        // Whitelist: a listed id stays even with its kind ignored.
        assert!(kept(&rules(&state).unwrap(), &[], &game, None));
        state.set_value(ID, "listMode", &json!(1));
        state.set_value(ID, "ignorePlaying", &json!(false));
        assert!(!kept(&rules(&state).unwrap(), &[], &game, None));
    }

    #[test]
    fn an_ignored_game_goes_by_its_id_or_its_exe() {
        let rules = rules(&on()).unwrap();
        let verified = json!({"type": 0, "name": "Game", "application_id": "7"});
        let unverified = json!({"type": 0, "name": "Local"});
        let ignored = vec!["7".to_owned(), "c:/games/local.exe".to_owned()];
        assert!(!kept(&rules, &ignored, &verified, None));
        assert!(!kept(
            &rules,
            &ignored,
            &unverified,
            Some("c:/games/local.exe")
        ));
        assert!(kept(
            &rules,
            &ignored,
            &unverified,
            Some("c:/games/other.exe")
        ));
        assert!(toggle_ignored("9", "Nine"));
        assert!(ignored_ids().contains(&"9".to_owned()));
        assert!(!toggle_ignored("9", "Nine"));
    }
}
