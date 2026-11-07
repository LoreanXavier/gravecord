//! The gitignored `secrets.toml`. The token cannot reach a log through `Debug` or `Display`.

use serde::Deserialize;
use std::fmt;
use std::path::Path;
use thiserror::Error;

#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The only way to read the value. Call it where the token goes on the wire, nowhere else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("cannot read the secrets file: {0}")]
    Read(#[from] std::io::Error),
    #[error("the secrets file is not valid TOML")]
    Parse,
    #[error("the secrets file has an empty token")]
    Empty,
}

#[derive(Deserialize)]
struct File {
    discord: Discord,
}

#[derive(Deserialize)]
struct Discord {
    token: Token,
}

pub fn load(path: &Path) -> Result<Token, SecretsError> {
    parse(&std::fs::read_to_string(path)?)
}

// The TOML error is dropped on purpose: it quotes the offending line, which could be the token.
fn parse(text: &str) -> Result<Token, SecretsError> {
    let file: File = toml::from_str(text).map_err(|_| SecretsError::Parse)?;
    if file.discord.token.0.trim().is_empty() {
        return Err(SecretsError::Empty);
    }
    Ok(file.discord.token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_shows_the_token() {
        let token = parse("[discord]\ntoken = \"abc.def.ghi\"\n").unwrap();
        assert_eq!(token.expose(), "abc.def.ghi");
        assert_eq!(format!("{token:?}"), "Token(<redacted>)");
    }

    #[test]
    fn parse_errors_do_not_quote_the_file() {
        let error = parse("[discord]\ntoken = abc.def.ghi\n").err().unwrap();
        assert!(!error.to_string().contains("abc"));
    }

    #[test]
    fn empty_token_is_an_error() {
        assert!(matches!(
            parse("[discord]\ntoken = \"\"\n"),
            Err(SecretsError::Empty)
        ));
    }
}
