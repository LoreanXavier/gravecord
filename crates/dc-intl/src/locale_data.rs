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
