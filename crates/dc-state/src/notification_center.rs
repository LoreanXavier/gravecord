//! NotificationCenterItemsStore (module 252431), the part the gateway and the item list drive: the server items sorted
//! newest first by id, the set of known ids, and the paging state. Local items (friend requests, scheduled events) are
//! not here.
//!
//! `NOTIFICATION_CENTER_ITEM_CREATE` (`T`) adds an item only after the first load (`initialized`) and only when its id is
//! new; `NOTIFICATION_CENTER_ITEM_DELETE` drops a known id; `NOTIFICATION_CENTER_ITEMS_ACK {id}` marks it acked
//! (`g(ids, true)`); `NOTIFICATION_CENTER_ITEM_COMPLETED {item_enum}` marks every item of that kind completed and acked.
use crate::{Stores, text};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Default)]
pub struct NotificationCenter {
    items: Vec<Value>,
    ids: HashSet<String>,
    loading: bool,
    initialized: bool,
    errored: bool,
    has_more: bool,
    cursor: Option<String>,
}

/// `u.default.compare`: snowflakes compared as numbers.
fn newest_first(a: &Value, b: &Value) -> std::cmp::Ordering {
    let id = |value: &Value| value["id"].as_str().and_then(|id| id.parse::<u64>().ok());
    id(b).cmp(&id(a))
}

impl NotificationCenter {
    /// `m`: the item as the store keeps it (`kind`, and the application id).
    fn shape(item: &Value) -> Value {
        let mut item = item.clone();
        item["kind"] = json!("notification-center-item");
        if let Some(application) = item["application"]["id"].as_str().map(str::to_owned) {
            item["applicationId"] = json!(application);
        }
        item
    }

    fn create(&mut self, item: &Value) {
        let Some(id) = text(&item["id"]) else {
            return;
        };
        if !self.initialized || item["type"].is_null() || self.ids.contains(&id) {
            return;
        }
        self.ids.insert(id);
        self.items.push(Self::shape(item));
        self.items.sort_by(newest_first);
    }

    fn ack(&mut self, ids: &[String], acked: bool) {
        for item in &mut self.items {
            if item["id"]
                .as_str()
                .is_some_and(|id| ids.iter().any(|known| known == id))
            {
                item["acked"] = json!(acked);
            }
        }
    }
}

