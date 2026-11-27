//! Built-in plugins: Rust ports of Vencord and Equicord plugins. Not part of the pinned
//! client. Each one is a switchable feature with its own settings, listed under the settings sidebar's Gravecord
//! group (`ui/settings/mods.rs`). Every plugin is off until it is turned on, except the ones whose definition says
//! otherwise, and the master switch on the Gravecord page turns them all off at once.
//!
//! The behaviour follows the original plugins, read in `Vendicated/Vencord` (`src/plugins`) and `Equicord/Equicord`
//! (`src/equicordplugins`), both GPL-3.0-or-later like gravecord. Each definition names the file it was read from.
//! The code is a native rewrite: there are no webpack patches, so a Discord update cannot break a port the way it
//! breaks a JS patch, and the plugins ship and update with the app (see `DECISIONS.md`, "Built-in plugins").
//!
//! The state is one JSON object in `storage.json` under `plugins`. Code that a plugin changes asks [`on`] at the
//! place it acts, so a switch takes effect on the next frame without a restart.

mod backup;
pub(crate) mod better_audio_player;
pub(crate) mod better_forwards;
pub(crate) mod clear_urls;
pub(crate) mod clip_upload;
pub(crate) mod custom_rpc;
pub(crate) mod decor;
pub(crate) mod experiments;
mod experiments_table;
pub(crate) mod gif_maker;
pub(crate) mod gif_provider;
pub(crate) mod keyword_notify;
pub(crate) mod limitless_screenshare;
pub(crate) mod message_logger_enhanced;
pub(crate) mod quoter;
mod registry;
pub(crate) mod sed_enhanced;
pub(crate) mod streaks;
mod state;
pub(crate) mod translate_plus;
pub(crate) mod voice_message_transcriber;
pub(crate) mod zip;
pub(crate) mod zip_preview;

pub(crate) use backup::{Report, export, import};
pub(crate) use registry::{DEFS, Def, IDLE_SETTINGS, Kind, SHOWN_WHEN, Setting, def};
pub(crate) use state::State;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

/// The `storage.json` key that holds [`State`].
const STORAGE_KEY: &str = "plugins";

fn cell() -> &'static RwLock<State> {
    static CELL: OnceLock<RwLock<State>> = OnceLock::new();
    CELL.get_or_init(|| {
        // Unit tests never read the user's storage: they start from the defaults.
        let saved = if cfg!(test) {
            None
        } else {
            crate::storage::get(STORAGE_KEY)
        };
        #[allow(unused_mut)]
        let mut state = saved.map_or_else(State::default, |value| State::from_value(&value));
        // Capture only: `GRAVECORD_PLUGINS_ALL_ON=1` starts with the master switch and every plugin on (their
        // settings as they are), for `cargo xtask perf` with the plugins on.
        #[cfg(feature = "capture")]
        if std::env::var("GRAVECORD_PLUGINS_ALL_ON").is_ok_and(|value| value == "1") {
            state.master = true;
            // `GRAVECORD_PLUGINS_EXCEPT`: comma-separated plugins left off (a perf script that clicks the rail by
            // its position cannot run with ServerListIndicators' extra rail item).
            let except = std::env::var("GRAVECORD_PLUGINS_EXCEPT").unwrap_or_default();
            for def in DEFS {
                let skipped = except.split(',').any(|id| id.trim() == def.id);
                state.set_enabled(def.id, !skipped);
            }
        }
        RwLock::new(state)
    })
}

/// The reference mode of the plugins-off check (`cargo xtask plugins-off`): with `GRAVECORD_PLUGINS_BYPASS=1` a
/// capture build takes the pinned client's path at every hook site without reading the plugin state at all, so its
/// frames show what the app draws with the plugin code skipped. Always false outside capture builds.
pub(crate) fn bypassed() -> bool {
    #[cfg(feature = "capture")]
    {
        static BYPASS: OnceLock<bool> = OnceLock::new();
        *BYPASS.get_or_init(|| {
            std::env::var("GRAVECORD_PLUGINS_BYPASS").is_ok_and(|value| value == "1")
        })
    }
    #[cfg(not(feature = "capture"))]
    false
}

