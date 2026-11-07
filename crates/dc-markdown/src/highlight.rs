//! Code block highlighting with syntect, mapped onto the official `--text-code-*` tokens.
//! Token-level parity with the bundle's highlighter is a documented gap in `BLOCKED.md`.

use std::sync::OnceLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Plain,
    Keyword,
    Title,
    Namespace,
    String,
    Number,
    Comment,
    Type,
}

impl Token {
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Plain => "--text-code",
            Self::Keyword => "--text-code-keyword",
            Self::Title => "--text-code-title",
            Self::Namespace => "--text-code-namespace",
            Self::String => "--text-code-string",
            Self::Number => "--text-code-number",
            Self::Comment => "--text-code-comment",
            Self::Type => "--text-code-type",
        }
    }
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn classify(stack: &[Scope]) -> Token {
    for scope in stack.iter().rev() {
        let name = scope.build_string();
        let token = match () {
            () if name.starts_with("comment") => Token::Comment,
            () if name.starts_with("string") => Token::String,
            () if name.starts_with("constant.numeric") => Token::Number,
            () if name.contains("macro") => Token::Namespace,
            () if name.starts_with("entity.name.function") => Token::Title,
            () if name.starts_with("entity.name") || name.starts_with("support.type") => {
                Token::Type
            }
            () if name.starts_with("keyword.operator") || name.starts_with("punctuation") => {
                Token::Plain
            }
            () if name.starts_with("keyword") || name.starts_with("storage") => Token::Keyword,
            () => continue,
        };
        return token;
    }
    Token::Plain
}

/// One line of source as `(byte length, token)` pairs. Unknown languages come back plain.
pub fn highlight(language: Option<&str>, code: &str) -> Vec<Vec<(usize, Token)>> {
    let set = syntaxes();
    let syntax = language.and_then(|name| set.find_syntax_by_token(name));
    let Some(syntax) = syntax else {
        return code
            .lines()
            .map(|line| vec![(line.len(), Token::Plain)])
            .collect();
    };
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut lines = Vec::new();
    for line in code.lines() {
        let source = format!("{line}\n");
        let Ok(operations) = state.parse_line(&source, set) else {
            lines.push(vec![(line.len(), Token::Plain)]);
            continue;
        };
        let mut spans: Vec<(usize, Token)> = Vec::new();
        let mut at = 0;
        let mut add = |end: usize, token: Token| {
            let end = end.min(line.len());
            if end > at {
                match spans.last_mut() {
                    Some((length, last)) if *last == token => *length += end - at,
                    _ => spans.push((end - at, token)),
                }
                at = end;
            }
        };
        for (offset, operation) in &operations {
            add(*offset, classify(&stack.scopes));
            if matches!(operation, ScopeStackOp::Noop) {
                continue;
            }
            let _ = stack.apply(operation);
        }
        add(line.len(), classify(&stack.scopes));
        lines.push(spans);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(line: &str, spans: &[(usize, Token)]) -> Vec<(String, Token)> {
        let mut at = 0;
        spans
            .iter()
            .map(|(length, token)| {
                let text = line[at..at + length].to_owned();
                at += length;
                (text, *token)
            })
            .collect()
    }

    #[test]
    fn rust_sample_gets_the_colors_the_official_client_shows() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight(Some("rust"), code);
        let first = tokens("fn main() {", &lines[0]);
        assert!(
            first
                .iter()
                .any(|(text, token)| text.trim() == "fn" && *token == Token::Keyword),
            "{first:?}"
        );
        assert!(
            first
                .iter()
                .any(|(text, token)| text == "main" && *token == Token::Title),
            "{first:?}"
        );
        let second = tokens("    println!(\"hello\");", &lines[1]);
        assert!(
            second
                .iter()
                .any(|(text, token)| text.contains("println") && *token == Token::Namespace),
            "{second:?}"
        );
        assert!(
            second
                .iter()
                .any(|(text, token)| text.contains("hello") && *token == Token::String),
            "{second:?}"
        );
    }

    #[test]
    fn spans_cover_every_byte() {
        let code = "let é = \"ü\"; // ç\nlet x = 1;";
        let lines = highlight(Some("rust"), code);
        for (line, spans) in code.lines().zip(&lines) {
            assert_eq!(
                spans.iter().map(|(length, _)| length).sum::<usize>(),
                line.len()
            );
        }
    }

    #[test]
    fn unknown_language_is_plain() {
        assert_eq!(
            highlight(Some("nonsense"), "abc"),
            [vec![(3, Token::Plain)]]
        );
        assert_eq!(highlight(None, "abc"), [vec![(3, Token::Plain)]]);
    }
}
