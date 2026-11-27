//! KeywordNotify (Equicord `src/equicordplugins/keywordNotify`, GPL-3.0-or-later): the keyword entries and the
//! original's rules for whether a message matches one (`applyKeywordEntries`). The entries keep the original's
//! fields (`regex`, `ignoreCase`, `ignoreBots`, `whitelist`, `blacklist`, `listPriority`) and read the older
//! `listIds`/`listType` form as `start` moves it. Expressions run on Rust's `regex`: one that does not compile there
//! (look-around, back references, an unfinished pattern) matches nothing, as `safeMatchesRegex` answers for a
//! pattern JavaScript refuses.

use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};

/// `ListType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum List {
    BlackList,
    Whitelist,
}

impl List {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            List::BlackList => "BlackList",
            List::Whitelist => "Whitelist",
        }
    }
}

/// `KeywordEntry`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub regex: String,
    pub ignore_case: bool,
    pub ignore_bots: bool,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub priority: List,
}

impl Entry {
    /// `addKeywordEntry`'s new entry.
    pub(crate) fn new() -> Self {
        Self {
            regex: String::new(),
            ignore_case: false,
            ignore_bots: true,
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            priority: List::BlackList,
        }
    }

    /// A stored entry as `start` reads it: `ignoreBots` falls back to the plugin's hidden setting, the lists and
    /// the priority to empty and Blacklist, and `listIds` with `listType` become the list they name.
    pub(crate) fn from_value(value: &Value, ignore_bots: bool) -> Self {
        let ids = |list: &Value| -> Vec<String> {
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        };
        let mut entry = Self {
            regex: value["regex"].as_str().unwrap_or_default().to_owned(),
            ignore_case: value["ignoreCase"].as_bool().unwrap_or(false),
            ignore_bots: value["ignoreBots"].as_bool().unwrap_or(ignore_bots),
            whitelist: ids(&value["whitelist"]),
            blacklist: ids(&value["blacklist"]),
            priority: if value["listPriority"].as_str() == Some("Whitelist") {
                List::Whitelist
            } else {
                List::BlackList
            },
        };
        if value["listIds"].is_array() && value["listType"].is_string() {
            if value["listType"].as_str() == Some("Whitelist") {
                entry.whitelist = ids(&value["listIds"]);
            } else {
                entry.blacklist = ids(&value["listIds"]);
            }
        }
        entry
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({
            "regex": self.regex,
            "ignoreCase": self.ignore_case,
            "ignoreBots": self.ignore_bots,
            "whitelist": self.whitelist,
            "blacklist": self.blacklist,
            "listPriority": self.priority.as_str(),
        })
    }

    /// The entry's expression with its flags (`"i"` with Ignore Case); `None` when empty or refused.
    pub(crate) fn compile(&self) -> Option<Regex> {
        if self.regex.is_empty() {
            return None;
        }
        RegexBuilder::new(&self.regex)
            .case_insensitive(self.ignore_case)
            .build()
            .ok()
    }
}

/// What the rules read of a message: its author, channel and server, and the texts it is matched on.
pub(crate) struct Seen<'a> {
    pub author: &'a str,
    pub bot: bool,
    pub channel: &'a str,
    pub guild: Option<&'a str>,
    pub message: &'a Value,
}

/// Whether an entry lets the message be matched at all (its lists and Ignore Bots), as `applyKeywordEntries`.
pub(crate) fn passes(entry: &Entry, seen: &Seen) -> bool {
    let holds = |list: &[String]| {
        list.iter().any(|id| {
            let id = id.trim();
            id == seen.channel || id == seen.author
        }) || seen
            .guild
            .is_some_and(|guild| list.iter().any(|id| id.trim() == guild))
    };
    let white = holds(&entry.whitelist);
    let black = holds(&entry.blacklist);
    if white && black {
        if entry.priority != List::Whitelist {
            return false;
        }
    } else if (!entry.whitelist.is_empty() && !white) || black {
        return false;
    }
    // `whitelist.includes(author.id)`: the untrimmed id.
    !(seen.bot
        && entry.ignore_bots
        && (entry.whitelist.is_empty() || !entry.whitelist.iter().any(|id| id == seen.author)))
}

/// `safeMatchesRegex` over the content, then each embed's description and title, then its fields' values and
/// names.
pub(crate) fn text_matches(regex: &Regex, message: &Value) -> bool {
    let hit = |value: &Value| value.as_str().is_some_and(|text| regex.is_match(text));
    if hit(&message["content"]) {
        return true;
    }
    message["embeds"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|embed| {
            hit(&embed["description"])
                || hit(&embed["title"])
                || embed["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|field| hit(&field["value"]) || hit(&field["name"]))
        })
}

/// Whether any entry matches the message.
pub(crate) fn matches(entries: &[(Entry, Option<Regex>)], seen: &Seen) -> bool {
    entries.iter().any(|(entry, regex)| {
        regex
            .as_ref()
            .is_some_and(|regex| passes(entry, seen) && text_matches(regex, seen.message))
    })
}

/// `highlightKeywords`: the text cut around the first match of the entries (before, match, after), as the log
/// draws it; `None` when nothing matches.
pub(crate) fn highlight<'a>(
    text: &'a str,
    entries: &[(Entry, Option<Regex>)],
) -> Option<(&'a str, &'a str, &'a str)> {
    // `regexes.map(r => str.match(r)).flat()[0]`, then `indexOf` of that text: the first entry's first match.
    let found = entries
        .iter()
        .filter_map(|(_, regex)| regex.as_ref()?.find(text))
        .find(|found| !found.as_str().is_empty())?;
    let at = text.find(found.as_str())?;
    let end = at + found.as_str().len();
    Some((&text[..at], &text[at..end], &text[end..]))
}

