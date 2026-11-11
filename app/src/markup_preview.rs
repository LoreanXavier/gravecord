//! The composer's markdown preview. With "Preview emoji, mentions, and markdown syntax as you type" on
//! (`useRichChatInput`, module 253932; `previewMarkdown` of the editor in module 68059) the text box keeps the typed
//! syntax and styles it as the message will read: the delimiters take `--textbox-markdown-syntax` at semibold weight
//! (normal for `__`, `~~` and `||`), bold text is bold, italics are italic, and so on (`.syntaxBefore`, `.bold`,
//! `.italics`, `.underline`, `.strikethrough`, `.inlineCode`, `.fakeLink` of CSS 358931). With the setting off the
//! box is plain text.

use std::sync::atomic::{AtomicBool, Ordering};

pub const SYNTAX: u16 = 1;
pub const BOLD: u16 = 1 << 1;
pub const ITALIC: u16 = 1 << 2;
pub const UNDERLINE: u16 = 1 << 3;
pub const STRIKE: u16 = 1 << 4;
pub const CODE: u16 = 1 << 5;
pub const SPOILER: u16 = 1 << 6;
pub const LINK: u16 = 1 << 7;
/// Delimiters of `__`, `~~` and `||` keep the normal weight (`.before_u`, `.before_s`, `.before_spoiler`).
pub const LIGHT_SYNTAX: u16 = 1 << 8;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Whether the preview is on, set each frame from the preference.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// The style flags of every byte of `text` (one paragraph).
pub fn flags(text: &str) -> Vec<u16> {
    let mut out = vec![0u16; text.len()];
    scan(text, 0, text.len(), 0, &mut out);
    out
}

fn paint(out: &mut [u16], from: usize, to: usize, flags: u16) {
    for slot in &mut out[from..to] {
        *slot |= flags;
    }
}

