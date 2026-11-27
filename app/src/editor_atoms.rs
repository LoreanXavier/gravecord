//! The rich editor's inline voids, over the plain text the composer keeps. The official composer is a Slate editor:
//! module 2368 turns tokens in the text into void inline nodes on every change (`userMention`, `roleMention`,
//! `channelMention`, `customEmoji`, `textMention` for `@everyone` and `@here`), not inside inline code or a code
//! block, and not after a backslash; module 151596 writes each node back as its raw token when the message is sent.
//! Module 787564 draws each void as a `contentEditable=false` inline-block span (`.inlineElement_T14dfz`,
//! `.inlineVoid_T14dfz`), so the caret steps over it as one unit and a delete removes all of it.
//!
//! Here the text keeps the raw tokens (`<@id>`); a void is a byte range of the text the editor lays out as one box.
//! Copy writes the `plain` serialization of module 323350 (`setFragmentData` of module 68059), e.g. `@username`.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AtomKind {
    /// `<@id>` or `<@!id>`.
    User(u64),
    /// `<@&id>`.
    Role(u64),
    /// `<#id>`.
    Channel(u64),
    /// `<:name:id>` or `<a:name:id>`.
    Emoji {
        name: String,
        id: u64,
        animated: bool,
    },
    /// `@everyone` or `@here` (`textMention`).
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub range: Range<usize>,
    pub kind: AtomKind,
}

fn digits(text: &str) -> Option<(u64, usize)> {
    let count = text.bytes().take_while(u8::is_ascii_digit).count();
    if count == 0 {
        return None;
    }
    Some((text[..count].parse().ok()?, count))
}

/// The token that starts at the beginning of `rest`, with its length.
fn token_at(before: &str, rest: &str) -> Option<(AtomKind, usize)> {
    // The mention rule's `@everyone` and `@here` stand between word boundaries (`dc_markdown`'s
    // `everyone_here_prefix`, the same rule the message text is parsed with).
    let boundary = |c: char| !c.is_alphanumeric() && c != '_';
    for (word, len) in [("@everyone", 9), ("@here", 5)] {
        if rest.starts_with(word)
            && before.chars().next_back().is_none_or(boundary)
            && rest[len..].chars().next().is_none_or(boundary)
        {
            return Some((AtomKind::Text(word.to_owned()), len));
        }
    }
    let inner = rest.strip_prefix('<')?;
    let closed = |kind: AtomKind, used: usize| {
        (inner.as_bytes().get(used) == Some(&b'>')).then_some((kind, used + 2))
    };
    if let Some(after) = inner.strip_prefix("@&") {
        let (id, count) = digits(after)?;
        return closed(AtomKind::Role(id), 2 + count);
    }
    if let Some(after) = inner.strip_prefix('@') {
        let bang = usize::from(after.starts_with('!'));
        let (id, count) = digits(&after[bang..])?;
        return closed(AtomKind::User(id), 1 + bang + count);
    }
    if let Some(after) = inner.strip_prefix('#') {
        let (id, count) = digits(after)?;
        return closed(AtomKind::Channel(id), 1 + count);
    }
    // customEmoji: `<(a)?:(\w+):(\d+)>`.
    let (animated, after) = match inner.strip_prefix("a:") {
        Some(after) => (true, after),
        None => (false, inner.strip_prefix(':')?),
    };
    let name_len = after
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    if name_len == 0 || after.as_bytes().get(name_len) != Some(&b':') {
        return None;
    }
    let (id, count) = digits(&after[name_len + 1..])?;
    let used = usize::from(animated) + 1 + name_len + 1 + count;
    closed(
        AtomKind::Emoji {
            name: after[..name_len].to_owned(),
            id,
            animated,
        },
        used,
    )
}

