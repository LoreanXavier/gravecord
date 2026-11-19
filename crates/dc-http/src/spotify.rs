//! Spotify playback transport (pinned modules 107750/272984).
//!
//! Credential/transport isolation follows Serein's `crates/discord-api/src/spotify.rs`
//! (Copyright 2026 Serein contributors, MIT): bounded bodies, an independent HTTPS client,
//! sensitive authorization headers and memory-only zeroized credentials. Playback behavior
//! and refresh rules come from the pinned client, rather than Serein's polling-only API.

use crate::Client;
use futures_util::{SinkExt, StreamExt};
use reqwest::{Method, Response, StatusCode, header::HeaderValue};
use serde::Deserialize;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{
    self, Message, client::IntoClientRequest, protocol::WebSocketConfig,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use zeroize::Zeroizing;

const API: &str = "https://api.spotify.com/v1";
const DEALER: &str = "wss://dealer.spotify.com/";
const TOKEN_BYTES: usize = 16 * 1024;
const PLAYER_BYTES: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_DELAY: Duration = Duration::from_secs(5);

/// Errors deliberately contain no response body, URL, account id or transport error text.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("Spotify request failed")]
    Transport,
    #[error("Spotify credential is invalid")]
    Credential,
    #[error("Spotify connection was revoked")]
    Revoked,
    #[error("Spotify request is rate limited")]
    RateLimited(Duration),
    #[error("Spotify response exceeds its size limit")]
    Capacity,
    #[error("Spotify response is invalid")]
    Protocol,
    #[error("Spotify returned HTTP {0}")]
    Status(u16),
}

pub struct Spotify {
    discord: Arc<Client>,
    http: reqwest::Client,
    account: String,
    credential: Option<Zeroizing<String>>,
    on_credential: Option<Arc<dyn Fn() + Send + Sync>>,
    api: String,
    dealer: String,
    retry_delay: Duration,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct LinkedAccount {
    pub id: String,
    #[serde(default)]
    pub show_activity: bool,
}

/// Refreshes only account metadata. Unknown fields, including credentials, are never copied out of the bounded body.
pub async fn connected_accounts(discord: &Client) -> Result<Vec<LinkedAccount>, Error> {
    tokio::time::timeout(TIMEOUT, async {
        let response = discord
            .send(&Method::GET, "/users/@me/connections", None)
            .await
            .map_err(|_| Error::Transport)?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Error::RateLimited(retry_after(&response)));
        }
        if !status.is_success() {
            return Err(Error::Status(status.as_u16()));
        }
        #[derive(Deserialize)]
        struct WireAccount {
            id: Option<String>,
            #[serde(rename = "type")]
            kind: Option<String>,
            show_activity: Option<bool>,
        }
        let bytes = bounded(response, PLAYER_BYTES).await?;
        let accounts: Vec<WireAccount> =
            serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?;
        Ok(accounts
            .into_iter()
            .filter(|account| account.kind.as_deref() == Some("spotify"))
            .filter_map(|account| {
                Some(LinkedAccount {
                    id: account.id?,
                    show_activity: account.show_activity.unwrap_or(false),
                })
            })
            .collect())
    })
    .await
    .map_err(|_| Error::Transport)?
}

async fn bounded(mut response: Response, limit: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::Capacity);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::Capacity);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn retry_after(response: &Response) -> Duration {
    response
        .headers()
        .get("retry-after")
        .and_then(|header| header.to_str().ok())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.)
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(RETRY_DELAY)
}

