//! `UnreadSettingNoticeStore2` (module 496990): under the new notification settings (`useNewNotifications`, flag 16
//! of the account's notification settings), a server channel whose unreads count only mentions is upgraded to "every
//! message" once the account has used it enough soon after joining: per channel the store keeps the time of the last
//! action, the time spent viewing it and the messages sent, and `maybeAutoUpgradeChannel` compares them with four
//! thresholds by the time since joining. An upgraded channel shows the "This channel will now be marked as unread"
//! notice (module 626977) with Undo.

use crate::guild_settings::INHERIT;
use crate::{Id, Stores};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 3_600_000;
const DAY: i64 = 86_400_000;
const WEEK: i64 = 604_800_000;
const DAYS_30: i64 = 2_592_000_000;

/// `UNREADS_ONLY_MENTIONS` and `UNREADS_ALL_MESSAGES` of a channel override's flags (module 355097 `vv`).
pub const UNREADS_ONLY_MENTIONS: u64 = 512;
pub const UNREADS_ALL_MESSAGES: u64 = 1024;

/// `E`: within `time_since_join` of joining, `sends` messages or `view_time` of viewing are enough.
struct Threshold {
    time_since_join: i64,
    sends: u32,
    view_time: i64,
}

const THRESHOLDS: [Threshold; 4] = [
    Threshold {
        time_since_join: HOUR,
        sends: 1,
        view_time: MINUTE,
    },
    Threshold {
        time_since_join: DAY,
        sends: 2,
        view_time: 2 * MINUTE,
    },
    Threshold {
        time_since_join: WEEK,
        sends: 5,
        view_time: 5 * MINUTE,
    },
    Threshold {
        time_since_join: DAYS_30,
        sends: 10,
        view_time: 30 * MINUTE,
    },
];

/// `S`: five times the last threshold's viewing time; past it the view clock stops counting for a day.
const VIEW_CAP: i64 = 5 * 30 * MINUTE;

/// How often the open channel's viewing time is added up (`setInterval(M, 15s)`).
pub const TICK_MS: u64 = 15_000;

/// The persist key of the official store.
pub const PERSIST_KEY: &str = "UnreadSettingNoticeStore2";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub last_action_time: i64,
    pub view_duration: i64,
    pub num_sends: u32,
}

#[derive(Clone, Debug, Default)]
pub struct UnreadSettingNotice {
    /// `_.channels`, persisted.
    pub channels: HashMap<Id, Record>,
    /// `N`: channels upgraded in this session, never again.
    upgraded: HashSet<Id>,
    /// `T` and `y`: the channel in view and since when its viewing time is counted.
    current: Option<Id>,
    since: i64,
}

fn snowflake_ms(id: Id) -> i64 {
    (id >> 22) as i64 + 1_420_070_400_000
}

/// `x`: the override of `channel` (a channel or its category) sets a notification level or an unread setting.
fn has_override(stores: &Stores, guild: Id, channel: Option<Id>) -> bool {
    let Some(channel) = channel else {
        return false;
    };
    stores
        .guild_settings
        .guilds
        .get(&guild)
        .and_then(|settings| settings.channels.get(&channel))
        .is_some_and(|over| {
            over.message_notifications != INHERIT
                || over.flags & (UNREADS_ALL_MESSAGES | UNREADS_ONLY_MENTIONS) != 0
        })
}

/// The channel's own override flags (`getChannelIdFlags`).
pub fn channel_flags(stores: &Stores, guild: Id, channel: Id) -> u64 {
    stores
        .guild_settings
        .guilds
        .get(&guild)
        .and_then(|settings| settings.channels.get(&channel))
        .map_or(0, |over| over.flags)
}

/// `mD`: the flags with the unread setting replaced by `setting`.
pub fn with_unread_setting(flags: u64, setting: u64) -> u64 {
    (flags & !(UNREADS_ALL_MESSAGES | UNREADS_ONLY_MENTIONS)) | setting
}

