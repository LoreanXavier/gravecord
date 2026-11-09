//! The generated pages' client preferences that do something. A switch or radio of a generated page only writes its
//! `setting_choices` entry; this module is where the ones outside Voice & Video and the sound lists go on from there.
//!
//! Two kinds. Proto preferences (`dc_proto::settings::PROTO_PREFS`) live in the account: a change is sent as the
//! `PATCH /users/@me/settings-proto/1` the official client sends, kept in `Stores::prefs` at once, and read back from
//! READY and `USER_SETTINGS_PROTO_UPDATE`, so another session's change shows here. Local preferences (`LOCAL`) are
//! the official `AccessibilityStore` and friends: they stay on this machine, in `storage.json`.

use super::model::Choice;
use crate::net::Request;
use crate::storage;
use crate::ui::Root;
use dc_proto::settings::{PROTO_PREFS, ProtoPref};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// Preferences kept on this machine: the row key and whether it is a radio (an index) rather than a switch.
pub(super) const LOCAL: &[(&str, bool)] = &[
    ("underline_links", false),
    ("reduced_motion", false),
    ("enable_switch_icons", false),
];

const STORAGE_KEY: &str = "settings.prefs";

/// `timestampHourCycle` as the Time format radio lists it: Auto, 12-hour, 24-hour.
pub(in crate::ui) const HOUR_24: usize = 2;
pub(in crate::ui) const TIME_FORMAT: &str = "time_format_setting";

fn local_spec(key: &str) -> Option<(&'static str, bool)> {
    LOCAL.iter().find(|(known, _)| *known == key).copied()
}

fn proto_spec(key: &str) -> Option<&'static ProtoPref> {
    PROTO_PREFS.iter().find(|pref| pref.key == key)
}

/// The local preferences saved by an earlier run, as the pages show them.
pub(in crate::ui) fn restore() -> HashMap<&'static str, Choice> {
    let saved = storage::get(STORAGE_KEY).unwrap_or(Value::Null);
    local_choices(&saved)
}

fn local_choices(saved: &Value) -> HashMap<&'static str, Choice> {
    LOCAL
        .iter()
        .filter_map(|(key, radio)| {
            let value = saved.get(*key)?;
            let choice = if *radio {
                Choice::Index(usize::try_from(value.as_u64()?).ok()?)
            } else {
                Choice::On(value.as_bool()?)
            };
            Some((*key, choice))
        })
        .collect()
}

/// Whether Time format is 24-hour, for the formatting functions that have no `Root` at hand. Set each frame.
static CLOCK_24: AtomicBool = AtomicBool::new(false);

pub(in crate::ui) fn set_clock_24(on: bool) {
    CLOCK_24.store(on, Ordering::Relaxed);
}

/// A time of day as the chosen Time format writes it: `1:05 PM`, or `13:05` in 24-hour.
pub(in crate::ui) fn clock<Tz: chrono::TimeZone>(time: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    clock_in(time, CLOCK_24.load(Ordering::Relaxed))
}

/// The same with seconds, for the `T` timestamp style: `1:05:09 PM`, or `13:05:09`.
pub(in crate::ui) fn clock_seconds<Tz: chrono::TimeZone>(time: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    time.format(if CLOCK_24.load(Ordering::Relaxed) {
        "%H:%M:%S"
    } else {
        "%-I:%M:%S %p"
    })
    .to_string()
}

fn clock_in<Tz: chrono::TimeZone>(time: &chrono::DateTime<Tz>, hour24: bool) -> String
where
    Tz::Offset: std::fmt::Display,
{
    time.format(if hour24 { "%H:%M" } else { "%-I:%M %p" })
        .to_string()
}

impl Root {
    /// A preference's switch: the user's choice here, else the account's, else the default.
    pub(in crate::ui) fn pref_on(&self, key: &'static str, default: bool) -> bool {
        match self.setting_choices.get(key) {
            Some(Choice::On(on)) => *on,
            _ => self
                .stores
                .prefs
                .get(key)
                .map_or(default, |value| *value != 0),
        }
    }

    pub(in crate::ui) fn pref_index(&self, key: &'static str, default: usize) -> usize {
        match self.setting_choices.get(key) {
            Some(Choice::Index(index)) => *index,
            _ => self
                .stores
                .prefs
                .get(key)
                .map_or(default, |value| *value as usize),
        }
    }