fn account_component(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

impl Spotify {
    pub fn new(discord: Arc<Client>, account: String) -> Result<Self, Error> {
        let http = Self::http_client(true)?;
        Ok(Self {
            discord,
            http,
            account,
            credential: None,
            on_credential: None,
            api: API.into(),
            dealer: DEALER.into(),
            retry_delay: RETRY_DELAY,
        })
    }

    fn http_client(https_only: bool) -> Result<reqwest::Client, Error> {
        reqwest::Client::builder()
            .https_only(https_only)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| Error::Transport)
    }

    pub fn clear_credential(&mut self) {
        self.credential = None;
    }

    pub fn on_credential(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_credential = Some(Arc::new(callback));
        self
    }

    /// The credential-bearing handshake stays inside the network layer and never appears in an error.
    pub async fn open_dealer(&mut self) -> Result<Dealer, Error> {
        if self.credential.is_none() {
            self.refresh().await?;
        }
        let encoded = Zeroizing::new(account_component(
            self.credential.as_deref().ok_or(Error::Credential)?,
        ));
        let address = Zeroizing::new(format!("{}?access_token={}", self.dealer, encoded.as_str()));
        let mut request = address
            .as_str()
            .into_client_request()
            .map_err(|_| Error::Transport)?;
        request.headers_mut().insert(
            "Origin",
            HeaderValue::from_static(dc_proto::pin::WEBAPP_ORIGIN),
        );
        request.headers_mut().insert(
            "User-Agent",
            HeaderValue::from_static(dc_proto::pin::USER_AGENT),
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(PLAYER_BYTES))
            .max_frame_size(Some(PLAYER_BYTES));
        let (socket, _) = tokio::time::timeout(
            TIMEOUT,
            connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
        Ok(Dealer { socket })
    }

    async fn refresh(&mut self) -> Result<(), Error> {
        self.clear_credential();
        let route = format!(
            "/users/@me/connections/spotify/{}/access-token",
            account_component(&self.account)
        );
        let discord = self.discord.clone();
        let token = tokio::time::timeout(TIMEOUT, async move {
            let response = discord
                .send(&Method::GET, &route, None)
                .await
                .map_err(|_| Error::Transport)?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(Error::RateLimited(retry_after(&response)));
            }
            let bytes = bounded(response, TOKEN_BYTES).await?;
            if !status.is_success() {
                #[derive(Deserialize)]
                struct Refusal {
                    code: Option<u64>,
                }
                let code = serde_json::from_slice::<Refusal>(&bytes)
                    .ok()
                    .and_then(|body| body.code);
                discord.note_status(status, &serde_json::json!({"code": code}));
                return Err(if code == Some(40012) {
                    Error::Revoked
                } else {
                    Error::Status(status.as_u16())
                });
            }
            #[derive(Deserialize)]
            struct Token<'a> {
                #[serde(borrow)]
                access_token: &'a str,
            }
            let body: Token<'_> = serde_json::from_slice(&bytes).map_err(|_| Error::Credential)?;
            if body.access_token.is_empty()
                || body.access_token.len() > 8192
                || !body
                    .access_token
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic())
            {
                return Err(Error::Credential);
            }
            Ok(Zeroizing::new(body.access_token.to_owned()))
        })
        .await
        .map_err(|_| Error::Transport)??;
        self.credential = Some(token);
        if let Some(callback) = &self.on_credential {
            callback();
        }
        Ok(())
    }

    async fn request(
        &mut self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        only_authorization_retry: bool,
    ) -> Result<Value, Error> {
        if self.credential.is_none() {
            self.refresh().await?;
        }
        for attempt in 0..=1 {
            let bearer = Zeroizing::new(format!(
                "Bearer {}",
                self.credential.as_deref().ok_or(Error::Credential)?
            ));
            let mut authorization =
                HeaderValue::from_str(&bearer).map_err(|_| Error::Credential)?;
            authorization.set_sensitive(true);
            let mut request = self
                .http
                .request(method.clone(), format!("{}{path}", self.api))
                .header(reqwest::header::AUTHORIZATION, authorization)
                .query(query);
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await.map_err(|_| Error::Transport)?;
            let status = response.status();
            let retry = status == StatusCode::UNAUTHORIZED
                || (status == StatusCode::ACCEPTED && !only_authorization_retry);
            if retry && attempt == 0 {
                drop(response);
                if status == StatusCode::ACCEPTED {
                    tokio::time::sleep(self.retry_delay).await;
                }
                self.refresh().await?;
                continue;
            }
            if status == StatusCode::UNAUTHORIZED {
                self.clear_credential();
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(Error::RateLimited(retry_after(&response)));
            }
            if !status.is_success() || status == StatusCode::ACCEPTED {
                return Err(Error::Status(status.as_u16()));
            }
            let bytes = bounded(response, PLAYER_BYTES).await?;
            return if bytes.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
            };
        }
        unreachable!("both attempts return or retry the first attempt")
    }

    pub async fn profile(&mut self) -> Result<Value, Error> {
        self.request(Method::GET, "/me", &[], None, false).await
    }

    /// The playlist a player state plays from (`ec` of module 655116 `GET`s the context's `href`). Only the three
    /// fields the player state keeps are asked for: a whole playlist with its tracks outgrows the body bound.
    pub async fn playlist(&mut self, id: &str) -> Result<Value, Error> {
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::Protocol);
        }
        let path = format!("/playlists/{id}");
        self.request(
            Method::GET,
            &path,
            &[("fields", "type,uri,public".into())],
            None,
            false,
        )
        .await
    }

    pub async fn devices(&mut self) -> Result<Value, Error> {
        self.request(Method::GET, "/me/player/devices", &[], None, false)
            .await
    }

    pub async fn player(&mut self) -> Result<Value, Error> {
        self.request(
            Method::GET,
            "/me/player",
            &[("additional_types", "track,episode".into())],
            None,
            true,
        )
        .await
    }

    pub async fn play(
        &mut self,
        device: &str,
        body: &Value,
        repeat: Option<bool>,
    ) -> Result<(), Error> {
        self.request(
            Method::PUT,
            "/me/player/play",
            &[("device_id", device.into())],
            Some(body),
            false,
        )
        .await?;
        if let Some(repeat) = repeat {
            self.request(
                Method::PUT,
                "/me/player/repeat",
                &[
                    ("device_id", device.into()),
                    ("state", if repeat { "context" } else { "off" }.into()),
                ],
                None,
                false,
            )
            .await?;
        }
        Ok(())
    }

    /// `PUT /me/player`: playback moves to `device` and plays there (Spotify Connect's transfer).
    pub async fn transfer(&mut self, device: &str) -> Result<(), Error> {
        self.request(
            Method::PUT,
            "/me/player",
            &[],
            Some(&serde_json::json!({ "device_ids": [device], "play": true })),
            false,
        )
        .await
        .map(|_| ())
    }

    /// `PUT /me/player/seek`: the playing track jumps to `position_ms`.
    pub async fn seek(&mut self, position_ms: i64, device: Option<&str>) -> Result<(), Error> {
        let mut query = vec![("position_ms", position_ms.max(0).to_string())];
        if let Some(device) = device {
            query.push(("device_id", device.to_owned()));
        }
        self.request(Method::PUT, "/me/player/seek", &query, None, false)
            .await
            .map(|_| ())
    }

    pub async fn pause(&mut self) -> Result<(), Error> {
        self.request(Method::PUT, "/me/player/pause", &[], None, false)
            .await
            .map(|_| ())
    }

    pub async fn register_notifications(&mut self, connection: &str) -> Result<(), Error> {
        let mut result = Ok(Value::Null);
        for attempt in 0..=2 {
            result = self
                .request(
                    Method::PUT,
                    "/me/notifications/player",
                    &[("connection_id", connection.into())],
                    None,
                    false,
                )
                .await;
            if result.is_ok() {
                break;
            }
            if attempt < 2 {
                let wait = match &result {
                    Err(Error::RateLimited(wait)) => (*wait).max(self.retry_delay),
                    _ => self.retry_delay,
                };
                tokio::time::sleep(wait).await;
            }
        }
        result.map(|_| ())
    }
}