impl Stores {
    pub(crate) fn notification_center_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.notification_center;
        match name {
            "NOTIFICATION_CENTER_ITEM_CREATE" => store.create(&data["item"]),
            "NOTIFICATION_CENTER_ITEM_DELETE" => {
                if let Some(id) = text(&data["id"])
                    && store.ids.remove(&id)
                {
                    store.items.retain(|item| item["id"].as_str() != Some(&id));
                }
            }
            "NOTIFICATION_CENTER_ITEMS_ACK" => {
                if let Some(id) = text(&data["id"]) {
                    store.ack(&[id], true);
                }
            }
            "NOTIFICATION_CENTER_ITEM_COMPLETED" => {
                for item in &mut store.items {
                    if item["item_enum"] == data["item_enum"] {
                        item["completed"] = json!(true);
                        item["acked"] = json!(true);
                    }
                }
            }
            "LOGOUT" => *store = NotificationCenter::default(),
            _ => {}
        }
    }

    /// `LOAD_NOTIFICATION_CENTER_ITEMS`.
    pub fn notification_center_loading(&mut self) {
        self.notification_center.loading = true;
    }

    /// `LOAD_NOTIFICATION_CENTER_ITEMS_FAILURE`.
    pub fn notification_center_load_failed(&mut self) {
        let store = &mut self.notification_center;
        store.loading = false;
        store.initialized = true;
        store.errored = true;
    }

    /// `LOAD_NOTIFICATION_CENTER_ITEMS_SUCCESS {items, hasMore, cursor}`: only while a load is under way.
    pub fn notification_center_loaded(
        &mut self,
        items: &[Value],
        has_more: bool,
        cursor: Option<&str>,
    ) {
        let store = &mut self.notification_center;
        if !store.loading {
            return;
        }
        store.loading = false;
        store.initialized = true;
        store.errored = false;
        if !cursor.is_some_and(|cursor| store.ids.contains(cursor)) {
            store.has_more = !items.is_empty() && has_more;
            store.cursor = if items.is_empty() {
                None
            } else {
                cursor.map(str::to_owned)
            };
        }
        for item in items {
            let shaped = NotificationCenter::shape(item);
            if shaped["id"]
                .as_str()
                .is_some_and(|id| !store.ids.contains(id))
            {
                store.items.push(shaped);
            }
        }
        store.items.sort_by(newest_first);
        for item in items {
            if let Some(id) = text(&item["id"]) {
                store.ids.insert(id);
            }
        }
    }

    /// `items`.
    pub fn notification_center_items(&self) -> &[Value] {
        &self.notification_center.items
    }

    /// The count the bell shows: items not acked.
    pub fn notification_center_unacked(&self) -> usize {
        self.notification_center
            .items
            .iter()
            .filter(|item| item["acked"] != true)
            .count()
    }

    pub fn notification_center_initialized(&self) -> bool {
        self.notification_center.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: u64) -> Value {
        json!({"id": id, "type": kind, "item_enum": kind, "acked": false})
    }

    #[test]
    fn items_arrive_after_the_first_load_and_stay_newest_first() {
        let mut stores = Stores::default();
        // Before the first load a created item is ignored.
        stores.dispatch("NOTIFICATION_CENTER_ITEM_CREATE", &item("5", 1));
        assert!(stores.notification_center_items().is_empty());
        stores.notification_center_loading();
        stores.notification_center_loaded(&[item("10", 1), item("30", 1)], true, Some("10"));
        let ids: Vec<_> = stores
            .notification_center_items()
            .iter()
            .map(|i| i["id"].clone())
            .collect();
        assert_eq!(ids, [json!("30"), json!("10")]);
        stores.dispatch(
            "NOTIFICATION_CENTER_ITEM_CREATE",
            &json!({"item": item("20", 2)}),
        );
        stores.dispatch(
            "NOTIFICATION_CENTER_ITEM_CREATE",
            &json!({"item": item("20", 2)}),
        );
        assert_eq!(stores.notification_center_items().len(), 3);
        assert_eq!(stores.notification_center_items()[1]["id"], "20");
        assert_eq!(
            stores.notification_center_items()[1]["kind"],
            "notification-center-item"
        );
    }

    #[test]
    fn ack_complete_and_delete_change_the_items() {
        let mut stores = Stores::default();
        stores.notification_center_loading();
        stores.notification_center_loaded(&[item("1", 7), item("2", 8)], false, None);
        assert_eq!(stores.notification_center_unacked(), 2);
        stores.dispatch("NOTIFICATION_CENTER_ITEMS_ACK", &json!({"id": "1"}));
        assert_eq!(stores.notification_center_unacked(), 1);
        stores.dispatch(
            "NOTIFICATION_CENTER_ITEM_COMPLETED",
            &json!({"item_enum": 8}),
        );
        assert_eq!(stores.notification_center_unacked(), 0);
        assert_eq!(stores.notification_center_items()[0]["completed"], true);
        stores.dispatch("NOTIFICATION_CENTER_ITEM_DELETE", &json!({"id": "2"}));
        assert_eq!(stores.notification_center_items().len(), 1);
        // A delete of an unknown id changes nothing.
        stores.dispatch("NOTIFICATION_CENTER_ITEM_DELETE", &json!({"id": "99"}));
        assert_eq!(stores.notification_center_items().len(), 1);
    }

    #[test]
    fn a_load_answer_without_a_load_under_way_is_dropped() {
        let mut stores = Stores::default();
        stores.notification_center_loaded(&[item("1", 1)], true, None);
        assert!(stores.notification_center_items().is_empty());
        stores.notification_center_loading();
        stores.notification_center_load_failed();
        assert!(stores.notification_center_initialized());
    }
}
