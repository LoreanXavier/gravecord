//! M0 subset of the message markdown: bold, italic, underline, strikethrough, inline code, block quotes, fenced code.
//! The full rule set is the simple-markdown port in the project plan section 5.6; its rows stay open until that replaces this.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Quote(Vec<Span>),
    Code {
        language: Option<String>,
        text: String,
    },
}

pub fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("```")
            && let Some(end) = after.find("```")
        {
            let body = &after[..end];
            let (language, text) = match body.split_once('\n') {
                Some((first, code)) if !first.contains(char::is_whitespace) => (
                    Some(first).filter(|l| !l.is_empty()).map(str::to_owned),
                    code,
                ),
                _ => (None, body),
            };
            blocks.push(Block::Code {
                language,
                text: text.strip_suffix('\n').unwrap_or(text).to_owned(),
            });
            rest = after[end + 3..]
                .strip_prefix('\n')
                .unwrap_or(&after[end + 3..]);
            continue;
        }
        let (line, next) = rest.split_once('\n').unwrap_or((rest, ""));
        rest = next;
        match line.strip_prefix("> ") {
            Some(quoted) => blocks.push(Block::Quote(inline(quoted))),
            None => blocks.push(Block::Paragraph(inline(line))),
        }
    }
    blocks
}

type Apply = fn(&mut Style);

/// Delimiters in match order: `**` before `*`, `__` before `_`.
const MARKS: [(&str, Apply); 6] = [
    ("**", |style| style.bold = true),
    ("__", |style| style.underline = true),
    ("~~", |style| style.strike = true),
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
        Some(last) if last.style == style => last.text.push_str(text),
        _ => spans.push(Span {
            text: text.to_owned(),
            style,
        }),
    }
}

/// The first closing mark with text before it. As in the original `\*\*(?!\*)`, a closer cannot be followed by more of
/// its own character, so `**a *b***` closes at the last two stars.
fn closing(body: &str, mark: &str) -> Option<usize> {
    let repeat = mark.chars().next()?;
    let mut from = 0;
    while let Some(found) = body[from..].find(mark) {
        let end = from + found;
        if end > 0 && !body[end + mark.len()..].starts_with(repeat) {
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
    fn multibyte_text_is_safe() {
        assert_eq!(inline("çığ **öş** ü")[1].text, "öş");
    }
}
