//! Dates and times as the official client writes them (module 5377).
//!
//! Moment's long-format tokens (`L`, `LL`, `LLL`, `LLLL`, `LT`, `LTS`, `L LT`) become one `dateStyle` / `timeStyle`
//! formatter (`longFormatters`): on the desktop the native `createDateFormatter` of `discord_utils`, which formats
//! with Windows' `GetDateFormatEx` / `GetTimeFormatEx` in the chosen locale (no locale, i.e. the user's own formats,
//! when the chosen locale is the one the client started in and shares the system's language), else
//! `Intl.DateTimeFormat`. The `hourCycle` follows Time format (`h12`, `h23`, or the locale's own for Auto). Everything
//! else in a format (the calendar's "Yesterday at", bracketed text) comes from the moment locale of the language
//! (module 608998 loads one per locale).
//!
//! - The message time's tooltip (module 776579): `delay: 750` and `i$(date, "LLLL")`.
//! - The message header (`C(timestamp, true)`, module 5377 `mk`): the time alone today, the calendar's `lastDay`
//!   yesterday, `L LT` before that, `nextDay` / `sameElse` ahead.
//! - The date divider (module 891522): `i$(timestamp, "LL")`.

use super::settings::prefs::{clock, clock_seconds, hour_cycle};
use chrono::{DateTime, Local, Timelike};

/// How long the pointer rests on the time before the tooltip opens.
pub(super) const DELAY_MS: u64 = 750;

/// `i$(date, "LLLL")`: "Wednesday, November 11, 2026 2:07 AM" in en-US.
pub(super) fn full_time(local: DateTime<Local>) -> String {
    long("LLLL", &local)
}

/// `i$(date, "LL")`, the date divider: "November 11, 2026" in en-US.
pub(super) fn long_date(local: DateTime<Local>) -> String {
    long("LL", &local)
}

/// `C(timestamp, true)`, the message header's time.
pub(super) fn header_time(local: DateTime<Local>, now: DateTime<Local>) -> String {
    calendar_time(local, now, true)
}