/// The bytes of `text` inside inline code or a fenced code block, where no void forms (module 2368's `v()` check of
/// `codeBlockText` and `inlineCode`).
fn code_mask(text: &str) -> Vec<bool> {
    let mut mask = vec![false; text.len()];
    // Fenced blocks first: ``` up to the next ```.
    let mut at = 0;
    while let Some(open) = text[at..].find("```") {
        let start = at + open;
        let Some(close) = text[start + 3..].find("```") else {
            break;
        };
        let end = start + 3 + close + 3;
        mask[start..end].iter_mut().for_each(|slot| *slot = true);
        at = end;
    }
    // Inline code per paragraph, from the preview's own flags.
    let mut offset = 0;
    for paragraph in text.split('\n') {
        for (index, flag) in crate::markup_preview::flags(paragraph)
            .into_iter()
            .enumerate()
        {
            if flag & crate::markup_preview::CODE != 0 {
                mask[offset + index] = true;
            }
        }
        offset += paragraph.len() + 1;
    }
    mask
}

/// Every void of `text`, in order.
pub fn find_atoms(text: &str) -> Vec<Atom> {
    let mask = code_mask(text);
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let first = rest.as_bytes()[0];
        if (first == b'<' || first == b'@')
            && !mask[at]
            && !text[..at].ends_with('\\')
            && let Some((kind, len)) = token_at(&text[..at], rest)
        {
            out.push(Atom {
                range: at..at + len,
                kind,
            });
            at += len;
            continue;
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Which way a caret that landed inside a void goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    Start,
    End,
    /// To the nearer edge (a click).
    Nearest,
}

/// An offset inside a void moved to its edge.
pub fn snap(atoms: &[Atom], at: usize, bias: Bias) -> usize {
    for atom in atoms {
        if atom.range.start < at && at < atom.range.end {
            return match bias {
                Bias::Start => atom.range.start,
                Bias::End => atom.range.end,
                Bias::Nearest => {
                    if at - atom.range.start <= atom.range.end - at {
                        atom.range.start
                    } else {
                        atom.range.end
                    }
                }
            };
        }
    }
    at
}

/// A selection grown so no void is cut: each end that falls inside one moves outward.
pub fn grow(atoms: &[Atom], range: Range<usize>) -> Range<usize> {
    snap(atoms, range.start, Bias::Start)..snap(atoms, range.end, Bias::End)
}

/// The void a Backspace at `caret` removes whole: the one that ends there.
pub fn ending_at(atoms: &[Atom], caret: usize) -> Option<Range<usize>> {
    atoms
        .iter()
        .find(|atom| atom.range.end == caret)
        .map(|atom| atom.range.clone())
}

/// The void a Delete at `caret` removes whole: the one that starts there.
pub fn starting_at(atoms: &[Atom], caret: usize) -> Option<Range<usize>> {
    atoms
        .iter()
        .find(|atom| atom.range.start == caret)
        .map(|atom| atom.range.clone())
}

/// The text a copy of `range` writes: each void as its plain label (`mode: "plain"` of module 323350), the rest as
/// typed. A void with no label is copied raw, as the serializer does for a user or channel it does not know.
pub fn plain_text(
    text: &str,
    range: Range<usize>,
    atoms: &[Atom],
    label: impl Fn(&AtomKind) -> Option<String>,
) -> String {
    let mut out = String::new();
    let mut at = range.start;
    for atom in atoms
        .iter()
        .filter(|atom| range.start <= atom.range.start && atom.range.end <= range.end)
    {
        out.push_str(&text[at..atom.range.start]);
        match label(&atom.kind) {
            Some(plain) => out.push_str(&plain),
            None => out.push_str(&text[atom.range.clone()]),
        }
        at = atom.range.end;
    }
    out.push_str(&text[at..range.end]);
    out
}

/// What a raw mention typed as text names (the groups of module 2368's `m`): `@name` or `@name#1234`, `:name:`, or
/// `#name` / `#"quoted name"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawKind {
    At {
        name: String,
        discriminator: Option<String>,
    },
    Emoji(String),
    Channel(String),
}

fn delimiter(c: char, hash: bool) -> bool {
    c.is_whitespace() || c == '@' || c == ':' || (hash && c == '#')
}

