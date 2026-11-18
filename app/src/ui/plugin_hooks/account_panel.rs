//! AccountPanelServerProfile (Vencord `src/plugins/accountPanelServerProfile`, GPL-3.0-or-later): a right click on the
//! account panel opens a menu with "View Server Profile" (your profile popout in the open server; disabled outside one)
//! and the "Prioritize Server Profile" check. With it checked, a left click shows the server profile and the menu row
//! reads "View Account Profile". Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row};
use crate::plugins::State;
use gpui::Context;
use serde_json::Value;

const ID: &str = "AccountPanelServerProfile";
/// Where the server profile popout points: the panel's avatar.
pub(in crate::ui) const ANCHOR: &str = "account-panel-server-profile";

/// `prioritizeServerProfile`, while the plugin is on.
fn read(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "prioritizeServerProfile") == Value::Bool(true))
}

fn prioritize() -> Option<bool> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(read)
}

/// The menu's rows (`AccountPanelContextMenu`).
pub(super) fn items(prioritize: bool, in_guild: bool) -> Vec<Entry> {
    let mut view = Row::new(
        Act::PluginAlternateProfile,
        if prioritize {
            "View Account Profile"
        } else {
            "View Server Profile"
        },
    );
    view.disabled = !in_guild;
    let mut check = Row::new(
        Act::PluginPrioritizeServerProfile,
        "Prioritize Server Profile",
    );
    check.check = Some(prioritize);
    vec![Entry::Row(view), Entry::Row(check)]
}

impl Root {
    /// Whether the account panel takes a right click (the plugin is on).
    pub(in crate::ui) fn account_panel_menu_on(&self) -> bool {
        prioritize().is_some()
    }

    /// The menu's rows, or none with the plugin off.
    pub(in crate::ui) fn account_panel_items(&self) -> Vec<Entry> {
        prioritize()
            .map(|prioritize| items(prioritize, self.guild.is_some()))
            .unwrap_or_default()
    }

    /// A left click on the panel: the server profile in a server while `prioritizeServerProfile` is checked, else the
    /// account popout as always.
    pub(in crate::ui) fn account_panel_clicked(&mut self, cx: &mut Context<Self>) {
        if prioritize() == Some(true) && self.guild.is_some() {
            self.open_own_server_profile(cx);
        } else {
            self.toggle_status_popout(cx);
        }
    }

    /// "View Server Profile" / "View Account Profile": the other of the two popouts.
    pub(in crate::ui) fn account_panel_alternate(&mut self, cx: &mut Context<Self>) {
        if prioritize() == Some(true) {
            self.toggle_status_popout(cx);
        } else if self.guild.is_some() {
            self.open_own_server_profile(cx);
        }
    }

    /// The "Prioritize Server Profile" check.
    pub(in crate::ui) fn account_panel_toggle_prioritize(&mut self, cx: &mut Context<Self>) {
        let next = prioritize() != Some(true);
        crate::plugins::update(|state| {
            state.set_value(ID, "prioritizeServerProfile", &Value::Bool(next));
        });
        cx.notify();
    }

    /// Your own profile popout in the open server (`UserProfile` with the current channel's guild).
    fn open_own_server_profile(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let Some(anchor) = self.tip_anchors.borrow().get(ANCHOR).copied() else {
            return;
        };
        self.status_popout = None;
        self.open_profile(me, anchor, false, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(entry: &Entry) -> &Row {
        match entry {
            Entry::Row(row) => row,
            Entry::Separator => panic!("a separator"),
        }
    }

    #[test]
    fn the_menu_names_the_other_popout_and_shows_the_check() {
        let menu = items(false, true);
        assert_eq!(row(&menu[0]).label, "View Server Profile");
        assert!(!row(&menu[0]).disabled);
        assert_eq!(row(&menu[1]).check, Some(false));
        let menu = items(true, false);
        assert_eq!(row(&menu[0]).label, "View Account Profile");
        assert!(row(&menu[0]).disabled);
        assert_eq!(row(&menu[1]).check, Some(true));
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some(false));
        state.set_value(ID, "prioritizeServerProfile", &json!(true));
        assert_eq!(read(&state), Some(true));
    }
}