/// Module 5377 `mk` (`C`): `L LT` before yesterday, the calendar's `lastDay` yesterday, today the time alone
/// (`time_alone`) or `sameDay`, then `nextDay`, and `sameElse` ("L" in every moment locale the client loads).
pub(super) fn calendar_time(
    local: DateTime<Local>,
    now: DateTime<Local>,
    time_alone: bool,
) -> String {
    let days = (local.date_naive() - now.date_naive()).num_days();
    let kind = match days {
        ..-1 => return long("L LT", &local),
        -1 => Calendar::Last,
        0 if time_alone => return long("LT", &local),
        0 => Calendar::Same,
        1 => Calendar::Next,
        _ => return long("L", &local),
    };
    render(&calendar(dc_intl::locale(), kind, local.hour()), &local)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Calendar {
    Last,
    Same,
    Next,
}

/// The calendar format of a moment locale (`calendar.lastDay` / `sameDay` / `nextDay`); `hour` is the date's local
/// hour, which the Italian, Spanish, Greek and Ukrainian ones read.
pub(super) fn calendar(locale: &str, kind: Calendar, hour: u32) -> String {
    let pick = |last: &'static str, same: &'static str, next: &'static str| match kind {
        Calendar::Last => last,
        Calendar::Same => same,
        Calendar::Next => next,
    };
    let fixed = |text: &'static str| text.to_owned();
    match locale {
        "bg" => fixed(pick("[Вчера в] LT", "[Днес в] LT", "[Утре в] LT")),
        "cs" => fixed(pick("[včera v] LT", "[dnes v] LT", "[zítra v] LT")),
        "da" | "no" => fixed(pick(
            "[i går kl.] LT",
            "[i dag kl.] LT",
            "[i morgen kl.] LT",
        )),
        "de" => fixed(pick(
            "[gestern um] LT [Uhr]",
            "[heute um] LT [Uhr]",
            "[morgen um] LT [Uhr]",
        )),
        // Module 466155's `calendar`: "{}" is στη at one o'clock (hours % 12 == 1), else στις.
        "el" => pick("[Χθες {}] LT", "[Σήμερα {}] LT", "[Αύριο {}] LT")
            .replace("{}", if hour % 12 == 1 { "στη" } else { "στις" }),
        // Module 725226: "a la" at one o'clock, "a las" otherwise.
        "es-ES" | "es-419" => format!(
            "[{} a la{}] LT",
            pick("ayer", "hoy", "mañana"),
            if hour != 1 { "s" } else { "" }
        ),
        "fi" => fixed(pick(
            "[eilen] [klo] LT",
            "[tänään] [klo] LT",
            "[huomenna] [klo] LT",
        )),
        "fr" => fixed(pick("[Hier à] LT", "[Aujourd’hui à] LT", "[Demain à] LT")),
        "hr" => fixed(pick("[jučer u] LT", "[danas u] LT", "[sutra u] LT")),
        "hu" => fixed(pick(
            "[tegnap] LT[-kor]",
            "[ma] LT[-kor]",
            "[holnap] LT[-kor]",
        )),
        // Module 492519: "alle " after one o'clock, " " at midnight, "ll'" at one.
        "it" => format!(
            "[{} a{}]LT",
            pick("Ieri", "Oggi", "Domani"),
            if hour > 1 {
                "lle "
            } else if hour == 0 {
                " "
            } else {
                "ll'"
            }
        ),
        "ja" => fixed(pick("[昨日] LT", "[今日] LT", "[明日] LT")),
        "ko" => fixed(pick("어제 LT", "오늘 LT", "내일 LT")),
        "lt" => fixed(pick("[Vakar] LT", "[Šiandien] LT", "[Rytoj] LT")),
        "nl" => fixed(pick(
            "[gisteren om] LT",
            "[vandaag om] LT",
            "[morgen om] LT",
        )),
        "pl" => fixed(pick("[Wczoraj o] LT", "[Dziś o] LT", "[Jutro o] LT")),
        "pt-BR" => fixed(pick("[Ontem às] LT", "[Hoje às] LT", "[Amanhã às] LT")),
        "ro" => fixed(pick("[ieri la] LT", "[azi la] LT", "[mâine la] LT")),
        "ru" => fixed(pick("[Вчера, в] LT", "[Сегодня, в] LT", "[Завтра, в] LT")),
        "sv-SE" => fixed(pick("[Igår] LT", "[Idag] LT", "[Imorgon] LT")),
        "th" => fixed(pick("[เมื่อวานนี้ เวลา] LT", "[วันนี้ เวลา] LT", "[พรุ่งนี้ เวลา] LT")),
        "tr" => fixed(pick("[dün] LT", "[bugün saat] LT", "[yarın saat] LT")),
        // Module 95122's `t`: "об" at eleven o'clock, else "о".
        "uk" => format!(
            "[{} о{}] LT",
            pick("Вчора", "Сьогодні", "Завтра"),
            if hour == 11 { "б" } else { "" }
        ),
        "vi" => fixed(pick(
            "[Hôm qua lúc] LT",
            "[Hôm nay lúc] LT",
            "[Ngày mai lúc] LT",
        )),
        "zh-CN" => fixed(pick("[昨天]LT", "[今天]LT", "[明天]LT")),
        "zh-TW" => fixed(pick("[昨天] LT", "[今天] LT", "[明天] LT")),
        "hi" => fixed(pick("[कल] LT", "[आज] LT", "[कल] LT")),
        // moment's own `en` (module 536637), also en-GB's (module 594091).
        _ => fixed(pick(
            "[Yesterday at] LT",
            "[Today at] LT",
            "[Tomorrow at] LT",
        )),
    }
}

