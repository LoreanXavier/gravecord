//! The date and time logic of the Create Event flow, ported from the official modules: the default start (`jd` of
//! 688218), the time picker's 15-minute options and its parser (201895), the repeat options and the recurrence rule
//! (`Xx` and `nG`/`RX` of 688218), the schedule validation, the review card's relative day (`CC` of 974930) and the
//! request times (UTC, millisecond ISO).

use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike,
    Utc, Weekday,
};
use serde_json::{Value, json};

/// `jd()`: an hour from now, on the hour, one more hour when the half hour had passed.
pub(super) fn default_start(now: NaiveDateTime) -> NaiveDateTime {
    let later = now + Duration::hours(1);
    let mut start = later
        .with_minute(0)
        .and_then(|time| time.with_second(0))
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(later);
    if later.minute() >= 30 {
        start += Duration::hours(1);
    }
    start
}

/// `moment.format("ll")`: `Nov 10, 2026`.
pub(in super::super) fn date_label(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
}

/// `moment.format("LT")`: `2:00 PM`.
pub(in super::super) fn time_label(time: NaiveTime) -> String {
    let (pm, hour) = time.hour12();
    format!(
        "{hour}:{:02} {}",
        time.minute(),
        if pm { "PM" } else { "AM" }
    )
}

/// The time picker's options: every 15 minutes of the day.
pub(in super::super) fn time_options() -> Vec<NaiveTime> {
    (0..96)
        .filter_map(|slot| NaiveTime::from_hms_opt(slot / 4, (slot % 4) * 15, 0))
        .collect()
}

/// A time typed into the picker: `2:30 PM`, `2pm`, `14:30`, `2.30`, `230 pm`. Without a meridiem the typed hour is
/// taken as written (`2` is 02:00, `14` is 14:00).
pub(in super::super) fn parse_time(text: &str) -> Option<NaiveTime> {
    let lower = text.trim().to_lowercase().replace(' ', "");
    let (digits, meridiem) = match lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        Some(rest) => (rest, Some(false)),
        None => match lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
            Some(rest) => (rest, Some(true)),
            None => (lower.as_str(), None),
        },
    };
    let (hour, minute) = match digits.split_once([':', '.']) {
        Some((hour, minute)) => (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?),
        None if digits.len() > 2 => {
            let split = digits.len() - 2;
            (
                digits[..split].parse::<u32>().ok()?,
                digits[split..].parse::<u32>().ok()?,
            )
        }
        None => (digits.parse::<u32>().ok()?, 0),
    };
    if minute > 59 {
        return None;
    }
    let hour = match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour % 12 + if pm { 12 } else { 0 }
        }
        None => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// The picker's list narrowed to the options whose label holds what was typed: those that start with it first, then
/// the others, each in clock order (an approximation of the official match-sorter ranking).
pub(in super::super) fn filter_times(options: &[NaiveTime], typed: &str) -> Vec<NaiveTime> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return options.to_vec();
    }
    let labelled = |time: &NaiveTime| time_label(*time).to_lowercase();
    let starting = options
        .iter()
        .copied()
        .filter(|time| labelled(time).starts_with(&typed));
    let containing = options.iter().copied().filter(|time| {
        let label = labelled(time);
        !label.starts_with(&typed) && label.contains(&typed)
    });
    starting.chain(containing).collect()
}

/// The recurrence frequencies the dropdown offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Frequency {
    None,
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
    Daily,
    Weekday,
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn nth_word(nth: u32) -> &'static str {
    match nth {
        1 => "first",
        2 => "second",
        3 => "third",
        4 => "fourth",
        _ => "fifth",
    }
}

/// `Xx`: the options for a start date, with the strings of 0bK0B1, B8/yfp, z+aIuX, mjOEBk, 5DFcVl, JX8E1E and
/// jYR/MY. The weekday option is left out on a weekend (the weekend option is staff only).
pub(super) fn frequency_options(start: NaiveDate) -> Vec<(Frequency, String)> {
    let weekday = weekday_name(start.weekday());
    let mut options = vec![
        (Frequency::None, "Does not repeat".to_owned()),
        (Frequency::Weekly, format!("Weekly on {weekday}")),
        (Frequency::Biweekly, format!("Every other {weekday}")),
        (
            Frequency::Monthly,
            format!(
                "Monthly on the {} {weekday}",
                nth_word(start.day().div_ceil(7))
            ),
        ),
        (
            Frequency::Yearly,
            format!("Annually on {}", start.format("%b %d")),
        ),
        (Frequency::Daily, "Every day".to_owned()),
    ];
    if !matches!(start.weekday(), Weekday::Sat | Weekday::Sun) {
        options.push((
            Frequency::Weekday,
            "Every weekday (Monday to Friday)".to_owned(),
        ));
    }
    options
}

