//! The overlay's persisted settings, each under the official store's key and in its shape.
//!
//! - `OverlayStoreV2` (module 373954, a PersistedStore): `{[userId]: settings}`, the defaults `v`.
//! - `OverlayStore6` (class `x`, module 211753): `{legacyEnabled, oopEnabled}` in local storage, both on by default in
//!   the desktop client (`O`, module 206885).
//! - `OverlaySettingsStore` (module 592598): `{gameSettings, notificationSettings}`, the second being the notification
//!   settings turned off (module 93465's names).
//!
//! Unknown fields of a stored object are kept as they were.

use serde_json::{Map, Value, json};

pub const OVERLAY_STORE: &str = "OverlayStoreV2";
pub const ENABLED_STORE: &str = "OverlayStore6";
pub const SETTINGS_STORE: &str = "OverlaySettingsStore";

/// `DisplayUserMode` (`f5z`, module 652215).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayUserMode {
    Always,
    OnlyWhileSpeaking,
}

/// `DisplayNameMode` (`pwA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayNameMode {
    Always,
    OnlyWhileSpeaking,
    Never,
}

/// `AvatarSize` (`OSZ`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarSize {
    Large,
    Small,
}

impl DisplayUserMode {
    /// The options as the Display Users select lists them (Always, Only While Speaking).
    pub const OPTIONS: [Self; 2] = [Self::Always, Self::OnlyWhileSpeaking];
    pub fn wire(self) -> &'static str {
        match self {
            Self::Always => "ALWAYS",
            Self::OnlyWhileSpeaking => "ONLY_WHILE_SPEAKING",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Self::OPTIONS.into_iter().find(|mode| mode.wire() == value)
    }
}

impl DisplayNameMode {
    /// Always, Only While Speaking, Never.
    pub const OPTIONS: [Self; 3] = [Self::Always, Self::OnlyWhileSpeaking, Self::Never];
    pub fn wire(self) -> &'static str {
        match self {
            Self::Always => "ALWAYS",
            Self::OnlyWhileSpeaking => "ONLY_WHILE_SPEAKING",
            Self::Never => "NEVER",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Self::OPTIONS.into_iter().find(|mode| mode.wire() == value)
    }
}

impl AvatarSize {
    /// Large, Small.
    pub const OPTIONS: [Self; 2] = [Self::Large, Self::Small];
    pub fn wire(self) -> &'static str {
        match self {
            Self::Large => "large",
            Self::Small => "small",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Self::OPTIONS.into_iter().find(|mode| mode.wire() == value)
    }
    /// `ep` (module 502812): `SIZE_32` for large, `SIZE_24` for small.
    pub fn pixels(self) -> f32 {
        match self {
            Self::Large => 32.,
            Self::Small => 24.,
        }
    }
}

/// One user's `OverlayStoreV2` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlaySettings {
    pub display_user_mode: DisplayUserMode,
    pub display_name_mode: DisplayNameMode,
    pub avatar_size_mode: AvatarSize,
    pub disable_clickable_regions: bool,
    /// Every other field (`notificationPositionMode`, `textWidgetOpacity`, ...), as stored.
    rest: Map<String, Value>,
}

impl Default for OverlaySettings {
    /// `v` (module 373954).
    fn default() -> Self {
        let mut rest = Map::new();
        rest.insert("selectedGuildId".into(), Value::Null);
        rest.insert("selectedChannelId".into(), Value::Null);
        rest.insert("notificationPositionMode".into(), json!("topLeft"));
        rest.insert("textChatNotifications".into(), json!("ENABLED"));
        rest.insert("disableExternalLinkAlert".into(), json!(false));
        rest.insert("disablePinTutorial".into(), json!(false));
        // `Li.LOWER` (module 672396).
        rest.insert("textWidgetOpacity".into(), json!(0.25));
        rest.insert("showGameInviteNotification".into(), json!(true));
        Self {
            display_user_mode: DisplayUserMode::Always,
            display_name_mode: DisplayNameMode::Always,
            avatar_size_mode: AvatarSize::Large,
            disable_clickable_regions: false,
            rest,
        }
    }
}

impl OverlaySettings {
    fn from_json(value: &Value) -> Self {
        let mut settings = Self::default();
        let Some(object) = value.as_object() else {
            return settings;
        };
        for (key, field) in object {
            match key.as_str() {
                "displayUserMode" => {
                    if let Some(mode) = field.as_str().and_then(DisplayUserMode::parse) {
                        settings.display_user_mode = mode;
                    }
                }
                "displayNameMode" => {
                    if let Some(mode) = field.as_str().and_then(DisplayNameMode::parse) {
                        settings.display_name_mode = mode;
                    }
                }
                "avatarSizeMode" => {
                    if let Some(mode) = field.as_str().and_then(AvatarSize::parse) {
                        settings.avatar_size_mode = mode;
                    }
                }
                "disableClickableRegions" => {
                    if let Some(on) = field.as_bool() {
                        settings.disable_clickable_regions = on;
                    }
                }
                _ => {
                    settings.rest.insert(key.clone(), field.clone());
                }
            }
        }
        settings
    }

    fn to_json(&self) -> Value {
        let mut map = self.rest.clone();
        map.insert(
            "displayUserMode".into(),
            json!(self.display_user_mode.wire()),
        );
        map.insert(
            "displayNameMode".into(),
            json!(self.display_name_mode.wire()),
        );
        map.insert("avatarSizeMode".into(), json!(self.avatar_size_mode.wire()));
        map.insert(
            "disableClickableRegions".into(),
            json!(self.disable_clickable_regions),
        );
        Value::Object(map)
    }
}

