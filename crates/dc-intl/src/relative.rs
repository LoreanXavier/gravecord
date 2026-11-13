//! moment's `duration.humanize()` in the interface locale: the relative-time words of the moment locale module 608998
//! loads for each language (bg 625881, cs 884168, da 9273, de 32021, el 466155, en-GB 594091, es-ES / es-419 725226,
//! fi 67207, fr 626482, hr 926412, hu 285241, it 492519, ja 842203, ko 431764, lt 484942, nl 773108, no 906378,
//! pl 274170, pt-BR 28839, ro 651025, ru 702551, sv-SE 39147, th 337474, tr 675936, uk 95122, vi 304375,
//! zh-CN 41360, zh-TW 105496, hi 98125), and moment's own `en` (536637) for every other locale.
//!
//! `humanize` (536637 `nm.humanize`) rounds the duration to seconds, minutes, hours, days, months and years
//! (`Math.round` of `as(unit)`) and picks the first unit under its threshold (`ss: 44, s: 45, m: 45, h: 22, d: 26,
//! w: null, M: 11`); the locale's `relativeTime` writes it, given the number, whether the suffix is left out, the unit
//! key and whether the duration is positive (`+this > 0`). With the suffix the locale's `future` / `past` wraps it
//! (`pastFuture`), and `postformat` runs last (only hi has one: Devanagari digits).

/// The unit keys of `relativeTime`, in the order the tables below list them after `future` and `past`.
const UNITS: [&str; 12] = [
    "s", "ss", "m", "mm", "h", "hh", "d", "dd", "M", "MM", "y", "yy",
];

