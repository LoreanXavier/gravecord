//! The text rules of the send path (module 751258 `S`, which module 699344 runs on a typed message after its checks
//! and before parsing it): `+:emoji:` reacts to the channel's last message (`reaction`) and `s/find/replace` edits your
//! last message (`searchReplace`). A rule that acts sends nothing of its own (`content: ""`); one that does not act
//! leaves the text to be sent as it is. The edit box runs the same rules with `isEdit`, where neither acts. The
//! other rules of the table (`tts`, `me`, `shrug`, ...) run only for the legacy chat input's typed `/` commands.

use super::Root;
use crate::net::Request;
use dc_state::{Id, Message, MessageState};
use gpui::Context;
use std::sync::LazyLock;

/// `IS_VOICE_MESSAGE`.
const VOICE_MESSAGE: u64 = 1 << 13;

/// `reaction.match`: `^\+:(.+?): *$` (JavaScript's `.` stops at line ends).
static REACTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\+:[^\n\r\u{2028}\u{2029}]+?: *$").expect("the reaction rule compiles")
});

/// `searchReplace.match`: `^s\/([^\/\\]*(?:\\.[^\/\\]*)*)\/([^\/\\]*(?:\\.[^\/\\]*)*)(?:\/([g]*))?$`.
static SEARCH_REPLACE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^s/([^/\\]*(?:\\[^\n\r\u{2028}\u{2029}][^/\\]*)*)/([^/\\]*(?:\\[^\n\r\u{2028}\u{2029}][^/\\]*)*)(?:/(g*))?$",
    )
    .expect("the searchReplace rule compiles")
});

/// The emoji name of a `+:name:` message: `e.trim().slice(2, -1)`, or `None` when the text is no reaction.
pub(super) fn reaction_name(content: &str) -> Option<&str> {
    if !REACTION.is_match(content) {
        return None;
    }
    let trimmed = content.trim();
    trimmed.get(2..trimmed.len() - 1)
}

/// `searchReplace`'s parts of an `s/find/replace/g` message: what to find and put, unescaped (`\*`, `\?`, `\+`, `\/`),
/// and whether every match changes (`g`).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SearchReplace {
    pub find: String,
    pub replace: String,
    pub global: bool,
}

/// `\\([*?+/])` replaced by the character it escapes.
pub(super) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(&next) = chars.peek()
            && matches!(next, '*' | '?' | '+' | '/')
        {
            out.push(next);
            chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

pub(super) fn search_replace(content: &str) -> Option<SearchReplace> {
    let parts = SEARCH_REPLACE.captures(content)?;
    Some(SearchReplace {
        find: unescape(parts.get(1).map_or("", |part| part.as_str())),
        replace: unescape(parts.get(2).map_or("", |part| part.as_str())),
        global: parts
            .get(3)
            .is_some_and(|flags| flags.as_str().contains('g')),
    })
}

/// One match for [`expand`]: the matched text, the text before and after it, and the groups by number and name.
pub(super) struct Found<'a> {
    pub matched: &'a str,
    pub before: &'a str,
    pub after: &'a str,
    /// `groups[0]` is group 1; `None` for a group that took no part. Empty for a text search.
    pub groups: Vec<Option<&'a str>>,
    /// Named groups, if the pattern has any (`$<name>` is literal otherwise).
    pub named: Option<Vec<(&'a str, Option<&'a str>)>>,
}

/// JavaScript's `GetSubstitution`: `$$`, `$&`, `` $` ``, `$'`, `$n` / `$nn` for an existing group and `$<name>` with
/// named groups; anything else stays as written. A text search has no groups, so `$1` stays `$1`.
pub(super) fn expand(replacement: &str, found: &Found) -> String {
    let bytes = replacement.as_bytes();
    let mut out = String::with_capacity(replacement.len());
    let mut index = 0;
    let count = found.groups.len();
    let group = |n: usize| found.groups[n - 1].unwrap_or("");
    while index < bytes.len() {
        if bytes[index] != b'$' || index + 1 >= bytes.len() {
            let next = replacement[index..]
                .chars()
                .next()
                .map_or(1, char::len_utf8);
            out.push_str(&replacement[index..index + next]);
            index += next;
            continue;
        }
        match bytes[index + 1] {
            b'$' => {
                out.push('$');
                index += 2;
            }
            b'&' => {
                out.push_str(found.matched);
                index += 2;
            }
            b'`' => {
                out.push_str(found.before);
                index += 2;
            }
            b'\'' => {
                out.push_str(found.after);
                index += 2;
            }
            digit @ b'0'..=b'9' => {
                let one = usize::from(digit - b'0');
                let two = bytes
                    .get(index + 2)
                    .filter(|next| next.is_ascii_digit())
                    .map(|next| one * 10 + usize::from(next - b'0'));
                if let Some(two) = two.filter(|two| (1..=count).contains(two)) {
                    out.push_str(group(two));
                    index += 3;
                } else if (1..=count).contains(&one) {
                    out.push_str(group(one));
                    index += 2;
                } else {
                    out.push('$');
                    index += 1;
                }
            }
            b'<' if found.named.is_some() => {
                let rest = &replacement[index + 2..];
                match rest.find('>') {
                    Some(end) => {
                        let name = &rest[..end];
                        let value = found
                            .named
                            .as_ref()
                            .and_then(|named| named.iter().find(|(known, _)| *known == name))
                            .and_then(|(_, value)| *value)
                            .unwrap_or("");
                        out.push_str(value);
                        index += 2 + end + 1;
                    }
                    None => {
                        out.push('$');
                        index += 1;
                    }
                }
            }
            _ => {
                out.push('$');
                index += 1;
            }
        }
    }
    out
}

