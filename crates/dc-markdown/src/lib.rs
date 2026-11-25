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
    /// An attachment's CDN address (the attachmentLink rule of module 258072): the span's text is the file's name,
    /// drawn as a link chip with the attachment icon (module 862316).
    AttachmentLink(String),
    User(u64),
    /// `<@$123>` (`gameMention`, module 654255 / 827669).
    Game(u64),
    Role(u64),
    Channel(u64),
    /// `@everyone` or `@here`, kept as written.
    Everyone,
    /// A unicode emoji; the span's text is the emoji.
    Unicode,
    Emoji {
        name: String,
        id: u64,
        animated: bool,
    },
    Timestamp {
        unix: i64,
        format: char,
    },
    /// `</name:id>` (the commandMention rule of module 605275): a slash command, drawn as "/name". `name` holds up to
    /// three words (a command, its group and its sub command).
    Command {
        name: String,
        id: u64,
    },
}

/// The name of a command mention: one to three words of 1 to 32 letters, numbers, `-` or `_` (Devanagari and Thai
/// count as letters), one space apart.
fn command_name(name: &str) -> bool {
    let words: Vec<&str> = name.split(' ').collect();
    (1..=3).contains(&words.len())
        && words.iter().all(|word| {
            (1..=32).contains(&word.chars().count())
                && word
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || is_indic_mark(c))
        })
}

