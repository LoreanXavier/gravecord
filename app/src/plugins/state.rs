//! Which plugins are on and what their settings hold.

use super::registry::{DEFS, def};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    /// The master switch: off, every plugin is off whatever its own switch says.
    pub master: bool,
    /// By plugin id. Plugins that are not ported yet keep what an import gave them, so a later port starts from
    /// the imported settings.
    pub plugins: BTreeMap<String, Entry>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Entry {
    /// `None` until the switch is used or imported: the definition's default applies.
    pub enabled: Option<bool>,
    /// As stored, valid or not; [`State::value`] checks a value against its setting when read.
    pub settings: Map<String, Value>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            // On, so the plugins that default to on (MessageClickActions) work out of the box; every other plugin
            // is off until its own switch is used.
            master: true,
            plugins: BTreeMap::new(),
        }
    }
}

impl State {
    /// The plugin's own switch, ignoring the master switch.
    pub(crate) fn enabled(&self, id: &str) -> bool {
        let Some(def) = def(id) else {
            return false;
        };
        self.plugins
            .get(id)
            .and_then(|entry| entry.enabled)
            .unwrap_or(def.default_on)
    }

    /// The plugin may act now: it is turned on and so is the master switch. Every yes is counted as an effect
    /// ([`super::note`]), so a run with the plugins off can prove that none of them acted.
    pub(crate) fn is_on(&self, id: &str) -> bool {
        let on = self.master && self.enabled(id);
        if on {
            super::note(id);
        }
        on
    }

    pub(crate) fn set_enabled(&mut self, id: &str, on: bool) {
        self.plugins.entry(id.to_owned()).or_default().enabled = Some(on);
    }

    /// The setting's stored value if it is valid for the setting, else its default; `Null` for an unknown setting.
    pub(crate) fn value(&self, id: &str, key: &str) -> Value {
        let Some(setting) = def(id).and_then(|def| def.setting(key)) else {
            return Value::Null;
        };
        self.plugins
            .get(id)
            .and_then(|entry| entry.settings.get(key))
            .and_then(|stored| setting.normalize(stored))
            .unwrap_or_else(|| setting.default_value())
    }

    /// Stores a setting, normalized. Returns false and changes nothing when the value cannot be the setting's.
    pub(crate) fn set_value(&mut self, id: &str, key: &str, value: &Value) -> bool {
        let Some(normal) = def(id)
            .and_then(|def| def.setting(key))
            .and_then(|setting| setting.normalize(value))
        else {
            return false;
        };
        self.plugins
            .entry(id.to_owned())
            .or_default()
            .settings
            .insert(key.to_owned(), normal);
        true
    }

    /// Ported plugins that are on (own switch), counting the default ones.
    pub(crate) fn enabled_count(&self) -> usize {
        DEFS.iter().filter(|def| self.enabled(def.id)).count()
    }

    pub(crate) fn from_value(value: &Value) -> Self {
        let master = value.get("master").and_then(Value::as_bool).unwrap_or(true);
        let plugins = value
            .get("plugins")
            .and_then(Value::as_object)
            .map(|plugins| {
                plugins
                    .iter()
                    .map(|(id, entry)| {
                        let entry = Entry {
                            enabled: entry.get("enabled").and_then(Value::as_bool),
                            settings: entry
                                .get("settings")
                                .and_then(Value::as_object)
                                .cloned()
                                .unwrap_or_default(),
                        };
                        (id.clone(), entry)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { master, plugins }
    }

    pub(crate) fn to_value(&self) -> Value {
        let plugins: Map<String, Value> = self
            .plugins
            .iter()
            .map(|(id, entry)| {
                let mut object = Map::new();
                if let Some(enabled) = entry.enabled {
                    object.insert("enabled".to_owned(), Value::Bool(enabled));
                }
                if !entry.settings.is_empty() {
                    object.insert("settings".to_owned(), Value::Object(entry.settings.clone()));
                }
                (id.clone(), Value::Object(object))
            })
            .collect();
        json!({ "master": self.master, "plugins": plugins })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_are_off_until_turned_on_except_their_defaults() {
        let mut state = State::default();
        assert!(!state.is_on("ClearURLs"));
        assert!(state.is_on("MessageClickActions"));
        state.set_enabled("ClearURLs", true);
        assert!(state.is_on("ClearURLs"));
        state.master = false;
        assert!(!state.is_on("ClearURLs"));
        assert!(!state.is_on("MessageClickActions"));
        assert!(
            state.enabled("ClearURLs"),
            "the plugin's own switch is kept"
        );
        assert!(!state.is_on("NoSuchPlugin"));
    }

    #[test]
    fn values_fall_back_to_the_default_when_missing_or_invalid() {
        let mut state = State::default();
        let id = "MessageClickActions";
        assert_eq!(state.value(id, "doubleClickAction"), json!("EDIT"));
        assert!(!state.set_value(id, "doubleClickAction", &json!("SPIN")));
        assert!(state.set_value(id, "doubleClickAction", &json!("REPLY")));
        assert_eq!(state.value(id, "doubleClickAction"), json!("REPLY"));
        state
            .plugins
            .get_mut(id)
            .unwrap()
            .settings
            .insert("disableInDms".to_owned(), json!("yes"));
        assert_eq!(state.value(id, "disableInDms"), json!(false));
        assert_eq!(state.value(id, "noSuchSetting"), Value::Null);
    }

    #[test]
    fn the_state_survives_a_round_trip_through_json() {
        let mut state = State {
            master: false,
            ..State::default()
        };
        state.set_enabled("ClearURLs", true);
        state.set_value("MessageClickActions", "disableInDms", &json!(true));
        state.plugins.insert(
            "NotPortedYet".to_owned(),
            Entry {
                enabled: Some(true),
                settings: json!({ "anything": [1, 2] }).as_object().unwrap().clone(),
            },
        );
        let back = State::from_value(&state.to_value());
        assert_eq!(back, state);
        assert_eq!(State::from_value(&Value::Null), State::default());
    }
}
