//! Backup & Restore: reads an Equicord or Vencord settings backup and our own export, writes our own export.
//!
//! An Equicord "Backup & Restore" file is `{ settings, quickCss, dataStore }` (Vencord's `exportSettings` in
//! `src/utils/settingsSync.ts`); a settings-only export is the `settings` object alone. Its `settings.plugins` maps a
//! plugin's name to `{ enabled, ...its settings }`. Only that part is read: Quick CSS and themes have no equivalent
//! here, and the data store holds other services' login tokens and logs, which are never copied.

use super::registry::def;
use super::state::{Entry, State};
use serde_json::{Map, Value, json};

/// The `format` field of our own export.
const FORMAT: &str = "gravecord-plugins";
const VERSION: u64 = 1;

/// The one data store key an import reads (VoiceStats' `storageKey`).
const VOICE_TOTALS_KEY: &str = "VoiceStats_totals";

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Report {
    /// Ported plugins the file set, and how many of them it turned on.
    pub ported: Vec<&'static str>,
    pub turned_on: usize,
    /// Plugins the file turns on that are not ported yet. Their settings are kept (as are those of the ones it has
    /// off, `kept_off` of them), so a later port starts from them.
    pub kept: Vec<String>,
    pub kept_off: usize,
    /// `Plugin.setting` values the file had that cannot be that setting's (left at what they were).
    pub invalid: Vec<String>,
    /// Parts of the file that were not read.
    pub skipped: Vec<&'static str>,
    /// VoiceStats' totals from the data store (`VoiceStats_totals`, user id to seconds), for the caller to keep.
    pub voice_totals: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImportError {
    /// Not a backup of any kind this reads.
    NotABackup,
    /// Our format, from a newer version of the app.
    NewerVersion(u64),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NotABackup => formatter.write_str("This file is not a plugin backup."),
            ImportError::NewerVersion(version) => write!(
                formatter,
                "This backup was made by a newer version (format {version})."
            ),
        }
    }
}

/// Our own export of the plugin state.
pub(crate) fn export(state: &State) -> Value {
    let mut value = state.to_value();
    if let Some(object) = value.as_object_mut() {
        object.insert("format".to_owned(), json!(FORMAT));
        object.insert("version".to_owned(), json!(VERSION));
    }
    value
}

/// Applies a backup to `state`. Plugins the file names replace their entries; plugins it does not name are left as
/// they are.
pub(crate) fn import(state: &mut State, file: &Value) -> Result<Report, ImportError> {
    if file.get("format").and_then(Value::as_str) == Some(FORMAT) {
        let version = file.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version > VERSION {
            return Err(ImportError::NewerVersion(version));
        }
        return Ok(import_ours(state, file));
    }
    // A whole backup carries `settings`; a settings-only export is that object itself.
    let settings = match file.get("settings") {
        Some(settings) if settings.get("plugins").is_some() => settings,
        _ => file,
    };
    let Some(plugins) = settings.get("plugins").and_then(Value::as_object) else {
        return Err(ImportError::NotABackup);
    };
    let mut report = Report::default();
    for (id, entry) in plugins {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let enabled = entry.get("enabled").and_then(Value::as_bool);
        let settings: Map<String, Value> = entry
            .iter()
            .filter(|(key, _)| key.as_str() != "enabled")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        apply(state, &mut report, id, enabled, settings);
    }
    if file
        .get("quickCss")
        .and_then(Value::as_str)
        .is_some_and(|css| !css.is_empty())
        || settings
            .get("enabledThemes")
            .and_then(Value::as_array)
            .is_some_and(|themes| !themes.is_empty())
        || settings
            .get("themeLinks")
            .and_then(Value::as_array)
            .is_some_and(|links| !links.is_empty())
    {
        report.skipped.push("Quick CSS and themes");
    }
    if let Some(store) = file.get("dataStore") {
        // The data store is a list of `[key, value]`. One key is read, by name: VoiceStats' totals (user ids and
        // seconds). Everything else there (logs, other services' logins) is never copied.
        report.voice_totals = store.as_array().and_then(|entries| {
            entries
                .iter()
                .find_map(|entry| match entry.as_array()?.as_slice() {
                    [key, value] if key.as_str() == Some(VOICE_TOTALS_KEY) && value.is_object() => {
                        Some(value.clone())
                    }
                    _ => None,
                })
        });
        if store.as_array().is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry[0].as_str() != Some(VOICE_TOTALS_KEY))
        }) {
            report
                .skipped
                .push("plugin data (logs and other services' logins)");
        }
    }
    report.ported.sort_unstable();
    report.kept.sort_unstable();
    report.invalid.sort_unstable();
    Ok(report)
}

fn import_ours(state: &mut State, file: &Value) -> Report {
    let theirs = State::from_value(file);
    let mut report = Report::default();
    state.master = theirs.master;
    for (id, entry) in theirs.plugins {
        apply(state, &mut report, &id, entry.enabled, entry.settings);
    }
    report.ported.sort_unstable();
    report.kept.sort_unstable();
    report.invalid.sort_unstable();
    report
}

/// A setting whose name says it holds a credential. `storage.json` never holds tokens, so such a setting is not read
/// from a backup, whatever plugin it belongs to.
fn credential_name(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "password", "secret", "apikey", "api_key", "cookie"]
        .iter()
        .any(|word| key.contains(word))
}