fn prev_char(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn next_char(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// One delimiter pair found at `at`: the delimiter's length, where the closing one starts and the style it gives.
struct Pair {
    open: usize,
    close: usize,
    style: u16,
    light: bool,
}

fn find_pair(text: &str, at: usize, hi: usize, delimiter: &str) -> Option<usize> {
    let from = at + delimiter.len();
    let mut search = from;
    while search < hi {
        let found = text[search..hi].find(delimiter)?;
        let close = search + found;
        if close > from {
            return Some(close);
        }
        search = close + delimiter.len();
    }
    None
}

fn pair_at(text: &str, at: usize, hi: usize) -> Option<Pair> {
    let rest = &text[at..hi];
    let doubles: [(&str, u16, bool); 4] = [
        ("||", SPOILER, true),
        ("**", BOLD, false),
        ("__", UNDERLINE, true),
        ("~~", STRIKE, true),
    ];
    for (delimiter, style, light) in doubles {
        if rest.starts_with(delimiter) {
            let content_start = at + delimiter.len();
            // The text right after the opening delimiter is not blank.
            if next_char(text, content_start).is_none_or(char::is_whitespace) {
                continue;
            }
            if let Some(mut close) = find_pair(text, at, hi, delimiter) {
                // `***x***`: the closing pair is the last two stars of the run, so an italic can close inside.
                if delimiter == "**" {
                    let run = text[close..hi].chars().take_while(|c| *c == '*').count();
                    close += run.saturating_sub(2);
                }
                if prev_char(text, close).is_none_or(char::is_whitespace) {
                    continue;
                }
                return Some(Pair {
                    open: delimiter.len(),
                    close,
                    style,
                    light,
                });
            }
        }
    }
    let first = rest.chars().next()?;
    if first == '*' {
        let after = next_char(text, at + 1)?;
        if after == '*' || after.is_whitespace() {
            return None;
        }
        let mut search = at + 1;
        while search < hi {
            let Some(found) = text[search..hi].find('*') else {
                break;
            };
            let close = search + found;
            let before = prev_char(text, close);
            // A delimiter of the enclosing pair starts at `hi`: it is not this pair's neighbour.
            let following = (close + 1 < hi)
                .then(|| next_char(text, close + 1))
                .flatten();
            if close > at + 1
                && !before.is_none_or(char::is_whitespace)
                && following != Some('*')
                && before != Some('*')
            {
                return Some(Pair {
                    open: 1,
                    close,
                    style: ITALIC,
                    light: false,
                });
            }
            search = close + 1;
        }
    }
    if first == '_' {
        let after = next_char(text, at + 1)?;
        if after == '_'
            || after.is_whitespace()
            || prev_char(text, at).is_some_and(char::is_alphanumeric)
        {
            return None;
        }
        let mut search = at + 1;
        while search < hi {
            let Some(found) = text[search..hi].find('_') else {
                break;
            };
            let close = search + found;
            let before = prev_char(text, close);
            // A delimiter of the enclosing pair starts at `hi`: it is not this pair's neighbour.
            let following = (close + 1 < hi)
                .then(|| next_char(text, close + 1))
                .flatten();
            if close > at + 1
                && !before.is_none_or(char::is_whitespace)
                && before != Some('_')
                && following != Some('_')
                && !following.is_some_and(char::is_alphanumeric)
            {
                return Some(Pair {
                    open: 1,
                    close,
                    style: ITALIC,
                    light: false,
                });
            }
            search = close + 1;
        }
    }
    None
}

fn code_at(text: &str, at: usize, hi: usize) -> Option<(usize, usize)> {
    let ticks = text[at..hi].chars().take_while(|c| *c == '`').count();
    if ticks == 0 {
        return None;
    }
    let mut search = at + ticks;
    while search < hi {
        let found = text[search..hi].find('`')?;
        let start = search + found;
        let run = text[start..hi].chars().take_while(|c| *c == '`').count();
        if run == ticks && start > at + ticks {
            return Some((ticks, start));
        }
        search = start + run;
    }
    None
}

fn link_len(text: &str, at: usize, hi: usize) -> Option<usize> {
    let rest = &text[at..hi];
    if !(rest.starts_with("http://") || rest.starts_with("https://")) {
        return None;
    }
    if prev_char(text, at).is_some_and(char::is_alphanumeric) {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let scheme = rest.find("//").map_or(0, |at| at + 2);
    (end > scheme).then_some(end)
}

fn scan(text: &str, lo: usize, hi: usize, base: u16, out: &mut [u16]) {
    let mut at = lo;
    while at < hi {
        let Some(c) = next_char(text, at) else {
            break;
        };
        let width = c.len_utf8();
        if c == '`'
            && let Some((ticks, close)) = code_at(text, at, hi)
        {
            paint(out, at, at + ticks, base | SYNTAX | CODE);
            paint(out, at + ticks, close, base | CODE);
            paint(out, close, close + ticks, base | SYNTAX | CODE);
            at = close + ticks;
            continue;
        }
        if matches!(c, '|' | '*' | '_' | '~')
            && let Some(pair) = pair_at(text, at, hi)
        {
            let delimiter = SYNTAX | if pair.light { LIGHT_SYNTAX } else { 0 };
            paint(out, at, at + pair.open, base | delimiter);
            scan(text, at + pair.open, pair.close, base | pair.style, out);
            paint(out, pair.close, pair.close + pair.open, base | delimiter);
            at = pair.close + pair.open;
            continue;
        }
        if c == 'h'
            && let Some(length) = link_len(text, at, hi)
        {
            paint(out, at, at + length, base | LINK);
            at += length;
            continue;
        }
        paint(out, at, at + width, base);
        at += width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, needle: &str) -> u16 {
        flags(text)[text.find(needle).unwrap()]
    }

    #[test]
    fn bold_keeps_its_delimiters_as_syntax() {
        let text = "a **bold** b";
        assert_eq!(at(text, "**"), SYNTAX);
        assert_eq!(at(text, "bold"), BOLD);
        assert_eq!(flags(text)[8], SYNTAX);
        assert_eq!(at(text, " b"), 0);
    }

    #[test]
    fn nested_styles_combine() {
        let text = "**bold *both***";
        assert_eq!(at(text, "both"), BOLD | ITALIC);
    }

    #[test]
    fn underline_strike_and_spoiler_delimiters_stay_light() {
        for (text, style) in [("__u__", UNDERLINE), ("~~s~~", STRIKE), ("||x||", SPOILER)] {
            assert_eq!(flags(text)[0], SYNTAX | LIGHT_SYNTAX, "{text}");
            assert_eq!(flags(text)[2], style, "{text}");
        }
    }

    #[test]
    fn code_is_not_parsed_inside() {
        let text = "`**x**`";
        assert_eq!(at(text, "**"), CODE);
        assert_eq!(flags(text)[0], SYNTAX | CODE);
    }

    #[test]
    fn unclosed_or_blank_markers_stay_plain() {
        assert!(flags("**not closed").iter().all(|flag| *flag == 0));
        assert!(flags("2 * 3 * 4").iter().all(|flag| *flag == 0));
        assert!(flags("snake_case_name").iter().all(|flag| *flag == 0));
    }

    #[test]
    fn links_are_marked() {
        assert_eq!(at("see https://x.io/a now", "https"), LINK);
        assert_eq!(at("see https://x.io/a now", " now"), 0);
    }
}
