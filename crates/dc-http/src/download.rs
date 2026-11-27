//! A file from an address outside the API routes (an attachment to save, a sound, a video): through the Discord
//! client ([`crate::chromium`]) for a Discord host, the plain client ([`crate::plain`]) for any other.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// How a file is fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fetch {
    /// The largest body read; a longer one fails with [`Failure::TooLarge`].
    pub max_bytes: usize,
    pub timeout: Option<Duration>,
    /// The redirects followed (0: none).
    pub redirects: usize,
    /// Whether a redirect must stay on `https`.
    pub https_redirects: bool,
}

impl Default for Fetch {
    /// Up to ten redirects, as a browser follows them, no timeout and no size limit.
    fn default() -> Self {
        Self {
            max_bytes: usize::MAX,
            timeout: None,
            redirects: 10,
            https_redirects: false,
        }
    }
}

/// A fetched file and its `Content-Type` (empty when the answer names none).
#[derive(Debug)]
pub struct File {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("status {0}")]
    Status(u16),
    #[error("file too large")]
    TooLarge,
    /// The request failed on the wire; the text shows no query of the address.
    #[error("{0}")]
    Transport(String),
}

#[derive(Clone)]
enum Http {
    Chromium(wreq::Client),
    Plain(reqwest::Client),
}

fn chromium(fetch: Fetch) -> Result<wreq::Client, Failure> {
    let https = fetch.https_redirects;
    let limit = fetch.redirects;
    let mut builder = crate::chromium::builder().redirect(if limit == 0 {
        wreq::redirect::Policy::none()
    } else {
        wreq::redirect::Policy::custom(move |attempt| {
            if attempt.previous.len() > limit {
                attempt.error("too many redirects")
            } else if https && attempt.uri.scheme_str() != Some("https") {
                attempt.error("refusing non-HTTPS redirect")
            } else if attempt
                .previous
                .last()
                .is_some_and(|previous| crate::chromium::may_follow(previous, &attempt.uri))
            {
                attempt.follow()
            } else {
                attempt.error("redirect away from Discord refused")
            }
        })
    });
    if let Some(timeout) = fetch.timeout {
        builder = builder.timeout(timeout);
    }
    builder
        .build()
        .map_err(|error| Failure::Transport(crate::redact::text(&error.to_string()).into_owned()))
}

fn plain(fetch: Fetch) -> Result<reqwest::Client, Failure> {
    let https = fetch.https_redirects;
    let limit = fetch.redirects;
    let mut builder = crate::plain::builder().redirect(if limit == 0 {
        reqwest::redirect::Policy::none()
    } else {
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > limit {
                attempt.error("too many redirects")
            } else if https && attempt.url().scheme() != "https" {
                attempt.error("refusing non-HTTPS redirect")
            } else {
                attempt.follow()
            }
        })
    });
    if let Some(timeout) = fetch.timeout {
        builder = builder.timeout(timeout);
    }
    builder
        .build()
        .map_err(|error| Failure::Transport(crate::redact::text(&error.to_string()).into_owned()))
}

/// One client per host kind and [`Fetch`], kept for the run.
fn client(url: &str, fetch: Fetch) -> Result<Http, Failure> {
    static CLIENTS: OnceLock<Mutex<HashMap<(bool, Fetch), Http>>> = OnceLock::new();
    let discord = crate::plain::is_discord_url(url);
    let mut clients = CLIENTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(known) = clients.get(&(discord, fetch)) {
        return Ok(known.clone());
    }
    let made = if discord {
        Http::Chromium(chromium(fetch)?)
    } else {
        Http::Plain(plain(fetch)?)
    };
    clients.insert((discord, fetch), made.clone());
    Ok(made)
}

fn transport(error: impl std::fmt::Display) -> Failure {
    Failure::Transport(crate::redact::text(&error.to_string()).into_owned())
}

/// `GET url` (rewritten onto the replay server in a replay run), its body read up to `fetch.max_bytes`. The client is
/// chosen on the address the app asked for.
pub async fn get(url: &str, fetch: Fetch) -> Result<File, Failure> {
    let target = dc_proto::replay::rewrite(url).into_owned();
    match client(url, fetch)? {
        Http::Chromium(client) => {
            let response = client.get(&target).send().await.map_err(transport)?;
            let content_type = check(
                response.status(),
                response.headers(),
                response.content_length(),
                fetch,
            )?;
            let mut stream = std::pin::pin!(response.bytes_stream());
            let mut bytes = Vec::new();
            while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
                append(&mut bytes, &chunk.map_err(transport)?, fetch)?;
            }
            Ok(File {
                bytes,
                content_type,
            })
        }
        Http::Plain(client) => {
            let mut response = client.get(&target).send().await.map_err(transport)?;
            let content_type = check(
                response.status(),
                response.headers(),
                response.content_length(),
                fetch,
            )?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(transport)? {
                append(&mut bytes, &chunk, fetch)?;
            }
            Ok(File {
                bytes,
                content_type,
            })
        }
    }
}

/// The answer's status and announced length checked, its `Content-Type` read.
fn check(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    length: Option<u64>,
    fetch: Fetch,
) -> Result<String, Failure> {
    if !status.is_success() {
        return Err(Failure::Status(status.as_u16()));
    }
    if length.is_some_and(|size| size > fetch.max_bytes as u64) {
        return Err(Failure::TooLarge);
    }
    Ok(headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned())
}

fn append(bytes: &mut Vec<u8>, chunk: &[u8], fetch: Fetch) -> Result<(), Failure> {
    if chunk.len() > fetch.max_bytes - bytes.len() {
        return Err(Failure::TooLarge);
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}