/// `String.prototype.replace` / `replaceAll` with a text to find: the first or every place it is, the replacement
/// expanded at each. An empty text is found before every character and at the end.
pub(super) fn replace_text(text: &str, find: &str, replacement: &str, global: bool) -> String {
    let mut places: Vec<usize> = Vec::new();
    if find.is_empty() {
        places.push(0);
        if global {
            places.extend(text.char_indices().map(|(at, ch)| at + ch.len_utf8()));
        }
    } else if global {
        places.extend(text.match_indices(find).map(|(at, _)| at));
    } else {
        places.extend(text.find(find));
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for at in places {
        out.push_str(&text[last..at]);
        out.push_str(&expand(
            replacement,
            &Found {
                matched: find,
                before: &text[..at],
                after: &text[at + find.len()..],
                groups: Vec::new(),
                named: None,
            },
        ));
        last = at + find.len();
    }
    out.push_str(&text[last..]);
    out
}

impl Root {
    /// `getLastEditableMessage`: your newest message that can be edited (module 697470: sent, a user message, not a
    /// voice message, a poll or a forward).
    pub(super) fn last_editable_message(&self, channel: Id) -> Option<&Message> {
        let me = self.stores.current_user.as_ref()?.id;
        self.stores
            .messages
            .get(&channel)?
            .iter()
            .rev()
            .find(|message| {
                message.author.id == me
                && message.state == MessageState::Sent
                && !dc_state::message_kinds::is_system(message.kind)
                && message.flags & VOICE_MESSAGE == 0
                && message.extra.poll.is_none()
                && message.forwarded.is_none()
                // Not in the pinned client: MessageLogger leaves deleted messages out
                // (`plugin_hooks/message_logger.rs`).
                && !super::plugin_hooks::message_logger_deleted(message.id)
            })
    }

    /// Module 751258 `S` for a message about to be sent: the text to send instead (empty when a rule acted), or
    /// `None` to send it as it is.
    pub(super) fn send_rules(&mut self, content: &str, cx: &mut Context<Self>) -> Option<String> {
        let channel = self.channel?;
        if let Some(name) = reaction_name(content) {
            return self.reaction_rule(channel, 1, name, cx);
        }
        // Not in the pinned client: SedEnhanced's rule (`plugin_hooks/sed_enhanced.rs`).
        if let Some(handled) = self.sed_enhanced_rule(channel, content, cx) {
            return handled;
        }
        let rule = search_replace(content)?;
        let Some(target) = self.last_editable_message(channel) else {
            return Some(String::new());
        };
        let changed = replace_text(&target.content, &rule.find, &rule.replace, rule.global);
        let (id, before, files) = (target.id, target.content.clone(), target.attachments.len());
        self.apply_rule_edit(channel, id, &before, files, changed, cx);
        Some(String::new())
    }

    /// `reaction.action`: the `back`-th message from the channel's newest (the newest for the rule itself), once the
    /// channel reaches the present, gets the emoji named so (`getDisambiguatedEmojiContext(guild).getByName`).
    fn reaction_rule(
        &mut self,
        channel: Id,
        back: usize,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let present = self
            .stores
            .windows
            .get(&channel)
            .is_some_and(|window| window.ready && window.has_present());
        if !present || back == 0 {
            return None;
        }
        let list = self.stores.messages.get(&channel)?;
        let message = list.len().checked_sub(back).and_then(|at| list.get(at))?.id;
        let emoji = self.emoji_route_by_name(name)?;
        tracing::info!("send rule: reacting to {message}");
        self.send_and_apply_reaction(
            channel,
            message,
            super::menu::ReactionOp {
                emoji,
                add: true,
                location: "Message",
                burst: false,
            },
            cx,
        );
        Some(String::new())
    }

    /// `getByName` of the emoji context (module 121681): a unicode emoji by its first name, then the custom emoji of
    /// this server and the others it may use (a name taken already gets `~1`, `~2`, ...), then a unicode alias. The
    /// reaction route: the characters, or `name:id`.
    fn emoji_route_by_name(&self, name: &str) -> Option<String> {
        let unicode = dc_tokens::emoji_by_name(name);
        if let Some(emoji) = unicode.filter(|emoji| emoji.names.first() == Some(&name)) {
            return Some(emoji.surrogates.to_owned());
        }
        let everywhere = dc_state::premium::PremiumType::from_wire(self.premium_wire())
            .has(dc_state::premium::Feature::EmojisEverywhere);
        let here = self.guild;
        let guilds = here.and_then(|id| self.stores.guild(id)).into_iter().chain(
            self.stores
                .guilds
                .iter()
                .filter(|guild| everywhere && Some(guild.id) != here),
        );
        let mut taken: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for guild in guilds {
            for emoji in guild.emojis.iter().filter(|emoji| emoji.available) {
                let count = taken.entry(emoji.name.as_str()).or_insert_with(|| {
                    usize::from(
                        dc_tokens::emoji_by_name(&emoji.name)
                            .is_some_and(|known| known.names.first() == Some(&emoji.name.as_str())),
                    )
                });
                let shown = if *count > 0 {
                    format!("{}~{count}", emoji.name)
                } else {
                    emoji.name.clone()
                };
                *count += 1;
                if shown == name {
                    return Some(format!("{}:{}", emoji.name, emoji.id));
                }
            }
        }
        unicode.map(|emoji| emoji.surrogates.to_owned())
    }

    /// The end of `searchReplace`: an empty text without attachments deletes the message (no confirmation), a
    /// changed one is saved as its edit.
    pub(super) fn apply_rule_edit(
        &mut self,
        channel: Id,
        message: Id,
        before: &str,
        attachments: usize,
        changed: String,
        cx: &mut Context<Self>,
    ) {
        if changed.trim().is_empty() && attachments == 0 {
            tracing::info!("send rule: deleting {message}");
            let _ = self.requests.send(Request::Delete { channel, message });
        } else if changed != before {
            tracing::info!("send rule: editing {message}");
            let _ = self.requests.send(Request::Edit {
                channel,
                message,
                content: changed,
            });
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reaction_rule_takes_the_name_between_the_colons() {
        assert_eq!(reaction_name("+:thumbsup:"), Some("thumbsup"));
        assert_eq!(reaction_name("+:thumbsup:   "), Some("thumbsup"));
        assert_eq!(reaction_name("+:a:b:"), Some("a:b"));
        assert_eq!(reaction_name("++:thumbsup:"), None);
        assert_eq!(reaction_name("+::"), None);
        assert_eq!(reaction_name("x +:thumbsup:"), None);
        assert_eq!(reaction_name("+:a\nb:"), None);
    }

    #[test]
    fn the_sed_rule_follows_the_pinned_pattern() {
        assert_eq!(
            search_replace("s/cat/dog"),
            Some(SearchReplace {
                find: "cat".into(),
                replace: "dog".into(),
                global: false
            })
        );
        assert_eq!(
            search_replace("s/a\\/b/c\\*/g"),
            Some(SearchReplace {
                find: "a/b".into(),
                replace: "c*".into(),
                global: true
            })
        );
        assert_eq!(
            search_replace("s/a/b/").map(|rule| rule.global),
            Some(false)
        );
        // Only `g` is a flag; a third slash needs one.
        assert_eq!(search_replace("s/a/b/i"), None);
        assert_eq!(search_replace("s|a|b"), None);
        assert_eq!(search_replace("say/a/b"), None);
    }

    #[test]
    fn text_replacement_is_javascripts() {
        assert_eq!(
            replace_text("a cat, a cat", "cat", "dog", false),
            "a dog, a cat"
        );
        assert_eq!(
            replace_text("a cat, a cat", "cat", "dog", true),
            "a dog, a dog"
        );
        assert_eq!(replace_text("cat", "cat", "[$&]", false), "[cat]");
        assert_eq!(replace_text("cat", "cat", "$$1 $1", false), "$1 $1");
        assert_eq!(replace_text("a-b", "-", "$`$'", false), "aabb");
        assert_eq!(replace_text("ab", "", "x", false), "xab");
        assert_eq!(replace_text("ab", "", "x", true), "xaxbx");
        assert_eq!(replace_text("ab", "z", "x", true), "ab");
    }

    #[test]
    fn groups_expand_by_number_and_name() {
        let found = Found {
            matched: "ab",
            before: "",
            after: "",
            groups: vec![Some("a"), None],
            named: Some(vec![("x", Some("a"))]),
        };
        assert_eq!(expand("$2$1$3$<x>$<y>$", &found), "a$3a$");
        assert_eq!(expand("$01$10", &found), "aa0");
    }
}
