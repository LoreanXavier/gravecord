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
fn token_at(rest: &str) -> Option<(AtomKind, usize)> {
    for (word, len) in [("@everyone", 9), ("@here", 5)] {
        if rest.starts_with(word) {
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
            && let Some((kind, len)) = token_at(rest)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Range<usize>, AtomKind)> {
        find_atoms(text)
            .into_iter()
            .map(|atom| (atom.range, atom.kind))
            .collect()
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
}
