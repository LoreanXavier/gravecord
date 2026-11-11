//! The 31 languages of the language select (`name` is what the list shows, module 296437, the `enabled` ones).

/// The codes are `dc_proto::settings::LOCALES`, in this order.
/// The native names, same order.
pub(super) const NAMES: &[&str] = &[
    "English, USA",
    "English, GB",
    "中文",
    "繁體中文",
    "Čeština",
    "Dansk",
    "Nederlands",
    "Français",
    "Deutsch",
    "Ελληνικά",
    "Magyar",
    "Italiano",
    "日本語",
    "한국어",
    "Norwegian",
    "Hrvatski",
    "Polski",
    "Português do Brasil",
    "Русский",
    "Español",
    "Español, LATAM",
    "Svenska",
    "Türkçe",
    "български",
    "Українська",
    "Suomi",
    "Română",
    "Lietuviškai",
    "ไทย",
    "Tiếng Việt",
    "हिंदी",
];

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_has_its_locale_code() {
        assert_eq!(super::NAMES.len(), dc_proto::settings::LOCALES.len());
        assert_eq!(dc_proto::settings::LOCALES[0], "en-US");
    }
}
