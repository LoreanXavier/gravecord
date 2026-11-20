//! Activity & library play-time formatter (module 764359 `PlayTime`, `f`, `d`, `c`; time unit math module 499979 `pJ`,
//! `Ul`, `eZ`).
//!
//! Formats live elapsed times (`Start`), ended timestamps (`End`), and accumulated play durations (`Time`) across all
//! six official locations: `ActivityFeed`, `ActivityFeedNew`, `UserActivity`, `GameLibraryTimePlayed`,
//! `GameLibraryLastPlayed`, and `VoiceChannelHistory`.

use serde_json::Value;

/// `pJ` of module 499979: time units ordered from coarsest fallback to finest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    None,
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

/// `d` of module 764359 (all 6 official locations are implemented and unit-tested).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Location {
    ActivityFeed,
    ActivityFeedNew,
    UserActivity,
    GameLibraryTimePlayed,
    GameLibraryLastPlayed,
    VoiceChannelHistory,
}

/// `c` of module 764359 (all 3 timestamp kinds are implemented and unit-tested).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Start,
    End,
    Time,
}

/// `E` of module 499979: `(unit, max_minutes)`.
const THRESHOLDS: &[(Unit, f64)] = &[
    (Unit::None, 0.),
    (Unit::Seconds, 1.),
    (Unit::Minutes, 60.),
    (Unit::Hours, 1440.),
    (Unit::Days, 44_640.),
    (Unit::Weeks, 40_320.),
    (Unit::Months, 525_600.),
    (Unit::Years, f64::INFINITY),
];

/// `eZ` of module 499979: converts `minutes` into `unit`.
pub(crate) fn in_unit(minutes: f64, unit: Unit) -> u64 {
    let raw = match unit {
        Unit::None => 0.,
        Unit::Seconds => minutes * 60.,
        Unit::Minutes => minutes,
        Unit::Hours => minutes / 60.,
        Unit::Days => minutes / 60. / 24.,
        Unit::Weeks => minutes / 60. / 24. / 7.,
        Unit::Months => minutes / 60. / 24. / 31.,
        Unit::Years => minutes / 60. / 24. / 365.,
    };
    raw.max(0.).floor() as u64
}

/// Supported units for `(location, kind)` in `f` of module 764359.
fn supports(location: Location, kind: Kind, unit: Unit) -> bool {
    match (location, kind) {
        (Location::ActivityFeed, Kind::Start)
        | (Location::ActivityFeedNew, Kind::Start | Kind::End)
        | (Location::UserActivity, Kind::Start)
        | (Location::VoiceChannelHistory, Kind::End) => {
            matches!(
                unit,
                Unit::Seconds | Unit::Minutes | Unit::Hours | Unit::Days
            )
        }
        (Location::ActivityFeed, Kind::End) => matches!(
            unit,
            Unit::Seconds
                | Unit::Minutes
                | Unit::Hours
                | Unit::Days
                | Unit::Weeks
                | Unit::Months
                | Unit::Years
        ),
        (Location::GameLibraryTimePlayed, Kind::Time) => {
            matches!(
                unit,
                Unit::None | Unit::Seconds | Unit::Minutes | Unit::Hours
            )
        }
        (Location::GameLibraryLastPlayed, Kind::End) => matches!(
            unit,
            Unit::None
                | Unit::Seconds
                | Unit::Minutes
                | Unit::Hours
                | Unit::Days
                | Unit::Weeks
                | Unit::Months
                | Unit::Years
        ),
        _ => false,
    }
}

/// `Ul` of module 499979: finds the first matching unit bucket in `THRESHOLDS`, falling back to the
/// last supported unit at or before that index (and then the first supported unit).
pub(crate) fn select_unit(minutes: f64, location: Location, kind: Kind) -> Option<Unit> {
    let idx = THRESHOLDS
        .iter()
        .position(|&(unit, max)| (unit == Unit::None && minutes == 0.) || minutes < max)
        .unwrap_or(THRESHOLDS.len() - 1);
    // Notice `WEEKS` (40320m = 4w) sits after `DAYS` (44640m = 31d) in module 499979's table; for
    // `DAYS` vs `WEEKS`, if >= 7 days (10080m) and < 28 days (40320m) and Weeks is supported, pick Weeks.
    if (10_080. ..40_320.).contains(&minutes) && supports(location, kind, Unit::Weeks) {
        return Some(Unit::Weeks);
    }
    for &(unit, _) in THRESHOLDS[..=idx].iter().rev() {
        if supports(location, kind, unit) {
            return Some(unit);
        }
    }
    THRESHOLDS
        .iter()
        .map(|&(unit, _)| unit)
        .find(|&unit| supports(location, kind, unit))
}

