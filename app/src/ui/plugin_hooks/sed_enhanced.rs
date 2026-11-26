//! SedEnhanced (Equicord `src/equicordplugins/sedEnhanced`, GPL-3.0-or-later): the send path's `s/find/replace` rule
//! takes the wider pattern of `crate::plugins::sed_enhanced` in place of the pinned one, and its action: the message
//! replied to is edited if it is yours (a reply to someone else's sends nothing), else your last editable message.
//! The `r` mode makes the find a regular expression with the modes `gmisudyv` as its flags (`regexByDefault` turns
//! that around); a pattern that does not compile sends nothing. Without `r` the find is text, `\*`, `\?`, `\+` and
//! `\/` unescaped as by the pinned rule. `g` changes every match. The replacement takes JavaScript's `$` forms
//! (`$&`, `$1`, `$<name>`, ...). An empty result without attachments deletes the message.
//!
//! GAP: expressions run on Rust's `regex` (linear time), which has no look-around or back references; such a
//! pattern does not compile and sends nothing. `y` (sticky) keeps only the matches that follow each other from the
//! start. Not part of the pinned client.

use super::super::Root;
use super::super::send_rules::{Found, expand, replace_text, unescape};
use crate::plugins::sed_enhanced::{Sed, parse};
use dc_state::Id;
use gpui::Context;
use serde_json::Value;

const ID: &str = "SedEnhanced";

fn regex_by_default() -> Option<bool> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(|state| {
        state
            .is_on(ID)
            .then(|| state.value(ID, "regexByDefault") == Value::Bool(true))
    })
}

/// `new RegExp(pattern, flags)`: `None` when it does not compile (an unknown or repeated flag, `u` with `v`, or a
/// pattern this engine cannot run).
fn compile(pattern: &str, flags: &str) -> Option<regex::Regex> {
    let mut seen = String::new();
    for flag in flags.chars() {
        if seen.contains(flag) {
            return None;
        }
        seen.push(flag);
    }
    if seen.contains('u') && seen.contains('v') {
        return None;
    }
    regex::RegexBuilder::new(pattern)
        .case_insensitive(seen.contains('i'))
        .multi_line(seen.contains('m'))
        .dot_matches_new_line(seen.contains('s'))
        .crlf(true)
        .build()
        .ok()
}

/// `content.replace(find, replace)` / `replaceAll` with an expression.
fn replace_regex(
    text: &str,
    find: &regex::Regex,
    replacement: &str,
    global: bool,
    sticky: bool,
) -> String {
    let names: Vec<Option<&str>> = find.capture_names().skip(1).collect();
    let named = names.iter().any(Option::is_some);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for captures in find.captures_iter(text) {
        let whole = captures.get(0).expect("a match has its whole text");
        if sticky && whole.start() != last {
            break;
        }
        out.push_str(&text[last..whole.start()]);
        let groups: Vec<Option<&str>> = (1..captures.len())
            .map(|index| captures.get(index).map(|group| group.as_str()))
            .collect();
        let found = Found {
            matched: whole.as_str(),
            before: &text[..whole.start()],
            after: &text[whole.end()..],
            named: named.then(|| {
                names
                    .iter()
                    .zip(&groups)
                    .filter_map(|(name, value)| name.map(|name| (name, *value)))
                    .collect()
            }),
            groups,
        };
        out.push_str(&expand(replacement, &found));
        last = whole.end();
        if !global {
            break;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// The text after the rule, or `None` when the expression does not compile.
fn apply(text: &str, sed: &Sed, regex_by_default: bool) -> Option<String> {
    let global = sed.modes.contains('g');
    if sed.modes.contains('r') == regex_by_default {
        return Some(replace_text(
            text,
            &unescape(&sed.pattern),
            &unescape(&sed.replace),
            global,
        ));
    }
    let flags: String = sed
        .modes
        .chars()
        .filter(|flag| "gmisudyv".contains(*flag))
        .collect();
    let find = compile(&sed.pattern, &flags)?;
    Some(replace_regex(
        text,
        &find,
        &sed.replace,
        global,
        flags.contains('y'),
    ))
}

impl Root {
    /// The plugin's `searchReplace`: `None` while it is off (the pinned rule runs); else what the send path gets,
    /// `None` inside when the text is no sed message (it is sent as it is).
    pub(in crate::ui) fn sed_enhanced_rule(
        &mut self,
        channel: Id,
        content: &str,
        cx: &mut Context<Self>,
    ) -> Option<Option<String>> {
        if !content.starts_with('s') {
            return None;
        }
        let regex_by_default = regex_by_default()?;
        let Some(sed) = parse(content) else {
            return Some(None);
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let target = match &self.replying {
            Some(reply) if Some(reply.author.id) != me => None,
            Some(reply) => Some((reply.id, reply.content.clone(), reply.attachments.len())),
            None => self.last_editable_message(channel).map(|message| {
                (
                    message.id,
                    message.content.clone(),
                    message.attachments.len(),
                )
            }),
        };
        let Some((id, before, attachments)) = target else {
            return Some(Some(String::new()));
        };
        let Some(changed) = apply(&before, &sed, regex_by_default) else {
            tracing::info!("SedEnhanced: the expression does not compile");
            return Some(Some(String::new()));
        };
        self.apply_rule_edit(channel, id, &before, attachments, changed, cx);
        Some(Some(String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, content: &str, by_default: bool) -> Option<String> {
        apply(text, &parse(content).unwrap(), by_default)
    }

    #[test]
    fn text_and_expressions_follow_the_original() {
        assert_eq!(run("a cat", "s/cat/dog", false).as_deref(), Some("a dog"));
        assert_eq!(run("a.b.c", "s/./-/g", false).as_deref(), Some("a-b-c"));
        assert_eq!(run("a.b.c", "s/./-/gr", false).as_deref(), Some("-----"));
        assert_eq!(run("a.b.c", "s/./-/g", true).as_deref(), Some("-----"));
        assert_eq!(run("a.b.c", "s/./-/gr", true).as_deref(), Some("a-b-c"));
        assert_eq!(
            run("Cat cat", "s/cat/dog/gir", false).as_deref(),
            Some("dog dog")
        );
        assert_eq!(
            run("john smith", "s|(\\w+) (\\w+)|$2, $1|r", false).as_deref(),
            Some("smith, john")
        );
        assert_eq!(
            run("john smith", "s|(?<a>\\w+) (?<b>\\w+)|$<b> $<a>|r", false).as_deref(),
            Some("smith john")
        );
        assert_eq!(run("aaba", "s/a/x/gyr", false).as_deref(), Some("xxba"));
        // A pattern that cannot compile, or a repeated flag, sends nothing.
        assert_eq!(run("abc", "s/(/x/r", false), None);
        assert_eq!(run("abc", "s/a/x/ggr", false), None);
        assert_eq!(run("abc", "s/(?=b)/x/r", false), None);
    }
}
