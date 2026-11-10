//! `LocalActivityStore` (module 491009): the activities of this session. The ones games set over RPC are kept per
//! socket as `[pid, activity, partyPrivacy]` (`LOCAL_ACTIVITY_UPDATE`, removed by `RPC_APP_DISCONNECTED`);
//! `getActivities` lists the custom status, then those activities in socket order, then the detected game unless an
//! RPC activity already stands for it.

use serde_json::Value;
use std::collections::BTreeMap;

/// One socket's entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub pid: Option<i64>,
    pub activity: Value,
    pub party_privacy: Option<i64>,
}

#[derive(Default)]
pub struct LocalActivityStore {
    /// Keyed by socket id. The official object keys are the numeric strings `_.uniqueId()` made, which JavaScript
    /// lists in ascending order: the order the sockets were opened.
    applications: BTreeMap<u64, Entry>,
}

impl LocalActivityStore {
    /// `LOCAL_ACTIVITY_UPDATE`: sets the socket's activity, or removes it when `activity` is `None`. Reports whether
    /// the entry changed.
    pub fn update(
        &mut self,
        socket: u64,
        pid: Option<i64>,
        activity: Option<Value>,
        party_privacy: Option<i64>,
    ) -> bool {
        match activity {
            Some(activity) => {
                let entry = Entry {
                    pid,
                    activity,
                    party_privacy,
                };
                if self.applications.get(&socket) == Some(&entry) {
                    return false;
                }
                self.applications.insert(socket, entry);
                true
            }
            None => self.applications.remove(&socket).is_some(),
        }
    }

    /// `RPC_APP_DISCONNECTED`: the socket's activity goes with it.
    pub fn disconnected(&mut self, socket: u64) -> bool {
        self.applications.remove(&socket).is_some()
    }

    /// `getApplicationActivities`: the entries by socket.
    pub fn application_activities(&self) -> &BTreeMap<u64, Entry> {
        &self.applications
    }

    /// `getActivityForPID`.
    pub fn activity_for_pid(&self, pid: i64) -> Option<&Value> {
        self.applications
            .values()
            .find(|entry| entry.pid == Some(pid))
            .map(|entry| &entry.activity)
    }

    /// `getApplicationActivity`: the first listed activity of the application, from `activities`.
    pub fn application_activity<'a>(
        activities: &'a [Value],
        application_id: &str,
    ) -> Option<&'a Value> {
        activities
            .iter()
            .find(|activity| activity["application_id"].as_str() == Some(application_id))
    }

    /// The activities of an RPC client that name an application (`B` pushes only those).
    pub fn rpc_activities(&self) -> Vec<Value> {
        self.applications
            .values()
            .filter(|entry| !entry.activity["application_id"].is_null())
            .map(|entry| entry.activity.clone())
            .collect()
    }

    /// `getActivities` (`B`): the custom status first, the RPC activities in socket order, then the detected game,
    /// left out when an RPC activity has its name or its application (the official check also asks the SDK
    /// resolution of the process and the application's linked games).
    pub fn activities(&self, custom: Option<Value>, detected: Option<Value>) -> Vec<Value> {
        let mut out: Vec<Value> = custom.into_iter().collect();
        let rpc = self.rpc_activities();
        let covered = detected.as_ref().is_some_and(|game| {
            rpc.iter().any(|activity| {
                activity["name"] == game["name"]
                    || (!game["application_id"].is_null()
                        && activity["application_id"] == game["application_id"])
            })
        });
        out.extend(rpc);
        if let Some(game) = detected.filter(|_| !covered) {
            out.push(game);
        }
        out
    }

    /// The whole store is cleared (`START_SESSION`).
    pub fn clear(&mut self) {
        self.applications.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn game(app: &str, name: &str) -> Value {
        json!({"name": name, "type": 0, "application_id": app, "platform": "desktop", "metadata": {}})
    }

    #[test]
    fn activities_are_kept_per_socket_and_listed_in_socket_order() {
        let mut store = LocalActivityStore::default();
        assert!(store.update(7, Some(70), Some(game("2", "Second")), None));
        assert!(store.update(3, Some(30), Some(game("1", "First")), None));
        let names: Vec<_> = store
            .activities(None, None)
            .iter()
            .map(|activity| activity["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["First", "Second"]);
    }

    #[test]
    fn an_update_replaces_and_an_identical_one_changes_nothing() {
        let mut store = LocalActivityStore::default();
        assert!(store.update(1, Some(10), Some(game("1", "A")), None));
        assert!(!store.update(1, Some(10), Some(game("1", "A")), None));
        assert!(store.update(1, Some(10), Some(game("1", "B")), None));
        assert_eq!(store.activities(None, None).len(), 1);
        assert_eq!(store.activities(None, None)[0]["name"], "B");
    }

    #[test]
    fn clearing_and_disconnecting_remove_the_socket() {
        let mut store = LocalActivityStore::default();
        store.update(1, Some(10), Some(game("1", "A")), None);
        store.update(2, Some(20), Some(game("2", "B")), None);
        assert!(store.update(1, Some(10), None, None));
        assert!(!store.update(1, Some(10), None, None));
        assert!(store.disconnected(2));
        assert!(!store.disconnected(2));
        assert!(store.activities(None, None).is_empty());
    }

    #[test]
    fn the_custom_status_leads_and_the_detected_game_follows() {
        let mut store = LocalActivityStore::default();
        store.update(1, Some(10), Some(game("1", "Rpc game")), None);
        let custom = json!({"type": 4, "name": "Custom Status", "state": "hi"});
        let detected = game("9", "Detected");
        let list = store.activities(Some(custom), Some(detected));
        let types: Vec<_> = list.iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert_eq!(types, ["Custom Status", "Rpc game", "Detected"]);
    }

    #[test]
    fn a_detected_game_an_rpc_activity_stands_for_is_left_out() {
        let mut store = LocalActivityStore::default();
        store.update(1, Some(10), Some(game("1", "P.T. Silent Hills")), None);
        let by_name = json!({"type": 0, "name": "P.T. Silent Hills"});
        assert_eq!(store.activities(None, Some(by_name)).len(), 1);
        let by_application = json!({"type": 0, "name": "pt.exe", "application_id": "1"});
        assert_eq!(store.activities(None, Some(by_application)).len(), 1);
    }

    #[test]
    fn an_activity_without_an_application_is_stored_but_not_listed() {
        let mut store = LocalActivityStore::default();
        store.update(1, Some(10), Some(json!({"name": "Loose", "type": 0})), None);
        assert!(store.activities(None, None).is_empty());
        assert_eq!(store.application_activities().len(), 1);
    }

    #[test]
    fn the_activity_of_a_process_is_found_by_pid() {
        let mut store = LocalActivityStore::default();
        store.update(1, Some(10), Some(game("1", "A")), None);
        store.update(2, Some(20), Some(game("2", "B")), None);
        assert_eq!(store.activity_for_pid(20).unwrap()["name"], "B");
        assert_eq!(store.activity_for_pid(99), None);
        let listed = store.activities(None, None);
        assert_eq!(
            LocalActivityStore::application_activity(&listed, "2").unwrap()["name"],
            "B"
        );
    }
}