/// The log as kept: newest first by time, each message once, at most `keep`.
pub(crate) fn add_to_log(log: &mut Vec<Value>, message: Value, keep: usize) -> bool {
    if log.iter().any(|known| known["id"] == message["id"]) {
        return false;
    }
    log.push(message);
    // `timestamp` is ISO 8601 with the same offset, so the text sorts as the time does.
    log.sort_by(|a, b| {
        b["timestamp"]
            .as_str()
            .unwrap_or_default()
            .cmp(a["timestamp"].as_str().unwrap_or_default())
    });
    log.truncate(keep);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen<'a>(message: &'a Value, author: &'a str, bot: bool) -> Seen<'a> {
        Seen {
            author,
            bot,
            channel: "10",
            guild: Some("20"),
            message,
        }
    }

    fn entry(regex: &str) -> Entry {
        Entry {
            regex: regex.to_owned(),
            ..Entry::new()
        }
    }

    #[test]
    fn lists_follow_apply_keyword_entries() {
        let message = json!({"content": "hello"});
        let plain = entry("hel+o");
        assert!(passes(&plain, &seen(&message, "1", false)));
        // A whitelist lets only its ids through; the server counts too.
        let mut white = entry("x");
        white.whitelist = vec!["2".to_owned()];
        assert!(!passes(&white, &seen(&message, "1", false)));
        white.whitelist = vec![" 20 ".to_owned()];
        assert!(passes(&white, &seen(&message, "1", false)));
        // A blacklisted channel, author or server is left out.
        let mut black = entry("x");
        black.blacklist = vec!["10".to_owned()];
        assert!(!passes(&black, &seen(&message, "1", false)));
        // On both lists: the priority decides.
        let mut both = entry("x");
        both.whitelist = vec!["1".to_owned()];
        both.blacklist = vec!["10".to_owned()];
        assert!(!passes(&both, &seen(&message, "1", false)));
        both.priority = List::Whitelist;
        assert!(passes(&both, &seen(&message, "1", false)));
        // Bots are ignored unless the entry says otherwise or the bot itself is whitelisted.
        assert!(!passes(&plain, &seen(&message, "1", true)));
        let mut bots = entry("x");
        bots.ignore_bots = false;
        assert!(passes(&bots, &seen(&message, "1", true)));
        let mut listed = entry("x");
        listed.whitelist = vec!["1".to_owned()];
        assert!(passes(&listed, &seen(&message, "1", true)));
    }

    #[test]
    fn content_and_embeds_are_matched_with_the_flags() {
        let entries = |e: Entry| {
            let regex = e.compile();
            vec![(e, regex)]
        };
        let message = json!({"content": "Deploy done", "embeds": [{"fields": [{"name": "n", "value": "build failed"}]}]});
        let s = seen(&message, "1", false);
        assert!(!matches(&entries(entry("deploy")), &s));
        let mut ci = entry("deploy");
        ci.ignore_case = true;
        assert!(matches(&entries(ci), &s));
        assert!(matches(&entries(entry("fail(ed)?")), &s));
        // Empty and refused expressions match nothing.
        assert!(!matches(&entries(entry("")), &s));
        assert!(!matches(&entries(entry("(?=x)")), &s));
        assert!(!matches(&entries(entry("(")), &s));
        assert_eq!(
            highlight("Deploy done", &entries(entry("o\\w+"))),
            Some(("Depl", "oy", " done"))
        );
        assert_eq!(highlight("Deploy", &entries(entry("zz"))), None);
    }

    #[test]
    fn stored_entries_read_their_old_form_and_defaults() {
        let old =
            json!({"regex": "a", "ignoreCase": true, "listIds": ["5"], "listType": "Whitelist"});
        let entry = Entry::from_value(&old, false);
        assert_eq!(entry.whitelist, ["5"]);
        assert!(entry.blacklist.is_empty());
        assert!(!entry.ignore_bots);
        assert_eq!(entry.priority, List::BlackList);
        assert_eq!(Entry::from_value(&entry.to_value(), true), entry);
    }

    #[test]
    fn the_log_keeps_the_newest_once() {
        let mut log = Vec::new();
        let at = |id: &str, t: &str| json!({"id": id, "timestamp": t});
        assert!(add_to_log(&mut log, at("1", "2026-01-01T00:00:01"), 2));
        assert!(add_to_log(&mut log, at("2", "2026-01-01T00:00:03"), 2));
        assert!(!add_to_log(&mut log, at("2", "2026-01-01T00:00:03"), 2));
        assert!(add_to_log(&mut log, at("3", "2026-01-01T00:00:02"), 2));
        let ids: Vec<&str> = log.iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["2", "3"]);
    }
}