type Table = [Option<&'static str>; 14];

/// `relativeTime` of each moment locale: `future`, `past`, then [`UNITS`]. `None` is a function of the locale's,
/// ported in [`by_function`].
fn table(locale: &str) -> &'static Table {
    match locale {
        "bg" => &[
            Some("след %s"),
            Some("преди %s"),
            Some("няколко секунди"),
            Some("%d секунди"),
            Some("минута"),
            Some("%d минути"),
            Some("час"),
            Some("%d часа"),
            Some("ден"),
            Some("%d дена"),
            Some("месец"),
            Some("%d месеца"),
            Some("година"),
            Some("%d години"),
        ],
        "cs" => &[
            Some("za %s"),
            Some("před %s"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "da" => &[
            Some("om %s"),
            Some("%s siden"),
            Some("få sekunder"),
            Some("%d sekunder"),
            Some("et minut"),
            Some("%d minutter"),
            Some("en time"),
            Some("%d timer"),
            Some("en dag"),
            Some("%d dage"),
            Some("en måned"),
            Some("%d måneder"),
            Some("et år"),
            Some("%d år"),
        ],
        "de" => &[
            Some("in %s"),
            Some("vor %s"),
            Some("ein paar Sekunden"),
            Some("%d Sekunden"),
            None,
            Some("%d Minuten"),
            None,
            Some("%d Stunden"),
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "el" => &[
            Some("σε %s"),
            Some("%s πριν"),
            Some("λίγα δευτερόλεπτα"),
            Some("%d δευτερόλεπτα"),
            Some("ένα λεπτό"),
            Some("%d λεπτά"),
            Some("μία ώρα"),
            Some("%d ώρες"),
            Some("μία μέρα"),
            Some("%d μέρες"),
            Some("ένας μήνας"),
            Some("%d μήνες"),
            Some("ένας χρόνος"),
            Some("%d χρόνια"),
        ],
        "es-ES" | "es-419" => &[
            Some("en %s"),
            Some("hace %s"),
            Some("unos segundos"),
            Some("%d segundos"),
            Some("un minuto"),
            Some("%d minutos"),
            Some("una hora"),
            Some("%d horas"),
            Some("un día"),
            Some("%d días"),
            Some("un mes"),
            Some("%d meses"),
            Some("un año"),
            Some("%d años"),
        ],
        "fi" => &[
            Some("%s päästä"),
            Some("%s sitten"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "fr" => &[
            Some("dans %s"),
            Some("il y a %s"),
            Some("quelques secondes"),
            Some("%d secondes"),
            Some("une minute"),
            Some("%d minutes"),
            Some("une heure"),
            Some("%d heures"),
            Some("un jour"),
            Some("%d jours"),
            Some("un mois"),
            Some("%d mois"),
            Some("un an"),
            Some("%d ans"),
        ],
        "hr" => &[
            Some("za %s"),
            Some("prije %s"),
            Some("par sekundi"),
            None,
            None,
            None,
            None,
            None,
            Some("dan"),
            None,
            Some("mjesec"),
            None,
            Some("godinu"),
            None,
        ],
        "hu" => &[
            Some("%s múlva"),
            Some("%s"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "it" => &[
            Some("tra %s"),
            Some("%s fa"),
            Some("alcuni secondi"),
            Some("%d secondi"),
            Some("un minuto"),
            Some("%d minuti"),
            Some("un'ora"),
            Some("%d ore"),
            Some("un giorno"),
            Some("%d giorni"),
            Some("un mese"),
            Some("%d mesi"),
            Some("un anno"),
            Some("%d anni"),
        ],
        "ja" => &[
            Some("%s後"),
            Some("%s前"),
            Some("数秒"),
            Some("%d秒"),
            Some("1分"),
            Some("%d分"),
            Some("1時間"),
            Some("%d時間"),
            Some("1日"),
            Some("%d日"),
            Some("1ヶ月"),
            Some("%dヶ月"),
            Some("1年"),
            Some("%d年"),
        ],
        "ko" => &[
            Some("%s 후"),
            Some("%s 전"),
            Some("몇 초"),
            Some("%d초"),
            Some("1분"),
            Some("%d분"),
            Some("한 시간"),
            Some("%d시간"),
            Some("하루"),
            Some("%d일"),
            Some("한 달"),
            Some("%d달"),
            Some("일 년"),
            Some("%d년"),
        ],
        "lt" => &[
            Some("po %s"),
            Some("prieš %s"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "nl" => &[
            Some("over %s"),
            Some("%s geleden"),
            Some("een paar seconden"),
            Some("%d seconden"),
            Some("één minuut"),
            Some("%d minuten"),
            Some("één uur"),
            Some("%d uur"),
            Some("één dag"),
            Some("%d dagen"),
            Some("één maand"),
            Some("%d maanden"),
            Some("één jaar"),
            Some("%d jaar"),
        ],
        "no" => &[
            Some("om %s"),
            Some("%s siden"),
            Some("noen sekunder"),
            Some("%d sekunder"),
            Some("ett minutt"),
            Some("%d minutter"),
            Some("én time"),
            Some("%d timer"),
            Some("én dag"),
            Some("%d dager"),
            Some("én måned"),
            Some("%d måneder"),
            Some("ett år"),
            Some("%d år"),
        ],
        "pl" => &[
            Some("za %s"),
            Some("%s temu"),
            Some("kilka sekund"),
            None,
            None,
            None,
            None,
            None,
            Some("1 dzień"),
            Some("%d dni"),
            Some("miesiąc"),
            None,
            Some("rok"),
            None,
        ],
        "pt-BR" => &[
            Some("em %s"),
            Some("há %s"),
            Some("poucos segundos"),
            Some("%d segundos"),
            Some("um minuto"),
            Some("%d minutos"),
            Some("uma hora"),
            Some("%d horas"),
            Some("um dia"),
            Some("%d dias"),
            Some("um mês"),
            Some("%d meses"),
            Some("um ano"),
            Some("%d anos"),
        ],
        "ro" => &[
            Some("peste %s"),
            Some("%s în urmă"),
            Some("câteva secunde"),
            None,
            Some("un minut"),
            None,
            Some("o oră"),
            None,
            Some("o zi"),
            None,
            Some("o lună"),
            None,
            Some("un an"),
            None,
        ],
        "ru" => &[
            Some("через %s"),
            Some("%s назад"),
            Some("несколько секунд"),
            None,
            None,
            None,
            Some("час"),
            None,
            Some("день"),
            None,
            Some("месяц"),
            None,
            Some("год"),
            None,
        ],
        "sv-SE" => &[
            Some("om %s"),
            Some("för %s sedan"),
            Some("några sekunder"),
            Some("%d sekunder"),
            Some("en minut"),
            Some("%d minuter"),
            Some("en timme"),
            Some("%d timmar"),
            Some("en dag"),
            Some("%d dagar"),
            Some("en månad"),
            Some("%d månader"),
            Some("ett år"),
            Some("%d år"),
        ],
        "th" => &[
            Some("อีก %s"),
            Some("%sที่แล้ว"),
            Some("ไม่กี่วินาที"),
            Some("%d วินาที"),
            Some("1 นาที"),
            Some("%d นาที"),
            Some("1 ชั่วโมง"),
            Some("%d ชั่วโมง"),
            Some("1 วัน"),
            Some("%d วัน"),
            Some("1 เดือน"),
            Some("%d เดือน"),
            Some("1 ปี"),
            Some("%d ปี"),
        ],
        "tr" => &[
            Some("%s sonra"),
            Some("%s önce"),
            Some("birkaç saniye"),
            Some("%d saniye"),
            Some("bir dakika"),
            Some("%d dakika"),
            Some("bir saat"),
            Some("%d saat"),
            Some("bir gün"),
            Some("%d gün"),
            Some("bir ay"),
            Some("%d ay"),
            Some("bir yıl"),
            Some("%d yıl"),
        ],
        "uk" => &[
            Some("за %s"),
            Some("%s тому"),
            Some("декілька секунд"),
            None,
            None,
            None,
            Some("годину"),
            None,
            Some("день"),
            None,
            Some("місяць"),
            None,
            Some("рік"),
            None,
        ],
        "vi" => &[
            Some("%s tới"),
            Some("%s trước"),
            Some("vài giây"),
            Some("%d giây"),
            Some("một phút"),
            Some("%d phút"),
            Some("một giờ"),
            Some("%d giờ"),
            Some("một ngày"),
            Some("%d ngày"),
            Some("một tháng"),
            Some("%d tháng"),
            Some("một năm"),
            Some("%d năm"),
        ],
        "zh-CN" => &[
            Some("%s后"),
            Some("%s前"),
            Some("几秒"),
            Some("%d 秒"),
            Some("1 分钟"),
            Some("%d 分钟"),
            Some("1 小时"),
            Some("%d 小时"),
            Some("1 天"),
            Some("%d 天"),
            Some("1 个月"),
            Some("%d 个月"),
            Some("1 年"),
            Some("%d 年"),
        ],
        "zh-TW" => &[
            Some("%s後"),
            Some("%s前"),
            Some("幾秒"),
            Some("%d 秒"),
            Some("1 分鐘"),
            Some("%d 分鐘"),
            Some("1 小時"),
            Some("%d 小時"),
            Some("1 天"),
            Some("%d 天"),
            Some("1 個月"),
            Some("%d 個月"),
            Some("1 年"),
            Some("%d 年"),
        ],
        "hi" => &[
            Some("%s में"),
            Some("%s पहले"),
            Some("कुछ ही क्षण"),
            Some("%d सेकंड"),
            Some("एक मिनट"),
            Some("%d मिनट"),
            Some("एक घंटा"),
            Some("%d घंटे"),
            Some("एक दिन"),
            Some("%d दिन"),
            Some("एक महीने"),
            Some("%d महीने"),
            Some("एक वर्ष"),
            Some("%d वर्ष"),
        ],
        // moment's `en` (536637); en-GB's (594091) has the same words.
        _ => &[
            Some("in %s"),
            Some("%s ago"),
            Some("a few seconds"),
            Some("%d seconds"),
            Some("a minute"),
            Some("%d minutes"),
            Some("an hour"),
            Some("%d hours"),
            Some("a day"),
            Some("%d days"),
            Some("a month"),
            Some("%d months"),
            Some("a year"),
            Some("%d years"),
        ],
    }
}

/// The three forms of an East Slavic noun by its number (`n % 10 == 1 && n % 100 != 11`, then 2 to 4 outside the
/// teens, else the third), as 702551 and 95122 pick them.
fn slavic(number: u64, forms: [&str; 3]) -> &str {
    if number % 10 == 1 && number % 100 != 11 {
        forms[0]
    } else if (2..=4).contains(&(number % 10)) && !(10..20).contains(&(number % 100)) {
        forms[1]
    } else {
        forms[2]
    }
}

/// The `relativeTime` functions of the locales whose table entry is one: (number, withoutSuffix, key, isFuture).
fn by_function(locale: &str, n: u64, without_suffix: bool, key: &str, future: bool) -> String {
    let with = |rest: &str| format!("{n} {rest}");
    match locale {
        // 884168 `t`; `s(e)` is 2 to 4 outside the teens.
        "cs" => {
            let few = n > 1 && n < 5 && (n / 10) != 1;
            let plain = without_suffix || future;
            let count = |few_form: &str, many: &str, instrumental: &str| {
                if plain {
                    with(if few { few_form } else { many })
                } else {
                    with(instrumental)
                }
            };
            match key {
                "s" => (if plain {
                    "pár sekund"
                } else {
                    "pár sekundami"
                })
                .to_owned(),
                "ss" => count("sekundy", "sekund", "sekundami"),
                "m" => (if without_suffix {
                    "minuta"
                } else if future {
                    "minutu"
                } else {
                    "minutou"
                })
                .to_owned(),
                "mm" => count("minuty", "minut", "minutami"),
                "h" => (if without_suffix {
                    "hodina"
                } else if future {
                    "hodinu"
                } else {
                    "hodinou"
                })
                .to_owned(),
                "hh" => count("hodiny", "hodin", "hodinami"),
                "d" => (if plain { "den" } else { "dnem" }).to_owned(),
                "dd" => count("dny", "dní", "dny"),
                "M" => (if plain { "měsíc" } else { "měsícem" }).to_owned(),
                "MM" => count("měsíce", "měsíců", "měsíci"),
                "y" => (if plain { "rok" } else { "rokem" }).to_owned(),
                "yy" => count("roky", "let", "lety"),
                _ => String::new(),
            }
        }
        // 32021 `n`: the nominative without the suffix, the dative with it.
        "de" => {
            let pick = |plain: String, dative: String| if without_suffix { plain } else { dative };
            match key {
                "m" => pick("eine Minute".into(), "einer Minute".into()),
                "h" => pick("eine Stunde".into(), "einer Stunde".into()),
                "d" => pick("ein Tag".into(), "einem Tag".into()),
                "dd" => pick(with("Tage"), with("Tagen")),
                "M" => pick("ein Monat".into(), "einem Monat".into()),
                "MM" => pick(with("Monate"), with("Monaten")),
                "y" => pick("ein Jahr".into(), "einem Jahr".into()),
                "yy" => pick(with("Jahre"), with("Jahren")),
                _ => String::new(),
            }
        }
        // 67207 `n`: the genitive for a positive duration; numbers under ten are written out.
        "fi" => {
            const NUMBERS: [&str; 10] = [
                "nolla",
                "yksi",
                "kaksi",
                "kolme",
                "neljä",
                "viisi",
                "kuusi",
                "seitsemän",
                "kahdeksan",
                "yhdeksän",
            ];
            const GENITIVE: [&str; 10] = [
                "nolla",
                "yhden",
                "kahden",
                "kolmen",
                "neljän",
                "viiden",
                "kuuden",
                "seitsemän",
                "kahdeksan",
                "yhdeksän",
            ];
            let one =
                |genitive: &str, plain: &str| (if future { genitive } else { plain }).to_owned();
            let unit = match key {
                "s" => return one("muutaman sekunnin", "muutama sekunti"),
                "ss" => {
                    if future {
                        "sekunnin"
                    } else {
                        "sekuntia"
                    }
                }
                "m" => return one("minuutin", "minuutti"),
                "mm" => {
                    if future {
                        "minuutin"
                    } else {
                        "minuuttia"
                    }
                }
                "h" => return one("tunnin", "tunti"),
                "hh" => {
                    if future {
                        "tunnin"
                    } else {
                        "tuntia"
                    }
                }
                "d" => return one("päivän", "päivä"),
                "dd" => {
                    if future {
                        "päivän"
                    } else {
                        "päivää"
                    }
                }
                "M" => return one("kuukauden", "kuukausi"),
                "MM" => {
                    if future {
                        "kuukauden"
                    } else {
                        "kuukautta"
                    }
                }
                "y" => return one("vuoden", "vuosi"),
                "yy" => {
                    if future {
                        "vuoden"
                    } else {
                        "vuotta"
                    }
                }
                _ => "",
            };
            let number = if n < 10 {
                (if future { GENITIVE } else { NUMBERS })[n as usize].to_owned()
            } else {
                n.to_string()
            };
            format!("{number} {unit}")
        }
        // 926412 `a`.
        "hr" => {
            let form = |one: &str, few: &str, many: &str| {
                with(match n {
                    1 => one,
                    2..=4 => few,
                    _ => many,
                })
            };
            match key {
                "ss" => form("sekunda", "sekunde", "sekundi"),
                "m" => (if without_suffix {
                    "jedna minuta"
                } else {
                    "jedne minute"
                })
                .to_owned(),
                "mm" => form("minuta", "minute", "minuta"),
                "h" => (if without_suffix {
                    "jedan sat"
                } else {
                    "jednog sata"
                })
                .to_owned(),
                "hh" => form("sat", "sata", "sati"),
                "dd" => form("dan", "dana", "dana"),
                "MM" => form("mjesec", "mjeseca", "mjeseci"),
                "yy" => form("godina", "godine", "godina"),
                _ => String::new(),
            }
        }
        // 285241 `s`. Its `ss` case reads `e + (a || r) ? " másodperc" : " másodperce"`, which is always the first.
        "hu" => {
            let plain = future || without_suffix;
            let tail = |short: &str, long: &str| if plain { short } else { long }.to_owned();
            match key {
                "s" => tail("néhány másodperc", "néhány másodperce"),
                "ss" => " másodperc".to_owned(),
                "m" => format!("egy{}", tail(" perc", " perce")),
                "mm" => format!("{n}{}", tail(" perc", " perce")),
                "h" => format!("egy{}", tail(" óra", " órája")),
                "hh" => format!("{n}{}", tail(" óra", " órája")),
                "d" => format!("egy{}", tail(" nap", " napja")),
                "dd" => format!("{n}{}", tail(" nap", " napja")),
                "M" => format!("egy{}", tail(" hónap", " hónapja")),
                "MM" => format!("{n}{}", tail(" hónap", " hónapja")),
                "y" => format!("egy{}", tail(" év", " éve")),
                "yy" => format!("{n}{}", tail(" év", " éve")),
                _ => String::new(),
            }
        }
        // 484942: `s` for a single unit, `_` for a count, the forms "nominative_genitive_accusative".
        "lt" => {
            let forms = |unit: &str| -> [&'static str; 3] {
                match unit {
                    "ss" => ["sekundė", "sekundžių", "sekundes"],
                    "m" => ["minutė", "minutės", "minutę"],
                    "mm" => ["minutės", "minučių", "minutes"],
                    "h" => ["valanda", "valandos", "valandą"],
                    "hh" => ["valandos", "valandų", "valandas"],
                    "d" => ["diena", "dienos", "dieną"],
                    "dd" => ["dienos", "dienų", "dienas"],
                    "M" => ["mėnuo", "mėnesio", "mėnesį"],
                    "MM" => ["mėnesiai", "mėnesių", "mėnesius"],
                    _ => ["metai", "metų", "metus"],
                }
            };
            let single = |unit: &str| {
                let forms = forms(unit);
                if without_suffix {
                    forms[0]
                } else if future {
                    forms[1]
                } else {
                    forms[2]
                }
            };
            let teen = n.is_multiple_of(10) || (n > 10 && n < 20);
            match key {
                "s" => (if without_suffix {
                    "kelios sekundės"
                } else if future {
                    "kelių sekundžių"
                } else {
                    "kelias sekundes"
                })
                .to_owned(),
                "m" | "h" | "d" | "M" | "y" => single(key).to_owned(),
                _ => {
                    let forms = forms(key);
                    if n == 1 {
                        with(single(&key[..1]))
                    } else if without_suffix {
                        with(if teen { forms[1] } else { forms[0] })
                    } else if future {
                        with(forms[1])
                    } else {
                        with(if teen { forms[1] } else { forms[2] })
                    }
                }
            }
        }
        // 274170 `r`; `a(e)` is a last digit of 2 to 4 outside the teens.
        "pl" => {
            let few = n % 10 < 5 && n % 10 > 1 && (n / 10) % 10 != 1;
            match key {
                "ss" => with(if few { "sekundy" } else { "sekund" }),
                "m" => (if without_suffix { "minuta" } else { "minutę" }).to_owned(),
                "mm" => with(if few { "minuty" } else { "minut" }),
                "h" => (if without_suffix {
                    "godzina"
                } else {
                    "godzinę"
                })
                .to_owned(),
                "hh" => with(if few { "godziny" } else { "godzin" }),
                "MM" => with(if few { "miesiące" } else { "miesięcy" }),
                "yy" => with(if few { "lata" } else { "lat" }),
                _ => String::new(),
            }
        }
        // 651025 `i`: " de " from twenty on, and at whole hundreds.
        "ro" => {
            let unit = match key {
                "ss" => "secunde",
                "mm" => "minute",
                "hh" => "ore",
                "dd" => "zile",
                "MM" => "luni",
                _ => "ani",
            };
            let joint = if n % 100 >= 20 || (n >= 100 && n.is_multiple_of(100)) {
                " de "
            } else {
                " "
            };
            format!("{n}{joint}{unit}")
        }
        // 702551 `_`.
        "ru" => {
            if key == "m" {
                return (if without_suffix {
                    "минута"
                } else {
                    "минуту"
                })
                .to_owned();
            }
            let forms = match key {
                "ss" if without_suffix => ["секунда", "секунды", "секунд"],
                "ss" => ["секунду", "секунды", "секунд"],
                "mm" if without_suffix => ["минута", "минуты", "минут"],
                "mm" => ["минуту", "минуты", "минут"],
                "hh" => ["час", "часа", "часов"],
                "dd" => ["день", "дня", "дней"],
                "MM" => ["месяц", "месяца", "месяцев"],
                _ => ["год", "года", "лет"],
            };
            with(slavic(n, forms))
        }
        // 95122 `e`.
        "uk" => {
            match key {
                "m" => {
                    return (if without_suffix {
                        "хвилина"
                    } else {
                        "хвилину"
                    })
                    .to_owned();
                }
                "h" => {
                    return (if without_suffix {
                        "година"
                    } else {
                        "годину"
                    })
                    .to_owned();
                }
                _ => {}
            }
            let forms = match key {
                "ss" if without_suffix => ["секунда", "секунди", "секунд"],
                "ss" => ["секунду", "секунди", "секунд"],
                "mm" if without_suffix => ["хвилина", "хвилини", "хвилин"],
                "mm" => ["хвилину", "хвилини", "хвилин"],
                "hh" if without_suffix => ["година", "години", "годин"],
                "hh" => ["годину", "години", "годин"],
                "dd" => ["день", "дні", "днів"],
                "MM" => ["місяць", "місяці", "місяців"],
                _ => ["рік", "роки", "років"],
            };
            with(slavic(n, forms))
        }
        _ => String::new(),
    }
}

/// `postformat` of the locale (98125: ASCII digits to Devanagari).
fn postformat(locale: &str, text: String) -> String {
    if locale != "hi" {
        return text;
    }
    text.chars()
        .map(|ch| match ch.to_digit(10) {
            Some(digit) => char::from_u32(0x0966 + digit).unwrap_or(ch),
            None => ch,
        })
        .collect()
}

/// `createDuration(milliseconds).humanize(with_suffix)` in `locale`.
pub fn humanize_in(locale: &str, milliseconds: i64, with_suffix: bool) -> String {
    let future = milliseconds > 0;
    let ms = milliseconds.unsigned_abs() as f64;
    // `as(unit)` of a duration made of milliseconds only: days and months by `t0` (4800 / 146097 months a day).
    let days = ms / 864e5;
    let round = |value: f64| value.round() as u64;
    let seconds = round(ms / 1e3);
    let minutes = round(ms / 6e4);
    let hours = round(ms / 36e5);
    let whole_days = round(days);
    let months = round(days * 4800. / 146_097.);
    let years = round(days * 4800. / 146_097. / 12.);
    let (key, number) = if seconds <= 44 {
        ("s", seconds)
    } else if seconds < 45 {
        ("ss", seconds)
    } else if minutes <= 1 {
        ("m", 1)
    } else if minutes < 45 {
        ("mm", minutes)
    } else if hours <= 1 {
        ("h", 1)
    } else if hours < 22 {
        ("hh", hours)
    } else if whole_days <= 1 {
        ("d", 1)
    } else if whole_days < 26 {
        ("dd", whole_days)
    } else if months <= 1 {
        ("M", 1)
    } else if months < 11 {
        ("MM", months)
    } else if years <= 1 {
        ("y", 1)
    } else {
        ("yy", years)
    };
    let table = table(locale);
    let slot = UNITS.iter().position(|unit| *unit == key).unwrap_or(0) + 2;
    let without_suffix = !with_suffix;
    let mut text = match table[slot] {
        Some(words) => words.replacen("%d", &number.to_string(), 1),
        None => by_function(locale, number, without_suffix, key, future),
    };
    if with_suffix && let Some(wrap) = table[if future { 0 } else { 1 }] {
        text = wrap.replacen("%s", &text, 1);
    }
    postformat(locale, text)
}

/// [`humanize_in`] in the current locale: `moment.duration(ms).humanize()` (`with_suffix` false, as a call's length
/// reads) or `humanize(true)` ("in 3 hours", "a day ago").
pub fn humanize(milliseconds: i64, with_suffix: bool) -> String {
    humanize_in(crate::locale(), milliseconds, with_suffix)
}
