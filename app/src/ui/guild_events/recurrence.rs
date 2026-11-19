//! The occurrences of a recurring event (`X7` and `er` of module 974930, the rrule the recurrence rule makes), for
//! the "Events in series" list of module 893110.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde_json::Value;

/// The rule's start, as the instant every occurrence keeps the time of.
fn rule_start(rule: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(rule["start"].as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn numbers(value: &Value) -> Option<Vec<u32>> {
    value.as_array().map(|list| {
        list.iter()
            .filter_map(|item| item.as_u64().and_then(|n| u32::try_from(n).ok()))
            .collect()
    })
}

/// Whether `day` is an occurrence of `rule` that started on `first` (frequencies 0 yearly, 1 monthly, 2 weekly,
/// 3 daily; weekdays count from Monday = 0; weeks start on Monday, the rrule default).
fn matches(rule: &Value, first: NaiveDate, day: NaiveDate) -> bool {
    let interval = rule["interval"].as_u64().unwrap_or(1).max(1) as i64;
    let weekday = day.weekday().num_days_from_monday();
    let by_weekday = numbers(&rule["by_weekday"]);
    let weekday_ok = by_weekday
        .as_ref()
        .is_none_or(|days| days.contains(&weekday));
    match rule["frequency"].as_u64() {
        Some(3) => (day - first).num_days() % interval == 0 && weekday_ok,
        Some(2) => {
            let week = |date: NaiveDate| {
                date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
            };
            let weeks = (week(day) - week(first)).num_days() / 7;
            let on = by_weekday.unwrap_or_else(|| vec![first.weekday().num_days_from_monday()]);
            weeks % interval == 0 && on.contains(&weekday)
        }
        Some(1) => {
            let months =
                (day.year() - first.year()) as i64 * 12 + day.month() as i64 - first.month() as i64;
            if months % interval != 0 {
                return false;
            }
            if let Some(nth) = rule["by_n_weekday"]
                .as_array()
                .filter(|list| !list.is_empty())
            {
                return nth.iter().any(|entry| {
                    let n = entry["n"].as_i64().unwrap_or(0);
                    let on = entry["day"].as_u64().unwrap_or(7) as u32;
                    on == weekday
                        && if n > 0 {
                            i64::from(day.day().div_ceil(7)) == n
                        } else {
                            // `-1`: the last such weekday of the month.
                            day.checked_add_signed(Duration::days(7))
                                .is_none_or(|next| next.month() != day.month())
                        }
                });
            }
            let days = numbers(&rule["by_month_day"]).unwrap_or_else(|| vec![first.day()]);
            days.contains(&day.day())
        }
        Some(0) => {
            let years = i64::from(day.year() - first.year());
            let months = numbers(&rule["by_month"]).unwrap_or_else(|| vec![first.month()]);
            let days = numbers(&rule["by_month_day"]).unwrap_or_else(|| vec![first.day()]);
            years % interval == 0 && months.contains(&day.month()) && days.contains(&day.day())
        }
        _ => false,
    }
}

/// `er(count, rrule, from, exclusive)`: the next `count` occurrences at or after `from` (after it when `exclusive`),
/// at the rule's time of day, none past the rule's `end` or its `count`, and none beyond five years from `from`.
pub(super) fn occurrences(
    rule: &Value,
    from: DateTime<Utc>,
    exclusive: bool,
    count: usize,
) -> Vec<DateTime<Utc>> {
    let Some(start) = rule_start(rule) else {
        return Vec::new();
    };
    let end = rule["end"]
        .as_str()
        .and_then(|end| DateTime::parse_from_rfc3339(end).ok())
        .map(|end| end.with_timezone(&Utc));
    let limit = rule["count"].as_u64().map(|n| n as usize);
    let time = start.time();
    let first = start.date_naive();
    let mut out = Vec::new();
    let mut seen = 0;
    let mut day = first;
    let last = from.date_naive() + Duration::days(366 * 5);
    while day <= last && out.len() < count {
        if matches(rule, first, day) {
            let at = day.and_time(time).and_utc();
            if end.is_some_and(|end| at > end) || limit.is_some_and(|limit| seen >= limit) {
                break;
            }
            seen += 1;
            if at > from || (!exclusive && at == from) {
                out.push(at);
            }
        }
        day += Duration::days(1);
    }
    out
}

/// The `canViewMoreRecurrences` test of module 893110: another occurrence follows the last shown within four years.
pub(super) fn has_more(rule: &Value, last: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    occurrences(rule, last, true, 1)
        .first()
        .is_some_and(|next| *next <= now + Duration::days(365 * 4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn weekly_and_every_other_week_keep_the_weekday_and_time() {
        // 2026-11-19 is a Thursday (3).
        let weekly = json!({"start": "2026-11-19T18:00:00.000Z", "frequency": 2, "interval": 1, "by_weekday": [3]});
        let next = occurrences(&weekly, at("2026-11-19T18:00:00Z"), false, 3);
        assert_eq!(
            next,
            vec![
                at("2026-11-19T18:00:00Z"),
                at("2026-11-26T18:00:00Z"),
                at("2026-12-03T18:00:00Z")
            ]
        );
        let other = json!({"start": "2026-11-19T18:00:00.000Z", "frequency": 2, "interval": 2, "by_weekday": [3]});
        let next = occurrences(&other, at("2026-11-19T18:00:00Z"), true, 2);
        assert_eq!(
            next,
            vec![at("2026-12-03T18:00:00Z"), at("2026-12-17T18:00:00Z")]
        );
    }

    #[test]
    fn monthly_nth_weekday_yearly_and_weekdays() {
        // The third Thursday of each month.
        let monthly = json!({"start": "2026-11-19T18:00:00.000Z", "frequency": 1, "interval": 1,
                             "by_n_weekday": [{"n": 3, "day": 3}]});
        let next = occurrences(&monthly, at("2026-11-20T00:00:00Z"), false, 2);
        assert_eq!(
            next,
            vec![at("2026-12-17T18:00:00Z"), at("2027-01-21T18:00:00Z")]
        );
        let yearly = json!({"start": "2026-11-19T18:00:00.000Z", "frequency": 0, "interval": 1,
                            "by_month": [11], "by_month_day": [19]});
        assert_eq!(
            occurrences(&yearly, at("2026-11-20T00:00:00Z"), false, 1),
            vec![at("2027-11-19T18:00:00Z")]
        );
        let weekdays = json!({"start": "2026-11-20T18:00:00.000Z", "frequency": 3, "interval": 1,
                              "by_weekday": [0, 1, 2, 3, 4]});
        assert_eq!(
            occurrences(&weekdays, at("2026-11-20T18:00:00Z"), true, 2),
            vec![at("2026-11-23T18:00:00Z"), at("2026-11-24T18:00:00Z")]
        );
    }

    #[test]
    fn the_end_and_count_stop_the_series() {
        let ended = json!({"start": "2026-11-19T18:00:00.000Z", "end": "2026-11-27T00:00:00.000Z",
                           "frequency": 3, "interval": 1, "count": null});
        assert_eq!(
            occurrences(&ended, at("2026-11-25T00:00:00Z"), false, 5).len(),
            2
        );
        let counted =
            json!({"start": "2026-11-19T18:00:00.000Z", "frequency": 3, "interval": 1, "count": 3});
        assert_eq!(
            occurrences(&counted, at("2026-11-19T00:00:00Z"), false, 9).len(),
            3
        );
        assert!(!has_more(
            &counted,
            at("2026-11-21T18:00:00Z"),
            at("2026-11-19T00:00:00Z")
        ));
        assert!(has_more(
            &ended,
            at("2026-11-25T18:00:00Z"),
            at("2026-11-19T00:00:00Z")
        ));
    }
}
