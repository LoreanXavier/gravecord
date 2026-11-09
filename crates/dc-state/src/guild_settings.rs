//! `UserGuildSettingsStore`: per guild the default notification level and mute, and per channel its overrides.
//! READY's `user_guild_settings.entries`, then `USER_GUILD_SETTINGS_UPDATE` with a whole guild entry.
//! Levels are `message_notifications`: 0 all, 1 mentions only, 2 nothing, 3 inherit (`NULL`).

use crate::Id;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

pub const ALL_MESSAGES: u64 = 0;
pub const ONLY_MENTIONS: u64 = 1;
pub const NO_MESSAGES: u64 = 2;
pub const INHERIT: u64 = 3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Override {
    pub flags: u64,
    pub message_notifications: u64,
    pub muted: bool,
    /// `mute_config.end_time`; none means until turned back on.
    pub mute_end: Option<DateTime<Utc>>,
    pub collapsed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildSettings {
    pub flags: u64,
    pub message_notifications: u64,
    pub muted: bool,
    pub mute_end: Option<DateTime<Utc>>,
    pub channels: HashMap<Id, Override>,
    /// The server menu's Hide Muted Channels.
    pub hide_muted_channels: bool,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    /// `NotifyHighlights`: 0 unset, 1 disabled, 2 enabled.
    pub notify_highlights: u64,
    pub mute_scheduled_events: bool,
    pub mobile_push: bool,
}

fn snowflake(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn end_time(config: &Value) -> Option<DateTime<Utc>> {
    config["end_time"]
        .as_str()
        .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
        .map(|time| time.with_timezone(&Utc))
}

impl GuildSettings {
    pub fn from_value(entry: &Value) -> Self {
        let channels = entry["channel_overrides"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|channel| {
                Some((
                    snowflake(&channel["channel_id"])?,
                    Override {
                        flags: channel["flags"].as_u64().unwrap_or(0),
                        message_notifications: channel["message_notifications"]
                            .as_u64()
                            .unwrap_or(INHERIT),
                        muted: channel["muted"].as_bool().unwrap_or(false),
                        mute_end: end_time(&channel["mute_config"]),
                        collapsed: channel["collapsed"].as_bool().unwrap_or(false),
                    },
                ))
            })
            .collect();
        Self {
            flags: entry["flags"].as_u64().unwrap_or(0),
            message_notifications: entry["message_notifications"]
                .as_u64()
                .unwrap_or(ALL_MESSAGES),
            muted: entry["muted"].as_bool().unwrap_or(false),
            mute_end: end_time(&entry["mute_config"]),
            channels,
            hide_muted_channels: entry["hide_muted_channels"].as_bool().unwrap_or(false),
            suppress_everyone: entry["suppress_everyone"].as_bool().unwrap_or(false),
            suppress_roles: entry["suppress_roles"].as_bool().unwrap_or(false),
            notify_highlights: entry["notify_highlights"].as_u64().unwrap_or(0),
            mute_scheduled_events: entry["mute_scheduled_events"].as_bool().unwrap_or(false),
            mobile_push: entry["mobile_push"].as_bool().unwrap_or(true),
        }
    }

    /// One field as `PATCH /users/@me/guilds/settings` sends it, applied here.
    pub fn apply(&mut self, field: &str, value: &Value) {
        let flag = value.as_bool().unwrap_or(false);
        match field {
            "muted" => self.muted = flag,
            "message_notifications" => {
                self.message_notifications = value.as_u64().unwrap_or(self.message_notifications)
            }
            "suppress_everyone" => self.suppress_everyone = flag,
            "suppress_roles" => self.suppress_roles = flag,
            "notify_highlights" => self.notify_highlights = value.as_u64().unwrap_or(0),
            "mute_scheduled_events" => self.mute_scheduled_events = flag,
            "mobile_push" => self.mobile_push = flag,
            "hide_muted_channels" => self.hide_muted_channels = flag,
            _ => {}
        }
    }
}

/// A mute holds while it has no end or the end is still ahead.
pub fn mute_active(muted: bool, end: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    muted && end.is_none_or(|end| end > now)
}

#[derive(Default)]
pub struct GuildSettingsStore {
    pub guilds: HashMap<Id, GuildSettings>,
}

impl GuildSettingsStore {
    pub fn load(&mut self, ready: &Value) {
        for entry in ready["entries"].as_array().into_iter().flatten() {
            self.update(entry);
        }
    }

    pub fn update(&mut self, entry: &Value) {
        if let Some(guild) = if entry.get("guild_id").is_some_and(Value::is_null) {
            Some(0)
        } else {
            snowflake(&entry["guild_id"])
        } {
            self.guilds.insert(guild, GuildSettings::from_value(entry));
        }
    }

    pub fn channel(&self, guild: Id, channel: Id) -> Override {
        self.guilds
            .get(&guild)
            .and_then(|settings| settings.channels.get(&channel))
            .cloned()
            .unwrap_or(Override {
                message_notifications: INHERIT,
                ..Override::default()
            })
    }

    /// The level a channel set to inherit gets: its category's own setting, else the guild's.
    pub fn inherited(&self, guild: Id, category: Option<Id>) -> u64 {
        let from_category = category
            .map(|category| self.channel(guild, category).message_notifications)
            .filter(|level| *level != INHERIT);
        from_category.unwrap_or_else(|| {
            self.guilds
                .get(&guild)
                .map_or(ALL_MESSAGES, |settings| settings.message_notifications)
        })
    }

    pub fn channel_muted(&self, guild: Id, channel: Id, now: DateTime<Utc>) -> bool {
        let own = self.channel(guild, channel);
        mute_active(own.muted, own.mute_end, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_recorded_update() {
        let mut store = GuildSettingsStore::default();
        store.update(&json!({
            "guild_id": "1", "message_notifications": 0, "muted": false,
            "channel_overrides": [
                {"channel_id": "5", "message_notifications": 1, "muted": true,
                 "mute_config": {"selected_time_window": 900, "end_time": "2026-11-08T01:46:03.459000+00:00"}},
                {"channel_id": "6", "message_notifications": 3, "muted": false, "mute_config": null}
            ]
        }));
        assert_eq!(store.channel(1, 5).message_notifications, 1);
        assert_eq!(store.inherited(1, None), ALL_MESSAGES);
        assert_eq!(store.inherited(1, Some(5)), ONLY_MENTIONS);
        let before = DateTime::parse_from_rfc3339("2026-11-08T01:40:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let after = DateTime::parse_from_rfc3339("2026-11-08T01:50:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(store.channel_muted(1, 5, before));
        assert!(!store.channel_muted(1, 5, after));
        assert!(!store.channel_muted(1, 6, before));
    }
}
