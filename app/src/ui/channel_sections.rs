//! Whether the member list and a DM's profile panel are open, kept across starts as `ChannelSectionStore` (module
//! 799704, persisted as `ChannelSectionStore2`) keeps `isMembersOpen` (closed at first) and `isProfileOpen` (open at
//! first). The store's other keys (summaries, friends, the sidebars) are left as they are.

use super::Root;
use crate::storage;
use serde_json::{Map, Value, json};

const STORE: &str = "ChannelSectionStore2";

/// `isMembersOpen` and `isProfileOpen` of a saved store, with the store's own defaults.
pub(super) fn sections_in(store: &Value) -> (bool, bool) {
    (
        store["isMembersOpen"].as_bool().unwrap_or(false),
        store["isProfileOpen"].as_bool().unwrap_or(true),
    )
}

pub(super) fn with_sections(store: Value, members: bool, profile: bool) -> Value {
    let mut store = match store {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    store.insert("isMembersOpen".to_owned(), json!(members));
    store.insert("isProfileOpen".to_owned(), json!(profile));
    Value::Object(store)
}

/// The two panels as the last run left them. A harness run starts from the defaults, as it does with the sidebar's
/// width, so every capture opens on the same frame.
pub(super) fn restored() -> (bool, bool) {
    if storage::is_capture_run() {
        return sections_in(&Value::Null);
    }
    sections_in(&storage::get(STORE).unwrap_or(Value::Null))
}

impl Root {
    /// The member list or the DM profile was opened or closed (`CHANNEL_TOGGLE_MEMBERS_SECTION`,
    /// `USER_PROFILE_SIDEBAR_TOGGLE_SECTION`): saved.
    pub(super) fn sections_toggled(&self) {
        let stored = storage::get(STORE).unwrap_or(Value::Null);
        if sections_in(&stored) == (self.show_members, self.show_dm_profile) && stored.is_object() {
            return;
        }
        storage::set(
            STORE,
            with_sections(stored, self.show_members, self.show_dm_profile),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_start_has_the_member_list_closed_and_the_profile_open() {
        assert_eq!(sections_in(&Value::Null), (false, true));
    }

    #[test]
    fn the_panels_survive_the_round_trip_and_the_rest_of_the_store_stays() {
        let store = json!({"isSummariesOpen": true, "sidebars": {"1": {"type": 0}}});
        let store = with_sections(store, true, false);
        assert_eq!(sections_in(&store), (true, false));
        assert_eq!(store["isSummariesOpen"], true);
        assert_eq!(store["sidebars"]["1"]["type"], 0);
    }
}
