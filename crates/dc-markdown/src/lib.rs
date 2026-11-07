//! Message markdown. Port of the rules in modules 605275, 223637 (heading), 683412 (subtext) and 873879 (list), on the
//! simple-markdown engine in 478676. Rust `regex` has no lookaround, so matchers are written by hand.

pub mod highlight;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub spoiler: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    Link(String),
    User(u64),
    Role(u64),
    Channel(u64),
    /// `@everyone` or `@here`, kept as written.
    Everyone,
    Emoji {
        name: String,
        id: u64,
        animated: bool,
    },
    Timestamp {
        unix: i64,
        format: char,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Quote(Vec<Span>),
    Code {
        language: Option<String>,
        text: String,
    },
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Subtext(Vec<Span>),
    /// One list row. `number` is set when the list is ordered, which its first item decides.
    ListItem {
        depth: u8,
        number: Option<u32>,
        spans: Vec<Span>,
    },
}

/// `^ *(#{1,3})(?:\s+)((?!\s*#{1,3}\s)[^\n]+?)#*\s*(?:\n|$)`
fn heading(line: &str) -> Option<(u8, &str)> {
    let rest = line.trim_start_matches(' ');
    let hashes = rest.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let after = &rest[hashes..];
    let body = after.trim_start();
    if body.len() == after.len() || body.is_empty() {
        return None;
    }
    let lead = body.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=3).contains(&lead) && body[lead..].starts_with(char::is_whitespace) {
        return None;
    }
    let text = body.trim_end().trim_end_matches('#').trim_end();
    (!text.is_empty()).then_some((hashes as u8, text))
}

/// `^ *-# +((?!-#)[^\n]+)(?:\n|$)`
fn subtext(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ').strip_prefix("-#")?;
    let body = rest.trim_start_matches(' ');
    (body.len() != rest.len() && !body.is_empty() && !body.starts_with("-#")).then_some(body.trim())
}

/// `( *)([*-]|\d+\.) +`
fn list_item(line: &str) -> Option<(usize, Option<u32>, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, after) = if digits > 0 && rest[digits..].starts_with('.') {
        (rest[..digits].parse().ok(), &rest[digits + 1..])
    } else if rest.starts_with(['*', '-']) {
        (None, &rest[1..])
    } else {
        return None;
    };
    let body = after.trim_start_matches(' ');
    (body.len() != after.len() && !body.is_empty()).then_some((indent, number, body))
}

/// `^```(?:([a-z0-9_+\-.#]+?)\n)?\n*([^\n][^]*?)\n*```` (case-insensitive).
fn code_block(source: &str) -> Option<(Option<String>, String, usize)> {
    let after = source.strip_prefix("```")?;
    let end = after.find("```")?;
    let body = &after[..end];
    let (language, text) = match body.split_once('\n') {
        Some((first, rest))
            if !first.is_empty()
                && first
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_+-.#".contains(c)) =>
        {
            (Some(first.to_owned()), rest)
        }
        _ => (None, body),
    };
    let text = text.trim_start_matches('\n').trim_end_matches('\n');
    if text.is_empty() {
        return None;
    }
    let consumed = 3 + end + 3;
    Some((language, text.to_owned(), consumed))
}

pub fn parse(source: &str) -> Vec<Block> {
    let source = source.replace("\r\n", "\n");
    let mut blocks = Vec::new();
    let mut rest = source.as_str();
    let mut list_ordered: Option<bool> = None;
    let mut list_number = 0;
    while !rest.is_empty() {
        if let Some((language, text, consumed)) = code_block(rest) {
            blocks.push(Block::Code { language, text });
            rest = rest[consumed..]
                .strip_prefix('\n')
                .unwrap_or(&rest[consumed..]);
            list_ordered = None;
            continue;
        }
        if let Some(quoted) = rest.strip_prefix(">>> ") {
            for line in quoted.lines() {
                blocks.push(Block::Quote(inline(line)));
            }
            break;
        }
        let (line, next) = rest.split_once('\n').unwrap_or((rest, ""));
        rest = next;
        if let Some((indent, number, body)) = list_item(line) {
            // The first item decides the list; later items follow it whatever marker they use.
            let ordered = *list_ordered.get_or_insert(number.is_some());
            if ordered && indent == 0 {
                list_number = if list_number == 0 {
                    number.unwrap_or(1)
                } else {
                    list_number + 1
                };
            }
            let depth = (indent / 2).min(10) as u8;
            let number = (ordered && depth == 0).then_some(list_number);
            blocks.push(Block::ListItem {
                depth,
                number,
                spans: inline(body),
            });
            continue;
        }
        list_ordered = None;
        list_number = 0;
        if let Some((level, text)) = heading(line) {
            blocks.push(Block::Heading {
                level,
                spans: inline(text),
            });
        } else if let Some(text) = subtext(line) {
            blocks.push(Block::Subtext(inline(text)));
        } else if let Some(quoted) = line.trim_start_matches(' ').strip_prefix("> ") {
            blocks.push(Block::Quote(inline(quoted)));
        } else {
            blocks.push(Block::Paragraph(inline(line)));
        }
    }
    blocks
}

