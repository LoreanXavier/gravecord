//! SteamStatusSync (Equicord `src/equicordplugins/steamStatusSync`, GPL-3.0-or-later): when your status changes (the
//! settings' `status` and `showCurrentGame`, as `USER_SETTINGS_PROTO_UPDATE` carries them), Steam's follows through
//! `steam://friends/status/<status>`: the Steam status chosen for Online, Idle, Do Not Disturb or Invisible ("Disabled"
//! leaves Steam alone), or Invisible whatever the status while game activity is hidden with
//! `goInvisibleIfActivityIsHidden`. A capture run opens nothing. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use serde_json::Value;
use std::cell::RefCell;

const ID: &str = "SteamStatusSync";

thread_local! {
    /// The status and activity sharing last seen; the first one seen only starts the watch.
    static SEEN: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
}

/// The `steam://` address for a status, if Steam's status changes.
fn steam_address(state: &State, status: &str, sharing: bool) -> Option<String> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    if flag("goInvisibleIfActivityIsHidden") && !sharing {
        return Some("steam://friends/status/invisible".to_owned());
    }
    let chosen = match status {
        "online" => key("onlineStatus"),
        "idle" => key("idleStatus"),
        "dnd" => key("dndStatus"),
        "invisible" => key("invisibleStatus"),
        _ => return None,
    };
    let chosen = chosen.as_str()?.to_owned();
    (chosen != "none").then(|| format!("steam://friends/status/{chosen}"))
}

impl Root {
    /// Before each frame: a change of status tells Steam.
    pub(in crate::ui) fn steam_status_sync(&mut self, cx: &mut gpui::Context<Self>) {
        if crate::plugins::bypassed() || !crate::plugins::on(ID) {
            SEEN.with(|seen| seen.borrow_mut().take());
            return;
        }
        let now = (
            self.stores.status.clone(),
            self.pref_on("activity_privacy_setting", true),
        );
        let changed = SEEN.with(|seen| {
            let mut seen = seen.borrow_mut();
            let changed = seen.as_ref().is_some_and(|before| *before != now);
            *seen = Some(now.clone());
            changed
        });
        if !changed {
            return;
        }
        let Some(address) = crate::plugins::with(|state| steam_address(state, &now.0, now.1))
        else {
            return;
        };
        if crate::storage::is_capture_run() {
            tracing::info!(%address, "steam status sync: not opened in a capture run");
            return;
        }
        cx.open_url(&address);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_as_the_original() {
        let mut state = State::default();
        assert_eq!(steam_address(&state, "online", true), None);
        state.set_enabled(ID, true);
        assert_eq!(
            steam_address(&state, "online", true).as_deref(),
            Some("steam://friends/status/online")
        );
        assert_eq!(
            steam_address(&state, "idle", true).as_deref(),
            Some("steam://friends/status/away")
        );
        assert_eq!(steam_address(&state, "dnd", true), None);
        assert_eq!(
            steam_address(&state, "invisible", true).as_deref(),
            Some("steam://friends/status/invisible")
        );
        state.set_value(ID, "goInvisibleIfActivityIsHidden", &Value::Bool(true));
        assert_eq!(
            steam_address(&state, "online", false).as_deref(),
            Some("steam://friends/status/invisible")
        );
    }
}