/// The local time as an instant.
pub(super) fn to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&local)
        .earliest()
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&local))
}

/// `toISOString()`: `2026-11-10T14:00:00.000Z`.
pub(in super::super) fn iso_utc(local: NaiveDateTime) -> String {
    to_utc(local).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// `nG` and `RX`: the recurrence rule body for a start, or null for "Does not repeat". Weekdays count from Monday = 0
/// and the date parts are the UTC ones, as the official builds them from the UTC start.
pub(super) fn recurrence_rule(frequency: Frequency, start: NaiveDateTime) -> Value {
    if frequency == Frequency::None {
        return Value::Null;
    }
    let utc = to_utc(start);
    let weekday = utc.weekday().num_days_from_monday();
    let local_weekday = start.weekday().num_days_from_monday();
    let mut rule = json!({
        "start": iso_utc(start),
        "end": null,
        "frequency": 2,
        "interval": 1,
        "by_weekday": null,
        "by_n_weekday": null,
        "by_month": null,
        "by_month_day": null,
        "by_year_day": null,
        "count": null,
    });
    match frequency {
        Frequency::None => {}
        Frequency::Weekly => rule["by_weekday"] = json!([weekday]),
        Frequency::Biweekly => {
            rule["interval"] = json!(2);
            rule["by_weekday"] = json!([weekday]);
        }
        Frequency::Monthly => {
            rule["frequency"] = json!(1);
            rule["by_n_weekday"] = json!([{"n": utc.day().div_ceil(7), "day": weekday}]);
        }
        Frequency::Yearly => {
            rule["frequency"] = json!(0);
            rule["by_month"] = json!([utc.month()]);
            rule["by_month_day"] = json!([utc.day()]);
        }
        Frequency::Daily => rule["frequency"] = json!(3),
        Frequency::Weekday => {
            rule["frequency"] = json!(3);
            rule["by_weekday"] = if weekday > local_weekday {
                json!([1, 2, 3, 4, 5])
            } else if weekday < local_weekday {
                json!([6, 0, 1, 2, 3])
            } else {
                json!([0, 1, 2, 3, 4])
            };
        }
    }
    rule
}

/// The option of the dropdown that a stored rule is, or none for a rule the dropdown cannot say.
pub(super) fn frequency_of(rule: &Value, start: NaiveDateTime) -> Option<Frequency> {
    if rule.is_null() {
        return Some(Frequency::None);
    }
    [
        Frequency::Weekly,
        Frequency::Biweekly,
        Frequency::Monthly,
        Frequency::Yearly,
        Frequency::Daily,
        Frequency::Weekday,
    ]
    .into_iter()
    .find(|frequency| {
        let known = recurrence_rule(*frequency, start);
        [
            "frequency",
            "interval",
            "by_weekday",
            "by_n_weekday",
            "by_month",
            "by_month_day",
        ]
        .iter()
        .all(|key| known[*key] == rule[*key])
    })
}

/// What is wrong with the schedule, in the order the official reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScheduleError {
    NoStart,
    EndRequired,
    StartPast,
    EndBeforeStart,
    EndPast,
}

impl ScheduleError {
    /// M73YyN, H16p/w, AXR5Ss, LpjF4K and ViDcm2.
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::NoStart => "Events must have a start date and time.",
            Self::EndRequired => "End date and time are required.",
            Self::StartPast => "The start time must be in the future.",
            Self::EndBeforeStart => "The end time must be after the start time.",
            Self::EndPast => "The end time must be in the future.",
        }
    }
}

#[cfg(test)]
pub(super) fn schedule_error(
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
    end_required: bool,
    now: NaiveDateTime,
) -> Option<ScheduleError> {
    schedule_error_of(start, end, end_required, now, false)
}

