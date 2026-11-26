//! Streaks (Equicord `src/equicordplugins/streaks`, GPL-3.0-or-later): the service's answers and the rules of a
//! streak's day. The badge and the sign-in are in `ui/plugin_hooks/streaks.rs`. Not part of the pinned client.

use dc_state::Id;
use serde_json::Value;
use std::collections::HashMap;

pub(crate) const API_URL: &str = "https://streaks.equicord.org/api";
/// `AUTHORIZE_URL`: the grant's redirect, which also trades its code for the service's token.
pub(crate) const AUTHORIZE_URL: &str = "https://streaks.equicord.org/api/authorize";
pub(crate) const CLIENT_ID: &str = "1243063117852835941";
/// The badge of a streak whose day is not complete on both sides (`#9ca3af`).
pub(crate) const IDLE_COLOR: u32 = 0x9ca3af;

/// `RemoteStreak`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Streak {
    pub user_a: Id,
    pub user_b: Id,
    pub count: u64,
    pub user_a_today: bool,
    pub user_b_today: bool,
    pub today_date: Option<String>,
    pub timezone_a: Option<String>,
    pub timezone_b: Option<String>,
    pub timezone_a_known: bool,
    pub timezone_b_known: bool,
    pub user_a_activity_date: Option<String>,
    pub user_b_activity_date: Option<String>,
    pub user_a_today_date: Option<String>,
    pub user_b_today_date: Option<String>,
}

fn id_of(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl Streak {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            user_a: id_of(&value["user_a_id"])?,
            user_b: id_of(&value["user_b_id"])?,
            count: value["count"].as_u64().unwrap_or(0),
            user_a_today: value["user_a_today"].as_bool().unwrap_or(false),
            user_b_today: value["user_b_today"].as_bool().unwrap_or(false),
            today_date: text(&value["today_date"]),
            timezone_a: text(&value["timezone_a"]),
            timezone_b: text(&value["timezone_b"]),
            timezone_a_known: value["timezone_a_known"].as_bool().unwrap_or(false),
            timezone_b_known: value["timezone_b_known"].as_bool().unwrap_or(false),
            user_a_activity_date: text(&value["user_a_activity_date"]),
            user_b_activity_date: text(&value["user_b_activity_date"]),
            user_a_today_date: text(&value["user_a_today_date"]),
            user_b_today_date: text(&value["user_b_today_date"]),
        })
    }

    /// The other person of the streak.
    pub(crate) fn other(&self, me: Id) -> Id {
        if self.user_a == me {
            self.user_b
        } else {
            self.user_a
        }
    }
}

/// `fetch`'s map: each streak under the other person's id.
pub(crate) fn by_other(answer: &Value, me: Id) -> HashMap<Id, Streak> {
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Streak::from_value)
        .map(|streak| (streak.other(me), streak))
        .collect()
}

/// Today's calendar date where it matters: in this machine's time zone (`localTimezone`), and in UTC.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Today {
    pub zone: String,
    pub local: String,
    pub utc: String,
}

impl Today {
    pub(crate) fn now(zone: &str) -> Self {
        let now = chrono::Utc::now();
        Self {
            zone: zone.to_owned(),
            local: now
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string(),
            utc: now.format("%Y-%m-%d").to_string(),
        }
    }

    /// `dateInTimezone(zone)`: this machine's date for its own zone (or none given). The app has no time zone
    /// database, so a day in another zone is taken as either this machine's date or UTC's, whichever matches.
    fn is_today(&self, zone: Option<&str>, date: &str) -> bool {
        match zone {
            None => date == self.local,
            Some(zone) if zone == self.zone => date == self.local,
            Some(_) => date == self.local || date == self.utc,
        }
    }
}

/// `didUserSendToday(streak, isA)`.
pub(crate) fn sent_today(streak: &Streak, a: bool, today: &Today) -> bool {
    let (zone, activity, flag, reported) = if a {
        (
            &streak.timezone_a,
            &streak.user_a_activity_date,
            streak.user_a_today,
            &streak.user_a_today_date,
        )
    } else {
        (
            &streak.timezone_b,
            &streak.user_b_activity_date,
            streak.user_b_today,
            &streak.user_b_today_date,
        )
    };
    let zone = zone.as_deref();
    if let Some(activity) = activity {
        return today.is_today(zone, activity);
    }
    flag && reported
        .as_ref()
        .or(streak.today_date.as_ref())
        .is_some_and(|date| today.is_today(zone, date))
}

/// The day is complete on both sides: the badge takes its tier's colour.
pub(crate) fn active(streak: &Streak, today: &Today) -> bool {
    sent_today(streak, true, today) && sent_today(streak, false, today)
}

