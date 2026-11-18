//! Formatter tests against real messages of the pinned build's catalogs. The foreign catalogs are read from
//! `design/snapshot/dumps/STRINGS/` (local only, `tools/dump-strings.mjs`); en-US is the compiled-in one.

use super::*;
use std::path::PathBuf;

fn dump(locale: &str) -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../design/snapshot/dumps/STRINGS")
        .join(format!("{locale}.json"));
    match std::fs::read_to_string(&path) {
        Ok(json) => Some(json),
        Err(_) => {
            eprintln!("{} is missing: run tools/dump-strings.mjs", path.display());
            None
        }
    }
}

fn catalog(locale: &str) -> Option<&'static Catalog> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, &'static Catalog>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(found) = cache.get(locale) {
        return Some(found);
    }
    let parsed: &'static Catalog =
        Box::leak(Box::new(Catalog::parse(locale, &[&dump(locale)?]).unwrap()));
    cache.insert(locale.to_owned(), parsed);
    Some(parsed)
}

/// A locale's catalogs of every loader, read from the pinned build's chunk files in `design/snapshot/assets/`.
fn every_catalog(locale: &str) -> Option<&'static Catalog> {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../design/snapshot/assets");
    let mut jsons = Vec::new();
    for file in chunk_files(locale) {
        let Ok(source) = std::fs::read_to_string(assets.join(file)) else {
            eprintln!("{file} is missing from design/snapshot/assets");
            return None;
        };
        jsons.push(catalog_json(&source).unwrap_or_else(|| panic!("{file} holds a catalog")));
    }
    let jsons: Vec<&str> = jsons.iter().map(String::as_str).collect();
    Some(Box::leak(Box::new(Catalog::parse(locale, &jsons).unwrap())))
}

fn plain_in(locale: &str, key: &str, values: &Args<'_>) -> Option<String> {
    let catalog = if locale == DEFAULT_LOCALE {
        en_us()
    } else {
        catalog(locale)?
    };
    let message = catalog
        .get(key)
        .unwrap_or_else(|| panic!("{locale} has {key}"));
    Some(format::plain(
        &message.nodes,
        values,
        locale_data::formatters(locale),
    ))
}

/// "Added {name}. {count, plural, =0 {...} one {# recipient selected} other {...}}." (`u2qHzE`).
fn recipients(locale: &str, count: f64) -> Option<String> {
    plain_in(
        locale,
        "u2qHzE",
        &[("name", Arg::from("Ann")), ("count", Arg::Num(count))],
    )
}

#[test]
fn english_plural_exact_and_categories() {
    assert_eq!(
        recipients("en-US", 0.0).unwrap(),
        "Added Ann. No recipients selected."
    );
    assert_eq!(
        recipients("en-US", 1.0).unwrap(),
        "Added Ann. 1 recipient selected."
    );
    assert_eq!(
        recipients("en-US", 1234.0).unwrap(),
        "Added Ann. 1,234 recipients selected."
    );
}

#[test]
fn russian_plural_one_few_many_other() {
    let Some(_) = catalog("ru") else { return };
    let at = |count| recipients("ru", count).unwrap();
    assert_eq!(at(0.0), "Пользователь Ann добавлен. Получатели не выбраны.");
    assert_eq!(
        at(1.0),
        "Пользователь Ann добавлен. Выбран 1\u{a0}получатель."
    );
    assert_eq!(
        at(21.0),
        "Пользователь Ann добавлен. Выбран 21\u{a0}получатель."
    );
    assert_eq!(
        at(3.0),
        "Пользователь Ann добавлен. Выбрано 3\u{a0}получателя."
    );
    assert_eq!(
        at(5.0),
        "Пользователь Ann добавлен. Выбрано 5\u{a0}получателей."
    );
    assert_eq!(
        at(11.0),
        "Пользователь Ann добавлен. Выбрано 11\u{a0}получателей."
    );
    // A fraction is `other` in Russian; `#` is the locale's number, grouped with a no-break space.
    assert_eq!(
        at(1.5),
        "Пользователь Ann добавлен. Выбрано 1,5\u{a0}получателей."
    );
    assert_eq!(
        at(1000.0),
        "Пользователь Ann добавлен. Выбрано 1\u{a0}000\u{a0}получателей."
    );
}