/// Bounded JSON messages from Spotify's dealer connection (655116 `et`).
/// No credential accessor or `Debug` implementation is exposed to callers.
pub struct Dealer {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl Dealer {
    pub async fn receive(&mut self) -> Result<Option<Value>, Error> {
        while let Some(message) = self.socket.next().await {
            let message = message.map_err(|error| match error {
                tungstenite::Error::Capacity(_) => Error::Capacity,
                _ => Error::Transport,
            })?;
            match message {
                Message::Text(text) => {
                    return serde_json::from_str(&text)
                        .map(Some)
                        .map_err(|_| Error::Protocol);
                }
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }

    /// The source sends this application ping every30seconds, in addition to WebSocket control frames.
    pub async fn ping(&mut self) -> Result<(), Error> {
        self.socket
            .send(Message::text(r#"{"type":"ping"}"#))
            .await
            .map_err(|_| Error::Transport)
    }

    pub async fn close(&mut self) -> Result<(), Error> {
        self.socket.close(None).await.map_err(|_| Error::Transport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::{identity::Identity, secrets::Token};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    struct Reply {
        status: u16,
        body: String,
        headers: String,
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            body: body.into(),
            headers: String::new(),
        }
    }

    async fn server(replies: Vec<Reply>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = tokio::time::timeout(TIMEOUT, listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let mut bytes = Vec::new();
                let mut block = [0; 4096];
                loop {
                    let read = tokio::time::timeout(TIMEOUT, stream.read(&mut block))
                        .await
                        .unwrap()
                        .unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&block[..read]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|length| length.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                let head = format!(
                    "HTTP/1.1 {} Fixture\r\ncontent-length: {}\r\nconnection: close\r\n{}\r\n",
                    reply.status,
                    reply.body.len(),
                    reply.headers
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(reply.body.as_bytes()).await;
            }
            requests
        });
        (format!("http://{address}"), task)
    }

    async fn fixture(
        tokens: Vec<Reply>,
        answers: Vec<Reply>,
    ) -> (Spotify, JoinHandle<Vec<String>>, JoinHandle<Vec<String>>) {
        let (discord, broker) = server(tokens).await;
        let (api, provider) = server(answers).await;
        let discord = Arc::new(
            Client::new(Token::new("fixture-discord"), Identity::new())
                .unwrap()
                .with_api_base(discord),
        );
        let mut spotify = Spotify::new(discord, "fixture/account".into()).unwrap();
        spotify.http = Spotify::http_client(false).unwrap();
        spotify.api = api;
        spotify.retry_delay = Duration::ZERO;
        (spotify, broker, provider)
    }

    fn credential() -> Reply {
        reply(200, r#"{"access_token":"fixture-provider"}"#)
    }

    #[tokio::test]
    async fn linked_account_reads_only_retain_spotify_metadata() {
        let (base, broker) = server(vec![reply(200, r#"[{"id":"a","type":"spotify","show_activity":true,"access_token":"fixture-discard"},{"type":"contacts"},{"id":"b","type":"twitch"},{"type":"spotify"}]"#)]).await;
        let discord = Client::new(Token::new("fixture"), Identity::new())
            .unwrap()
            .with_api_base(base);
        let accounts = connected_accounts(&discord).await.unwrap();
        assert_eq!(
            accounts,
            [LinkedAccount {
                id: "a".into(),
                show_activity: true
            }]
        );
        assert!(!format!("{accounts:?}").contains("fixture-discard"));
        assert!(broker.await.unwrap()[0].starts_with("GET /users/@me/connections "));
    }

    #[tokio::test]
    async fn provider_has_only_its_own_credential_and_refreshes_once_on_401() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (mut spotify, broker, provider) = fixture(
            vec![
                credential(),
                reply(200, r#"{"access_token":"fixture-refreshed"}"#),
            ],
            vec![
                reply(401, "private refusal"),
                reply(200, r#"{"product":"premium"}"#),
            ],
        )
        .await;
        let notices = Arc::new(AtomicUsize::new(0));
        let observed = notices.clone();
        spotify = spotify.on_credential(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(spotify.profile().await.unwrap()["product"], "premium");
        assert_eq!(notices.load(Ordering::SeqCst), 2);
        let broker = broker.await.unwrap();
        assert_eq!(broker.len(), 2);
        assert!(
            broker[0]
                .starts_with("GET /users/@me/connections/spotify/fixture%2Faccount/access-token ")
        );
        let requests = provider.await.unwrap();
        assert!(requests[0].contains("authorization: Bearer fixture-provider"));
        assert!(requests[1].contains("authorization: Bearer fixture-refreshed"));
        for request in requests {
            assert!(!request.contains("fixture-discord"));
            assert!(!request.contains("x-super-properties"));
        }
    }

    #[tokio::test]
    async fn play_retries_202_then_sets_repeat_after_playback() {
        let (mut spotify, broker, provider) = fixture(
            vec![credential(), credential()],
            vec![reply(202, ""), reply(204, ""), reply(204, "")],
        )
        .await;
        let body = serde_json::json!({"uris":["spotify:track:fixture"],"position_ms":4000});
        spotify.play("device /1", &body, Some(false)).await.unwrap();
        assert_eq!(broker.await.unwrap().len(), 2);
        let requests = provider.await.unwrap();
        assert!(requests[0].starts_with("PUT /me/player/play?device_id=device+%2F1 "));
        assert!(requests[1].ends_with(&body.to_string()));
        assert!(requests[2].starts_with("PUT /me/player/repeat?device_id=device+%2F1&state=off "));
    }

    #[tokio::test]
    async fn player_snapshot_does_not_retry_202_and_empty_player_is_null() {
        let (mut spotify, broker, provider) =
            fixture(vec![credential()], vec![reply(202, ""), reply(204, "")]).await;
        assert_eq!(spotify.player().await, Err(Error::Status(202)));
        assert_eq!(spotify.player().await.unwrap(), Value::Null);
        assert_eq!(broker.await.unwrap().len(), 1);
        let requests = provider.await.unwrap();
        assert!(requests[0].starts_with("GET /me/player?additional_types=track%2Cepisode "));
    }

    #[tokio::test]
    async fn a_second_401_drops_the_credential_without_exposing_the_refusal() {
        let (mut spotify, broker, provider) = fixture(
            vec![credential(), credential()],
            vec![
                reply(401, "fixture-private-response"),
                reply(401, "fixture-private-response"),
            ],
        )
        .await;
        let error = spotify.devices().await.unwrap_err();
        assert_eq!(error, Error::Status(401));
        assert!(!format!("{error:?} {error}").contains("fixture-private"));
        assert!(spotify.credential.is_none());
        assert_eq!(broker.await.unwrap().len(), 2);
        assert_eq!(provider.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn redirect_is_not_followed_and_large_player_body_is_refused() {
        let mut redirect = reply(302, "");
        redirect.headers = "location: http://127.0.0.1:1/must-not-follow\r\n".into();
        let (mut spotify, broker, provider) = fixture(
            vec![credential()],
            vec![redirect, reply(200, &"x".repeat(PLAYER_BYTES + 1))],
        )
        .await;
        assert_eq!(spotify.profile().await, Err(Error::Status(302)));
        assert_eq!(spotify.player().await, Err(Error::Capacity));
        assert_eq!(broker.await.unwrap().len(), 1);
        assert_eq!(provider.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn token_revocation_and_rate_limits_remain_structured_errors() {
        let mut limited = reply(429, "");
        limited.headers = "retry-after: 1.25\r\n".into();
        let (mut spotify, broker, provider) =
            fixture(vec![reply(403, r#"{"code":40012}"#), limited], vec![]).await;
        assert_eq!(spotify.profile().await, Err(Error::Revoked));
        assert_eq!(
            spotify.profile().await,
            Err(Error::RateLimited(Duration::from_millis(1250)))
        );
        assert_eq!(broker.await.unwrap().len(), 2);
        assert!(provider.await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn credential_body_and_value_are_bounded() {
        let oversized = serde_json::json!({"access_token":"x".repeat(8193)}).to_string();
        let (mut spotify, broker, provider) = fixture(
            vec![
                reply(200, &oversized),
                reply(200, &"x".repeat(TOKEN_BYTES + 1)),
            ],
            vec![],
        )
        .await;
        assert_eq!(spotify.devices().await, Err(Error::Credential));
        assert_eq!(spotify.devices().await, Err(Error::Capacity));
        assert!(spotify.credential.is_none());
        assert_eq!(broker.await.unwrap().len(), 2);
        assert!(provider.await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_length_player_stream_cannot_exceed_the_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chunked = tokio::spawn(async move {
            let (mut stream, _) = tokio::time::timeout(TIMEOUT, listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
                    .await
                    .unwrap()
                    .unwrap();
                if count == 0 {
                    return;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let block = format!("4000\r\n{}\r\n", "x".repeat(16 * 1024));
            for _ in 0..5 {
                if stream.write_all(block.as_bytes()).await.is_err() {
                    return;
                }
            }
            let _ = stream.write_all(b"0\r\n\r\n").await;
        });
        let (mut spotify, broker, unused) = fixture(vec![credential()], vec![]).await;
        spotify.api = format!("http://{address}");
        assert_eq!(spotify.player().await, Err(Error::Capacity));
        assert_eq!(broker.await.unwrap().len(), 1);
        unused.await.unwrap();
        chunked.await.unwrap();
    }

    #[tokio::test]
    #[allow(
        clippy::result_large_err,
        reason = "tungstenite's handshake callback requires an HTTP response as its error"
    )]
    async fn dealer_handshake_stays_private_and_carries_bounded_json_and_ping() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = tokio::time::timeout(TIMEOUT, listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.uri().query(), Some("access_token=fixture-provider"));
                    assert_eq!(request.headers()["origin"], dc_proto::pin::WEBAPP_ORIGIN);
                    assert!(!request.headers().contains_key("authorization"));
                    Ok(response)
                },
            )
            .await
            .unwrap();
            socket
                .send(Message::text(
                    r#"{"type":"message","uri":"hm://pusher/v1/connections/fixture"}"#,
                ))
                .await
                .unwrap();
            let ping = tokio::time::timeout(TIMEOUT, socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(ping.into_text().unwrap(), r#"{"type":"ping"}"#);
            socket.close(None).await.unwrap();
        });
        let (mut spotify, broker, unused) = fixture(vec![credential()], vec![]).await;
        spotify.dealer = format!("ws://{address}/");
        let mut dealer = spotify.open_dealer().await.unwrap();
        assert_eq!(
            dealer.receive().await.unwrap().unwrap()["uri"],
            "hm://pusher/v1/connections/fixture"
        );
        dealer.ping().await.unwrap();
        assert_eq!(dealer.receive().await.unwrap(), None);
        peer.await.unwrap();
        broker.await.unwrap();
        unused.await.unwrap();
    }

    #[tokio::test]
    async fn dealer_rejects_oversized_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = tokio::time::timeout(TIMEOUT, listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = socket
                .send(Message::text("x".repeat(PLAYER_BYTES + 1)))
                .await;
        });
        let (mut spotify, broker, unused) = fixture(vec![credential()], vec![]).await;
        spotify.dealer = format!("ws://{address}/");
        let mut dealer = spotify.open_dealer().await.unwrap();
        assert_eq!(dealer.receive().await, Err(Error::Capacity));
        peer.await.unwrap();
        broker.await.unwrap();
        unused.await.unwrap();
    }
}
