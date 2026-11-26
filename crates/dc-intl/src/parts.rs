//! Texts the interface draws in parts (plain words, links, bold words), put back into the locale whole.
//!
//! A settings description such as "Control who can see your profile info … accounts. " + link "Learn more" is, in
//! the catalog, either one message with a `$link` tag or two literal messages. [`parts`] finds the message whose
//! literal text and `$link` / `$b` tags read exactly as the parts do and formats it in the current locale (the order
//! of words and links follows the locale's message); failing that, each part that is a whole literal message by
//! itself (its spaces aside) takes its translation; the rest stays as written.

use crate::ast::Node;
use crate::{DEFAULT_LOCALE, Resolved};
use std::collections::HashMap;
use std::sync::OnceLock;

/// How a part is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    /// `$link`.
    Link,
    /// `$b`.
    Bold,
}

const LINK: char = '\u{1}';
const BOLD: char = '\u{2}';

/// The message's text with its `$link` and `$b` tags marked, when it is literals and such tags around literals.
fn shape(nodes: &[Node]) -> Option<String> {
    let mut out = String::new();
    let mut tagged = false;
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Tag { name, children, .. } if &**name == "$link" || &**name == "$b" => {
                let mark = if &**name == "$link" { LINK } else { BOLD };
                out.push(mark);
                out.push_str(&crate::ast::literal(children)?);
                out.push(mark);
                tagged = true;
            }
            _ => return None,
        }
    }
    tagged.then_some(out)
}

/// en-US's tagged literal messages by their shape, the first key in catalog order kept.
fn shapes() -> &'static HashMap<String, Box<str>> {
    static SHAPES: OnceLock<HashMap<String, Box<str>>> = OnceLock::new();
    SHAPES.get_or_init(|| {
        let mut map = HashMap::new();
        crate::en_us().each_message(|key, message| {
            if let Some(shape) = shape(&message.nodes) {
                map.entry(shape).or_insert_with(|| key.into());
            }
        });
        map
    })
}

/// The key of the en-US message that reads as `parts`, tags included.
pub fn key_of(parts: &[(&str, Style)]) -> Option<&'static str> {
    let mut whole = String::new();
    for (text, style) in parts {
        match style {
            Style::Plain => whole.push_str(text),
            Style::Link => {
                whole.push(LINK);
                whole.push_str(text);
                whole.push(LINK);
            }
            Style::Bold => {
                whole.push(BOLD);
                whole.push_str(text);
                whole.push(BOLD);
            }
        }
    }
    let shapes = shapes();
    shapes
        .get(&whole)
        .or_else(|| shapes.get(whole.trim_end()))
        .map(|key| &**key)
}

/// `parts` in the current locale (see the module's notes). In en-US they come back as they are.
pub fn parts(parts: &[(&str, Style)]) -> Vec<(String, Style)> {
    let unchanged = || {
        parts
            .iter()
            .map(|(text, style)| ((*text).to_owned(), *style))
            .collect()
    };
    let (locale, catalog) = crate::current();
    if locale == DEFAULT_LOCALE {
        return unchanged();
    }
    if parts.len() > 1
        && let Some(key) = key_of(parts)
        && catalog.is_some_and(|catalog| catalog.get(key).is_some())
    {
        let mut out: Vec<(String, Style)> = crate::format_runs(key, &[])
            .into_iter()
            .map(|(text, tag)| {
                let style = match tag.as_deref() {
                    Some("$link") => Style::Link,
                    Some("$b") => Style::Bold,
                    _ => Style::Plain,
                };
                (text, style)
            })
            .collect();
        // The en-US parts' trailing space (a gap before what the view draws next) is kept.
        let trailing = &parts[parts.len() - 1].0[parts[parts.len() - 1].0.trim_end().len()..];
        if let Some((last, Style::Plain)) = out.last_mut()
            && !trailing.is_empty()
            && !last.ends_with(trailing)
        {
            last.push_str(trailing);
        }
        return out;
    }
    parts
        .iter()
        .map(|(text, style)| {
            let core = text.trim();
            let shown = match crate::resolve(core, catalog) {
                Resolved::One(found) | Resolved::Ambiguous(found) if !core.is_empty() => {
                    let lead = &text[..text.len() - text.trim_start().len()];
                    let tail = &text[text.trim_end().len()..];
                    format!("{lead}{found}{tail}")
                }
                _ => (*text).to_owned(),
            };
            (shown, *style)
        })
        .collect()
}