#[test]
fn polish_plural_few_and_many() {
    let Some(_) = catalog("pl") else { return };
    let at = |count| recipients("pl", count).unwrap();
    assert_eq!(at(1.0), "Dodano Ann. 1 odbiorca wybrany.");
    assert_eq!(at(22.0), "Dodano Ann. 22 odbiorcy wybrani.");
    assert_eq!(at(12.0), "Dodano Ann. 12 odbiorców wybranych.");
    assert_eq!(at(25.0), "Dodano Ann. 25 odbiorców wybranych.");
    // Polish groups from five digits on (CLDR minimumGroupingDigits 2).
    assert_eq!(at(1000.0), "Dodano Ann. 1000 odbiorców wybranych.");
}

#[test]
fn czech_few_and_numbers_inside_options() {
    let Some(_) = catalog("cs") else { return };
    assert_eq!(
        recipients("cs", 4.0).unwrap(),
        "Přidaný příjemce: Ann. 4\u{a0}vybraní příjemci."
    );
    // Turkish and Japanese write the count with a Number node inside `other`.
    if catalog("tr").is_some() {
        assert_eq!(
            recipients("tr", 1000.0).unwrap(),
            "Ann eklendi. 1.000 alıcı seçildi."
        );
    }
    if catalog("ja").is_some() {
        assert_eq!(
            recipients("ja", 1000.0).unwrap(),
            "Annを追加しました。1,000人の受信者が選択されました。"
        );
    }
}

#[test]
fn select_with_booleans_and_strings() {
    // "Noise cancellation is {enabled, select, ...} by the {inputProfile, select, ...} input profile." (`/tqmfM`).
    let values = [
        ("enabled", Arg::from(false)),
        ("inputProfile", Arg::from("STUDIO")),
    ];
    assert_eq!(
        plain_in("en-US", "/tqmfM", &values).unwrap(),
        "Noise cancellation is disabled by the Studio input profile."
    );
    let other = [
        ("enabled", Arg::from("maybe")),
        ("inputProfile", Arg::from("NONE")),
    ];
    assert_eq!(
        plain_in("en-US", "/tqmfM", &other).unwrap(),
        "Noise cancellation is controlled by the selected input profile."
    );
    // "Monthly on the {nth, select, 1 {first} ...} {weekday}" with a number for the select (`mjOEBk`).
    if catalog("ru").is_some() {
        let values = [("nth", Arg::Num(2.0)), ("weekday", Arg::from("вторник"))];
        assert_eq!(
            plain_in("ru", "mjOEBk", &values).unwrap(),
            "Ежемесячно (каждый второй день: вторник)"
        );
    }
}

#[test]
fn tags_inside_plural_options_keep_the_pound() {
    // "{emojiName} Super reacted by {a} and {n, plural, one {$link[1 other]} other {$link[# others]}}" (`G98B0W`).
    let values = [
        ("emojiName", Arg::from(":fire:")),
        ("a", Arg::from("Ann")),
        ("n", Arg::Num(3.0)),
        ("onClick", Arg::from("")),
    ];
    assert_eq!(
        plain_in("en-US", "G98B0W", &values).unwrap(),
        ":fire: Super reacted by Ann and 3 others"
    );
    if catalog("pl").is_some() {
        let mut one = values.clone();
        one[2].1 = Arg::Num(1.0);
        assert_eq!(
            plain_in("pl", "G98B0W", &one).unwrap(),
            "Użytkownik Ann i 1 inny użyli superreakcji z emoji :fire:"
        );
    }
}

#[test]
fn reserialize_writes_icu_source_and_markdown() {
    let nodes = &en_us().get("u2qHzE").unwrap().nodes;
    assert_eq!(
        format::reserialize(nodes),
        "Added {name}. {count, plural, =0 {No recipients selected} one {# recipient selected} other {# recipients selected}}."
    );
    let nodes = &en_us().get("G98B0W").unwrap().nodes;
    assert!(
        format::reserialize(nodes)
            .ends_with("one {[1 other]({onClick})} other {[# others]({onClick})}}")
    );
}