impl UnreadSettingNotice {
    pub fn from_value(value: &Value) -> Self {
        let channels = value["channels"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, record)| {
                Some((
                    id.parse().ok()?,
                    Record {
                        last_action_time: record["lastActionTime"].as_i64().unwrap_or(0),
                        view_duration: record["viewDuration"].as_i64().unwrap_or(0),
                        num_sends: record["numSends"].as_u64().unwrap_or(0) as u32,
                    },
                ))
            })
            .collect();
        Self {
            channels,
            ..Self::default()
        }
    }

    /// `getState()`, as it is persisted.
    pub fn to_value(&self) -> Value {
        let channels: serde_json::Map<String, Value> = self
            .channels
            .iter()
            .map(|(id, record)| {
                (
                    id.to_string(),
                    json!({
                        "lastActionTime": record.last_action_time,
                        "viewDuration": record.view_duration,
                        "numSends": record.num_sends,
                    }),
                )
            })
            .collect();
        json!({ "channels": channels })
    }

    /// `O`: the store counts this channel. The new notification settings are on, it has not been upgraded in this
    /// session, it is a server channel that is not muted (with its server and category), neither it nor its category
    /// has a notification or unread override, and its unreads resolve to mentions only.
    pub fn eligible(&self, stores: &Stores, channel: Id, now_ms: i64) -> bool {
        if stores.notification_flags & 16 == 0 || self.upgraded.contains(&channel) {
            return false;
        }
        let Some(guild) = stores.guild_of_channel(channel) else {
            return false;
        };
        let Some(known) = guild.channels.iter().find(|known| known.id == channel) else {
            return false;
        };
        let now = chrono::DateTime::from_timestamp_millis(now_ms).unwrap_or_default();
        if crate::unread::muted(stores, guild.id, channel, known.parent_id, now)
            || has_override(stores, guild.id, Some(channel))
            || has_override(stores, guild.id, known.parent_id)
        {
            return false;
        }
        // `getChannelUnreadSetting(...) === UNSET` holds: the channel's own unread flags were ruled out above.
        !crate::unread::all_unreads(stores, guild, known)
    }

    fn record(&mut self, channel: Id) -> &mut Record {
        self.channels.entry(channel).or_default()
    }

    /// `M`: adds the time the open channel has been viewed. False when nothing was counted.
    pub fn tick(&mut self, stores: &Stores, now_ms: i64) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        if !self.eligible(stores, current, now_ms) {
            return false;
        }
        let since = self.since;
        let record = self.record(current);
        if record.last_action_time > now_ms - DAY && record.view_duration > VIEW_CAP {
            return false;
        }
        record.last_action_time = now_ms;
        record.view_duration += now_ms - since;
        self.since = now_ms;
        true
    }

    /// `CHANNEL_SELECT`: the channel left is counted up to now, then the new one is timed.
    pub fn channel_select(&mut self, stores: &Stores, channel: Option<Id>, now_ms: i64) -> bool {
        let changed = self.tick(stores, now_ms);
        self.current = channel;
        self.since = now_ms;
        changed
    }

    /// The channel being timed.
    pub fn current(&self) -> Option<Id> {
        self.current
    }

    /// `CONNECTION_OPEN`: timing starts again, and records without an action for a week go.
    pub fn connection_open(&mut self, channel: Option<Id>, now_ms: i64) {
        self.current = channel;
        self.since = now_ms;
        self.channels
            .retain(|_, record| record.last_action_time >= now_ms - WEEK);
    }

    /// `MESSAGE_CREATE` of the account's own message (not the local copy): a send in the channel.
    pub fn message_sent(&mut self, stores: &Stores, channel: Id, now_ms: i64) -> bool {
        if !self.eligible(stores, channel, now_ms) {
            return false;
        }
        let record = self.record(channel);
        record.last_action_time = now_ms;
        record.num_sends += 1;
        true
    }

    /// `getLastActionTime`.
    pub fn last_action_time(&self, channel: Id) -> i64 {
        self.channels
            .get(&channel)
            .map_or(0, |record| record.last_action_time)
    }

    /// `maybeAutoUpgradeChannel`: when the channel has been used enough for the time since joining (the younger of
    /// the channel and the membership), it is forgotten, kept from counting again, and the flags its override takes
    /// (unreads on every message) are returned for `updateChannelOverrideSettings`.
    pub fn maybe_auto_upgrade(
        &mut self,
        stores: &Stores,
        channel: Id,
        now_ms: i64,
    ) -> Option<(Id, u64)> {
        if !self.eligible(stores, channel, now_ms) {
            return None;
        }
        let guild = stores.guild_of_channel(channel)?;
        let joined = chrono::DateTime::parse_from_rfc3339(&guild.joined_at)
            .map_or(now_ms, |joined| joined.timestamp_millis());
        let age = (now_ms - snowflake_ms(channel)).min(now_ms - joined);
        let record = self.channels.get(&channel)?;
        if record.last_action_time < now_ms - WEEK {
            return None;
        }
        let used = THRESHOLDS.iter().any(|threshold| {
            age < threshold.time_since_join
                && (record.num_sends >= threshold.sends
                    || record.view_duration >= threshold.view_time)
        });
        if !used {
            return None;
        }
        let guild = guild.id;
        self.channels.remove(&channel);
        self.upgraded.insert(channel);
        let flags =
            with_unread_setting(channel_flags(stores, guild, channel), UNREADS_ALL_MESSAGES);
        Some((guild, flags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    /// A server joined `joined_ago` before [`NOW`] with a channel made long before, unreads on mentions only.
    fn stores(joined_ago: i64) -> (Stores, Id) {
        let mut stores = Stores::default();
        let channel: Id = ((NOW - 400 * DAY - 1_420_070_400_000) as u64) << 22;
        let joined = chrono::DateTime::from_timestamp_millis(NOW - joined_ago)
            .unwrap()
            .to_rfc3339();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "guilds": [{"id": "10", "joined_at": joined,
                    "properties": {"name": "fixture", "default_message_notifications": 1},
                    "roles": [{"id": "10", "permissions": "1024"}],
                    "channels": [{"id": channel.to_string(), "name": "chat", "type": 0}]}],
            }),
        );
        stores.notification_flags = 16;
        (stores, channel)
    }

    #[test]
    fn one_send_within_an_hour_of_joining_upgrades_the_channel_once() {
        let (stores, channel) = stores(10 * MINUTE);
        let mut notice = UnreadSettingNotice::default();
        assert!(notice.eligible(&stores, channel, NOW));
        assert_eq!(notice.maybe_auto_upgrade(&stores, channel, NOW), None);
        assert!(notice.message_sent(&stores, channel, NOW));
        assert_eq!(
            notice.maybe_auto_upgrade(&stores, channel, NOW),
            Some((10, UNREADS_ALL_MESSAGES))
        );
        // Forgotten and never counted again in this session.
        assert!(notice.channels.is_empty());
        assert!(!notice.message_sent(&stores, channel, NOW));
        assert_eq!(notice.maybe_auto_upgrade(&stores, channel, NOW), None);
    }

    #[test]
    fn viewing_time_counts_against_the_threshold_of_the_age() {
        let (stores, channel) = stores(3 * DAY);
        let mut notice = UnreadSettingNotice::default();
        notice.channel_select(&stores, Some(channel), NOW);
        assert!(notice.tick(&stores, NOW + 4 * MINUTE));
        // Three days in: five minutes or five sends are needed.
        assert_eq!(
            notice.maybe_auto_upgrade(&stores, channel, NOW + 4 * MINUTE),
            None
        );
        assert!(notice.tick(&stores, NOW + 5 * MINUTE));
        assert!(
            notice
                .maybe_auto_upgrade(&stores, channel, NOW + 5 * MINUTE)
                .is_some()
        );
    }

    #[test]
    fn old_memberships_settings_and_mutes_are_not_counted() {
        let (mut stores, channel) = stores(40 * DAY);
        let mut notice = UnreadSettingNotice::default();
        notice.message_sent(&stores, channel, NOW);
        assert_eq!(notice.maybe_auto_upgrade(&stores, channel, NOW), None);
        // Unreads already on every message, or the old settings: not counted.
        stores.guilds[0].default_message_notifications = 0;
        assert!(!notice.eligible(&stores, channel, NOW));
        stores.guilds[0].default_message_notifications = 1;
        stores.notification_flags = 0;
        assert!(!notice.eligible(&stores, channel, NOW));
        stores.notification_flags = 16;
        stores.guild_settings.update(&json!({"guild_id": "10",
            "channel_overrides": [{"channel_id": channel.to_string(), "flags": 512}]}));
        assert!(!notice.eligible(&stores, channel, NOW));
    }

    #[test]
    fn the_state_round_trips_and_a_week_old_record_is_dropped() {
        let mut notice = UnreadSettingNotice::default();
        notice.channels.insert(
            5,
            Record {
                last_action_time: NOW - 2 * DAY,
                view_duration: 7,
                num_sends: 2,
            },
        );
        notice.channels.insert(
            6,
            Record {
                last_action_time: NOW - 8 * DAY,
                ..Record::default()
            },
        );
        let mut back = UnreadSettingNotice::from_value(&notice.to_value());
        assert_eq!(back.channels, notice.channels);
        back.connection_open(None, NOW);
        assert_eq!(back.channels.keys().copied().collect::<Vec<_>>(), vec![5]);
        assert_eq!(
            with_unread_setting(UNREADS_ONLY_MENTIONS | 2048, UNREADS_ALL_MESSAGES),
            UNREADS_ALL_MESSAGES | 2048
        );
    }
}
