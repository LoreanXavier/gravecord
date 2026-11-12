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

/// The flag of each locale, an asset of the pinned build (module 579832 maps `./<code>.png` to the modules that export
/// these files); a locale without one shows module 432706's.
const FLAGS: &[(&str, &str)] = &[
    ("bg", "5cbafc331d3abb31.png"),
    ("cs", "f950a34a4f3890db.png"),
    ("da", "923bad6dac42d2aa.png"),
    ("de", "0cdfd952a5fdb034.png"),
    ("el", "b9be8a73de16f11c.png"),
    ("en-GB", "6f77a7600f51e495.png"),
    ("en-US", "a3a24dc7f81098cf.png"),
    ("es-419", "ab0d69ef0e26ae64.png"),
    ("es-ES", "06ee56f5b41cac26.png"),
    ("fi", "74ef1946f62f8534.png"),
    ("fr", "91468121eea0e245.png"),
    ("hi", "409aeaf4be671895.png"),
    ("hr", "6c4b5f62a7a07905.png"),
    ("hu", "0ae442e371482c6f.png"),
    ("it", "4a99d3f98a414def.png"),
    ("ja", "143187ba920ece23.png"),
    ("ko", "b9268d0e9656ab10.png"),
    ("lt", "e3ee79f55b67020e.png"),
    ("nl", "3629eb7368c61620.png"),
    ("no", "e2e922612cb4eca2.png"),
    ("pl", "06909f9fe15ac80d.png"),
    ("pt-BR", "80bc29d8282cca84.png"),
    ("ro", "4a43f8fe5a97a73b.png"),
    ("ru", "e6c45ebabdae3967.png"),
    ("sv-SE", "6e4382d8fdba7d89.png"),
    ("th", "092f90a3d18dc8f2.png"),
    ("tr", "e39943b6ada93bf4.png"),
    ("uk", "9947a07640ef8423.png"),
    ("vi", "6e359d178d011781.png"),
    ("zh-CN", "966fcf4564ca26f2.png"),
    ("zh-TW", "2877967b53eb4312.png"),
];
const FALLBACK_FLAG: &str = "57c49989a786d704.png";

/// The image key of a locale's flag.
pub(super) fn flag_key(code: &str) -> String {
    format!("language-flag:{code}")
}

/// Every flag's image key and URL, fetched when settings open.
pub(super) fn flag_requests() -> impl Iterator<Item = (String, String)> {
    dc_proto::settings::LOCALES.iter().map(|code| {
        let file = FLAGS
            .iter()
            .find(|(known, _)| known == code)
            .map_or(FALLBACK_FLAG, |(_, file)| *file);
        (
            flag_key(code),
            format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_has_its_locale_code() {
        assert_eq!(super::NAMES.len(), dc_proto::settings::LOCALES.len());
        assert_eq!(dc_proto::settings::LOCALES[0], "en-US");
    }

    #[test]
    fn every_language_has_its_own_flag() {
        for code in dc_proto::settings::LOCALES {
            assert!(
                super::FLAGS.iter().any(|(known, _)| known == code),
                "{code}"
            );
        }
    }

    /// Module 226536 `getAvailableLocales` sorts by the lower-cased native name (code units, so Čeština after
    /// Tiếng Việt), the Cyrillic, Greek, Thai, Devanagari, CJK and Hangul names after the Latin ones.
    #[test]
    fn the_list_order_is_the_lower_cased_native_names() {
        let mut order: Vec<&str> = super::NAMES.to_vec();
        order.sort_by_key(|name| name.to_lowercase());
        assert_eq!(
            &order[..4],
            ["Dansk", "Deutsch", "English, GB", "English, USA"]
        );
        assert_eq!(order.last(), Some(&"한국어"));
    }
}