/// The raw mention that `m` matches at the start of `rest`, with its length. The lookaheads of the regex need the
/// delimiter after the word to be there already: a mention turns into a void once the word is finished.
fn raw_at(rest: &str) -> Option<(RawKind, usize)> {
    let first = rest.chars().next()?;
    // Past the first character, not its first byte: a word may start with a letter of two or more bytes (`ç`).
    let body = &rest[first.len_utf8()..];
    match first {
        '@' => {
            // `@[^@#]+(?:#0|#\d{4})`, within the line.
            let run = body
                .find(['@', '#', '\n'])
                .unwrap_or(body.len());
            if run > 0 && body[run..].starts_with('#') {
                let after = &body[run + 1..];
                let digits = after.bytes().take_while(u8::is_ascii_digit).count();
                let used = if after.starts_with('0') {
                    Some(1)
                } else if digits >= 4 {
                    Some(4)
                } else {
                    None
                };
                if let Some(used) = used {
                    return Some((
                        RawKind::At {
                            name: body[..run].to_owned(),
                            discriminator: Some(after[..used].to_owned()),
                        },
                        1 + run + 1 + used,
                    ));
                }
            }
            // `@[^\s\t@#:]+(?=[\s\t@:])`
            let run = body
                .find(|c: char| c.is_whitespace() || c == '@' || c == '#' || c == ':')
                .unwrap_or(body.len());
            let next = body[run..].chars().next()?;
            (run > 0 && delimiter(next, false)).then(|| {
                (
                    RawKind::At {
                        name: body[..run].to_owned(),
                        discriminator: None,
                    },
                    1 + run,
                )
            })
        }
        ':' => {
            // `:[a-zA-Z0-9_~]+:`
            let run = body
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'~')
                .count();
            (run > 0 && body[run..].starts_with(':'))
                .then(|| (RawKind::Emoji(body[..run].to_owned()), run + 2))
        }
        '#' => {
            if let Some(quoted) = body.strip_prefix('"') {
                // `#"(?:\\\\|\\"|(?!")[^\\])+"`, unescaped as module 47167 `LG` does.
                let mut name = String::new();
                let mut chars = quoted.char_indices();
                while let Some((at, c)) = chars.next() {
                    match c {
                        '"' if !name.is_empty() => {
                            return Some((RawKind::Channel(name), 2 + at + 1));
                        }
                        '"' => return None,
                        '\\' => match chars.next() {
                            Some((_, escaped @ ('\\' | '"'))) => name.push(escaped),
                            _ => return None,
                        },
                        other => name.push(other),
                    }
                }
                return None;
            }
            // `#[^\s\t@#:]+(?=[\s\t@#:])`
            let run = body
                .find(|c: char| c.is_whitespace() || c == '@' || c == '#' || c == ':')
                .unwrap_or(body.len());
            let next = body[run..].chars().next()?;
            (run > 0 && delimiter(next, true))
                .then(|| (RawKind::Channel(body[..run].to_owned()), 1 + run))
        }
        _ => None,
    }
}