/// Combining marks of the Devanagari and Thai blocks, which `\p{sc=Deva}` and `\p{sc=Thai}` take in.
fn is_indic_mark(c: char) -> bool {
    matches!(c, '\u{0900}'..='\u{097F}' | '\u{A8E0}'..='\u{A8FF}' | '\u{0E00}'..='\u{0E7F}')
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

#[derive(Clone, Copy)]
struct Rules {
    mentions: bool,
    links: bool,
    /// `[label](address)` (the `link` rule, gated by `allowLinks` in module 694403); bare addresses stay links.
    masked: bool,
}

pub fn inline(source: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    walk(
        source,
        Style::default(),
        Rules {
            mentions: true,
            links: true,
            masked: true,
        },
        &mut spans,
    );
    spans
}

/// The one line a reply or thread preview shows of a message (`parseInlineReply`, `INLINE_REPLY_RULES` of module
/// 605275: the message rules without `blockQuote` and `br`, with a code block drawn as inline code). Headings, lists
/// and quotes are not rules there, so their markers stay text. The line is clamped to one line
/// (`.repliedTextContent_2i8Bfm { -webkit-line-clamp: 1 }` under `white-space: pre`), so only what comes before the
/// first line break shows. `masked_links` is `allowLinks`: off for a reply (module 234957), on for a thread's last
/// message (module 679636).
pub fn inline_reply(source: &str, masked_links: bool) -> Vec<Span> {
    let source = source.replace("\r\n", "\n");
    let rules = Rules {
        mentions: true,
        links: true,
        masked: masked_links,
    };
    let mut spans = Vec::new();
    let mut rest = source.as_str();
    while !rest.is_empty() {
        match rest.find("```") {
            Some(at) => {
                if let Some((_, text, consumed)) = code_block(&rest[at..]) {
                    walk(&rest[..at], Style::default(), rules, &mut spans);
                    spans.push(Span {
                        text,
                        style: Style {
                            code: true,
                            ..Style::default()
                        },
                        kind: Kind::Text,
                    });
                    rest = &rest[at + consumed..];
                } else {
                    walk(&rest[..at + 3], Style::default(), rules, &mut spans);
                    rest = &rest[at + 3..];
                }
            }
            None => {
                walk(rest, Style::default(), rules, &mut spans);
                rest = "";
            }
        }
    }
    first_line(spans)
}

/// The spans up to the first line break.
fn first_line(spans: Vec<Span>) -> Vec<Span> {
    let mut out = Vec::new();
    for mut span in spans {
        if let Some(cut) = span.text.find('\n') {
            span.text.truncate(cut);
            if !span.text.is_empty() {
                out.push(span);
            }
            break;
        }
        out.push(span);
    }
    out
}

/// `parseEmbedTitle` / `parseEmbedTitleWithoutLinks` (`EMBED_TITLE_RULES`, modules 605275, 46054 and 679636):
/// inline markdown without `@user`, `<@&role>` or `@everyone`/`@here`, and without links when `!allow_links`.
pub fn embed_title(source: &str, allow_links: bool) -> Vec<Span> {
    let mut spans = Vec::new();
    walk(
        source,
        Style::default(),
        Rules {
            mentions: false,
            links: allow_links,
            masked: allow_links,
        },
        &mut spans,
    );
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
fn angle_token(rest: &str, mentions: bool) -> Option<(usize, Kind, String)> {
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
    if mentions {
        if let Some(id) = inner.strip_prefix("@$").and_then(whole_number) {
            return Some((consumed, Kind::Game(id), raw));
        }
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
    }
    if let Some(id) = inner.strip_prefix('#').and_then(whole_number) {
        return Some((consumed, Kind::Channel(id), raw));
    }
    if let Some((name, id)) = inner
        .strip_prefix('/')
        .and_then(|command| command.rsplit_once(':'))
        .filter(|(name, _)| command_name(name))
        .and_then(|(name, id)| Some((name, whole_number(id)?)))
    {
        let name = name.to_owned();
        return Some((consumed, Kind::Command { name, id }, raw));
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

/// `[label](url)` or `[label](url "title")` with an http or https target (modules 46054 and 605275).
fn masked_link(rest: &str) -> Option<(usize, &str, &str)> {
    let body = rest.strip_prefix('[')?;
    let mut depth = 1usize;
    let mut index = 0;
    while index < body.len() {
        let ch = body[index..].chars().next()?;
        if ch == '\\' {
            let escaped = body[index + 1..].chars().next().map_or(0, char::len_utf8);
            index += 1 + escaped;
            continue;
        }
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        index += ch.len_utf8();
    }
    if depth != 0 {
        return None;
    }
    let label = &body[..index];
    if label.trim().is_empty() {
        return None;
    }
    let after_label = body[index + 1..].strip_prefix('(')?;
    let trimmed = after_label.trim_start();
    let lead_ws = after_label.len() - trimmed.len();
    let (address, url_consumed, tail) = if let Some(angled) = trimmed.strip_prefix('<') {
        let gt = angled.find('>')?;
        (&angled[..gt], 1 + gt + 1, &angled[gt + 1..])
    } else {
        let mut parens = 0usize;
        let mut at = 0;
        while at < trimmed.len() {
            let ch = trimmed[at..].chars().next()?;
            if ch.is_whitespace() || (ch == ')' && parens == 0) {
                break;
            }
            if ch == '\\' {
                let escaped = trimmed[at + 1..].chars().next().map_or(0, char::len_utf8);
                at += 1 + escaped;
                continue;
            }
            if ch == '(' {
                parens += 1;
            } else if ch == ')' {
                parens -= 1;
            }
            at += ch.len_utf8();
        }
        (&trimmed[..at], at, &trimmed[at..])
    };
    let tail_trimmed = tail.trim_start();
    let mid_ws = tail.len() - tail_trimmed.len();
    let close_in_after = if tail_trimmed.starts_with(')') {
        lead_ws + url_consumed + mid_ws + 1
    } else if mid_ws > 0 && tail_trimmed.starts_with(['"', '\'']) {
        let quote = tail_trimmed.chars().next()?;
        let inner = &tail_trimmed[quote.len_utf8()..];
        let mut pos = 0;
        let mut found = None;
        while pos < inner.len() {
            let ch = inner[pos..].chars().next()?;
            if ch == '\\' {
                let escaped = inner[pos + 1..].chars().next().map_or(0, char::len_utf8);
                pos += 1 + escaped;
                continue;
            }
            if ch == quote {
                let rest_after = &inner[pos + quote.len_utf8()..];
                let rest_trimmed = rest_after.trim_start();
                if rest_trimmed.starts_with(')') {
                    let trail_ws = rest_after.len() - rest_trimmed.len();
                    found = Some(
                        lead_ws
                            + url_consumed
                            + mid_ws
                            + quote.len_utf8()
                            + pos
                            + quote.len_utf8()
                            + trail_ws
                            + 1,
                    );
                    break;
                }
            }
            pos += ch.len_utf8();
        }
        found?
    } else {
        return None;
    };
    let valid = (address.starts_with("https://") || address.starts_with("http://"))
        && !address.chars().any(char::is_whitespace);
    valid.then_some((1 + index + 2 + close_in_after, label, address))
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

/// Module 258072, by hand: `^https://(?:[A-Za-z0-9-]+\.)*(?:(?:media|images)S\.discordapp\.net|cdnS\.discordapp\.com)`
/// `/(?:attachments|ephemeral-attachments)/\d+/\d+/([A-Za-z0-9._-]*[A-Za-z0-9_-])(?:[?][a-zA-Z0-9?&=_-]*)?` with
/// `S = (?:-[A-Za-z0-9]+)?`. The length matched and the file name.
pub fn attachment_link(text: &str) -> Option<(usize, &str)> {
    let rest = text.strip_prefix("https://")?;
    let host_end = rest.find('/')?;
    let host = &rest[..host_end];
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |label: &str| {
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    // `name` or `name-<alphanumerics>`.
    let service = |label: &str, names: &[&str]| {
        names.iter().any(|name| {
            label.strip_prefix(name).is_some_and(|tail| {
                tail.is_empty()
                    || tail.strip_prefix('-').is_some_and(|sub| {
                        !sub.is_empty() && sub.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
            })
        })
    };
    let [prefix @ .., service_label, domain, tld] = labels.as_slice() else {
        return None;
    };
    let known = *domain == "discordapp"
        && match *tld {
            "net" => service(service_label, &["media", "images"]),
            "com" => service(service_label, &["cdn"]),
            _ => false,
        };
    if !known || !prefix.iter().all(|label| label_ok(label)) {
        return None;
    }
    let path = &rest[host_end + 1..];
    let path = path
        .strip_prefix("attachments/")
        .or_else(|| path.strip_prefix("ephemeral-attachments/"))?;
    let mut at = text.len() - path.len();
    for _ in 0..2 {
        let digits = text[at..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || text.as_bytes().get(at + digits) != Some(&b'/') {
            return None;
        }
        at += digits + 1;
    }
    let run = text[at..]
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        .count();
    // The name ends on a character other than a dot.
    let name = text[at..at + run].trim_end_matches('.');
    if name.is_empty() {
        return None;
    }
    let mut end = at + name.len();
    if name.len() == run && text[end..].starts_with('?') {
        end += 1 + text[end + 1..]
            .bytes()
            .take_while(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'?' | b'&' | b'=' | b'_' | b'-')
            })
            .count();
    }
    Some((end, name))
}

fn everyone_here_prefix(source: &str, at: usize) -> Option<usize> {
    let rest = &source[at..];
    let length = if rest.starts_with("@everyone") {
        9
    } else if rest.starts_with("@here") {
        5
    } else {
        return None;
    };
    let prev = source[..at].chars().next_back();
    let next = rest[length..].chars().next();
    let valid_boundary = |c: char| !c.is_alphanumeric() && c != '_';
    if prev.is_some_and(|c| !valid_boundary(c)) || next.is_some_and(|c| !valid_boundary(c)) {
        return None;
    }
    Some(length)
}

fn walk(source: &str, style: Style, rules: Rules, spans: &mut Vec<Span>) {
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
        if let Some((consumed, kind, text)) = angle_token(rest, rules.mentions) {
            spans.push(Span { text, style, kind });
            at += consumed;
            continue;
        }
        if rules.mentions
            && let Some(length) = everyone_here_prefix(source, at)
        {
            spans.push(Span {
                text: rest[..length].to_owned(),
                style,
                kind: Kind::Everyone,
            });
            at += length;
            continue;
        }
        if rules.links
            && rules.masked
            && let Some((consumed, label, address)) = masked_link(rest)
        {
            let mut inner_spans = Vec::new();
            walk(
                label,
                style,
                Rules {
                    mentions: rules.mentions,
                    links: false,
                    masked: false,
                },
                &mut inner_spans,
            );
            if inner_spans.is_empty() {
                spans.push(Span {
                    text: label.to_owned(),
                    style,
                    kind: Kind::Link(address.to_owned()),
                });
            } else {
                for mut span in inner_spans {
                    if span.kind == Kind::Text {
                        span.kind = Kind::Link(address.to_owned());
                    }
                    spans.push(span);
                }
            }
            at += consumed;
            continue;
        }
        // `attachmentLink` comes just before `url` (order `url.order - .5`).
        if rules.links
            && let Some((length, name)) = attachment_link(rest)
        {
            spans.push(Span {
                text: name.to_owned(),
                style,
                kind: Kind::AttachmentLink(rest[..length].to_owned()),
            });
            at += length;
            continue;
        }
        if rules.links
            && let Some(length) = url(rest)
        {
            spans.push(Span {
                text: rest[..length].to_owned(),
                style,
                kind: Kind::Link(rest[..length].to_owned()),
            });
            at += length;
            continue;
        }
        if let Some(length) = dc_tokens::emoji_prefix(rest) {
            spans.push(Span {
                text: rest[..length].to_owned(),
                style,
                kind: Kind::Unicode,
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
                walk(inner, nested, rules, spans);
            }
            at += length * 2 + end;
            continue;
        }
        let character = rest.chars().next().unwrap_or(' ');
        push(&rest[..character.len_utf8()], style, spans);
        at += character.len_utf8();
    }
}

/// Jumbo rule from module 354777: nothing but emoji and whitespace, and at most 30 of them.
pub fn is_jumbo(spans: &[Span]) -> bool {
    let emoji = |span: &&Span| matches!(span.kind, Kind::Unicode | Kind::Emoji { .. });
    let only_emoji = spans
        .iter()
        .all(|span| emoji(&span) || (span.kind == Kind::Text && span.text.trim().is_empty()));
    only_emoji && (1..=30).contains(&spans.iter().filter(emoji).count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_emoji_become_their_own_spans() {
        let spans = inline("hi \u{1f600} there");
        assert_eq!(spans[1].kind, Kind::Unicode);
        assert_eq!(spans[2].text, " there");
    }

    #[test]
    fn jumbo_needs_only_emoji_and_at_most_thirty() {
        assert!(is_jumbo(&inline("\u{1f600}\u{1f525}")));
        assert!(is_jumbo(&inline("\u{1f600} <:a:1>")));
        assert!(!is_jumbo(&inline("hi \u{1f600}")));
        assert!(!is_jumbo(&inline("")));
        assert!(!is_jumbo(&inline(&"\u{1f600}".repeat(31))));
        assert!(is_jumbo(&inline(&"\u{1f600}".repeat(30))));
    }

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

    /// The commandMention rule of module 605275: up to three words before the id.
    #[test]
    fn command_mentions_take_up_to_three_words() {
        assert_eq!(
            kinds("use </ping:12> or </role add user:34>"),
            [
                Kind::Command {
                    name: "ping".into(),
                    id: 12
                },
                Kind::Command {
                    name: "role add user".into(),
                    id: 34
                },
            ]
        );
        assert!(kinds("</a b c d:1>").is_empty());
        assert!(kinds("</ping:x>").is_empty());
        assert!(kinds("</:1>").is_empty());
        assert!(kinds("</two  spaces:1>").is_empty());
        assert!(kinds(&format!("</{}:1>", "a".repeat(33))).is_empty());
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
        assert_eq!(kinds("<@$789>"), [Kind::Game(789)]);
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

    /// Module 258072: CDN attachment addresses become chips named by the file; other addresses stay links.
    #[test]
    fn attachment_addresses_become_chips_with_the_file_name() {
        let url =
            "https://cdn.discordapp.com/attachments/12/34/report.final.pdf?ex=1a&is=2b&hm=3c&";
        let spans = inline(&format!("see {url} now"));
        assert_eq!(spans[1].text, "report.final.pdf");
        assert_eq!(spans[1].kind, Kind::AttachmentLink(url.into()));
        assert_eq!(spans[2].text, " now");
        assert_eq!(
            attachment_link("https://media-x1.discordapp.net/ephemeral-attachments/1/2/a_b-c.png"),
            Some((67, "a_b-c.png"))
        );
        assert_eq!(
            attachment_link("https://images.discordapp.net/attachments/1/2/name..."),
            Some((50, "name"))
        );
        assert_eq!(
            attachment_link("https://eu.cdn-2.discordapp.com/attachments/1/2/x"),
            Some((49, "x"))
        );
        // The rest of an address the pattern does not cover stays text after the chip.
        let spaced = inline("https://cdn.discordapp.com/attachments/1/2/a%20b.txt");
        assert_eq!(spaced[0].text, "a");
        for plain in [
            "https://cdn.discordapp.com/emojis/1.png",
            "https://cdn.discord.com/attachments/1/2/a.png",
            "http://cdn.discordapp.com/attachments/1/2/a.png",
            "https://cdn.discordapp.com/attachments/x/2/a.png",
            "https://cdnx.discordapp.com/attachments/1/2/a.png",
            "https://media.discordapp.com/attachments/1/2/a.png",
        ] {
            assert_eq!(attachment_link(plain), None, "{plain}");
            assert!(matches!(inline(plain)[0].kind, Kind::Link(_)), "{plain}");
        }
    }

    #[test]
    fn a_reply_line_keeps_mentions_links_and_marks_but_not_blocks() {
        let spans = inline_reply("<@12> see https://youtu.be/x and `c` ||s||", false);
        let kinds: Vec<&Kind> = spans
            .iter()
            .map(|span| &span.kind)
            .filter(|kind| **kind != Kind::Text)
            .collect();
        assert_eq!(
            kinds,
            [
                &Kind::User(12),
                &Kind::Link("https://youtu.be/x".to_owned())
            ]
        );
        assert!(spans.iter().any(|span| span.style.code && span.text == "c"));
        assert!(
            spans
                .iter()
                .any(|span| span.style.spoiler && span.text == "s")
        );
        // No heading, list or quote rule: the markers stay text.
        let text = |source: &str| -> String {
            inline_reply(source, false)
                .into_iter()
                .map(|span| span.text)
                .collect()
        };
        assert_eq!(text("# big"), "# big");
        assert_eq!(text("> quoted"), "> quoted");
        assert_eq!(text("- item"), "- item");
        // One line shows.
        assert_eq!(text("first\nsecond"), "first");
        // A code block is drawn as inline code (its first line).
        let code = inline_reply("```rust\nfn x() {}\nmore```", false);
        assert_eq!(code.len(), 1);
        assert!(code[0].style.code);
        assert_eq!(code[0].text, "fn x() {}");
    }

    #[test]
    fn masked_links_in_a_reply_follow_allow_links() {
        let off = inline_reply("[label](https://example.com)", false);
        assert!(off.iter().all(|span| span.text != "label"));
        assert!(off.first().is_some_and(|span| span.text.starts_with('[')));
        let on = inline_reply("[label](https://example.com)", true);
        assert_eq!(on[0].text, "label");
        assert_eq!(on[0].kind, Kind::Link("https://example.com".to_owned()));
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

    #[test]
    fn masked_links_accept_optional_titles_and_balanced_parens() {
        let titled = inline("[docs](https://example.com/a \"Read more\")!");
        assert_eq!(titled[0].text, "docs");
        assert_eq!(titled[0].kind, Kind::Link("https://example.com/a".into()));
        assert_eq!(titled[1].text, "!");

        let single = inline("[wiki](https://example.com/Rust_(lang) 'Wiki')");
        assert_eq!(single[0].text, "wiki");
        assert_eq!(
            single[0].kind,
            Kind::Link("https://example.com/Rust_(lang)".into())
        );

        let styled_label = inline("[**bold** link](<https://example.com/x>)");
        assert!(styled_label[0].style.bold);
        assert_eq!(styled_label[0].text, "bold");
        assert_eq!(
            styled_label[0].kind,
            Kind::Link("https://example.com/x".into())
        );
        assert_eq!(
            styled_label[1].kind,
            Kind::Link("https://example.com/x".into())
        );
    }

    #[test]
    fn embed_title_omits_user_role_and_everyone_mentions_and_gates_links() {
        let with_links = embed_title(
            "**Hi** <@12> <@&34> @everyone @here <#56> <:ok:9> <t:1794000000:R> ||sec|| [go](https://a.io \"t\") https://b.io",
            true,
        );
        assert!(with_links[0].style.bold);
        assert!(
            !with_links
                .iter()
                .any(|span| matches!(span.kind, Kind::User(_) | Kind::Role(_) | Kind::Everyone))
        );
        assert!(with_links.iter().any(|span| span.kind == Kind::Channel(56)));
        assert!(with_links.iter().any(|span| matches!(
            &span.kind,
            Kind::Emoji {
                name,
                id: 9,
                animated: false
            } if name == "ok"
        )));
        assert!(with_links.iter().any(|span| matches!(
            span.kind,
            Kind::Timestamp {
                unix: 1794000000,
                format: 'R'
            }
        )));
        assert!(
            with_links
                .iter()
                .any(|span| span.style.spoiler && span.text == "sec")
        );
        assert!(
            with_links
                .iter()
                .any(|span| span.text == "go" && span.kind == Kind::Link("https://a.io".into()))
        );
        assert!(
            with_links
                .iter()
                .any(|span| span.kind == Kind::Link("https://b.io".into()))
        );

        let without_links = embed_title("**Title** [go](https://a.io) https://b.io", false);
        assert!(without_links[0].style.bold);
        assert!(
            !without_links
                .iter()
                .any(|span| matches!(span.kind, Kind::Link(_)))
        );
    }

    #[test]
    fn everyone_and_here_require_word_boundaries() {
        assert!(!kinds("mail foo@here.com and @everyones or @here1").contains(&Kind::Everyone));
        assert!(!kinds("user@everyone").contains(&Kind::Everyone));
        assert!(kinds("hello @here!").contains(&Kind::Everyone));
        assert!(kinds("(@everyone)").contains(&Kind::Everyone));
    }
}