thread_local! {
    /// What the plugins did on this thread: one count per plugin whose gate let it act (`State::is_on` answered yes)
    /// and per write to the plugins' storage. All of it is zero while the plugins are off; the tests and the
    /// `plugin effects` harness verb read it.
    static EFFECTS: RefCell<BTreeMap<String, u64>> = const { RefCell::new(BTreeMap::new()) };
}

/// Counts one effect (see [`EFFECTS`]).
/// Counted in tests and capture builds only; the shipped app has nothing to prove it to.
pub(crate) fn note(what: &str) {
    if cfg!(any(test, feature = "capture")) {
        EFFECTS.with(|effects| {
            let mut effects = effects.borrow_mut();
            match effects.get_mut(what) {
                Some(count) => *count += 1,
                None => {
                    effects.insert(what.to_owned(), 1);
                }
            }
        });
    }
}

/// The effects counted on this thread since the last [`reset_effects`].
#[cfg_attr(not(any(test, feature = "capture")), allow(dead_code))]
pub(crate) fn effects() -> BTreeMap<String, u64> {
    EFFECTS.with(|effects| effects.borrow().clone())
}

#[cfg_attr(not(any(test, feature = "capture")), allow(dead_code))]
pub(crate) fn reset_effects() {
    EFFECTS.with(|effects| effects.borrow_mut().clear());
}

/// A copy of the current state, for the settings pages.
pub(crate) fn snapshot() -> State {
    cell().read().map(|state| state.clone()).unwrap_or_default()
}

/// Reads the state without copying it (the settings pages copy it with [`snapshot`]).
pub(crate) fn with<R>(read: impl FnOnce(&State) -> R) -> R {
    match cell().read() {
        Ok(state) => read(&state),
        Err(_) => read(&State::default()),
    }
}

/// Bumped on every [`update`]: a hook that builds something from the settings each frame builds it again only when
/// this moved.
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The settings' revision (see [`REVISION`]).
pub(crate) fn revision() -> u64 {
    REVISION.load(std::sync::atomic::Ordering::Relaxed)
}

/// The plugin is turned on and the master switch is on.
pub(crate) fn on(id: &str) -> bool {
    !bypassed() && cell().read().is_ok_and(|state| state.is_on(id))
}

/// Changes the state and writes it through to `storage.json` (a harness run keeps it in memory, see `storage`).
pub(crate) fn update(change: impl FnOnce(&mut State)) {
    let Ok(mut state) = cell().write() else {
        return;
    };
    change(&mut state);
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    note("storage:plugins");
    if !cfg!(test) {
        crate::storage::set(STORAGE_KEY, state.to_value());
    }
}

/// The plugin is on: [`on`] for a plugin known at compile time, named once so a typo cannot pass silently.
#[macro_export]
macro_rules! plugin_on {
    ($id:literal) => {{
        const _: () = assert!($crate::plugins::registry_has($id), "no such plugin");
        $crate::plugins::on($id)
    }};
}

/// Whether a plugin of this id is defined (usable in constant context).
pub(crate) const fn registry_has(id: &str) -> bool {
    let mut index = 0;
    while index < DEFS.len() {
        if const_eq(DEFS[index].id, id) {
            return true;
        }
        index += 1;
    }
    false
}

const fn const_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The text sent for a message, after the plugins that rewrite outgoing text (`onBeforeMessageSend` and
/// `onBeforeMessageEdit` in Vencord's MessageEvents API).
pub(crate) fn outgoing(content: &str) -> String {
    let mut content = content.to_owned();
    if !bypassed() && on("ClearURLs") {
        content = clear_urls::clean_message(&content);
    }
    // MoreCommands' `uwuEveryMessage` (its pre-send and pre-edit listeners, after ClearURLs').
    if !bypassed() {
        content = crate::ui::more_commands_outgoing(content);
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_check_finds_known_ids_only() {
        assert!(registry_has("ClearURLs"));
        assert!(!registry_has("ClearURL"));
        assert!(!registry_has(""));
    }

    #[test]
    fn outgoing_text_is_unchanged_while_every_plugin_is_off() {
        let text = "see https://example.com/?utm_source=x";
        assert_eq!(outgoing(text), text);
    }
}
