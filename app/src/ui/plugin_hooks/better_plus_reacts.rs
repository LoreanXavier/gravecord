//! BetterPlusReacts (Equicord `src/equicordplugins/betterPlusReacts`, GPL-3.0-or-later): the send path's `+:emoji:`
//! reaction takes more pluses, one per message back: `+:x:` reacts to the newest message, `++:x:` to the one before,
//! and so on (`getByIndex(length - pluses)`). The rule's pattern becomes `^\+*:(.+?): *$` and the name is read with
//! the pluses folded into one; with no plus there is no message, so `:x:` is sent as written. The composer's reaction
//! list opens for a first word of several pluses too (`currentWord.replace(/^\++/, '+')`), and choosing an emoji
//! there reacts to the message those pluses pick. Not part of the pinned client.

use dc_state::autocomplete::{Kind, Trigger};
use std::sync::LazyLock;

const ID: &str = "BetterPlusReacts";

/// The patched `reaction.match`.
static RULE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\+*:[^\n\r\u{2028}\u{2029}]+?: *$").expect("the rule compiles")
});

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// How many messages back, and the emoji name: `e.trim().replace(/^\++/, '+').slice(2, -1)`.
fn read(content: &str) -> Option<(usize, String)> {
    if !RULE.is_match(content) {
        return None;
    }
    let trimmed = content.trim();
    let pluses = trimmed.chars().take_while(|ch| *ch == '+').count();
    let folded = format!("+{}", &trimmed[pluses..]);
    Some((pluses, folded.get(2..folded.len() - 1)?.to_owned()))
}

/// The plugin's reaction rule: `None` while it is off (the pinned rule runs); else the message back and the name,
/// `None` inside when the text is no reaction.
pub(in crate::ui) fn better_plus_reacts_rule(content: &str) -> Option<Option<(usize, String)>> {
    if !content.trim_start().starts_with(['+', ':']) || !on() {
        return None;
    }
    Some(read(content))
}

/// The reaction list for a first word of two pluses or more (one plus is the pinned client's own).
pub(in crate::ui) fn better_plus_reacts_trigger(text: &str, caret: usize) -> Option<Trigger> {
    if !text.trim_start().starts_with("++") || !on() {
        return None;
    }
    trigger(text, caret)
}

fn trigger(text: &str, caret: usize) -> Option<Trigger> {
    let word_start = text.len() - text.trim_start().len();
    let word = text.get(word_start..caret)?;
    if word.contains(char::is_whitespace) {
        return None;
    }
    let pluses = word.chars().take_while(|ch| *ch == '+').count();
    word[pluses..].starts_with(':').then(|| Trigger {
        kind: Kind::Reaction,
        query: word[pluses + 1..].to_lowercase(),
        start: word_start,
        end: caret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluses_count_the_messages_back() {
        assert_eq!(read("+:x:"), Some((1, "x".into())));
        assert_eq!(read("+++:thumbsup: "), Some((3, "thumbsup".into())));
        assert_eq!(read(":x:"), Some((0, "x".into())));
        assert_eq!(read("++x:"), None);
    }

    #[test]
    fn the_list_opens_for_several_pluses() {
        let found = trigger("+++:thu", 7).unwrap();
        assert_eq!(
            (found.kind, found.query.as_str(), found.start),
            (Kind::Reaction, "thu", 0)
        );
        assert_eq!(trigger(" ++:a", 5).map(|found| found.start), Some(1));
        assert!(trigger("++:a b", 6).is_none());
        assert!(trigger("++a", 3).is_none());
    }
}
