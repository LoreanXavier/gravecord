//! Translate+'s translation (Equicord `src/equicordplugins/translatePlus/utils/translator.ts` and `misc/languages.ts`,
//! GPL-3.0-or-later): which of its languages a text is in, the Shavian and sitelen pona dictionaries applied as the
//! original applies them, and Google Translate's address and answer. The calls go out from `net.rs` (its
//! `PluginTranslate` request); this side only reads and writes text.

use serde_json::Value;

/// The Shavian dictionary the original fetches (pinned to its commit).
pub(crate) const SHAVIAN_URL: &str = "https://raw.githubusercontent.com/ForkPrince/TranslatePlus/322199d5fdb1a9506591c9f4a2826338b5d67e38/shavian.json";
/// The sitelen pona dictionary the original fetches (pinned to its commit).
pub(crate) const SITELEN_URL: &str = "https://raw.githubusercontent.com/ForkPrince/TranslatePlus/5ca152b134ea11433971f21b2ef8d556d4306717/sitelen-pona.json";
/// The Toki Pona translator the original posts to.
pub(crate) const TOKI_URL: &str = "https://aiapi.serversmp.xyz/toki";
/// What `google` gives when the call fails.
pub(crate) const FAILED: &str = "Translation failed due to an error.";

/// The words of `isTokiPona`'s dictionary.
const TOKI_WORDS: &[&str] = &[
    "leko",
    "weka",
    "pan",
    "lete",
    "linja",
    "lipu",
    "suli",
    "nimi",
    "akesi",
    "misikeke",
    "selo",
    "ike",
    "sijelo",
    "sona",
    "lili",
    "pimeja",
    "ante",
    "jo",
    "loje",
    "telo",
    "walo",
    "kijetesantakalu",
    "kasi",
    "waso",
    "wile",
    "utala",
    "lukin",
    "sina",
    "lape",
    "ma",
    "pilin",
    "jasima",
    "la",
    "olin",
    "pipi",
    "meso",
    "lawa",
    "pi",
    "pakala",
    "oko",
    "tan",
    "ken",
    "jaki",
    "unpa",
    "esun",
    "seme",
    "sitelen",
    "len",
    "kule",
    "soko",
    "open",
    "ala",
    "tenpo",
    "lon",
    "sinpin",
    "pini",
    "kokosila",
    "mama",
    "musi",
    "monsi",
    "mewika",
    "taso",
    "ona",
    "mun",
    "kiwen",
    "tomo",
    "mute",
    "mi",
    "nena",
    "palisa",
    "meli",
    "laso",
    "wawa",
    "ale",
    "kipisi",
    "kulupu",
    "ilo",
    "lupa",
    "nanpa",
    "en",
    "mu",
    "jelo",
    "kili",
    "tonsi",
    "moku",
    "ni",
    "kama",
    "pu",
    "poki",
    "monsuta",
    "sin",
    "lasina",
    "poka",
    "soweli",
    "sewi",
    "elena",
    "epiku",
    "moli",
    "pona",
    "lanpan",
    "alasa",
    "anu",
    "kute",
    "uta",
    "luka",
    "suno",
    "sama",
    "awen",
    "namako",
    "suwi",
    "noka",
    "seli",
    "mije",
    "sike",
    "jan",
    "pali",
    "tawa",
    "inli",
    "nasa",
    "mani",
    "wan",
    "insa",
    "nijon",
    "nasin",
    "kalama",
    "ijo",
    "toki",
    "anpa",
    "kala",
    "kepeken",
    "ko",
    "kon",
    "pana",
    "tu",
    "supa",
    "kin",
    "usawi",
    "yupekosi",
];