#[test]
fn locale_names_are_under_the_hash_of_their_code() {
    // `runtimeHashMessageKey("tr")`: module 226536 reads "Turkish" / "Türkçe" there.
    assert_eq!(message_key("tr"), "N4rhgZ");
    assert!(en_us_text(&message_key("ja")).is_some());
    for code in locales().filter(|code| *code != "id") {
        assert!(
            en_us_text(&message_key(code)).is_some(),
            "{code} has a localized name"
        );
    }
}

#[test]
fn the_reverse_table_is_the_literal_en_us_messages() {
    // Regenerate with tools/gen-intl.py when this fails.
    let mut literal = 0;
    for (key, message) in &en_us().messages {
        if let Some(text) = ast::literal(&message.nodes).filter(|text| !text.is_empty()) {
            literal += 1;
            assert!(
                keys_of(&text).any(|known| known == &**key),
                "{key} {text:?}"
            );
        }
    }
    assert_eq!(literal, reverse::REVERSE.len());
    assert!(
        reverse::REVERSE
            .windows(2)
            .all(|pair| pair[0].0 <= pair[1].0)
    );
}

#[test]
fn ambiguous_literals_resolve_by_their_translations() {
    assert_eq!(resolve("No such literal anywhere", None), Resolved::Unknown);
    assert_eq!(resolve("Friends", None), Resolved::One("Friends"));
    let Some(turkish) = every_catalog("tr") else {
        return;
    };
    // "Friends" is four keys of three catalogs that Turkish translates alike.
    assert_eq!(keys_of("Friends").count(), 4);
    assert_eq!(
        resolve("Friends", Some(turkish)),
        Resolved::One("Arkadaşlar")
    );
    // "Preview" is several keys; Turkish has "Önizleme" for most and "Ön İzleme" for some.
    assert!(keys_of("Preview").count() >= 15);
    assert_eq!(
        resolve("Preview", Some(turkish)),
        Resolved::Ambiguous("Önizleme")
    );
}

#[test]
fn system_languages_match_like_module_226536() {
    let available = [
        "en-US", "en-GB", "zh-CN", "zh-TW", "es-ES", "es-419", "no", "pt-BR", "de",
    ];
    assert_eq!(match_locale("de-AT", &available, "en-US"), "de");
    assert_eq!(match_locale("es", &available, "en-US"), "es-ES");
    assert_eq!(match_locale("nb", &available, "en-US"), "no");
    assert_eq!(match_locale("zh-Hant-HK", &available, "en-US"), "zh-TW");
    assert_eq!(match_locale("zh-HK", &available, "en-US"), "zh-CN");
    assert_eq!(match_locale("pt-PT", &available, "en-US"), "pt-BR");
    assert_eq!(match_locale("en-AU", &available, "en-US"), "en-US");
    assert_eq!(match_locale("xx", &available, "en-US"), "en-US");
}

#[test]
fn the_chunk_of_a_locale_holds_its_dumped_catalog() {
    let file = chunk_files("tr").next().unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../design/snapshot/assets")
        .join(file);
    let (Ok(source), Some(dumped)) = (std::fs::read_to_string(&path), dump("tr")) else {
        return;
    };
    let json = catalog_json(&source).unwrap();
    let read: serde_json::Value = serde_json::from_str(&json).unwrap();
    let dumped: serde_json::Value = serde_json::from_str(&dumped).unwrap();
    assert_eq!(read, dumped);
}

