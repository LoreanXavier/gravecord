//! SilentMessageToggle (Vencord `src/plugins/silentMessageToggle`, GPL-3.0-or-later): a chat bar button (after
//! PreviewMessage's, in the plugins' order) toggles silent sending: the bell with "z", or crossed out in the danger
//! colour while off; "Enable Silent Message" / "Disable Silent Message". While on, a message you send gets the
//! `@silent ` prefix (which the send turns into SUPPRESS_NOTIFICATIONS) and a voice message the flag itself;
//! `autoDisable` turns the toggle off after one. `persistState`: "none" turns it off when the channel changes,
//! "channels" keeps it across channels, "restarts" also across restarts. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use dc_state::Id;
use gpui::Context;
use serde_json::{Value, json};
use std::cell::Cell;

const ID: &str = "SilentMessageToggle";
/// The plugin's private `silent` setting, kept when `persistState` is "restarts".
const STORAGE: &str = "plugin-silent-message-toggle";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Persist {
    None,
    Channels,
    Restarts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Settings {
    persist: Persist,
    auto_disable: bool,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    Some(Settings {
        persist: match key("persistState").as_str() {
            Some("channels") => Persist::Channels,
            Some("restarts") => Persist::Restarts,
            _ => Persist::None,
        },
        auto_disable: flag("autoDisable"),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

thread_local! {
    /// The toggle; `None` until first read (`start` clears it unless it persists across restarts).
    static SILENT: Cell<Option<bool>> = const { Cell::new(None) };
    /// The channel the button was last drawn for, and the persistence it was drawn with.
    static SEEN: Cell<Option<(Option<Id>, Persist)>> = const { Cell::new(None) };
}

fn silent(settings: Settings) -> bool {
    SILENT.with(|cell| match cell.get() {
        Some(silent) => silent,
        None => {
            let stored = settings.persist == Persist::Restarts
                && crate::storage::get(STORAGE)
                    .is_some_and(|value| value["silent"] == Value::Bool(true));
            cell.set(Some(stored));
            stored
        }
    })
}

fn set_silent(on: bool) {
    SILENT.with(|cell| cell.set(Some(on)));
    if !cfg!(test) {
        crate::storage::set(STORAGE, json!({ "silent": on }));
    }
}

/// `takeSilentFlag`: whether this send is silent; `autoDisable` turns the toggle off as it is taken.
fn take(settings: Settings) -> bool {
    if !silent(settings) {
        return false;
    }
    if settings.auto_disable {
        set_silent(false);
    }
    true
}

/// The pre-send listener: `@silent ` before the text unless it starts with the prefix already.
fn prefixed(content: String) -> String {
    let (already, _) = dc_state::split_silent_prefix(&content);
    if already {
        content
    } else {
        format!("@silent {content}")
    }
}

impl Root {
    /// The button's state for this frame: `None` while the plugin is off, else whether silent sending is on. A channel
    /// change turns it off with `persistState` "none", as does choosing "none".
    pub(in crate::ui) fn silent_message_state(&self) -> Option<bool> {
        let settings = current()?;
        let now = (self.channel, settings.persist);
        let before = SEEN.with(|seen| seen.replace(Some(now)));
        if settings.persist == Persist::None
            && before.is_some_and(|(channel, persist)| channel != now.0 || persist != Persist::None)
        {
            SILENT.with(|cell| cell.set(Some(false)));
        }
        Some(silent(settings))
    }

    pub(in crate::ui) fn silent_message_toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(settings) = current() {
            set_silent(!silent(settings));
            cx.notify();
        }
    }

    /// A message is sent: with the toggle on it goes out silent.
    pub(in crate::ui) fn silent_message_presend(&self, content: String) -> String {
        match current() {
            Some(settings) if take(settings) => prefixed(content),
            _ => content,
        }
    }

    /// A voice message is sent: SUPPRESS_NOTIFICATIONS while the toggle is on (the plugin's patch of its flags).
    pub(in crate::ui) fn silent_message_take_flag(&self) -> u64 {
        match current() {
            Some(settings) if take(settings) => dc_state::SUPPRESS_NOTIFICATIONS_FLAG,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefix_and_settings_follow_the_original() {
        assert_eq!(prefixed("hi".to_owned()), "@silent hi");
        assert_eq!(prefixed("@silent hi".to_owned()), "@silent hi");
        assert_eq!(prefixed("@silently".to_owned()), "@silent @silently");
        assert_eq!(prefixed(String::new()), "@silent ");
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(
            settings_of(&state),
            Some(Settings {
                persist: Persist::None,
                auto_disable: true
            })
        );
    }

    #[test]
    fn auto_disable_takes_the_toggle_once() {
        let settings = Settings {
            persist: Persist::Channels,
            auto_disable: true,
        };
        SILENT.with(|cell| cell.set(Some(true)));
        assert!(take(settings));
        assert!(!take(settings));
        let keep = Settings {
            auto_disable: false,
            ..settings
        };
        SILENT.with(|cell| cell.set(Some(true)));
        assert!(take(keep) && take(keep));
    }
}
