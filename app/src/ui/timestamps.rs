//! The tooltip of a message's time (module 776579): `delay: 750` and the text `i$(date, "LLLL")`, Moment's long date
//! with the weekday for the `en-US` locale, "Wednesday, November 11, 2026 2:07 AM", the hour written by the clock setting.

use super::settings::prefs::clock;
use chrono::{DateTime, Local};

/// How long the pointer rests on the time before the tooltip opens.
pub(super) const DELAY_MS: u64 = 750;

pub(super) fn full_time(local: DateTime<Local>) -> String {
    format!("{} {}", local.format("%A, %B %-d, %Y"), clock(&local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn the_long_form_names_the_weekday_and_does_not_pad_the_day() {
        let at = Local.with_ymd_and_hms(2026, 11, 4, 14, 17, 0).unwrap();
        let text = full_time(at);
        assert!(text.starts_with("Wednesday, November 4, 2026 "), "{text}");
    }
}