/// The one test that moves the global locale, so no other test sees it change.
#[test]
fn the_current_locale_switches_and_falls_back() {
    assert_eq!(locale(), "en-US");
    assert_eq!(t("cduTBL"), "User Settings");
    assert_eq!(tr("User Settings"), "User Settings");
    let Some(json) = dump("de") else { return };
    let before = generation();
    assert!(set_locale("de"));
    assert!(generation() > before);
    // Not installed yet: en-US shows, as the official loader's fallback does while the chunk loads.
    assert_eq!(tr("User Settings"), "User Settings");
    install("de", &[&json]).unwrap();
    assert_eq!(tr("User Settings"), "Benutzereinstellungen");
    assert_eq!(t("cduTBL"), "Benutzereinstellungen");
    assert_eq!(
        t!("u2qHzE", name = "Ann", count = 2),
        "Ann hinzugefügt. 2 Empfänger ausgewählt."
    );
    assert_eq!(number(1234.5), "1.234,5");
    assert_eq!(localized_locale_name("de"), "Deutsch");
    // A key the locale lacks comes from en-US.
    let missing = en_us()
        .messages
        .iter()
        .find(|(key, message)| message.text.is_some() && catalog("de").unwrap().get(key).is_none())
        .map(|(key, message)| (key.clone(), message.text.clone().unwrap()))
        .unwrap();
    assert_eq!(t(&missing.0), &*missing.1);
    assert!(!set_locale("de"));
    assert!(set_locale("xx-unknown"));
    assert_eq!(locale(), "en-US");
    assert_eq!(tr("User Settings"), "User Settings");
}

#[test]
fn numbers_follow_the_locale_like_intl_number_format() {
    let at = |locale: &str, value| locale_data::formatters(locale).number(value, None);
    assert_eq!(at("en-US", 1234567.891), "1,234,567.891");
    assert_eq!(at("en-US", 0.12345), "0.123");
    assert_eq!(at("en-US", 2.5), "2.5");
    assert_eq!(at("de", 1234.5), "1.234,5");
    assert_eq!(at("fr", 1234.5), "1\u{202f}234,5");
    assert_eq!(at("no", 1234.5), "1\u{a0}234,5");
    assert_eq!(at("hi", 1234567.0), "12,34,567");
    assert_eq!(at("es-ES", 1234.0), "1234");
    assert_eq!(at("es-ES", 12345.0), "12.345");
    assert_eq!(at("ja", 1234.0), "1,234");
    // Plural categories of locales the catalogs use beyond one/other.
    let category =
        |locale: &str, value| locale_data::formatters(locale).plural_category(value, false);
    assert_eq!(category("lt", 2.0), "few");
    assert_eq!(category("lt", 0.5), "many");
    assert_eq!(category("uk", 11.0), "many");
    assert_eq!(category("hr", 23.0), "few");
    assert_eq!(category("ja", 1.0), "other");
    assert_eq!(category("fr", 0.0), "one");
}

/// "{count}+ new messages since {timestamp, time, short} on {timestamp, date, long}" (`tHqbtg`): one value, its time
/// in the Time node and its date in the Date node.
#[test]
fn a_moment_prints_its_time_and_its_date_where_the_message_asks() {
    let when = Arg::When {
        date: "November 8, 2026".into(),
        time: "7:36 AM".into(),
    };
    let text = format("tHqbtg", &[("count", Arg::Num(50.0)), ("timestamp", when)]);
    assert_eq!(text, "50+ new messages since 7:36 AM on November 8, 2026");
}

/// "**{a}** and **{b}** are typing..." (`rB0CUa`): the names in bold runs, the rest plain.
#[test]
fn runs_keep_the_bold_names_apart() {
    let runs = format_runs("rB0CUa", &[("a", Arg::from("Ann")), ("b", Arg::from("Bo"))]);
    let shown: Vec<(&str, Option<&str>)> = runs
        .iter()
        .map(|(text, tag)| (text.as_str(), tag.as_deref()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Ann", Some("$b")),
            (" and ", None),
            ("Bo", Some("$b")),
            (" are typing...", None)
        ]
    );
}