/// A moment format of bracketed text, long-format tokens and literal characters (module 5377 `N`; the calendar
/// formats use no other tokens).
fn render(format: &str, local: &DateTime<Local>) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(ch) = rest.chars().next() {
        if ch == '[' {
            match rest.find(']') {
                Some(end) => {
                    out.push_str(&rest[1..end]);
                    rest = &rest[end + 1..];
                }
                None => {
                    out.push('[');
                    rest = &rest[1..];
                }
            }
            continue;
        }
        if ch == 'L' {
            let token = long_token(rest);
            out.push_str(&long(token, local));
            rest = &rest[token.len()..];
            continue;
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// `/^L(?:TS?|L*(?: LTS?)?)/`.
fn long_token(text: &str) -> &str {
    for token in [
        "LLLL LTS", "LLLL LT", "LLL LTS", "LLL LT", "LL LTS", "LL LT", "L LTS", "L LT", "LLLL",
        "LLL", "LTS", "LL", "LT",
    ] {
        if text.starts_with(token) {
            return &text[..token.len()];
        }
    }
    "L"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DateStyle {
    Full,
    Long,
    Short,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeStyle {
    Short,
    Medium,
}

/// The styles of a long-format token (module 5377's `longFormatters`).
fn styles(token: &str) -> (Option<DateStyle>, Option<TimeStyle>) {
    match token {
        "LLLL" => (Some(DateStyle::Full), Some(TimeStyle::Short)),
        "LLL" => (Some(DateStyle::Long), Some(TimeStyle::Short)),
        "LL" => (Some(DateStyle::Long), None),
        "L" => (Some(DateStyle::Short), None),
        "LT" => (None, Some(TimeStyle::Short)),
        "LTS" => (None, Some(TimeStyle::Medium)),
        "L LT" => (Some(DateStyle::Short), Some(TimeStyle::Short)),
        _ => (Some(DateStyle::Short), Some(TimeStyle::Medium)),
    }
}

/// One long-format token in the current locale.
pub(super) fn long(token: &str, local: &DateTime<Local>) -> String {
    let (date, time) = styles(token);
    #[cfg(windows)]
    if let Some(text) = native::format(native_locale().as_deref(), date, time, local) {
        return text;
    }
    fallback(date, time, local)
}

/// The locale the native formatter is given: none (the user's own formats) when the chosen locale is the one the
/// client started in and its language is the system's (module 5377: `e === initialLocale && language of e ===
/// language of systemLocale`).
#[cfg(windows)]
fn native_locale() -> Option<String> {
    let chosen = dc_intl::locale();
    let initial = crate::intl::initial_locale();
    let system = sys_locale::get_locale().unwrap_or_default();
    let language = |tag: &str| tag.split('-').next().unwrap_or_default().to_owned();
    if chosen == initial && language(chosen) == language(&system) {
        None
    } else {
        Some(chosen.to_owned())
    }
}

/// en-US's formats, for a platform or locale the native formatter cannot write.
fn fallback(date: Option<DateStyle>, time: Option<TimeStyle>, local: &DateTime<Local>) -> String {
    let date = date.map(|style| {
        local
            .format(match style {
                DateStyle::Full => "%A, %B %-d, %Y",
                DateStyle::Long => "%B %-d, %Y",
                DateStyle::Short => "%-m/%-d/%Y",
            })
            .to_string()
    });
    let time = time.map(|style| match style {
        TimeStyle::Short => clock(local),
        TimeStyle::Medium => clock_seconds(local),
    });
    match (date, time) {
        (Some(date), Some(time)) => format!("{date} {time}"),
        (Some(text), None) | (None, Some(text)) => text,
        (None, None) => String::new(),
    }
}

#[cfg(windows)]
mod native {
    use super::{DateStyle, TimeStyle, hour_cycle};
    use chrono::{DateTime, Datelike, Local, Timelike};
    use windows::Win32::Foundation::{LPARAM, SYSTEMTIME};
    use windows::Win32::Globalization::{
        DATE_LONGDATE, DATE_SHORTDATE, ENUM_DATE_FORMATS_FLAGS, EnumDateFormatsExEx,
        GetDateFormatEx, GetTimeFormatEx, TIME_FORCE24HOURFORMAT, TIME_FORMAT_FLAGS,
        TIME_NOSECONDS, TIME_NOTIMEMARKER,
    };
    use windows::core::{BOOL, PCWSTR};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn system_time(local: &DateTime<Local>) -> SYSTEMTIME {
        SYSTEMTIME {
            wYear: local.year() as u16,
            wMonth: local.month() as u16,
            wDayOfWeek: local.weekday().num_days_from_sunday() as u16,
            wDay: local.day() as u16,
            wHour: local.hour() as u16,
            wMinute: local.minute() as u16,
            wSecond: local.second() as u16,
            wMilliseconds: 0,
        }
    }

    fn text(buffer: &[u16], written: i32) -> Option<String> {
        (written > 1).then(|| String::from_utf16_lossy(&buffer[..written as usize - 1]))
    }

    fn date(
        locale: PCWSTR,
        flags: ENUM_DATE_FORMATS_FLAGS,
        picture: Option<&str>,
        at: &SYSTEMTIME,
    ) -> Option<String> {
        let picture = picture.map(wide);
        let picture = picture
            .as_ref()
            .map_or(PCWSTR::null(), |picture| PCWSTR(picture.as_ptr()));
        let flags = if picture.is_null() {
            flags
        } else {
            ENUM_DATE_FORMATS_FLAGS(0)
        };
        let mut buffer = [0u16; 256];
        // SAFETY: the strings are NUL-terminated and outlive the call; the buffer's length is passed with it.
        let written = unsafe {
            GetDateFormatEx(
                locale,
                flags,
                Some(at),
                picture,
                Some(&mut buffer),
                PCWSTR::null(),
            )
        };
        text(&buffer, written)
    }

    fn time(locale: PCWSTR, flags: TIME_FORMAT_FLAGS, at: &SYSTEMTIME) -> Option<String> {
        let mut buffer = [0u16; 128];
        // SAFETY: as above.
        let written =
            unsafe { GetTimeFormatEx(locale, flags, Some(at), PCWSTR::null(), Some(&mut buffer)) };
        text(&buffer, written)
    }

    unsafe extern "system" fn collect(picture: PCWSTR, _calendar: u32, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the `Vec` `long_pictures` passes, alive for the enumeration; `picture` is a
        // NUL-terminated string the system owns for the call.
        unsafe {
            let out = &mut *(out.0 as *mut Vec<String>);
            if let Ok(text) = picture.to_string() {
                out.push(text);
            }
        }
        BOOL(1)
    }

    /// The locale's long date pictures (`EnumDateFormatsExEx`, `DATE_LONGDATE`), its default first.
    fn long_pictures(locale: PCWSTR) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // SAFETY: `collect` reads `out` only during the call.
        let _ = unsafe {
            EnumDateFormatsExEx(
                Some(collect),
                locale,
                DATE_LONGDATE,
                LPARAM(&mut out as *mut Vec<String> as isize),
            )
        };
        out
    }

    pub(super) fn format(
        locale: Option<&str>,
        date_style: Option<DateStyle>,
        time_style: Option<TimeStyle>,
        local: &DateTime<Local>,
    ) -> Option<String> {
        let name = locale.map(wide);
        let locale = name
            .as_ref()
            .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
        let at = system_time(local);
        let date = match date_style {
            None => None,
            Some(DateStyle::Short) => Some(date(locale, DATE_SHORTDATE, None, &at)?),
            Some(DateStyle::Full) => Some(date(locale, DATE_LONGDATE, None, &at)?),
            // The long date without the weekday: the first of the locale's long pictures that names no day.
            Some(DateStyle::Long) => {
                let pictures = long_pictures(locale);
                let plain = pictures.iter().find(|picture| !picture.contains("ddd"));
                Some(date(locale, DATE_LONGDATE, plain.map(String::as_str), &at)?)
            }
        };
        let time = match time_style {
            None => None,
            Some(style) => {
                let mut flags = match style {
                    TimeStyle::Short => TIME_NOSECONDS,
                    TimeStyle::Medium => TIME_FORMAT_FLAGS(0),
                };
                if hour_cycle() == Some(true) {
                    flags = flags | TIME_FORCE24HOURFORMAT | TIME_NOTIMEMARKER;
                }
                Some(time(locale, flags, &at)?)
            }
        };
        Some(match (date, time) {
            (Some(date), Some(time)) => format!("{date} {time}"),
            (Some(text), None) | (None, Some(text)) => text,
            (None, None) => String::new(),
        })
    }
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

    #[test]
    fn the_divider_is_the_long_date_without_the_weekday() {
        let at = Local.with_ymd_and_hms(2026, 11, 8, 9, 5, 0).unwrap();
        assert_eq!(long_date(at), "November 8, 2026");
    }

    #[test]
    fn calendar_formats_follow_the_moment_locales() {
        assert_eq!(calendar("en-US", Calendar::Last, 9), "[Yesterday at] LT");
        assert_eq!(calendar("tr", Calendar::Last, 9), "[dün] LT");
        assert_eq!(calendar("ja", Calendar::Last, 9), "[昨日] LT");
        assert_eq!(calendar("es-419", Calendar::Last, 1), "[ayer a la] LT");
        assert_eq!(calendar("es-ES", Calendar::Last, 9), "[ayer a las] LT");
        assert_eq!(calendar("it", Calendar::Last, 0), "[Ieri a ]LT");
        assert_eq!(calendar("it", Calendar::Last, 1), "[Ieri all']LT");
        assert_eq!(calendar("el", Calendar::Last, 13), "[Χθες στη] LT");
        assert_eq!(calendar("uk", Calendar::Last, 11), "[Вчора об] LT");
        assert_eq!(calendar("de", Calendar::Last, 9), "[gestern um] LT [Uhr]");
    }

    #[test]
    fn a_format_keeps_its_bracketed_text_and_replaces_its_tokens() {
        let at = Local.with_ymd_and_hms(2026, 11, 7, 22, 26, 0).unwrap();
        let rendered = render("[Yesterday at] LT", &at);
        assert_eq!(rendered, format!("Yesterday at {}", long("LT", &at)));
        assert_eq!(long_token("L LT x"), "L LT");
        assert_eq!(long_token("LT[-kor]"), "LT");
        assert_eq!(long_token("LLLL"), "LLLL");
    }

    #[test]
    fn header_times_follow_the_calendar_rule() {
        let now = Local.with_ymd_and_hms(2026, 11, 8, 1, 0, 0).unwrap();
        let at = |day, hour, minute| {
            Local
                .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
                .unwrap()
        };
        assert_eq!(header_time(at(8, 0, 30), now), "12:30 AM");
        assert_eq!(header_time(at(7, 22, 26), now), "Yesterday at 10:26 PM");
        assert_eq!(header_time(at(5, 9, 5), now), "11/5/2026 9:05 AM");
    }
}
