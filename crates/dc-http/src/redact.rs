//! Addresses as logs and error messages may show them: scheme, host, port and path, never the query, the fragment or
//! a user name and password. A query can carry what the user typed (a search, a translated text), a signed download
//! grant or a refresh token, and debug logs leave the machine in bug reports and Upload Logs.

use std::borrow::Cow;

/// `address` without its query, fragment and credentials: `https://host:port/path` for an absolute address, the path
/// alone for an API route (`/guilds/1/messages/search?content=x` becomes `/guilds/1/messages/search`).
pub fn url(address: &str) -> String {
    if address.contains("://")
        && let Ok(parsed) = reqwest::Url::parse(address)
        && parsed.has_host()
    {
        return origin_and_path(&parsed);
    }
    let cut = address
        .find(['?', '#'])
        .map_or(address, |at| &address[..at]);
    // An address that did not parse: drop a `user:password@` before its host by hand.
    match cut.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            format!("{scheme}://{host}{path}")
        }
        None => cut.to_owned(),
    }
}

fn origin_and_path(parsed: &reqwest::Url) -> String {
    let host = parsed.host_str().unwrap_or_default();
    match parsed.port() {
        Some(port) => format!("{}://{host}:{port}{}", parsed.scheme(), parsed.path()),
        None => format!("{}://{host}{}", parsed.scheme(), parsed.path()),
    }
}

/// `error` with its address cut to [`url`]'s form, so its `Display` and `Debug` show no query.
pub fn error(error: reqwest::Error) -> reqwest::Error {
    let Some(address) = error.url().cloned() else {
        return error;
    };
    let mut safe = address;
    safe.set_query(None);
    safe.set_fragment(None);
    let _ = safe.set_username("");
    let _ = safe.set_password(None);
    error.with_url(safe)
}

/// [`error`] for an error of the Discord client ([`crate::chromium`]).
pub fn chromium_error(mut error: wreq::Error) -> wreq::Error {
    if let Some(uri) = error.uri_mut() {
        let safe = url(&uri.to_string());
        match safe.parse() {
            Ok(safe) => *uri = safe,
            Err(_) => return error.without_uri(),
        }
    }
    error
}

/// Characters that end an address inside a line of text.
fn ends_address(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | ')' | ']' | '}' | '|')
}

/// `line` with every absolute address in it (`scheme://...`) cut to [`url`]'s form. Text without one is returned as
/// it is.
pub fn text(line: &str) -> Cow<'_, str> {
    if !line.contains("://") {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("://") {
        let scheme_start = rest[..at]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            .last()
            .map_or(at, |(index, _)| index);
        let end = rest[at + 3..]
            .find(ends_address)
            .map_or(rest.len(), |offset| at + 3 + offset);
        let starts_with_letter = rest[scheme_start..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
        if scheme_start == at || !starts_with_letter {
            out.push_str(&rest[..at + 3]);
            rest = &rest[at + 3..];
            continue;
        }
        out.push_str(&rest[..scheme_start]);
        out.push_str(&url(&rest[scheme_start..end]));
        rest = &rest[end..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_keeps_scheme_host_port_and_path_only() {
        assert_eq!(
            url("https://user:pass@cdn.example.com:8443/a/b.png?ex=1&sig=secret#frag"),
            "https://cdn.example.com:8443/a/b.png"
        );
        assert_eq!(
            url("https://translate.googleapis.com/translate_a/single?q=hello%20there"),
            "https://translate.googleapis.com/translate_a/single"
        );
        assert_eq!(
            url("/guilds/1/messages/search?content=my%20search"),
            "/guilds/1/messages/search"
        );
        assert_eq!(url("/channels/1/messages"), "/channels/1/messages");
        assert_eq!(url("/users/@me#x"), "/users/@me");
        // Not parseable as a URL: the query and credentials still go.
        assert_eq!(url("https://a:b@[bad/x?q=1"), "https://[bad/x");
    }

    #[test]
    fn a_line_has_every_query_cut() {
        let line = "GET failed: error sending request for url \
                    (https://objects.githubusercontent.com/x?X-Amz-Signature=secret&q=1) then \
                    wss://gateway.discord.gg/?v=9&encoding=json, \"http://127.0.0.1:1/p?token=t\" done";
        let safe = text(line);
        assert_eq!(
            safe,
            "GET failed: error sending request for url (https://objects.githubusercontent.com/x) then \
             wss://gateway.discord.gg/ \"http://127.0.0.1:1/p\" done"
        );
        assert!(!safe.contains("secret"));
        assert!(matches!(text("no address here"), Cow::Borrowed(_)));
        assert_eq!(text("odd :// spot"), "odd :// spot");
    }

    #[test]
    fn a_request_error_shows_no_query() {
        let failed = reqwest::Client::new()
            .get("https://example.invalid/")
            .header("invalid\nheader", "value")
            .build()
            .unwrap_err()
            .with_url(
                reqwest::Url::parse("https://u:p@example.invalid/search?q=private#frag").unwrap(),
            );
        let safe = error(failed);
        for shown in [format!("{safe}"), format!("{safe:?}")] {
            assert!(!shown.contains("private"), "{shown}");
            assert!(!shown.contains("frag"), "{shown}");
            assert!(!shown.contains("u:p"), "{shown}");
        }
        assert_eq!(
            safe.url().map(reqwest::Url::as_str),
            Some("https://example.invalid/search")
        );
    }
}