/// The user's entry of the stored `OverlayStoreV2` object (`V(userId)`: the defaults when there is none).
pub fn settings_for(stored: &Value, user: &str) -> OverlaySettings {
    stored
        .get(user)
        .map(OverlaySettings::from_json)
        .unwrap_or_default()
}

/// The stored object with the user's entry replaced; the other users' entries stay.
pub fn with_settings(stored: &Value, user: &str, settings: &OverlaySettings) -> Value {
    let mut map = stored.as_object().cloned().unwrap_or_default();
    map.insert(user.to_owned(), settings.to_json());
    Value::Object(map)
}

/// `OverlayStore6`: `{legacyEnabled, oopEnabled}`, both on when missing.
pub fn enabled(stored: &Value) -> crate::decision::Global {
    crate::decision::Global {
        oop: stored["oopEnabled"].as_bool().unwrap_or(true),
        legacy: stored["legacyEnabled"].as_bool().unwrap_or(true),
    }
}

pub fn enabled_json(global: crate::decision::Global) -> Value {
    json!({"legacyEnabled": global.legacy, "oopEnabled": global.oop})
}

/// Module 93465: the notification settings that can be turned off.
pub const TEXT_CHAT: &str = "TEXT_CHAT";
pub const WELCOME_GENERAL: &str = "WELCOME_GENERAL";
pub const GO_LIVE_NUDGE: &str = "GO_LIVE_NUDGE";
pub const GAME_ACTIVITY: &str = "GAME_ACTIVITY";
pub const NOW_PLAYING: &str = "NOW_PLAYING";

/// `isNotificationDisabledBySetting` of `OverlaySettingsStore`.
pub fn notification_disabled(stored: &Value, setting: &str) -> bool {
    stored["notificationSettings"]
        .as_array()
        .is_some_and(|list| list.iter().any(|entry| entry == setting))
}

/// `OVERLAY_SET_NOTIFICATION_DISABLED_SETTING`: adds or removes the setting; `gameSettings` and the rest stay.
pub fn set_notification_disabled(stored: &Value, setting: &str, disabled: bool) -> Value {
    let mut map = stored.as_object().cloned().unwrap_or_default();
    let mut list: Vec<Value> = map
        .get("notificationSettings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.retain(|entry| entry != setting);
    if disabled {
        list.push(json!(setting));
    }
    map.entry("gameSettings").or_insert_with(|| json!({}));
    map.insert("notificationSettings".into(), Value::Array(list));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_and_round_trip_per_user() {
        let empty = Value::Null;
        let fresh = settings_for(&empty, "1");
        assert_eq!(fresh, OverlaySettings::default());
        assert_eq!(fresh.avatar_size_mode, AvatarSize::Large);
        assert!(!fresh.disable_clickable_regions);
        let mut changed = fresh.clone();
        changed.avatar_size_mode = AvatarSize::Small;
        changed.display_name_mode = DisplayNameMode::Never;
        changed.display_user_mode = DisplayUserMode::OnlyWhileSpeaking;
        changed.disable_clickable_regions = true;
        let stored = with_settings(&json!({"2": {"avatarSizeMode": "small"}}), "1", &changed);
        assert_eq!(stored["1"]["avatarSizeMode"], "small");
        assert_eq!(stored["1"]["displayNameMode"], "NEVER");
        assert_eq!(stored["1"]["displayUserMode"], "ONLY_WHILE_SPEAKING");
        assert_eq!(stored["1"]["textWidgetOpacity"], 0.25);
        assert_eq!(stored["2"]["avatarSizeMode"], "small");
        assert_eq!(settings_for(&stored, "1"), changed);
        // An unknown value keeps the default, an unknown field is kept.
        let odd = json!({"1": {"avatarSizeMode": "huge", "customInviteMessage": "hi"}});
        let read = settings_for(&odd, "1");
        assert_eq!(read.avatar_size_mode, AvatarSize::Large);
        assert_eq!(read.to_json()["customInviteMessage"], "hi");
        assert_eq!(AvatarSize::Small.pixels(), 24.);
    }

    #[test]
    fn global_switches_default_on() {
        let global = enabled(&Value::Null);
        assert!(global.oop && global.legacy);
        let stored = enabled_json(crate::decision::Global {
            oop: false,
            legacy: true,
        });
        assert_eq!(stored, json!({"legacyEnabled": true, "oopEnabled": false}));
        assert!(!enabled(&stored).oop);
    }

    #[test]
    fn notification_settings_hold_the_disabled_ones() {
        let stored = set_notification_disabled(&Value::Null, WELCOME_GENERAL, true);
        assert!(notification_disabled(&stored, WELCOME_GENERAL));
        assert!(!notification_disabled(&stored, TEXT_CHAT));
        assert_eq!(stored["gameSettings"], json!({}));
        let again = set_notification_disabled(&stored, WELCOME_GENERAL, true);
        assert_eq!(
            again["notificationSettings"].as_array().map(Vec::len),
            Some(1)
        );
        let back = set_notification_disabled(&again, WELCOME_GENERAL, false);
        assert!(!notification_disabled(&back, WELCOME_GENERAL));
    }
}