type Apply = fn(&mut Style);

/// Delimiters in match order: `**` before `*`, `__` before `_`.
const MARKS: [(&str, Apply); 7] = [
    ("**", |style| style.bold = true),
    ("__", |style| style.underline = true),
    ("~~", |style| style.strike = true),
    ("||", |style| style.spoiler = true),
    ("*", |style| style.italic = true),
    ("_", |style| style.italic = true),
    ("`", |style| style.code = true),
];

pub fn inline(source: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    walk(source, Style::default(), &mut spans);
    spans
}

fn push(text: &str, style: Style, spans: &mut Vec<Span>) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.style == style && last.kind == Kind::Text => last.text.push_str(text),
        _ => spans.push(Span {
            text: text.to_owned(),
            style,
            kind: Kind::Text,
        }),
    }
}

fn digits(text: &str) -> &str {
    &text[..text.bytes().take_while(u8::is_ascii_digit).count()]
}

/// `<@123>`, `<@!123>`, `<@&123>`, `<#123>`, `<:name:123>`, `<a:name:123>`, `<t:123>`, `<t:123:R>`.
fn angle_token(rest: &str) -> Option<(usize, Kind, String)> {
    let body = rest.strip_prefix('<')?;
    let end = body.find('>')?;
    let inner = &body[..end];
    let consumed = end + 2;
    let raw = rest[..consumed].to_owned();
    let whole_number = |text: &str| {
        (!text.is_empty() && digits(text).len() == text.len())
            .then(|| text.parse().ok())
            .flatten()
    };
    if let Some(id) = inner.strip_prefix("@&").and_then(whole_number) {
        return Some((consumed, Kind::Role(id), raw));
    }
    if let Some(id) = inner
        .strip_prefix("@!")
        .or(inner.strip_prefix('@'))
        .and_then(whole_number)
    {
        return Some((consumed, Kind::User(id), raw));
    }
    if let Some(id) = inner.strip_prefix('#').and_then(whole_number) {
        return Some((consumed, Kind::Channel(id), raw));
    }
    if let Some(stamp) = inner.strip_prefix("t:") {
        let (time, format) = stamp
            .split_once(':')
            .map_or((stamp, 'f'), |(time, format)| {
                (time, format.chars().next().unwrap_or('f'))
            });
        let valid = "tTdDfFR".contains(format) && !stamp.ends_with(':');
        return (valid
            && digits(time.trim_start_matches('-')).len() == time.trim_start_matches('-').len())
        .then(|| time.parse().ok())
        .flatten()
        .map(|unix| (consumed, Kind::Timestamp { unix, format }, raw));
    }
    let (animated, emoji) = match inner.strip_prefix("a:") {
        Some(rest) => (true, rest),
        None => (false, inner.strip_prefix(':')?),
    };
    let (name, id) = emoji.split_once(':')?;
    let id = whole_number(id)?;
    let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    valid.then(|| {
        let name = name.to_owned();
        (consumed, Kind::Emoji { name, id, animated }, raw)
    })
}

/// `^((?:https?|steam):\/\/[^\s<]+[^<.,:;"'\]\s])`
fn url(rest: &str) -> Option<usize> {
    let scheme = ["https://", "http://", "steam://"]
        .iter()
        .find(|scheme| rest.starts_with(**scheme))?;
    let body = &rest[scheme.len()..];
    let run = body
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(body.len());
    let trimmed = body[..run].trim_end_matches(['.', ',', ':', ';', '"', '\'', ']']);
    (!trimmed.is_empty()).then_some(scheme.len() + trimmed.len())
}

/// `[label](url)` with an http or https target.
fn masked_link(rest: &str) -> Option<(usize, &str, &str)> {
    let body = rest.strip_prefix('[')?;
    let close = body.find("](")?;
    let label = &body[..close];
    let target = &body[close + 2..];
    let end = target.find(')')?;
    let address = &target[..end];
    let valid =
        !label.is_empty() && (address.starts_with("https://") || address.starts_with("http://"));
    valid.then_some((close + end + 4, label, address))
}