/// Formats a play-time duration in milliseconds (`elapsed_ms`) using `f` of module 764359.
///
/// - `activity_name`: used by `ActivityFeedNew` `Start` (`Bf3PRU`, `A9P+dT`, `KwVzfh`, `hbUpBN`).
/// - `is_streaming`: used by `UserActivity` `Start` (true -> `"Playing for ..."`, false -> `"for ..."`).
pub(crate) fn format_play_time(
    elapsed_ms: u64,
    location: Location,
    kind: Kind,
    activity_name: Option<&str>,
    is_streaming: bool,
) -> Option<String> {
    let minutes = elapsed_ms as f64 / 60_000.;
    let unit = select_unit(minutes, location, kind)?;
    let amount = in_unit(minutes, unit) as i64;
    let text = match (location, kind, unit) {
        // ACTIVITY_FEED.START
        (Location::ActivityFeed, Kind::Start, Unit::Seconds) => dc_intl::t("ahzZr+").to_owned(),
        (Location::ActivityFeed, Kind::Start, Unit::Minutes) => {
            dc_intl::format("03mIHW", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::Start, Unit::Hours) => {
            dc_intl::format("eNoooU", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::Start, Unit::Days) => {
            dc_intl::format("2rUo/p", &[("time", amount.into())])
        }

        // ACTIVITY_FEED.END
        (Location::ActivityFeed, Kind::End, Unit::Seconds) => dc_intl::t("EluAd9").to_owned(),
        (Location::ActivityFeed, Kind::End, Unit::Minutes) => {
            dc_intl::format("BZxG8Z", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::End, Unit::Hours) => {
            dc_intl::format("cRMUpw", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::End, Unit::Days) => {
            dc_intl::format("yP1T84", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::End, Unit::Weeks) => {
            dc_intl::format("AWkdqe", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::End, Unit::Months) => {
            dc_intl::format("upamA7", &[("time", amount.into())])
        }
        (Location::ActivityFeed, Kind::End, Unit::Years) => {
            dc_intl::format("vfMC2h", &[("time", amount.into())])
        }

        // ACTIVITY_FEED_NEW.START
        (Location::ActivityFeedNew, Kind::Start, Unit::Seconds) => {
            let name = activity_name.unwrap_or_default();
            dc_intl::format("Bf3PRU", &[("name", name.into()), ("time", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::Start, Unit::Minutes) => {
            let name = activity_name.unwrap_or_default();
            dc_intl::format("A9P+dT", &[("name", name.into()), ("time", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::Start, Unit::Hours) => {
            let name = activity_name.unwrap_or_default();
            dc_intl::format("KwVzfh", &[("name", name.into()), ("time", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::Start, Unit::Days) => {
            let name = activity_name.unwrap_or_default();
            dc_intl::format("hbUpBN", &[("name", name.into()), ("time", amount.into())])
        }

        // ACTIVITY_FEED_NEW.END
        (Location::ActivityFeedNew, Kind::End, Unit::Seconds) => {
            dc_intl::format("C3MTOD", &[("seconds", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::End, Unit::Minutes) => {
            dc_intl::format("GqQ/Y9", &[("minutes", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::End, Unit::Hours) => {
            dc_intl::format("c5zfWZ", &[("hours", amount.into())])
        }
        (Location::ActivityFeedNew, Kind::End, Unit::Days) => {
            dc_intl::format("amjnaI", &[("days", amount.into())])
        }

        // USER_ACTIVITY.START
        (Location::UserActivity, Kind::Start, Unit::Seconds) => dc_intl::t("ahzZr+").to_owned(),
        (Location::UserActivity, Kind::Start, Unit::Minutes) => {
            let key = if is_streaming { "03mIHW" } else { "vQml+P" };
            dc_intl::format(key, &[("time", amount.into())])
        }
        (Location::UserActivity, Kind::Start, Unit::Hours) => {
            let key = if is_streaming { "eNoooU" } else { "4Wq+6x" };
            dc_intl::format(key, &[("time", amount.into())])
        }
        (Location::UserActivity, Kind::Start, Unit::Days) => {
            let key = if is_streaming { "2rUo/p" } else { "jN3/fR" };
            dc_intl::format(key, &[("time", amount.into())])
        }

        // GAME_LIBRARY_TIME_PLAYED.TIME
        (Location::GameLibraryTimePlayed, Kind::Time, Unit::None) => {
            dc_intl::t("LqDiCt").to_owned()
        }
        (Location::GameLibraryTimePlayed, Kind::Time, Unit::Seconds) => {
            dc_intl::format("xDIDBL", &[("time", amount.into())])
        }
        (Location::GameLibraryTimePlayed, Kind::Time, Unit::Minutes) => {
            dc_intl::format("KGqJce", &[("time", amount.into())])
        }
        (Location::GameLibraryTimePlayed, Kind::Time, Unit::Hours) => {
            dc_intl::format("OLchUE", &[("time", amount.into())])
        }

        // GAME_LIBRARY_LAST_PLAYED.END
        (Location::GameLibraryLastPlayed, Kind::End, Unit::None) => dc_intl::t("EoWLru").to_owned(),
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Seconds) => {
            dc_intl::t("QTHa8b").to_owned()
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Minutes) => {
            dc_intl::format("SVEfcd", &[("time", amount.into())])
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Hours) => {
            dc_intl::format("0Gbusd", &[("time", amount.into())])
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Days) => {
            dc_intl::format("hnPZZ7", &[("time", amount.into())])
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Weeks) => {
            dc_intl::format("L3fTpL", &[("time", amount.into())])
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Months) => {
            dc_intl::format("xAFWQV", &[("time", amount.into())])
        }
        (Location::GameLibraryLastPlayed, Kind::End, Unit::Years) => {
            dc_intl::format("9Yr3NW", &[("time", amount.into())])
        }

        // VOICE_CHANNEL_HISTORY.END
        (Location::VoiceChannelHistory, Kind::End, Unit::Seconds) => {
            dc_intl::t("1QsMmY").to_owned()
        }
        (Location::VoiceChannelHistory, Kind::End, Unit::Minutes) => {
            dc_intl::format("LRNgHp", &[("count", amount.into())])
        }
        (Location::VoiceChannelHistory, Kind::End, Unit::Hours) => {
            dc_intl::format("raJpz3", &[("count", amount.into())])
        }
        (Location::VoiceChannelHistory, Kind::End, Unit::Days) => {
            dc_intl::format("KkvKhi", &[("count", amount.into())])
        }

        _ => return None,
    };
    let _ = Value::Null;
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000;
    const MIN: u64 = 60_000;
    const HOUR: u64 = 3_600_000;
    const DAY: u64 = 86_400_000;

    #[test]
    fn activity_feed_start_and_end_cover_all_units() {
        assert_eq!(
            format_play_time(20 * SEC, Location::ActivityFeed, Kind::Start, None, false).as_deref(),
            Some("Just started playing")
        );
        assert_eq!(
            format_play_time(5 * MIN, Location::ActivityFeed, Kind::Start, None, false).as_deref(),
            Some("Playing for 5 minutes")
        );
        assert_eq!(
            format_play_time(2 * HOUR, Location::ActivityFeed, Kind::Start, None, false).as_deref(),
            Some("Playing for 2 hours")
        );
        assert_eq!(
            format_play_time(3 * DAY, Location::ActivityFeed, Kind::Start, None, false).as_deref(),
            Some("Playing for 3 days")
        );

        assert_eq!(
            format_play_time(15 * SEC, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Just stopped playing")
        );
        assert_eq!(
            format_play_time(10 * MIN, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 10 minutes ago")
        );
        assert_eq!(
            format_play_time(3 * HOUR, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 3 hours ago")
        );
        assert_eq!(
            format_play_time(2 * DAY, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 2 days ago")
        );
        assert_eq!(
            format_play_time(14 * DAY, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 2 weeks ago")
        );
        assert_eq!(
            format_play_time(62 * DAY, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 2 months ago")
        );
        assert_eq!(
            format_play_time(400 * DAY, Location::ActivityFeed, Kind::End, None, false).as_deref(),
            Some("Played 1 year ago")
        );
    }

    #[test]
    fn activity_feed_new_and_user_activity_cover_all_variants() {
        assert_eq!(
            format_play_time(
                10 * SEC,
                Location::ActivityFeedNew,
                Kind::Start,
                Some("Hades"),
                false
            )
            .as_deref(),
            Some("Hades - just now")
        );
        assert_eq!(
            format_play_time(
                12 * MIN,
                Location::ActivityFeedNew,
                Kind::Start,
                Some("Hades"),
                false
            )
            .as_deref(),
            Some("Hades \u{2013} 12m")
        );
        assert_eq!(
            format_play_time(
                4 * HOUR,
                Location::ActivityFeedNew,
                Kind::Start,
                Some("Hades"),
                false
            )
            .as_deref(),
            Some("Hades \u{2013} 4h")
        );
        assert_eq!(
            format_play_time(
                2 * DAY,
                Location::ActivityFeedNew,
                Kind::Start,
                Some("Hades"),
                false
            )
            .as_deref(),
            Some("Hades \u{2013} 2d")
        );

        assert_eq!(
            format_play_time(30 * SEC, Location::ActivityFeedNew, Kind::End, None, false)
                .as_deref(),
            Some("30 seconds ago")
        );
        assert_eq!(
            format_play_time(7 * MIN, Location::ActivityFeedNew, Kind::End, None, false).as_deref(),
            Some("7 minutes ago")
        );
        assert_eq!(
            format_play_time(5 * HOUR, Location::ActivityFeedNew, Kind::End, None, false)
                .as_deref(),
            Some("5 hours ago")
        );
        assert_eq!(
            format_play_time(2 * DAY, Location::ActivityFeedNew, Kind::End, None, false).as_deref(),
            Some("2 days ago")
        );

        assert_eq!(
            format_play_time(10 * SEC, Location::UserActivity, Kind::Start, None, false).as_deref(),
            Some("Just started playing")
        );
        assert_eq!(
            format_play_time(4 * MIN, Location::UserActivity, Kind::Start, None, false).as_deref(),
            Some("for 4 minutes")
        );
        assert_eq!(
            format_play_time(4 * MIN, Location::UserActivity, Kind::Start, None, true).as_deref(),
            Some("Playing for 4 minutes")
        );
        assert_eq!(
            format_play_time(2 * HOUR, Location::UserActivity, Kind::Start, None, false).as_deref(),
            Some("for 2 hours")
        );
        assert_eq!(
            format_play_time(2 * DAY, Location::UserActivity, Kind::Start, None, false).as_deref(),
            Some("for 2 days")
        );
    }

    #[test]
    fn library_and_voice_history_cover_all_units() {
        assert_eq!(
            format_play_time(0, Location::GameLibraryTimePlayed, Kind::Time, None, false)
                .as_deref(),
            Some("never played")
        );
        assert_eq!(
            format_play_time(
                45 * SEC,
                Location::GameLibraryTimePlayed,
                Kind::Time,
                None,
                false
            )
            .as_deref(),
            Some("45  seconds")
        );
        assert_eq!(
            format_play_time(
                15 * MIN,
                Location::GameLibraryTimePlayed,
                Kind::Time,
                None,
                false
            )
            .as_deref(),
            Some("15  minutes")
        );
        assert_eq!(
            format_play_time(
                10 * HOUR,
                Location::GameLibraryTimePlayed,
                Kind::Time,
                None,
                false
            )
            .as_deref(),
            Some("10  hours")
        );

        assert_eq!(
            format_play_time(0, Location::GameLibraryLastPlayed, Kind::End, None, false).as_deref(),
            Some("never played")
        );
        assert_eq!(
            format_play_time(
                20 * SEC,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("just now")
        );
        assert_eq!(
            format_play_time(
                5 * MIN,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("5 minutes ago")
        );
        assert_eq!(
            format_play_time(
                3 * HOUR,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("3 hours ago")
        );
        assert_eq!(
            format_play_time(DAY, Location::GameLibraryLastPlayed, Kind::End, None, false)
                .as_deref(),
            Some("yesterday")
        );
        assert_eq!(
            format_play_time(
                7 * DAY,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("last week")
        );
        assert_eq!(
            format_play_time(
                35 * DAY,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("last month")
        );
        assert_eq!(
            format_play_time(
                400 * DAY,
                Location::GameLibraryLastPlayed,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("last year")
        );

        assert_eq!(
            format_play_time(
                25 * SEC,
                Location::VoiceChannelHistory,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("Just now")
        );
        assert_eq!(
            format_play_time(
                9 * MIN,
                Location::VoiceChannelHistory,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("9m ago")
        );
        assert_eq!(
            format_play_time(
                6 * HOUR,
                Location::VoiceChannelHistory,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("6h ago")
        );
        assert_eq!(
            format_play_time(
                3 * DAY,
                Location::VoiceChannelHistory,
                Kind::End,
                None,
                false
            )
            .as_deref(),
            Some("3d ago")
        );
    }
}
