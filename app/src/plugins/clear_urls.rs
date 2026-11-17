//! ClearURLs (Vencord `src/plugins/clearURLs/index.ts`, GPL-3.0-or-later): tracking parameters are removed from the
//! links in a message before it is sent or edited.
//!
//! The rules are the ClearURLs project's own (`ClearURLs/Rules`, `data.min.json`, LGPL-3.0), bundled as
//! `assets/plugins/clearurls-rules.json` from commit c080ff46cb (2026-11-03). Vencord downloads them on every start;
//! here they ship with the app and update with it.
//!
//! One difference from Vencord: a rule matches a whole parameter name, as the ClearURLs extension tests it
//! (`^rule$`). Vencord tests the rule anywhere in the name, so Amazon's `th` rule also removed `width` or `depth`.

use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

const RULES_JSON: &str = include_str!("../../assets/plugins/clearurls-rules.json");

struct Provider {
    url_pattern: Regex,
    exceptions: Vec<Regex>,
    rules: Vec<Regex>,
    raw_rules: Vec<Regex>,
}

fn compile(pattern: &str) -> Option<Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

fn list(provider: &serde_json::Value, field: &str, anchored: bool) -> Vec<Regex> {
    provider
        .get(field)
        .and_then(|list| list.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|rule| rule.as_str())
                .filter_map(|rule| {
                    if anchored {
                        compile(&format!("^(?:{rule})$"))
                    } else {
                        compile(rule)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

static PROVIDERS: OnceLock<Vec<Provider>> = OnceLock::new();

/// The rules were read and compiled (only a message sent with ClearURLs on does that).
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub(crate) fn rules_loaded() -> bool {
    PROVIDERS.get().is_some()
}

fn providers() -> &'static [Provider] {
    PROVIDERS.get_or_init(|| {
        let Ok(data) = serde_json::from_str::<serde_json::Value>(RULES_JSON) else {
            return Vec::new();
        };
        let Some(providers) = data.get("providers").and_then(|value| value.as_object()) else {
            return Vec::new();
        };
        providers
            .values()
            .filter_map(|provider| {
                Some(Provider {
                    url_pattern: compile(provider.get("urlPattern")?.as_str()?)?,
                    exceptions: list(provider, "exceptions", false),
                    rules: list(provider, "rules", true),
                    raw_rules: list(provider, "rawRules", false),
                })
            })
            .collect()
    })
}

/// A link as it is sent: the same text when no rule removes anything.
pub(crate) fn clean_url(link: &str) -> String {
    let Ok(mut url) = url::Url::parse(link) else {
        return link.to_owned();
    };
    // Only links with a query are looked at (Vencord's "cheap way to check if there are any search params").
    if url.query_pairs().next().is_none() {
        return link.to_owned();
    }
    let mut changed = false;
    for provider in providers() {
        let href = url.as_str();
        if !provider.url_pattern.is_match(href)
            || provider
                .exceptions
                .iter()
                .any(|exception| exception.is_match(href))
        {
            continue;
        }
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let kept: Vec<&(String, String)> = pairs
            .iter()
            .filter(|(key, _)| !provider.rules.iter().any(|rule| rule.is_match(key)))
            .collect();
        if kept.len() != pairs.len() {
            changed = true;
            if kept.is_empty() {
                url.set_query(None);
            } else {
                url.query_pairs_mut()
                    .clear()
                    .extend_pairs(kept.iter().map(|(key, value)| (key, value)));
            }
        }
        // A raw rule removes its first match from the whole address (`String.replace` without the g flag).
        let mut cleaned = url.as_str().to_owned();
        for raw in &provider.raw_rules {
            if let Some(found) = raw.find(&cleaned) {
                cleaned.replace_range(found.range(), "");
            }
        }
        if cleaned != url.as_str() {
            match url::Url::parse(&cleaned) {
                Ok(parsed) => {
                    url = parsed;
                    changed = true;
                }
                Err(_) => continue,
            }
        }
    }
    if changed { url.into() } else { link.to_owned() }
}

/// `cleanMessage`: every link in the text, as Vencord finds them.
pub(crate) fn clean_message(content: &str) -> String {
    static LINK: OnceLock<Regex> = OnceLock::new();
    if !content.contains("http://") && !content.contains("https://") {
        return content.to_owned();
    }
    let link = LINK.get_or_init(|| {
        Regex::new(r#"https?://[^\s<]+[^<.,:;"'>)|\]\s]"#).expect("the link pattern compiles")
    });
    link.replace_all(content, |found: &regex::Captures<'_>| clean_url(&found[0]))
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_rule_compiles() {
        let data: serde_json::Value = serde_json::from_str(RULES_JSON).unwrap();
        let count = data["providers"].as_object().unwrap().len();
        assert!(count > 150, "{count} providers");
        assert_eq!(
            providers().len(),
            count,
            "a provider's pattern did not compile"
        );
        for (name, provider) in data["providers"].as_object().unwrap() {
            for field in ["rules", "rawRules", "exceptions"] {
                let given = provider
                    .get(field)
                    .and_then(|list| list.as_array())
                    .map_or(0, Vec::len);
                assert_eq!(
                    list(provider, field, field == "rules").len(),
                    given,
                    "{name}.{field}"
                );
            }
        }
    }

    #[test]
    fn tracking_parameters_are_removed_and_the_rest_kept() {
        assert_eq!(
            clean_url("https://example.com/page?id=7&utm_source=news&utm_medium=mail"),
            "https://example.com/page?id=7"
        );
        assert_eq!(
            clean_url("https://example.com/page?utm_source=news"),
            "https://example.com/page"
        );
        assert_eq!(
            clean_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=abcdef"),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[test]
    fn links_without_tracking_are_left_exactly_as_typed() {
        for link in [
            "https://example.com",
            "https://example.com/a?b=1",
            "https://example.com/?q=%20x+y",
            "not a link",
        ] {
            assert_eq!(clean_url(link), link);
        }
    }

    #[test]
    fn a_rule_matches_the_whole_parameter_name() {
        // Amazon's `th` rule: `width` stays.
        let cleaned = clean_url("https://www.amazon.com/dp/B000?th=1&width=200");
        assert_eq!(cleaned, "https://www.amazon.com/dp/B000?width=200");
    }

    #[test]
    fn raw_rules_cut_their_match_out_of_the_address() {
        let cleaned = clean_url("https://www.amazon.com/dp/B000/ref=sr_1_1?keywords=x");
        assert!(!cleaned.contains("ref="), "{cleaned}");
        assert!(!cleaned.contains("keywords"), "{cleaned}");
    }

    #[test]
    fn messages_keep_their_text_and_punctuation() {
        assert_eq!(
            clean_message(
                "look: https://example.com/?utm_source=x, and (https://example.com/a?fbclid=1)."
            ),
            "look: https://example.com/, and (https://example.com/a)."
        );
        assert_eq!(clean_message("no links here"), "no links here");
    }
}