/// `languages`: Google's codes and names, with Toki Pona (`tp`) and Shavian (`sh`).
const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Detect language"),
    ("af", "Afrikaans"),
    ("sq", "Albanian"),
    ("am", "Amharic"),
    ("ar", "Arabic"),
    ("hy", "Armenian"),
    ("as", "Assamese"),
    ("ay", "Aymara"),
    ("az", "Azerbaijani"),
    ("bm", "Bambara"),
    ("eu", "Basque"),
    ("be", "Belarusian"),
    ("bn", "Bengali"),
    ("bho", "Bhojpuri"),
    ("bs", "Bosnian"),
    ("bg", "Bulgarian"),
    ("ca", "Catalan"),
    ("ceb", "Cebuano"),
    ("ny", "Chichewa"),
    ("zh-CN", "Chinese (Simplified)"),
    ("zh-TW", "Chinese (Traditional)"),
    ("co", "Corsican"),
    ("hr", "Croatian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("dv", "Dhivehi"),
    ("doi", "Dogri"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("eo", "Esperanto"),
    ("et", "Estonian"),
    ("ee", "Ewe"),
    ("tl", "Filipino"),
    ("tp", "Toki Pona"),
    ("sh", "Shavian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("fy", "Frisian"),
    ("gl", "Galician"),
    ("ka", "Georgian"),
    ("de", "German"),
    ("el", "Greek"),
    ("gn", "Guarani"),
    ("gu", "Gujarati"),
    ("ht", "Haitian Creole"),
    ("ha", "Hausa"),
    ("haw", "Hawaiian"),
    ("iw", "Hebrew"),
    ("hi", "Hindi"),
    ("hmn", "Hmong"),
    ("hu", "Hungarian"),
    ("is", "Icelandic"),
    ("ig", "Igbo"),
    ("ilo", "Ilocano"),
    ("id", "Indonesian"),
    ("ga", "Irish"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("jw", "Javanese"),
    ("kn", "Kannada"),
    ("kk", "Kazakh"),
    ("km", "Khmer"),
    ("rw", "Kinyarwanda"),
    ("gom", "Konkani"),
    ("ko", "Korean"),
    ("kri", "Krio"),
    ("ku", "Kurdish (Kurmanji)"),
    ("ckb", "Kurdish (Sorani)"),
    ("ky", "Kyrgyz"),
    ("lo", "Lao"),
    ("la", "Latin"),
    ("lv", "Latvian"),
    ("ln", "Lingala"),
    ("lt", "Lithuanian"),
    ("lg", "Luganda"),
    ("lb", "Luxembourgish"),
    ("mk", "Macedonian"),
    ("mai", "Maithili"),
    ("mg", "Malagasy"),
    ("ms", "Malay"),
    ("ml", "Malayalam"),
    ("mt", "Maltese"),
    ("mi", "Maori"),
    ("mr", "Marathi"),
    ("mni-Mtei", "Meiteilon (Manipuri)"),
    ("lus", "Mizo"),
    ("mn", "Mongolian"),
    ("my", "Myanmar (Burmese)"),
    ("ne", "Nepali"),
    ("no", "Norwegian"),
    ("or", "Odia (Oriya)"),
    ("om", "Oromo"),
    ("ps", "Pashto"),
    ("fa", "Persian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("pa", "Punjabi"),
    ("qu", "Quechua"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sm", "Samoan"),
    ("sa", "Sanskrit"),
    ("gd", "Scots Gaelic"),
    ("nso", "Sepedi"),
    ("sr", "Serbian"),
    ("st", "Sesotho"),
    ("sn", "Shona"),
    ("sd", "Sindhi"),
    ("si", "Sinhala"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("so", "Somali"),
    ("es", "Spanish"),
    ("su", "Sundanese"),
    ("sw", "Swahili"),
    ("sv", "Swedish"),
    ("tg", "Tajik"),
    ("ta", "Tamil"),
    ("tt", "Tatar"),
    ("te", "Telugu"),
    ("th", "Thai"),
    ("ti", "Tigrinya"),
    ("ts", "Tsonga"),
    ("tr", "Turkish"),
    ("tk", "Turkmen"),
    ("ak", "Twi"),
    ("uk", "Ukrainian"),
    ("ur", "Urdu"),
    ("ug", "Uyghur"),
    ("uz", "Uzbek"),
    ("vi", "Vietnamese"),
    ("cy", "Welsh"),
    ("xh", "Xhosa"),
    ("yi", "Yiddish"),
    ("yo", "Yoruba"),
    ("zu", "Zulu"),
];

/// `languages[src] ?? src`.
pub(crate) fn language_name(src: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|(code, _)| *code == src)
        .map_or(src, |(_, name)| name)
}

/// `text.split(/\s+/)`: the pieces between runs of white space (an empty one at an end that is white space).
fn split_spaces(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (index, character) in text.char_indices() {
        let space = character.is_whitespace();
        if space && !in_space {
            pieces.push(&text[start..index]);
        }
        if !space && in_space {
            start = index;
        }
        in_space = space;
    }
    pieces.push(if in_space { "" } else { &text[start..] });
    pieces
}

/// `isTokiPona`: at least half the pieces are whole Toki Pona words (`\b` is ASCII's word boundary).
pub(crate) fn is_toki_pona(text: &str) -> bool {
    let words = text
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|word| TOKI_WORDS.contains(word))
        .count();
    words as f64 >= split_spaces(text).len() as f64 * 0.5
}

/// `isSitelen`: any sitelen pona glyph of the UCSUR block the original lists.
pub(crate) fn is_sitelen(text: &str) -> bool {
    text.chars().any(|character| {
        matches!(
            u32::from(character),
            0xF1900..=0xF1988 | 0xF1990..=0xF199D | 0xF19A0..=0xF19A3
        )
    })
}

/// `isShavian`: any letter of the Shavian block.
pub(crate) fn is_shavian(text: &str) -> bool {
    text.chars()
        .any(|character| matches!(u32::from(character), 0x10450..=0x1047F))
}

/// `punctuationMap`: the marks kept off a word's ends, each mapped to itself but the guillemets (straight quotes).
fn punctuation(character: char) -> Option<char> {
    match character {
        '«' | '»' => Some('"'),
        '"' | ',' | '!' | '?' | '.' | '(' | ')' | '/' | ';' | ':' => Some(character),
        _ => None,
    }
}

/// `translateShavian`: each piece looked up in the dictionary, one punctuation mark kept off each end.
pub(crate) fn translate_shavian(text: &str, dictionary: &Value) -> String {
    let mut out = String::new();
    for piece in split_spaces(text) {
        let mut word = piece;
        let mut before = None;
        let mut after = None;
        if let Some(first) = word.chars().next()
            && let Some(mark) = punctuation(first)
        {
            before = Some(mark);
            word = &word[first.len_utf8()..];
        }
        if let Some(last) = word.chars().next_back()
            && let Some(mark) = punctuation(last)
        {
            after = Some(mark);
            word = &word[..word.len() - last.len_utf8()];
        }
        out.extend(before);
        out.push_str(dictionary.get(word).and_then(Value::as_str).unwrap_or(word));
        out.extend(after);
        out.push(' ');
    }
    out.trim().to_owned()
}

/// `translateSitelen`: the glyphs spaced apart, then every dictionary key replaced, the longest tried first.
pub(crate) fn translate_sitelen(text: &str, dictionary: &Value) -> String {
    let spaced = text.chars().map(String::from).collect::<Vec<_>>().join(" ");
    let Some(map) = dictionary.as_object() else {
        return spaced;
    };
    let mut keys: Vec<(&str, &str)> = map
        .iter()
        .filter(|(key, _)| !key.is_empty())
        .filter_map(|(key, value)| Some((key.as_str(), value.as_str()?)))
        .collect();
    // `b.length - a.length`: UTF-16 lengths, a stable sort.
    keys.sort_by_key(|(key, _)| std::cmp::Reverse(key.encode_utf16().count()));
    let mut out = String::new();
    let mut rest = spaced.as_str();
    while let Some(character) = rest.chars().next() {
        match keys.iter().find(|(key, _)| rest.starts_with(key)) {
            Some((key, value)) => {
                out.push_str(value);
                rest = &rest[key.len()..];
            }
            None => {
                out.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
    }
    out
}

/// The address of `google(target, text)`.
pub(crate) fn google_url(target: &str, text: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client", "gtx")
        .append_pair("sl", "auto")
        .append_pair("tl", target)
        .append_pair("dt", "t")
        .append_pair("dj", "1")
        .append_pair("source", "input")
        .append_pair("q", text)
        .finish();
    format!("https://translate.googleapis.com/translate_a/single?{query}")
}

/// Google's answer: the language it found and the sentences' translations, one per line.
pub(crate) fn google_answer(body: &Value) -> (String, String) {
    let text = body["sentences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sentence| sentence["trans"].as_str())
        .filter(|trans| !trans.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (body["src"].as_str().unwrap_or_default().to_owned(), text)
}

/// Which way `translate` goes for a text with the plugin's switches.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Toki Pona (sitelen pona glyphs first read back into words when `sitelen` is on).
    Toki {
        sitelen: bool,
    },
    Shavian,
    Google,
}

pub(crate) fn route(text: &str, toki: bool, sitelen: bool, shavian: bool) -> Route {
    let glyphs = is_sitelen(text);
    if (is_toki_pona(text) || glyphs) && (toki || sitelen) {
        Route::Toki {
            sitelen: glyphs && sitelen,
        }
    } else if is_shavian(text) && shavian {
        Route::Shavian
    } else {
        Route::Google
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn languages_are_found_as_the_original_finds_them() {
        assert!(is_toki_pona("mi pona"));
        assert!(is_toki_pona("toki! sina pona ala pona?"));
        assert!(!is_toki_pona("hello there my friend"));
        assert!(is_toki_pona("mi hello"));
        assert!(!is_toki_pona("Mi Pona"));
        assert!(is_sitelen("\u{F1900}\u{F19A3}"));
        assert!(!is_sitelen("\u{F1989}"));
        assert!(is_shavian("\u{10451}\u{10452}"));
        assert_eq!(
            route("mi pona", true, true, true),
            Route::Toki { sitelen: false }
        );
        assert_eq!(
            route("\u{F1900}", false, true, true),
            Route::Toki { sitelen: true }
        );
        assert_eq!(route("\u{F1900}", false, false, true), Route::Google);
        assert_eq!(route("\u{10451}", true, true, true), Route::Shavian);
        assert_eq!(route("\u{10451}", true, true, false), Route::Google);
        assert_eq!(language_name("tr"), "Turkish");
        assert_eq!(language_name("tp"), "Toki Pona");
        assert_eq!(language_name("xx"), "xx");
    }

    #[test]
    fn dictionaries_apply_as_in_the_original() {
        let shavian = json!({"\u{10451}": "the", "\u{10452}\u{10453}": "cat"});
        assert_eq!(
            translate_shavian("«\u{10451} \u{10452}\u{10453}!»  x", &shavian),
            "\"the \u{10452}\u{10453}!\" x"
        );
        assert_eq!(translate_shavian("\u{10452}\u{10453}.", &shavian), "cat.");
        let sitelen = json!({"\u{F1900}": "a", "\u{F1900} \u{F1901}": "ab"});
        assert_eq!(
            translate_sitelen("\u{F1900}\u{F1901}\u{F1900}", &sitelen),
            "ab a"
        );
        assert_eq!(
            google_url("tr", "a b&c"),
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=tr&dt=t&dj=1&source=input&q=a+b%26c"
        );
        let (src, text) = google_answer(
            &json!({"src": "de", "sentences": [{"trans": "Hi"}, {"trans": ""}, {"trans": "you"}]}),
        );
        assert_eq!((src.as_str(), text.as_str()), ("de", "Hi\nyou"));
    }
}