/// `colorFor(streak)`: the setting that holds the tier's colour (`STREAK_THRESHOLDS`).
pub(crate) fn color_key(count: u64) -> &'static str {
    match count {
        100.. => "eliteColor",
        60.. => "diamondColor",
        45.. => "platinumColor",
        30.. => "goldColor",
        14.. => "silverColor",
        7.. => "bronzeColor",
        _ => "defaultColor",
    }
}

/// A CSS colour as the settings hold it: `#rgb` or `#rrggbb`.
pub(crate) fn parse_color(text: &str) -> Option<u32> {
    let hex = text.trim().strip_prefix('#')?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(value),
        3 => {
            let (r, g, b) = ((value >> 8) & 0xf, (value >> 4) & 0xf, value & 0xf);
            Some(((r * 0x11) << 16) | ((g * 0x11) << 8) | (b * 0x11))
        }
        _ => None,
    }
}

/// `MESSAGE_CREATE`: whether a DM message from `author` is reported (`update(recipient, author)`): the sender's day
/// is not complete yet, or this machine's time zone is not the one the service has for you.
pub(crate) fn reports(
    cached: Option<&Streak>,
    me: Id,
    author: Id,
    recipient: Id,
    today: &Today,
) -> bool {
    let mine_a = cached.is_some_and(|streak| streak.user_a == me);
    let my_flag = cached.is_some_and(|streak| sent_today(streak, mine_a, today));
    let their_flag = cached.is_some_and(|streak| sent_today(streak, !mine_a, today));
    let (my_zone, known) = cached.map_or((None, false), |streak| {
        if mine_a {
            (streak.timezone_a.as_deref(), streak.timezone_a_known)
        } else {
            (streak.timezone_b.as_deref(), streak.timezone_b_known)
        }
    });
    let report_zone = !known || my_zone != Some(today.zone.as_str());
    if my_flag && their_flag && !report_zone {
        return false;
    }
    (author == me && (!my_flag || report_zone))
        || (author == recipient && (!their_flag || report_zone))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn today() -> Today {
        Today {
            zone: "Europe/Istanbul".to_owned(),
            local: "2026-11-26".to_owned(),
            utc: "2026-11-25".to_owned(),
        }
    }

    #[test]
    fn streak_days_follow_the_activity_dates() {
        let answer = json!([{"user_a_id": "1", "user_b_id": "2", "count": 31,
            "timezone_a": "Europe/Istanbul", "timezone_b": "America/New_York",
            "timezone_a_known": true, "timezone_b_known": true,
            "user_a_activity_date": "2026-11-26", "user_b_activity_date": "2026-11-25"}]);
        let streaks = by_other(&answer, 1);
        let streak = &streaks[&2];
        assert_eq!(streak.count, 31);
        // B's zone is another one: UTC's date counts as their today.
        assert!(sent_today(streak, true, &today()));
        assert!(sent_today(streak, false, &today()));
        assert!(active(streak, &today()));
        assert_eq!(color_key(streak.count), "goldColor");
        let mut stale = streak.clone();
        stale.user_a_activity_date = Some("2026-11-24".to_owned());
        assert!(!active(&stale, &today()));
        // Without activity dates, the flag with the reported (or the service's) date.
        let flags = Streak {
            user_a: 1,
            user_b: 2,
            user_a_today: true,
            today_date: Some("2026-11-26".to_owned()),
            ..Streak::default()
        };
        assert!(sent_today(&flags, true, &today()));
        assert!(!sent_today(&flags, false, &today()));
        assert_eq!(color_key(0), "defaultColor");
        assert_eq!(color_key(100), "eliteColor");
        assert_eq!(color_key(7), "bronzeColor");
        assert_eq!(parse_color("#9b39fe"), Some(0x9b39fe));
        assert_eq!(parse_color("#abc"), Some(0xaabbcc));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn streak_messages_report_until_the_day_is_complete() {
        let today = today();
        // No streak yet: both sides report.
        assert!(reports(None, 1, 1, 2, &today));
        assert!(reports(None, 1, 2, 2, &today));
        let done = Streak {
            user_a: 1,
            user_b: 2,
            timezone_a: Some(today.zone.clone()),
            timezone_a_known: true,
            user_a_activity_date: Some(today.local.clone()),
            user_b_activity_date: Some(today.local.clone()),
            ..Streak::default()
        };
        assert!(!reports(Some(&done), 1, 1, 2, &today));
        assert!(!reports(Some(&done), 1, 2, 2, &today));
        // A third person's message never reports.
        let mut moved = done.clone();
        moved.timezone_a = Some("Asia/Tokyo".to_owned());
        assert!(reports(Some(&moved), 1, 1, 2, &today));
        assert!(!reports(Some(&done), 1, 3, 2, &today));
    }
}