/// `schedule_error`, where an event being edited may have started already (`n` of the validation in module 688218).
pub(super) fn schedule_error_of(
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
    end_required: bool,
    now: NaiveDateTime,
    editing: bool,
) -> Option<ScheduleError> {
    let Some(start) = start else {
        return Some(ScheduleError::NoStart);
    };
    if end_required && end.is_none() {
        return Some(ScheduleError::EndRequired);
    }
    if !editing && start < now {
        return Some(ScheduleError::StartPast);
    }
    if let Some(end) = end {
        if end < start {
            return Some(ScheduleError::EndBeforeStart);
        }
        if end < now {
            return Some(ScheduleError::EndPast);
        }
    }
    None
}

fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// `moment(text)` in the local zone: an RFC 3339 time of the API as the wall clock shows it.
pub(super) fn local_time(text: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Local).naive_local())
}

/// `S`: `Tue Nov 10th · 2:00 PM`, with the year when it is not this one.
fn full_label(time: NaiveDateTime, now: NaiveDateTime) -> String {
    let year = if time.year() == now.year() {
        String::new()
    } else {
        format!(", {}", time.year())
    };
    format!(
        "{} {} {}{year} \u{b7} {}",
        time.format("%a"),
        time.format("%b"),
        ordinal(time.day()),
        time_label(time.time())
    )
}

/// `CC`: the card's date line, `Today at 2:00 PM`, `Tomorrow at ...`, else `Tue Nov 10th · 2:00 PM` (with the year
/// when it is not this one).
pub(super) fn relative_label(start: NaiveDateTime, now: NaiveDateTime) -> String {
    let time = time_label(start.time());
    match (start.date() - now.date()).num_days() {
        0 => format!("Today at {time}"),
        1 => format!("Tomorrow at {time}"),
        _ => full_label(start, now),
    }
}

/// What `CC` of module 974930 answers for a start and an end.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TimeStrings {
    pub start: String,
    /// The end: its time alone when it is on the start's day, else the full label.
    pub end: Option<String>,
    pub current_or_past: bool,
    pub upcoming: bool,
    pub diff_minutes: i64,
}