/// moment's `humanize()`: its rounding and thresholds, and the words of the moment locales (functions included).
#[test]
fn durations_humanize_in_the_moment_locales() {
    let minute = 60_000;
    let hour = 60 * minute;
    let day = 24 * hour;
    assert_eq!(humanize_in("en-US", 10_000, false), "a few seconds");
    assert_eq!(humanize_in("en-US", 60_000, false), "a minute");
    assert_eq!(humanize_in("en-US", 89_000, false), "a minute");
    assert_eq!(humanize_in("en-US", 120_000, false), "2 minutes");
    assert_eq!(humanize_in("en-US", 44 * minute, false), "44 minutes");
    assert_eq!(humanize_in("en-US", 45 * minute, false), "an hour");
    assert_eq!(humanize_in("en-US", 3 * hour, false), "3 hours");
    assert_eq!(humanize_in("en-US", 30 * hour, false), "a day");
    assert_eq!(humanize_in("en-US", 25 * day, false), "25 days");
    assert_eq!(humanize_in("en-US", 26 * day, false), "a month");
    assert_eq!(humanize_in("en-US", 100 * day, false), "3 months");
    assert_eq!(humanize_in("en-US", 400 * day, false), "a year");
    assert_eq!(humanize_in("en-US", 800 * day, false), "2 years");
    assert_eq!(humanize_in("en-US", -day, true), "a day ago");
    assert_eq!(humanize_in("en-US", 3 * hour, true), "in 3 hours");
    assert_eq!(humanize_in("tr", 120_000, false), "2 dakika");
    assert_eq!(humanize_in("tr", 10_000, false), "birkaç saniye");
    assert_eq!(humanize_in("tr", -day, true), "bir gün önce");
    assert_eq!(humanize_in("de", 30 * hour, false), "ein Tag");
    assert_eq!(humanize_in("de", -30 * hour, true), "vor einem Tag");
    assert_eq!(humanize_in("ru", 2 * minute, false), "2 минуты");
    assert_eq!(humanize_in("ru", 5 * minute, true), "через 5 минут");
    assert_eq!(humanize_in("ru", -21 * hour, true), "21 час назад");
    assert_eq!(humanize_in("pl", 22 * minute, false), "22 minuty");
    assert_eq!(humanize_in("pl", 12 * minute, false), "12 minut");
    assert_eq!(humanize_in("cs", -3 * hour, true), "před 3 hodinami");
    assert_eq!(humanize_in("fi", 3 * hour, false), "kolmen tunnin");
    assert_eq!(humanize_in("fi", -3 * hour, true), "kolme tuntia sitten");
    assert_eq!(humanize_in("lt", -12 * minute, true), "prieš 12 minučių");
    assert_eq!(humanize_in("ro", 20 * minute, false), "20 de minute");
    assert_eq!(humanize_in("hu", -2 * day, true), "2 napja");
    assert_eq!(humanize_in("uk", hour, false), "годину");
    assert_eq!(humanize_in("hi", 3 * hour, false), "३ घंटे");
    assert_eq!(humanize_in("id", 3 * hour, false), "3 hours");
}

/// A text in parts is found as the one message with its `$link` (`LJzl+0`); a lone link is no tagged message.
#[test]
fn parts_find_their_whole_message() {
    use crate::parts::{Style, key_of};
    let text = [
        (
            "Due to system permissions, keybinds won't work during gameplay. To enable, install Discord System Helper. ",
            Style::Plain,
        ),
        ("Learn more", Style::Link),
    ];
    assert_eq!(key_of(&text), Some("LJzl+0"));
    assert_eq!(key_of(&[("Learn more", Style::Plain)]), None);
}

/// "<$p>Cancellation will take effect ... on <$b>{date}</>, ...</><$p>You can restart ...</>" (`X7i9D8`): one list of
/// runs per paragraph, the date bold inside the first.
#[test]
fn paragraphs_split_at_their_p_tags() {
    let when = Arg::When {
        date: "Dec 2, 2026".into(),
        time: String::new().into(),
    };
    let paragraphs = paragraph_runs("X7i9D8", &[("date", when)]);
    assert_eq!(paragraphs.len(), 2);
    assert_eq!(
        paragraphs[0][1],
        ("Dec 2, 2026".to_owned(), Some("$b".into()))
    );
    assert_eq!(
        paragraphs[1][0].0,
        "You can restart your subscription anytime."
    );
    assert_eq!(paragraph_runs("FClXh9", &[]).len(), 1);
}
