//! EntitlementStore (module 469778): the account's entitlements by id, with the indexes by application, by SKU and by
//! subscription the getters read. `ENTITLEMENT_CREATE` and `ENTITLEMENT_UPDATE` (`y`: `O(e.entitlement)`) put one in,
//! `ENTITLEMENT_DELETE` takes it out of the map and of the three indexes, and a fetch for the user adds a list (`O`
//! for each), marking the store fetched. `LOGOUT` empties it.
use crate::{Stores, text};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entitlement {
    pub id: String,
    pub sku_id: String,
    pub application_id: String,
    pub subscription_id: Option<String>,
    /// The whole object as the server sent it (`createFromServer` keeps every field).
    pub raw: Value,
}

impl Entitlement {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: text(&value["id"])?,
            sku_id: text(&value["sku_id"]).unwrap_or_default(),
            application_id: text(&value["application_id"]).unwrap_or_default(),
            subscription_id: text(&value["subscription_id"]),
            raw: value.clone(),
        })
    }
}

#[derive(Default)]
pub struct Entitlements {
    by_id: HashMap<String, Entitlement>,
    by_application: HashMap<String, BTreeSet<String>>,
    by_sku: HashMap<String, BTreeSet<String>>,
    by_subscription: HashMap<String, BTreeSet<String>>,
    fetching: bool,
    fetched: bool,
}

impl Entitlements {
    /// `O` of 469778.
    fn put(&mut self, value: &Value) {
        let Some(entitlement) = Entitlement::from_value(value) else {
            return;
        };
        let id = entitlement.id.clone();
        self.by_sku
            .entry(entitlement.sku_id.clone())
            .or_default()
            .insert(id.clone());
        self.by_application
            .entry(entitlement.application_id.clone())
            .or_default()
            .insert(id.clone());
        if let Some(subscription) = &entitlement.subscription_id {
            self.by_subscription
                .entry(subscription.clone())
                .or_default()
                .insert(id.clone());
        }
        self.by_id.insert(id, entitlement);
    }

    /// The `ENTITLEMENT_DELETE` function of 469778.
    fn remove(&mut self, value: &Value) {
        let Some(id) = text(&value["id"]) else {
            return;
        };
        self.by_id.remove(&id);
        for (key, index) in [
            (text(&value["application_id"]), &mut self.by_application),
            (text(&value["sku_id"]), &mut self.by_sku),
            (text(&value["subscription_id"]), &mut self.by_subscription),
        ] {
            if let Some(set) = key.and_then(|key| index.get_mut(&key)) {
                set.remove(&id);
            }
        }
    }
}

impl Stores {
    pub(crate) fn entitlements_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.entitlements;
        match name {
            "ENTITLEMENT_CREATE" | "ENTITLEMENT_UPDATE" => store.put(&data["entitlement"]),
            "ENTITLEMENT_DELETE" => store.remove(&data["entitlement"]),
            "LOGOUT" => *store = Entitlements::default(),
            _ => {}
        }
    }

    /// `ENTITLEMENTS_FETCH_FOR_USER_START`.
    pub fn entitlements_fetching(&mut self) {
        self.entitlements.fetching = true;
    }

    /// `ENTITLEMENTS_FETCH_FOR_USER_SUCCESS`.
    pub fn entitlements_fetched(&mut self, list: &Value) {
        self.entitlements.fetched = true;
        self.entitlements.fetching = false;
        for value in list.as_array().into_iter().flatten() {
            self.entitlements.put(value);
        }
    }

    /// `ENTITLEMENTS_FETCH_FOR_USER_FAIL`.
    pub fn entitlements_fetch_failed(&mut self) {
        self.entitlements.fetched = false;
        self.entitlements.fetching = false;
    }

    /// `get(id)`.
    pub fn entitlement(&self, id: &str) -> Option<&Entitlement> {
        self.entitlements.by_id.get(id)
    }

    fn entitlements_of(&self, ids: Option<&BTreeSet<String>>) -> Option<Vec<&Entitlement>> {
        ids.map(|ids| {
            ids.iter()
                .filter_map(|id| self.entitlements.by_id.get(id))
                .collect()
        })
    }

    /// `getForSku(sku)`: `None` when the SKU was never seen.
    pub fn entitlements_for_sku(&self, sku: &str) -> Option<Vec<&Entitlement>> {
        self.entitlements_of(self.entitlements.by_sku.get(sku))
    }

    /// `getForApplication(application)`.
    pub fn entitlements_for_application(&self, application: &str) -> Option<Vec<&Entitlement>> {
        self.entitlements_of(self.entitlements.by_application.get(application))
    }

    pub fn entitlements_for_subscription(&self, subscription: &str) -> Option<Vec<&Entitlement>> {
        self.entitlements_of(self.entitlements.by_subscription.get(subscription))
    }

    pub fn entitlements_are_fetched(&self) -> bool {
        self.entitlements.fetched
    }

    pub fn entitlements_are_fetching(&self) -> bool {
        self.entitlements.fetching
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entitlement(id: &str, sku: &str, application: &str, subscription: Option<&str>) -> Value {
        json!({"id": id, "sku_id": sku, "application_id": application, "subscription_id": subscription, "type": 8})
    }

    #[test]
    fn create_update_and_delete_follow_the_official_indexes() {
        let mut stores = Stores::default();
        stores.dispatch(
            "ENTITLEMENT_CREATE",
            &json!({"entitlement": entitlement("1", "s1", "a1", Some("sub"))}),
        );
        stores.dispatch(
            "ENTITLEMENT_CREATE",
            &json!({"entitlement": entitlement("2", "s1", "a2", None)}),
        );
        assert_eq!(stores.entitlements_for_sku("s1").map(|l| l.len()), Some(2));
        assert_eq!(
            stores.entitlements_for_application("a1").map(|l| l.len()),
            Some(1)
        );
        assert_eq!(
            stores.entitlements_for_subscription("sub").map(|l| l.len()),
            Some(1)
        );
        assert!(stores.entitlements_for_sku("none").is_none());
        // An update replaces the stored object.
        let mut changed = entitlement("1", "s1", "a1", Some("sub"));
        changed["ends_at"] = json!("2027-01-01T00:00:00Z");
        stores.dispatch("ENTITLEMENT_UPDATE", &json!({"entitlement": changed}));
        assert_eq!(
            stores.entitlement("1").map(|e| e.raw["ends_at"].clone()),
            Some(json!("2027-01-01T00:00:00Z"))
        );
        stores.dispatch(
            "ENTITLEMENT_DELETE",
            &json!({"entitlement": entitlement("1", "s1", "a1", Some("sub"))}),
        );
        assert!(stores.entitlement("1").is_none());
        assert_eq!(stores.entitlements_for_sku("s1").map(|l| l.len()), Some(1));
        assert_eq!(
            stores.entitlements_for_subscription("sub").map(|l| l.len()),
            Some(0)
        );
        stores.dispatch("LOGOUT", &json!({}));
        assert!(stores.entitlement("2").is_none());
    }

    #[test]
    fn a_fetch_marks_the_store_fetched() {
        let mut stores = Stores::default();
        stores.entitlements_fetching();
        assert!(stores.entitlements_are_fetching() && !stores.entitlements_are_fetched());
        stores.entitlements_fetched(&json!([entitlement("3", "s", "a", None)]));
        assert!(stores.entitlements_are_fetched() && !stores.entitlements_are_fetching());
        assert!(stores.entitlement("3").is_some());
        stores.entitlements_fetch_failed();
        assert!(!stores.entitlements_are_fetched());
    }
}
