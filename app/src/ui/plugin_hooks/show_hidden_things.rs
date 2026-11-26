//! ShowHiddenThings (Vencord `src/plugins/showHiddenThings`, GPL-3.0-or-later): moderator-only marks shown whatever
//! the account may do. `showTimeouts` drops the `canManageUser(MODERATE_MEMBERS)` test of
//! `showCommunicationDisabledStyles` (module 549587), so the clock by a timed-out member's name shows to everyone;
//! `showInvitesPaused` drops the `MANAGE_GUILD` test of the server rail's "Invites paused for this server" line
//! (module 347951); `showModView` makes the Mod View gate (module 985925) answer yes in every server, so the user
//! menu offers Open in Mod View everywhere. Not part of the pinned client.

use crate::plugins::State;

const ID: &str = "ShowHiddenThings";

fn on(value: serde_json::Value) -> bool {
    value == serde_json::Value::Bool(true)
}

fn timeouts_in(state: &State) -> bool {
    state.is_on(ID) && on(state.value(ID, "showTimeouts"))
}

fn invites_paused_in(state: &State) -> bool {
    state.is_on(ID) && on(state.value(ID, "showInvitesPaused"))
}

fn mod_view_in(state: &State) -> bool {
    state.is_on(ID) && on(state.value(ID, "showModView"))
}

fn read(test: fn(&State) -> bool) -> bool {
    !crate::plugins::bypassed() && crate::plugins::with(test)
}

/// `showTimeouts`: the timeout clock skips the moderator test. Read only for a member whose timeout runs.
pub(in crate::ui) fn show_hidden_timeouts() -> bool {
    read(timeouts_in)
}

/// `showInvitesPaused`: the paused line skips the Manage Server test.
pub(in crate::ui) fn show_hidden_invites_paused() -> bool {
    read(invites_paused_in)
}

/// `showModView`: the Mod View gate answers yes in any server.
pub(in crate::ui) fn show_hidden_mod_view() -> bool {
    read(mod_view_in)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn every_switch_is_on_by_default_and_off_with_the_plugin() {
        let mut state = State::default();
        assert!(!timeouts_in(&state) && !invites_paused_in(&state) && !mod_view_in(&state));
        state.set_enabled(ID, true);
        assert!(timeouts_in(&state) && invites_paused_in(&state) && mod_view_in(&state));
        assert!(state.set_value(ID, "showInvitesPaused", &Value::Bool(false)));
        assert!(timeouts_in(&state) && !invites_paused_in(&state) && mod_view_in(&state));
        state.master = false;
        assert!(!timeouts_in(&state) && !mod_view_in(&state));
    }
}