fn apply(
    state: &mut State,
    report: &mut Report,
    id: &str,
    enabled: Option<bool>,
    settings: Map<String, Value>,
) {
    let settings: Map<String, Value> = settings
        .into_iter()
        .filter(|(key, _)| !credential_name(key))
        .collect();
    let Some(def) = def(id) else {
        // Not ported: kept as the file has it.
        if enabled == Some(true) {
            report.kept.push(id.to_owned());
        } else {
            report.kept_off += 1;
        }
        state
            .plugins
            .insert(id.to_owned(), Entry { enabled, settings });
        return;
    };
    report.ported.push(def.id);
    if enabled == Some(true) {
        report.turned_on += 1;
    }
    // A ported plugin keeps only what its settings can hold; other keys (settings this port does not have) are
    // kept too, unread, so nothing of the backup is lost.
    let mut kept = Map::new();
    for (key, value) in settings {
        match def.setting(&key) {
            Some(setting) => match setting.normalize(&value) {
                Some(normal) => {
                    kept.insert(key, normal);
                }
                None => report.invalid.push(format!("{}.{key}", def.id)),
            },
            None => {
                kept.insert(key, value);
            }
        }
    }
    let entry = state.plugins.entry(def.id.to_owned()).or_default();
    if enabled.is_some() {
        entry.enabled = enabled;
    }
    entry.settings.extend(kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of an Equicord "Backup & Restore" file, with made-up values (a real backup holds account ids and
    /// other services' tokens).
    fn equicord_backup() -> Value {
        json!({
            "settings": {
                "autoUpdate": true,
                "enabledThemes": [],
                "themeLinks": [],
                "plugins": {
                    "ClearURLs": { "enabled": true },
                    "AlwaysTrust": { "enabled": false, "domain": true, "file": true },
                    "MessageClickActions": {
                        "enabled": true,
                        "doubleClickAction": "REPLY",
                        "doubleClickOthersAction": "SPIN",
                        "clickTimeout": 300,
                        "disableInDms": true
                    },
                    "BadgeAPI": { "enabled": true },
                    "SomethingNew": { "enabled": true, "level": 3, "authToken": "secret2", "apiKey": "secret3" }
                }
            },
            "quickCss": "",
            "dataStore": [["some-auth", { "token": "secret" }], ["VoiceStats_totals", { "7": 30 }]]
        })
    }

    #[test]
    fn an_equicord_backup_sets_the_ported_plugins_and_keeps_the_rest() {
        let mut state = State::default();
        let report = import(&mut state, &equicord_backup()).unwrap();
        assert_eq!(
            report.ported,
            vec!["AlwaysTrust", "ClearURLs", "MessageClickActions"]
        );
        assert_eq!(report.turned_on, 2);
        assert_eq!(report.kept, vec!["BadgeAPI", "SomethingNew"]);
        // SPIN is not an action of the original: the setting keeps its value and the import says so.
        assert_eq!(
            report.invalid,
            vec!["MessageClickActions.doubleClickOthersAction"]
        );
        assert_eq!(
            report.skipped,
            vec!["plugin data (logs and other services' logins)"]
        );
        // Only VoiceStats' totals come from the data store, and never the login next to them.
        assert_eq!(report.voice_totals, Some(json!({ "7": 30 })));
        assert!(state.is_on("ClearURLs"));
        assert!(!state.is_on("AlwaysTrust"));
        assert_eq!(
            state.value("MessageClickActions", "doubleClickAction"),
            json!("REPLY")
        );
        assert_eq!(
            state.value("MessageClickActions", "doubleClickOthersAction"),
            json!("REPLY")
        );
        assert_eq!(
            state.value("MessageClickActions", "disableInDms"),
            json!(true)
        );
        // Unported plugins and unknown keys stay as the file had them.
        assert_eq!(
            state.plugins["SomethingNew"].settings.get("level"),
            Some(&json!(3))
        );
        assert_eq!(
            state.plugins["MessageClickActions"]
                .settings
                .get("clickTimeout"),
            Some(&json!(300))
        );
        // Nothing of the data store is kept anywhere, nor a setting named like a credential.
        assert!(!state.to_value().to_string().contains("secret"));
        assert!(
            !state.plugins["SomethingNew"]
                .settings
                .contains_key("authToken")
        );
    }

    #[test]
    fn a_settings_only_export_is_read_too() {
        let backup = equicord_backup();
        let mut state = State::default();
        let report = import(&mut state, &backup["settings"]).unwrap();
        assert!(report.ported.contains(&"ClearURLs"));
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn our_export_imports_back_to_the_same_state() {
        let mut state = State::default();
        import(&mut state, &equicord_backup()).unwrap();
        state.master = false;
        let file = export(&state);
        assert_eq!(file["format"], json!(FORMAT));
        let mut fresh = State::default();
        import(&mut fresh, &file).unwrap();
        assert_eq!(fresh, state);
    }

    #[test]
    fn other_files_are_refused() {
        let mut state = State::default();
        assert_eq!(
            import(&mut state, &json!({ "hello": 1 })),
            Err(ImportError::NotABackup)
        );
        assert_eq!(
            import(&mut state, &json!({ "format": FORMAT, "version": 99 })),
            Err(ImportError::NewerVersion(99))
        );
        assert_eq!(state, State::default());
    }

    #[test]
    fn plugins_the_file_leaves_out_are_untouched() {
        let mut state = State::default();
        state.set_enabled("NoTypingAnimation", true);
        import(&mut state, &equicord_backup()).unwrap();
        assert!(state.is_on("NoTypingAnimation"));
    }
}