/// Module 2368's first pass: each raw mention that starts the text, a line or the text after a void, or follows a
/// space, outside code, which `resolve` turns into a token. A match that does not resolve is skipped one character
/// on (`lastIndex = index + 1`); one that does continues after itself.
pub fn raw_mentions(
    text: &str,
    mut resolve: impl FnMut(&RawKind) -> Option<String>,
) -> Vec<(Range<usize>, String)> {
    let mask = code_mask(text);
    let void_ends: Vec<usize> = find_atoms(text).iter().map(|atom| atom.range.end).collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let starts = at == 0
            || void_ends.contains(&at)
            || text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if starts
            && !mask[at]
            && let Some((kind, len)) = raw_at(rest)
            && let Some(token) = resolve(&kind)
        {
            out.push((at..at + len, token));
            at += len;
            continue;
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Range<usize>, AtomKind)> {
        find_atoms(text)
            .into_iter()
            .map(|atom| (atom.range, atom.kind))
            .collect()
    }

    fn raw(text: &str) -> Vec<(Range<usize>, RawKind)> {
        let mut seen = Vec::new();
        let found = raw_mentions(text, |kind| {
            seen.push(kind.clone());
            Some(String::new())
        });
        found
            .into_iter()
            .zip(seen)
            .map(|((range, _), kind)| (range, kind))
            .collect()
    }

    fn at(name: &str, discriminator: Option<&str>) -> RawKind {
        RawKind::At {
            name: name.into(),
            discriminator: discriminator.map(Into::into),
        }
    }

    #[test]
    fn raw_mentions_wait_for_the_word_to_end() {
        assert!(raw("hi @bob").is_empty());
        assert_eq!(raw("hi @bob "), vec![(3..7, at("bob", None))]);
        assert_eq!(raw("@bob#1234"), vec![(0..9, at("bob", Some("1234")))]);
        assert_eq!(raw("@old name#0"), vec![(0..11, at("old name", Some("0")))]);
        assert_eq!(
            raw("#general "),
            vec![(0..8, RawKind::Channel("general".into()))]
        );
        assert_eq!(
            raw("#\"my \\\"room\\\"\""),
            vec![(0..14, RawKind::Channel("my \"room\"".into()))]
        );
        assert_eq!(raw(":blob:"), vec![(0..6, RawKind::Emoji("blob".into()))]);
    }

    #[test]
    fn raw_mentions_need_a_space_or_a_void_before_and_skip_code() {
        assert!(raw("mail@bob ").is_empty());
        assert!(raw("`@bob `").is_empty());
        assert_eq!(raw("<@1>@bob "), vec![(4..8, at("bob", None))]);
        assert_eq!(raw("a\n@bob "), vec![(2..6, at("bob", None))]);
    }

    #[test]
    fn an_unresolved_match_is_skipped_and_a_resolved_one_becomes_its_token() {
        let found = raw_mentions("@nobody @bob ", |kind| match kind {
            RawKind::At { name, .. } if name == "bob" => Some("<@7>".into()),
            _ => None,
        });
        assert_eq!(found, vec![(8..12, "<@7>".to_owned())]);
    }

    #[test]
    fn tokens_become_voids() {
        assert_eq!(
            kinds("hi <@174540485778472960> and <@!5>"),
            vec![
                (3..24, AtomKind::User(174540485778472960)),
                (29..34, AtomKind::User(5)),
            ]
        );
        assert_eq!(kinds("<@&9>"), vec![(0..5, AtomKind::Role(9))]);
        assert_eq!(kinds("<#12>"), vec![(0..5, AtomKind::Channel(12))]);
        assert_eq!(
            kinds("<:blob:77><a:dance:8>"),
            vec![
                (
                    0..10,
                    AtomKind::Emoji {
                        name: "blob".into(),
                        id: 77,
                        animated: false
                    }
                ),
                (
                    10..21,
                    AtomKind::Emoji {
                        name: "dance".into(),
                        id: 8,
                        animated: true
                    }
                ),
            ]
        );
        assert_eq!(
            kinds("@everyone @here"),
            vec![
                (0..9, AtomKind::Text("@everyone".into())),
                (10..15, AtomKind::Text("@here".into())),
            ]
        );
    }

    #[test]
    fn unfinished_escaped_and_code_tokens_stay_text() {
        assert!(kinds("<@123").is_empty());
        assert!(kinds("<@>").is_empty());
        assert!(kinds("<:x:>").is_empty());
        assert!(kinds("\\<@123>").is_empty());
        assert!(kinds("`<@123>`").is_empty());
        assert!(kinds("```\n<@123>\n```").is_empty());
        assert_eq!(kinds("`a` <@1>"), vec![(4..8, AtomKind::User(1))]);
        assert!(kinds("bob@hereford.com @everyonex user@everyone").is_empty());
        assert_eq!(
            kinds("(@here) @everyone-free"),
            vec![
                (1..6, AtomKind::Text("@here".into())),
                (8..17, AtomKind::Text("@everyone".into())),
            ]
        );
    }

    #[test]
    fn the_caret_steps_over_a_void() {
        let atoms = find_atoms("a <@123> b");
        // Left from the void's end lands inside it, then snaps to its start.
        assert_eq!(snap(&atoms, 7, Bias::Start), 2);
        assert_eq!(snap(&atoms, 3, Bias::End), 8);
        assert_eq!(snap(&atoms, 3, Bias::Nearest), 2);
        assert_eq!(snap(&atoms, 7, Bias::Nearest), 8);
        assert_eq!(snap(&atoms, 2, Bias::End), 2);
        assert_eq!(snap(&atoms, 8, Bias::Start), 8);
        assert_eq!(grow(&atoms, 4..9), 2..9);
        assert_eq!(grow(&atoms, 0..5), 0..8);
    }

    #[test]
    fn backspace_and_delete_take_the_whole_void() {
        let atoms = find_atoms("a <@123> b");
        assert_eq!(ending_at(&atoms, 8), Some(2..8));
        assert_eq!(ending_at(&atoms, 9), None);
        assert_eq!(starting_at(&atoms, 2), Some(2..8));
        assert_eq!(starting_at(&atoms, 1), None);
    }

    #[test]
    fn copy_writes_plain_labels() {
        let text = "hey <@1> in <#2> <@3>";
        let atoms = find_atoms(text);
        let label = |kind: &AtomKind| match kind {
            AtomKind::User(1) => Some("@alice".to_owned()),
            AtomKind::Channel(2) => Some("#general".to_owned()),
            _ => None,
        };
        assert_eq!(
            plain_text(text, 0..text.len(), &atoms, label),
            "hey @alice in #general <@3>"
        );
        assert_eq!(plain_text(text, 0..3, &atoms, label), "hey");
    }

    #[test]
    fn words_that_start_with_a_letter_of_several_bytes_are_scanned_without_a_panic() {
        // A word starting with `ç`, `ş`, `ğ` or an emoji once cut the text inside that letter.
        for text in [
            "çok güzel @ali şimdi #genel",
            "ç",
            "ğ@ş:#",
            "@çağrı ",
            "#çay ",
            ":çiçek:",
            "😀 @😀 #😀 :😀: ",
            "e\u{301}@x ",
            "İstanbul ıı @İ: #ş\n@ç#1234 ",
            "日本語を入力 @テスト #チャンネル :絵文字: ",
            "にほんご",
            "漢@字#か:な",
        ] {
            let found = raw_mentions(text, |kind| Some(format!("{kind:?}")));
            for (range, _) in &found {
                assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
            }
            let atoms = find_atoms(text);
            let _ = plain_text(text, 0..text.len(), &atoms, |_| None);
        }
        let found = raw_mentions("merhaba @çağrı ", |kind| match kind {
            RawKind::At { name, .. } => Some(format!("<@{name}>")),
            _ => None,
        });
        assert_eq!(found, vec![(8..17, "<@çağrı>".to_owned())]);
    }

    #[test]
    fn mixed_letters_and_mention_marks_never_cut_inside_a_letter() {
        const PIECES: &[&str] = &[
            "ç", "ş", "ğ", "ı", "İ", "😀", "e\u{301}", "a", "1", " ", "\n", "@", "#", ":", "\"", "\\", "<@",
            "<#", "<:", ">", "`", "```", "0", "1234",
        ];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20_000 {
            let mut text = String::new();
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            for _ in 0..(state >> 59) + 1 {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                text.push_str(PIECES[(state >> 33) as usize % PIECES.len()]);
            }
            let atoms = find_atoms(&text);
            let _ = plain_text(&text, 0..text.len(), &atoms, |_| None);
            let _ = raw_mentions(&text, |kind| Some(format!("{kind:?}")));
        }
    }
}
