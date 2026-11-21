//! The data formatters of module 495581 (`makeDataFormatters`) for one locale: `getPluralRules` and `formatNumber`. The
//! official client gets them from the browser's `Intl.PluralRules` and `Intl.NumberFormat` (module 499179), which
//! Chromium answers from ICU's CLDR data; here ICU4X's compiled CLDR data answers the same questions.

use fixed_decimal::{Decimal, FloatPrecision, SignedRoundingMode, UnsignedRoundingMode};
use icu_decimal::DecimalFormatter;
use icu_decimal::options::DecimalFormatterOptions;
use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub struct Formatters {
    cardinal: Option<PluralRules>,
    ordinal: Option<PluralRules>,
    decimal: Option<DecimalFormatter>,
}

/// The ICU locale the official locale code formats with. `Intl` is given `[locale, "en-US"]` (IntlManager of
/// module 461217), so a code ICU cannot read formats as en-US.
fn icu_locale(code: &str) -> Locale {
    code.parse::<Locale>()
        .unwrap_or_else(|_| "en-US".parse().expect("en-US is a locale"))
}

impl Formatters {
    fn new(code: &str) -> Self {
        let locale = icu_locale(code);
        Formatters {
            cardinal: PluralRules::try_new_cardinal((&locale).into()).ok(),
            ordinal: PluralRules::try_new_ordinal((&locale).into()).ok(),
            decimal: DecimalFormatter::try_new(
                (&locale).into(),
                DecimalFormatterOptions::default(),
            )
            .ok(),
        }
    }

    /// `Intl.NumberFormat`'s default: at most three fraction digits (or `max_fraction`), rounded half away from
    /// zero, trailing zeros dropped, the locale's grouping and symbols.
    fn decimal(value: f64, max_fraction: i16) -> Option<Decimal> {
        let mut decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip).ok()?;
        decimal.round_with_mode(
            -max_fraction,
            SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfExpand),
        );
        decimal.absolute.trim_end();
        Some(decimal)
    }

    pub fn number(&self, value: f64, max_fraction: Option<i16>) -> String {
        let Some(decimal) = Self::decimal(value, max_fraction.unwrap_or(3)) else {
            return crate::format::js_number(value);
        };
        match &self.decimal {
            Some(formatter) => formatter.format(&decimal).to_string(),
            None => decimal.to_string(),
        }
    }

    /// `Intl.PluralRules.select`: the CLDR category name of a number.
    pub fn plural_category(&self, value: f64, ordinal: bool) -> &'static str {
        let rules = if ordinal {
            &self.ordinal
        } else {
            &self.cardinal
        };
        let (Some(rules), Some(decimal)) = (rules, Self::decimal(value, 3)) else {
            return "other";
        };
        match rules.category_for(&decimal) {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }
}

/// The formatters of a locale, made once and kept.
pub fn formatters(code: &str) -> &'static Formatters {
    static CACHE: OnceLock<RwLock<HashMap<String, &'static Formatters>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.read().ok().and_then(|map| map.get(code).copied()) {
        return found;
    }
    let made: &'static Formatters = Box::leak(Box::new(Formatters::new(code)));
    match cache.write() {
        Ok(mut map) => map.entry(code.to_owned()).or_insert(made),
        Err(_) => made,
    }
}