    /// A generated row of one of these preferences changed (`setting_choices` already holds the new value). True when
    /// the key is one of them.
    pub(super) fn pref_changed(&mut self, key: &'static str, on: bool, index: usize) -> bool {
        if let Some(pref) = proto_spec(key) {
            let radio = matches!(self.setting_choices.get(key), Some(Choice::Index(_)));
            let value = if radio { index as u64 } else { u64::from(on) };
            // The account's value shows from now on; the page reads it from `Stores::prefs`.
            self.setting_choices.remove(key);
            let patch = self.stores.set_pref(pref, value);
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(json!({"settings": patch})),
            });
            return true;
        }
        if let Some((key, radio)) = local_spec(key) {
            let mut saved = storage::get(STORAGE_KEY)
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            saved[key] = if radio { json!(index) } else { json!(on) };
            storage::set(STORAGE_KEY, saved);
            return true;
        }
        false
    }

    /// The harness's `pref get <key>` and `pref set <key> on|off|<index>`: what the account holds for a proto
    /// preference, and the same path a click on its row takes. Returns the line for the log.
    #[cfg(feature = "capture")]
    pub(crate) fn script_pref(&mut self, words: &[&str]) -> String {
        let [verb, key, rest @ ..] = words else {
            return "usage: pref get <key> | pref set <key> on|off|<index>".to_owned();
        };
        if *key == "theme" {
            if let ("set", Some(Ok(value))) = (*verb, rest.first().map(|text| text.parse::<u64>()))
            {
                let patch = self.stores.set_pref(&dc_proto::settings::THEME, value);
                self.stores.theme = value;
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: "/users/@me/settings-proto/1".to_owned(),
                    body: Some(json!({"settings": patch})),
                });
            }
            return format!("theme = {}", self.stores.theme);
        }
        let Some(pref) = proto_spec(key) else {
            return format!("{key} is not a proto preference");
        };
        if *verb == "set" {
            let value = rest.first().copied().unwrap_or("");
            let choice = match value {
                "on" => Choice::On(true),
                "off" => Choice::On(false),
                other => match other.parse::<usize>() {
                    Ok(index) => Choice::Index(index),
                    Err(_) => return format!("cannot read {other}"),
                },
            };
            self.setting_choices.insert(pref.key, choice);
            self.voice_setting_changed(pref.key);
        }
        match self.stores.prefs.get(pref.key) {
            Some(value) => format!("{key} = {value}"),
            None => format!("{key} unset"),
        }
    }

    /// The links of messages are underlined at rest (`underline_links`, the accessibility setting).
    pub(in crate::ui) fn underline_links(&self) -> bool {
        self.pref_on("underline_links", false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn the_clock_follows_the_time_format() {
        let time = chrono::Utc.with_ymd_and_hms(2026, 11, 9, 13, 5, 0).unwrap();
        assert_eq!(clock_in(&time, false), "1:05 PM");
        assert_eq!(clock_in(&time, true), "13:05");
        let morning = chrono::Utc.with_ymd_and_hms(2026, 11, 9, 0, 7, 0).unwrap();
        assert_eq!(clock_in(&morning, false), "12:07 AM");
        assert_eq!(clock_in(&morning, true), "00:07");
    }

    #[test]
    fn saved_local_preferences_come_back_as_choices() {
        let saved = json!({"underline_links": true, "reduced_motion": false, "stranger": true});
        let choices = local_choices(&saved);
        assert_eq!(choices.get("underline_links"), Some(&Choice::On(true)));
        assert_eq!(choices.get("reduced_motion"), Some(&Choice::On(false)));
        assert_eq!(choices.len(), 2);
        assert!(local_choices(&Value::Null).is_empty());
    }

    #[test]
    fn every_pref_names_a_row_of_a_generated_page() {
        let pages = include_str!("pages.rs");
        for pref in PROTO_PREFS {
            assert!(
                pages.contains(&format!("key: \"{}\"", pref.key)),
                "{} has no row",
                pref.key
            );
        }
        for (key, _) in LOCAL {
            assert!(
                pages.contains(&format!("key: \"{key}\"")),
                "{key} has no row"
            );
        }
    }
}