pub(super) fn time_strings(
    start: NaiveDateTime,
    end: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> TimeStrings {
    TimeStrings {
        start: relative_label(start, now),
        end: end.map(|end| {
            if end.date() == start.date() {
                time_label(end.time())
            } else {
                full_label(end, now)
            }
        }),
        current_or_past: start <= now,
        upcoming: start <= now + Duration::hours(1),
        diff_minutes: (start - now).num_minutes(),
    }
}

/// How the status line of an event reads and is tinted (`D` of module 103355).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Tone {
    /// Upcoming: a brand icon, the text in the default colour.
    Plain,
    /// Starting soon or running late: the text is brand coloured too.
    Soon,
    /// Running now: green.
    Live,
    /// Completed.
    Over,
    /// Canceled: a critical icon.
    Canceled,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Status {
    pub tone: Tone,
    pub text: String,
    /// For a running event with an end: the green lead of the text, which `text` then continues.
    pub lead: Option<String>,
}

/// `z` and `R` of module 103355: the tone and the words of the status line. `with_end` is whether the caller shows the
/// end (the detail does, the card does not); `external` picks "Happening Now" over "Live Now".
pub(super) fn status_line(
    strings: &TimeStrings,
    status: u64,
    external: bool,
    with_end: bool,
) -> Status {
    use dc_state::scheduled_events::{STATUS_ACTIVE, STATUS_CANCELED, STATUS_COMPLETED};
    let end = if with_end { strings.end.clone() } else { None };
    let joined = match &end {
        Some(end) => format!("{} \u{2014} {end}", strings.start),
        None => strings.start.clone(),
    };
    let tone = if status == STATUS_CANCELED {
        Tone::Canceled
    } else if status == STATUS_ACTIVE {
        Tone::Live
    } else if status == STATUS_COMPLETED {
        Tone::Over
    } else if strings.current_or_past || strings.upcoming {
        Tone::Soon
    } else {
        Tone::Plain
    };
    let live = if external {
        "Happening Now"
    } else {
        "Live Now"
    };
    match tone {
        Tone::Soon if strings.current_or_past => Status {
            tone,
            text: "Starting soon".to_owned(),
            lead: None,
        },
        Tone::Soon => Status {
            tone,
            text: if strings.diff_minutes > 0 {
                format!("Starting in {}m", strings.diff_minutes)
            } else {
                "Starting soon".to_owned()
            },
            lead: None,
        },
        Tone::Live => match end {
            Some(end) => Status {
                tone,
                text: format!(" \u{2014} Ends {end}"),
                lead: Some(live.to_owned()),
            },
            None => Status {
                tone,
                text: live.to_owned(),
                lead: None,
            },
        },
        _ => Status {
            tone,
            text: joined,
            lead: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn the_default_start_is_the_next_hour_or_the_one_after() {
        assert_eq!(
            default_start(at(2026, 11, 10, 13, 10)),
            at(2026, 11, 10, 14, 0)
        );
        assert_eq!(
            default_start(at(2026, 11, 10, 13, 40)),
            at(2026, 11, 10, 15, 0)
        );
        assert_eq!(
            default_start(at(2026, 11, 10, 23, 45)),
            at(2026, 11, 11, 1, 0)
        );
    }

    #[test]
    fn labels_follow_the_moment_formats() {
        assert_eq!(
            date_label(NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()),
            "Nov 10, 2026"
        );
        assert_eq!(
            date_label(NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()),
            "Nov 5, 2026"
        );
        assert_eq!(
            time_label(NaiveTime::from_hms_opt(14, 0, 0).unwrap()),
            "2:00 PM"
        );
        assert_eq!(
            time_label(NaiveTime::from_hms_opt(0, 15, 0).unwrap()),
            "12:15 AM"
        );
        assert_eq!(
            time_label(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
            "12:00 PM"
        );
        assert_eq!(time_options().len(), 96);
    }

    #[test]
    fn typed_times_are_parsed() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0);
        assert_eq!(parse_time("2:30 PM"), t(14, 30));
        assert_eq!(parse_time("2pm"), t(14, 0));
        assert_eq!(parse_time("12am"), t(0, 0));
        assert_eq!(parse_time("14:30"), t(14, 30));
        assert_eq!(parse_time("2.45"), t(2, 45));
        assert_eq!(parse_time("930pm"), t(21, 30));
        assert_eq!(parse_time("13pm"), None);
        assert_eq!(parse_time("2:75"), None);
        assert_eq!(parse_time("soon"), None);
        let filtered = filter_times(&time_options(), "2:30 p");
        assert_eq!(
            filtered,
            vec![
                NaiveTime::from_hms_opt(14, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(12, 30, 0).unwrap(),
            ]
        );
    }

    #[test]
    fn repeat_options_name_the_start_date() {
        let labels: Vec<String> = frequency_options(NaiveDate::from_ymd_opt(2026, 11, 10).unwrap())
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert_eq!(
            labels,
            [
                "Does not repeat",
                "Weekly on Tuesday",
                "Every other Tuesday",
                "Monthly on the second Tuesday",
                "Annually on Nov 10",
                "Every day",
                "Every weekday (Monday to Friday)",
            ]
        );
        // A Saturday offers no weekday option.
        assert_eq!(
            frequency_options(NaiveDate::from_ymd_opt(2026, 11, 14).unwrap()).len(),
            6
        );
    }

    #[test]
    fn rules_follow_the_start() {
        let start = at(2026, 11, 10, 12, 0);
        assert_eq!(recurrence_rule(Frequency::None, start), Value::Null);
        let weekly = recurrence_rule(Frequency::Weekly, start);
        assert_eq!(weekly["frequency"], 2);
        assert_eq!(weekly["interval"], 1);
        assert_eq!(weekly["by_weekday"].as_array().map(Vec::len), Some(1));
        let biweekly = recurrence_rule(Frequency::Biweekly, start);
        assert_eq!(biweekly["interval"], 2);
        let monthly = recurrence_rule(Frequency::Monthly, start);
        assert_eq!(monthly["frequency"], 1);
        assert_eq!(monthly["by_n_weekday"][0]["n"], 2);
        let yearly = recurrence_rule(Frequency::Yearly, start);
        assert_eq!(yearly["frequency"], 0);
        assert_eq!(yearly["by_month"], json!([11]));
        assert_eq!(yearly["by_month_day"], json!([10]));
        assert_eq!(recurrence_rule(Frequency::Daily, start)["frequency"], 3);
    }

    #[test]
    fn the_schedule_is_checked_in_the_official_order() {
        let now = at(2026, 11, 10, 13, 10);
        let start = at(2026, 11, 10, 14, 0);
        assert_eq!(
            schedule_error(None, None, false, now),
            Some(ScheduleError::NoStart)
        );
        assert_eq!(
            schedule_error(Some(start), None, true, now),
            Some(ScheduleError::EndRequired)
        );
        assert_eq!(schedule_error(Some(start), None, false, now), None);
        assert_eq!(
            schedule_error(Some(at(2026, 11, 10, 9, 0)), None, false, now),
            Some(ScheduleError::StartPast)
        );
        assert_eq!(
            schedule_error(Some(start), Some(at(2026, 11, 10, 13, 0)), false, now),
            Some(ScheduleError::EndBeforeStart)
        );
        assert_eq!(
            schedule_error(Some(start), Some(at(2026, 11, 10, 16, 0)), true, now),
            None
        );
    }

    #[test]
    fn the_card_names_the_day() {
        let now = at(2026, 11, 10, 13, 10);
        assert_eq!(
            relative_label(at(2026, 11, 10, 14, 0), now),
            "Today at 2:00 PM"
        );
        assert_eq!(
            relative_label(at(2026, 11, 11, 9, 30), now),
            "Tomorrow at 9:30 AM"
        );
        assert_eq!(
            relative_label(at(2026, 11, 13, 9, 30), now),
            "Fri Nov 13th \u{b7} 9:30 AM"
        );
        assert_eq!(
            relative_label(at(2027, 1, 2, 9, 30), now),
            "Sat Jan 2nd, 2027 \u{b7} 9:30 AM"
        );
    }

    #[test]
    fn request_times_are_utc_with_milliseconds() {
        let text = iso_utc(at(2026, 11, 10, 14, 0));
        assert!(text.ends_with(":00.000Z"), "{text}");
        assert_eq!(text.len(), 24);
    }

    #[test]
    fn a_stored_rule_is_found_among_the_options() {
        let start = at(2026, 11, 11, 14, 0);
        for frequency in [
            Frequency::Weekly,
            Frequency::Biweekly,
            Frequency::Monthly,
            Frequency::Yearly,
            Frequency::Daily,
            Frequency::Weekday,
        ] {
            assert_eq!(
                frequency_of(&recurrence_rule(frequency, start), start),
                Some(frequency)
            );
        }
        assert_eq!(frequency_of(&Value::Null, start), Some(Frequency::None));
        let mut odd = recurrence_rule(Frequency::Weekly, start);
        odd["by_weekday"] = json!([0, 2]);
        assert_eq!(frequency_of(&odd, start), None);
    }

    #[test]
    fn an_edited_event_may_have_started() {
        let now = at(2026, 11, 10, 13, 10);
        let started = at(2026, 11, 10, 9, 0);
        assert_eq!(
            schedule_error(Some(started), None, false, now),
            Some(ScheduleError::StartPast)
        );
        assert_eq!(
            schedule_error_of(Some(started), None, false, now, true),
            None
        );
        assert_eq!(
            schedule_error_of(
                Some(started),
                Some(at(2026, 11, 10, 10, 0)),
                false,
                now,
                true
            ),
            Some(ScheduleError::EndPast)
        );
    }

    #[test]
    fn the_status_line_follows_the_card_rules() {
        let now = at(2026, 11, 10, 13, 10);
        let strings = time_strings(at(2026, 11, 11, 14, 0), Some(at(2026, 11, 11, 16, 0)), now);
        assert_eq!(strings.start, "Tomorrow at 2:00 PM");
        assert_eq!(strings.end.as_deref(), Some("4:00 PM"));
        let card = status_line(&strings, 1, true, false);
        assert_eq!(
            (card.tone, card.text.as_str()),
            (Tone::Plain, "Tomorrow at 2:00 PM")
        );
        let detail = status_line(&strings, 1, true, true);
        assert_eq!(detail.text, "Tomorrow at 2:00 PM \u{2014} 4:00 PM");
        let soon = time_strings(at(2026, 11, 10, 13, 40), None, now);
        assert_eq!(status_line(&soon, 1, true, false).text, "Starting in 30m");
        let late = time_strings(at(2026, 11, 10, 13, 0), None, now);
        assert_eq!(status_line(&late, 1, true, false).text, "Starting soon");
        let live = status_line(&late, 2, true, false);
        assert_eq!(
            (live.tone, live.text.as_str()),
            (Tone::Live, "Happening Now")
        );
        let far = time_strings(at(2026, 11, 20, 9, 0), Some(at(2026, 11, 21, 9, 0)), now);
        assert_eq!(far.start, "Fri Nov 20th \u{b7} 9:00 AM");
        assert_eq!(far.end.as_deref(), Some("Sat Nov 21st \u{b7} 9:00 AM"));
    }
}