/// Localized month names (`moment().localeData().months()`) for all official Discord locales (module 201505).
pub fn month_names(locale: &str) -> [&'static str; 12] {
    match locale {
        "bg" => [
            "януари",
            "февруари",
            "март",
            "април",
            "май",
            "юни",
            "юли",
            "август",
            "септември",
            "октомври",
            "ноември",
            "декември",
        ],
        "cs" => [
            "leden",
            "únor",
            "březen",
            "duben",
            "květen",
            "červen",
            "červenec",
            "srpen",
            "září",
            "říjen",
            "listopad",
            "prosinec",
        ],
        "da" => [
            "januar",
            "februar",
            "marts",
            "april",
            "maj",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "december",
        ],
        "de" => [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ],
        "el" => [
            "Ιανουάριος",
            "Φεβρουάριος",
            "Μάρτιος",
            "Απρίλιος",
            "Μάιος",
            "Ιούνιος",
            "Ιούλιος",
            "Αύγουστος",
            "Σεπτέμβριος",
            "Οκτώβριος",
            "Νοέμβριος",
            "Δεκέμβριος",
        ],
        "es-ES" | "es-419" => [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
        "fi" => [
            "tammikuu",
            "helmikuu",
            "maaliskuu",
            "huhtikuu",
            "toukokuu",
            "kesäkuu",
            "heinäkuu",
            "elokuu",
            "syyskuu",
            "lokakuu",
            "marraskuu",
            "joulukuu",
        ],
        "fr" => [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ],
        "hi" => [
            "जनवरी",
            "फ़रवरी",
            "मार्च",
            "अप्रैल",
            "मई",
            "जून",
            "जुलाई",
            "अगस्त",
            "सितंबर",
            "अक्टूबर",
            "नवंबर",
            "दिसंबर",
        ],
        "hr" => [
            "siječanj",
            "veljača",
            "ožujak",
            "travanj",
            "svibanj",
            "lipanj",
            "srpanj",
            "kolovoz",
            "rujan",
            "listopad",
            "studeni",
            "prosinac",
        ],
        "hu" => [
            "január",
            "február",
            "március",
            "április",
            "május",
            "június",
            "július",
            "augusztus",
            "szeptember",
            "október",
            "november",
            "december",
        ],
        "id" => [
            "Januari",
            "Februari",
            "Maret",
            "April",
            "Mei",
            "Juni",
            "Juli",
            "Agustus",
            "September",
            "Oktober",
            "November",
            "Desember",
        ],
        "it" => [
            "gennaio",
            "febbraio",
            "marzo",
            "aprile",
            "maggio",
            "giugno",
            "luglio",
            "agosto",
            "settembre",
            "ottobre",
            "novembre",
            "dicembre",
        ],
        "ja" => [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        "ko" => [
            "1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월", "12월",
        ],
        "lt" => [
            "sausis",
            "vasaris",
            "kovas",
            "balandis",
            "gegužė",
            "birželis",
            "liepa",
            "rugpjūtis",
            "rugsėjis",
            "spalis",
            "lapkritis",
            "gruodis",
        ],
        "nl" => [
            "januari",
            "februari",
            "maart",
            "april",
            "mei",
            "juni",
            "juli",
            "augustus",
            "september",
            "oktober",
            "november",
            "december",
        ],
        "no" => [
            "januar",
            "februar",
            "mars",
            "april",
            "mai",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "desember",
        ],
        "pl" => [
            "styczeń",
            "luty",
            "marzec",
            "kwiecień",
            "maj",
            "czerwiec",
            "lipiec",
            "sierpień",
            "wrzesień",
            "październik",
            "listopad",
            "grudzień",
        ],
        "pt-BR" => [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        "ro" => [
            "ianuarie",
            "februarie",
            "martie",
            "aprilie",
            "mai",
            "iunie",
            "iulie",
            "august",
            "septembrie",
            "octombrie",
            "noiembrie",
            "decembrie",
        ],
        "ru" => [
            "январь",
            "февраль",
            "март",
            "апрель",
            "май",
            "июнь",
            "июль",
            "август",
            "сентябрь",
            "октябрь",
            "ноябрь",
            "декабрь",
        ],
        "sv-SE" => [
            "januari",
            "februari",
            "mars",
            "april",
            "maj",
            "juni",
            "juli",
            "augusti",
            "september",
            "oktober",
            "november",
            "december",
        ],
        "th" => [
            "มกราคม",
            "กุมภาพันธ์",
            "มีนาคม",
            "เมษายน",
            "พฤษภาคม",
            "มิถุนายน",
            "กรกฎาคม",
            "สิงหาคม",
            "กันยายน",
            "ตุลาคม",
            "พฤศจิกายน",
            "ธันวาคม",
        ],
        "tr" => [
            "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül",
            "Ekim", "Kasım", "Aralık",
        ],
        "uk" => [
            "січень",
            "лютий",
            "березень",
            "квітень",
            "травень",
            "червень",
            "липень",
            "серпень",
            "вересень",
            "жовтень",
            "листопад",
            "грудень",
        ],
        "vi" => [
            "tháng 1",
            "tháng 2",
            "tháng 3",
            "tháng 4",
            "tháng 5",
            "tháng 6",
            "tháng 7",
            "tháng 8",
            "tháng 9",
            "tháng 10",
            "tháng 11",
            "tháng 12",
        ],
        "zh-CN" | "zh-TW" => [
            "一月",
            "二月",
            "三月",
            "四月",
            "五月",
            "六月",
            "七月",
            "八月",
            "九月",
            "十月",
            "十一月",
            "十二月",
        ],
        _ => [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
    }
}
