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
    ("display_name_styles", false),
    ("reduced_motion", false),
    ("enable_switch_icons", false),
    ("os_minimize_to_tray", false),
    ("os_start_minimized", false),
    // `enableHardwareAcceleration` of the desktop settings. The window always renders on the GPU here, so the switch
    // is kept and applies to nothing.
    ("hardware_acceleration", false),
    // `AccessibilityStore` (module 775602): contrast, saturation of custom colours, the system hooks, how role
    // colours and official messages show, the send button, and whether a profile's theme follows the user's.
    ("desaturate_custom_colors", false),
    ("high_contrast_mode", false),
    ("sync_forced_colors", false),
    ("sync_reduced_motion", false),
    ("role_style", true),
    ("official_message_style", true),
    ("enable_send_button_outside_experiment", false),
    ("sync_profile_themes", false),
    // `ThemeStore.syncedThemesEnabled` ("Same as Device Theme") and the theme sync switch of this device.
    ("appearance_same_as_device_theme", false),
    ("appearance_sync_theme", false),
    // `StreamerModeStore`.
    ("streamer_mode_hide_personal_information", false),
    ("streamer_mode_hide_invite_links", false),
    (
        "streamer_mode_hide_discord_window_from_screen_capture",
        false,
    ),
    ("chat_favorites_toggle", false),
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
    let mut choices = local_choices(&saved);
    let generic = storage::get(GENERIC_KEY).unwrap_or(Value::Null);
    choices.extend(generic_choices(&generic));
    choices
}

fn local_choices(saved: &Value) -> HashMap<&'static str, Choice> {
    let switches = LOCAL.iter().filter_map(|(key, radio)| {
        let value = saved.get(*key)?;
        let choice = if *radio {
            Choice::Index(usize::try_from(value.as_u64()?).ok()?)
        } else {
            Choice::On(value.as_bool()?)
        };
        Some((*key, choice))
    });
    let numbers = super::controls::SLIDERS.iter().filter_map(|spec| {
        let value = saved.get(spec.key)?.as_f64()? as f32;
        Some((spec.key, Choice::Number(value)))
    });
    switches.chain(numbers).collect()
}

/// Every row key of every generated page, to give a saved key its `'static` name.
fn page_keys() -> &'static HashMap<&'static str, ()> {
    static KEYS: std::sync::OnceLock<HashMap<&'static str, ()>> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        fn walk(nodes: &'static [super::model::Node], keys: &mut HashMap<&'static str, ()>) {
            use super::model::Node;
            for node in nodes {
                if !node.key().is_empty() {
                    keys.insert(node.key(), ());
                }
                if let Node::Category { children, .. } | Node::Accordion { children, .. } = node {
                    walk(children, keys);
                }
            }
        }
        let mut keys = HashMap::new();
        for page in super::ALL_PAGES {
            walk(super::pages::nodes(*page), &mut keys);
        }
        keys
    })
}

/// Choices of rows that no other module handles (`GENERIC_KEY`): a switch is a boolean, a radio or select an
/// integer, a slider a float.
fn generic_choices(saved: &Value) -> HashMap<&'static str, Choice> {
    let Some(map) = saved.as_object() else {
        return HashMap::new();
    };
    page_keys()
        .keys()
        .filter_map(|key| {
            let value = map.get(*key)?;
            let choice = if let Some(on) = value.as_bool() {
                Choice::On(on)
            } else if let Some(index) = value.as_u64() {
                Choice::Index(usize::try_from(index).ok()?)
            } else {
                Choice::Number(value.as_f64()? as f32)
            };
            Some((*key, choice))
        })
        .collect()
}

const GENERIC_KEY: &str = "settings.generic";

/// A harness run that must not write settings keeps its choices in memory only.
fn writes_blocked() -> bool {
    storage::is_capture_run() && std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() == Ok("1")
}

fn save_local(key: &str, value: Value) {
    if writes_blocked() {
        return;
    }
    let mut saved = storage::get(STORAGE_KEY)
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    saved[key] = value;
    storage::set(STORAGE_KEY, saved);
}

fn save_generic(key: &str, value: Value) {
    if writes_blocked() {
        return;
    }
    let mut saved = storage::get(GENERIC_KEY)
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    saved[key] = value;
    storage::set(GENERIC_KEY, saved);
}

/// The raw `ui_density` values the Spacing radio holds, in the page's order: Compact, Default, Spacious.
/// Radios whose options are not the enum's own numbering: the enum value of each option, in the page's order.
/// `ui_density` lists Compact, Default, Spacious (`eJ.NS` 1, 4, 2); Profile Privacy lists friends and all servers,
/// friends and small servers, friends only (`profile_visibility` 3, 2, 1); the spam filter lists Filter all, Filter
/// non-friends, Don't filter (`dm_spam_filter_v2` 3, 2, 1).
const RAW: &[(&str, &[u64])] = &[
    (UI_DENSITY, &[1, 4, 2]),
    // Push notification inactive timeout: one to eleven minutes, in seconds (`afk_timeout`).
    (
        super::notifications_page::MOBILE_DELAY,
        &super::notifications_page::DELAYS,
    ),
    // Activity Privacy's default for new servers: Don't share, Share in large servers, Share (`eJ.Qd` 1, 2, 3; 0 is
    // unset).
    ("activity_sharing_per_guild_default_setting", &[1, 2, 3]),
    ("profile_privacy_setting", &[3, 2, 1]),
    ("dm_spam_setting", &[3, 2, 1]),
];