/// The first closing mark with text before it. As in the original `\*\*(?!\*)`, a closer cannot be followed by more of
/// its own character, and `~~` cannot be followed by `_`.
fn closing(body: &str, mark: &str) -> Option<usize> {
    let repeat = mark.chars().next()?;
    let mut from = 0;
    while let Some(found) = body[from..].find(mark) {
        let end = from + found;
        let after = &body[end + mark.len()..];
        let blocked = after.starts_with(repeat) || (mark == "~~" && after.starts_with('_'));
        if end > 0 && !blocked {
            return Some(end);
        }
        from = end + repeat.len_utf8();
    }
    None
}

fn walk(source: &str, style: Style, spans: &mut Vec<Span>) {
    let mut at = 0;
    while at < source.len() {
        let rest = &source[at..];
        if let Some(next) = rest.strip_prefix('\\')
            && let Some(escaped) = next.chars().next().filter(|c| !c.is_alphanumeric())
        {
            push(&escaped.to_string(), style, spans);
            at += 1 + escaped.len_utf8();
            continue;
        }
        if let Some((consumed, kind, text)) = angle_token(rest) {
            spans.push(Span { text, style, kind });
            at += consumed;
            continue;
        }
        if rest.starts_with("@everyone") || rest.starts_with("@here") {
            let length = if rest.starts_with("@everyone") { 9 } else { 5 };
            spans.push(Span {
                text: rest[..length].to_owned(),
                style,
                kind: Kind::Everyone,
            });
            at += length;
            continue;
        }
        if let Some((consumed, label, address)) = masked_link(rest) {
            spans.push(Span {
                text: label.to_owned(),
                style,
                kind: Kind::Link(address.to_owned()),
            });
            at += consumed;
            continue;
        }
        if let Some(length) = url(rest) {
            spans.push(Span {
                text: rest[..length].to_owned(),
                style,
                kind: Kind::Link(rest[..length].to_owned()),
            });
            at += length;
            continue;
        }
        let mark = MARKS.iter().find_map(|(mark, apply)| {
            let body = rest.strip_prefix(mark)?;
            let end = closing(body, mark)?;
            Some((mark.len(), end, apply))
        });
        if let Some((length, end, apply)) = mark {
            let inner = &rest[length..length + end];
            let mut nested = style;
            apply(&mut nested);
            if nested.code {
                push(inner, nested, spans);
            } else {
                walk(inner, nested, spans);
            }
            at += length * 2 + end;
            continue;
        }
        let character = rest.chars().next().unwrap_or(' ');
        push(&rest[..character.len_utf8()], style, spans);
        at += character.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled(spans: &[Span]) -> Vec<(&str, Style)> {
        spans
            .iter()
            .map(|span| (span.text.as_str(), span.style))
            .collect()
    }

    fn kinds(source: &str) -> Vec<Kind> {
        inline(source)
            .into_iter()
            .map(|span| span.kind)
            .filter(|kind| *kind != Kind::Text)
            .collect()
    }

    #[test]
    fn plain_text_stays_one_span() {
        assert_eq!(
            styled(&inline("hello there")),
            [("hello there", Style::default())]
        );
    }

    #[test]
    fn fixture_line_splits_into_styled_spans() {
        let spans = inline("**bold** *italic* __underline__ ~~strike~~ `code`");
        let texts: Vec<&str> = spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "bold",
                " ",
                "italic",
                " ",
                "underline",
                " ",
                "strike",
                " ",
                "code"
            ]
        );
        assert!(spans[0].style.bold);
        assert!(spans[2].style.italic);
        assert!(spans[4].style.underline);
        assert!(spans[6].style.strike);
        assert!(spans[8].style.code);
    }

    #[test]
    fn styles_nest() {
        let spans = inline("**a *b***");
        assert!(
            spans
                .iter()
                .any(|span| span.style.bold && span.style.italic)
        );
    }

    #[test]
    fn unmatched_marks_are_literal() {
        assert_eq!(styled(&inline("2 * 3")), [("2 * 3", Style::default())]);
    }

    #[test]
    fn escapes_remove_the_mark() {
        assert_eq!(
            styled(&inline(r"\*not italic\*")),
            [("*not italic*", Style::default())]
        );
    }

    #[test]
    fn code_keeps_marks_literal() {
        assert_eq!(inline("`**x**`")[0].text, "**x**");
    }

    #[test]
    fn strike_cannot_close_before_an_underscore() {
        assert!(!inline("~~a~~_ b")[0].style.strike);
    }

    #[test]
    fn multibyte_text_is_safe() {
        assert_eq!(inline("çığ **öş** ü")[1].text, "öş");
    }

    #[test]
    fn mentions_parse_to_their_ids() {
        assert_eq!(kinds("<@12>"), [Kind::User(12)]);
        assert_eq!(kinds("<@!12>"), [Kind::User(12)]);
        assert_eq!(kinds("<@&34>"), [Kind::Role(34)]);
        assert_eq!(kinds("<#56>"), [Kind::Channel(56)]);
        assert_eq!(kinds("hi @everyone-free"), [Kind::Everyone]);
        assert_eq!(kinds("@here"), [Kind::Everyone]);
    }

    #[test]
    fn broken_tokens_stay_text() {
        assert!(kinds("<@abc> <#> <@&> <t:x>").is_empty());
    }

    #[test]
    fn custom_emoji_keep_name_and_animation() {
        assert_eq!(
            kinds("<:wave:9>"),
            [Kind::Emoji {
                name: "wave".into(),
                id: 9,
                animated: false
            }]
        );
        assert_eq!(
            kinds("<a:spin:8>"),
            [Kind::Emoji {
                name: "spin".into(),
                id: 8,
                animated: true
            }]
        );
    }

    #[test]
    fn timestamps_default_to_f() {
        assert_eq!(
            kinds("<t:1794000000>"),
            [Kind::Timestamp {
                unix: 1794000000,
                format: 'f'
            }]
        );
        assert_eq!(
            kinds("<t:1794000000:R>"),
            [Kind::Timestamp {
                unix: 1794000000,
                format: 'R'
            }]
        );
        assert!(kinds("<t:1794000000:Z>").is_empty());
    }

    #[test]
    fn urls_drop_trailing_punctuation() {
        assert_eq!(
            kinds("see https://a.io/x."),
            [Kind::Link("https://a.io/x".into())]
        );
        assert_eq!(
            kinds("(https://a.io/x)"),
            [Kind::Link("https://a.io/x)".into())]
        );
        assert!(kinds("https://").is_empty());
    }

    #[test]
    fn masked_links_keep_their_label() {
        let spans = inline("[masked](https://example.com) end");
        assert_eq!(spans[0].text, "masked");
        assert_eq!(spans[0].kind, Kind::Link("https://example.com".into()));
        assert!(kinds("[bad](ftp://x)").is_empty());
    }

    #[test]
    fn spoilers_mark_their_content() {
        let spans = inline("a ||b|| c");
        assert!(spans[1].style.spoiler);
        assert!(!spans[2].style.spoiler);
    }

    #[test]
    fn headings_need_a_space_and_stop_at_three() {
        assert_eq!(heading("# one"), Some((1, "one")));
        assert_eq!(heading("### three ##"), Some((3, "three")));
        assert_eq!(heading("#nospace"), None);
        assert_eq!(heading("#### four"), None);
    }

    #[test]
    fn subtext_needs_its_space() {
        assert_eq!(subtext("-# small"), Some("small"));
        assert_eq!(subtext("-#small"), None);
    }

    #[test]
    fn blocks_split_quote_code_and_paragraph() {
        let blocks =
            parse("> quoted line\nand a normal line after it\n```rust\nfn main() {}\n```\nend");
        assert!(matches!(blocks[0], Block::Quote(_)));
        assert!(matches!(blocks[1], Block::Paragraph(_)));
        assert_eq!(
            blocks[2],
            Block::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into()
            }
        );
        assert!(matches!(blocks[3], Block::Paragraph(_)));
    }

    #[test]
    fn triple_quote_takes_the_rest_of_the_message() {
        let blocks = parse(">>> one\ntwo");
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|block| matches!(block, Block::Quote(_))));
    }

    #[test]
    fn the_first_item_decides_the_list_type() {
        let blocks = parse("- first\n1. second");
        assert!(matches!(&blocks[1], Block::ListItem { number: None, .. }));
    }

    #[test]
    fn ordered_lists_count_from_their_first_number() {
        let blocks = parse("3. a\n1. b");
        assert!(matches!(
            &blocks[0],
            Block::ListItem {
                number: Some(3),
                ..
            }
        ));
        assert!(matches!(
            &blocks[1],
            Block::ListItem {
                number: Some(4),
                ..
            }
        ));
    }

    #[test]
    fn nested_items_carry_their_depth() {
        let blocks = parse("- a\n  - b");
        assert!(matches!(&blocks[1], Block::ListItem { depth: 1, .. }));
    }
}