fn raw_of(key: &str) -> Option<&'static [u64]> {
    RAW.iter()
        .find(|(known, _)| *known == key)
        .map(|(_, raw)| *raw)
}
const UI_DENSITY: &str = "appearance_ui_density";
/// The Language select (`localization.locale`).
pub(in crate::ui) const LANGUAGE: &str = "language_select_setting";

/// Whether Time format is 24-hour, for the formatting functions that have no `Root` at hand. Set each frame.
static CLOCK_24: AtomicBool = AtomicBool::new(false);

pub(in crate::ui) fn clock_24() -> bool {
    CLOCK_24.load(Ordering::Relaxed)
}

pub(in crate::ui) fn set_clock_24(on: bool) {
    CLOCK_24.store(on, Ordering::Relaxed);
}

/// Time format's option (Auto 0, 12-hour 1, 24-hour 2), set each frame with [`set_clock_24`].
static TIME_FORMAT_INDEX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(in crate::ui) fn set_time_format(index: usize) {
    TIME_FORMAT_INDEX.store(index, Ordering::Relaxed);
    set_clock_24(index == HOUR_24);
}

/// The `hourCycle` the date formatters take (module 5377 `I`): none for Auto, `h12` (false) or `h23` (true).
pub(in crate::ui) fn hour_cycle() -> Option<bool> {
    match TIME_FORMAT_INDEX.load(Ordering::Relaxed) {
        1 => Some(false),
        HOUR_24 => Some(true),
        _ => None,
    }
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
        if key == "chat_favorites_toggle" {
            return self.favorites_guild_enabled();
        }
        match self.setting_choices.get(key) {
            Some(Choice::On(on)) => *on,
            _ => self.stores.prefs.get(key).map_or(default, |value| {
                // `drops_opted_out` and friends: the switch is the opposite of what the account stores.
                (*value != 0) != proto_spec(key).is_some_and(|pref| pref.invert)
            }),
        }
    }

    pub(in crate::ui) fn pref_index(&self, key: &'static str, default: usize) -> usize {
        match self.setting_choices.get(key) {
            Some(Choice::Index(index)) => *index,
            _ => self.stores.prefs.get(key).map_or(default, |value| {
                match raw_of(key) {
                    // The account holds the enum's value; the page lists its options in its own order.
                    Some(raw) => raw.iter().position(|raw| raw == value).unwrap_or(default),
                    None => *value as usize,
                }
            }),
        }
    }

    /// A generated row of one of these preferences changed (`setting_choices` already holds the new value). True when
    /// the key is one of them.
    pub(super) fn pref_changed(&mut self, key: &'static str, on: bool, index: usize) -> bool {
        if key == "chat_favorites_toggle" {
            // Show Favorites server: `favorites.guild_visible` of the settings proto (`favoriteGuildEnabled`).
            self.setting_choices.remove(key);
            self.favorites_toggled(on);
            return true;
        }
        if let Some(pref) = proto_spec(key) {
            let radio = matches!(self.setting_choices.get(key), Some(Choice::Index(_)));
            let value = if let Some(raw) = raw_of(key) {
                raw[index.min(raw.len() - 1)]
            } else if radio {
                index as u64
            } else {
                u64::from(on != pref.invert)
            };
            // The account's value shows from now on; the page reads it from `Stores::prefs`.
            self.setting_choices.remove(key);
            let patch = self.stores.set_pref(pref, value);
            if key == "activity_privacy_setting" {
                // Share my detected activities: what the presence carries changes at once.
                self.refresh_own_presence();
            }
            if !patch.is_empty() {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: "/users/@me/settings-proto/1".to_owned(),
                    body: Some(json!({"settings": patch})),
                });
            }
            return true;
        }
        if let Some((key, radio)) = local_spec(key) {
            save_local(key, if radio { json!(index) } else { json!(on) });
            return true;
        }
        // Rows no other module handles keep their value on this machine, so they come back after a restart.
        match self.setting_choices.get(key) {
            Some(Choice::On(on)) => save_generic(key, json!(on)),
            Some(Choice::Index(index)) => save_generic(key, json!(index)),
            Some(Choice::Number(value)) => save_generic(key, json!(value)),
            None => return false,
        }
        true
    }

    /// A slider was let go (or moved by a key): its value is a device setting.
    pub(super) fn slider_committed(&mut self, key: &'static str, value: f32) {
        if self.overlay_slider_committed(key, value) {
            return;
        }
        self.setting_choices.insert(key, Choice::Number(value));
        save_local(key, json!(value));
    }

    /// The harness's `pref get <key>` and `pref set <key> on|off|<index>`: what the account holds for a proto
    /// preference, and the same path a click on its row takes. `local` only overrides this capture.
    #[cfg(feature = "capture")]
    pub(crate) fn script_pref(&mut self, words: &[&str]) -> String {
        let [verb, key, rest @ ..] = words else {
            return "usage: pref get <key> | pref set <key> on|off|<index>".to_owned();
        };
        if *verb == "local" {
            let key_name = proto_spec(key)
                .map(|pref| pref.key)
                .or_else(|| local_spec(key).map(|(known, _)| known))
                .or_else(|| page_keys().get_key_value(*key).map(|(known, _)| *known));
            let Some(key_name) = key_name else {
                return format!("{key} is not a preference");
            };
            let choice = match rest.first().copied() {
                Some("on") => Choice::On(true),
                Some("off") => Choice::On(false),
                Some(number) if number.contains('.') => match number.parse::<f32>() {
                    Ok(value) => Choice::Number(value),
                    Err(_) => return format!("cannot read {number}"),
                },
                Some(index) => match index.parse::<usize>() {
                    Ok(index) => Choice::Index(index),
                    Err(_) => return format!("cannot read {index}"),
                },
                None => return "usage: pref local <key> on|off|<index>|<number.0>".to_owned(),
            };
            // A numeric slider given as a whole number ("16") is still a number.
            let choice = match (choice, super::controls::spec(key_name)) {
                (Choice::Index(index), Some(_)) => Choice::Number(index as f32),
                (other, _) => other,
            };
            self.setting_choices.insert(key_name, choice);
            return format!("{key} local = {choice:?}");
        }
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
        // The Messaging Permissions switches read the proto's lists and flags through `privacy.rs`: what the switch
        // shows, and the stored value behind it.
        if *verb == "get"
            && let Some(view) = self.messaging_view(key)
        {
            return format!(
                "{key} = {} (disabled {}) friend_source_flags {} dm_guild {}",
                if view.on { "on" } else { "off" },
                view.disabled,
                self.friend_flags_for_script(),
                self.settings_extra.dm_guild
            );
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

    /// The interface language (`LocaleStore.locale`, module 773669): the Language select's choice, i.e. the account's
    /// `localization.locale`, else the locale shown before the account's arrived.
    pub(in crate::ui) fn interface_locale(&self) -> &'static str {
        let index = self.pref_index(LANGUAGE, usize::MAX);
        match dc_proto::settings::LOCALES.get(index) {
            Some(code) => {
                if self.stores.prefs.contains_key(LANGUAGE) {
                    crate::intl::remember(code);
                }
                code
            }
            None => crate::intl::startup_locale(),
        }
    }

    /// The harness's `locale <code>`: shows the interface in a locale as a choice in the Language select would, on
    /// this capture only (nothing is sent).
    #[cfg(feature = "capture")]
    pub(crate) fn script_locale(&mut self, code: &str) -> String {
        let Some(index) = dc_proto::settings::LOCALES
            .iter()
            .position(|known| *known == code)
        else {
            return format!("{code} is not a locale of the Language select");
        };
        self.setting_choices.insert(LANGUAGE, Choice::Index(index));
        format!("locale {code} (loaded {})", dc_intl::is_loaded(code))
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
        let saved = json!({"underline_links": true, "reduced_motion": false,
            "os_minimize_to_tray": false, "stranger": true});
        let choices = local_choices(&saved);
        assert_eq!(choices.get("underline_links"), Some(&Choice::On(true)));
        assert_eq!(choices.get("reduced_motion"), Some(&Choice::On(false)));
        assert_eq!(choices.get("os_minimize_to_tray"), Some(&Choice::On(false)));
        assert_eq!(choices.len(), 3);
        assert!(local_choices(&Value::Null).is_empty());
    }

    #[test]
    fn every_pref_names_a_row_of_a_generated_page() {
        let pages = include_str!("pages.rs");
        // The colour theme is set by the Appearance page's colour tiles (`client_theme.rs`), which have no row key.
        // "Enable Family Center tab" is drawn by the Family Center panel's Settings tab (`account_nested.rs`).
        const TILES: &[&str] = &["appearance_client_theme_preset", "family_center_enabled"];
        for pref in PROTO_PREFS.iter().filter(|pref| !TILES.contains(&pref.key)) {
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

    #[test]
    fn radios_with_their_own_order_map_to_the_enum_values() {
        // Profile Privacy lists friends and all servers first, which is `profile_visibility` 3.
        let raw = raw_of("profile_privacy_setting").unwrap();
        assert_eq!(raw, &[3, 2, 1]);
        assert_eq!(raw.iter().position(|value| *value == 1), Some(2));
        assert_eq!(raw_of("appearance_ui_density"), Some(&[1, 4, 2][..]));
        assert!(raw_of("reaction_notifications").is_none());
        // Every option of such a radio has an enum value.
        let pages = include_str!("pages.rs");
        assert!(pages.contains("key: \"dm_spam_setting\""));
    }
}
