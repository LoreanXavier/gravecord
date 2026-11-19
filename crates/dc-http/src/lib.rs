//! REST client with the pinned header set from `TRAFFIC/identify.json`.

use dc_proto::identity::Identity;
use dc_proto::pin;
use dc_proto::secrets::Token;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Response, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, warn};

pub mod spotify;
mod upload;
mod webhook;

pub use upload::Progress;

/// The answer of [`Client::fetch_public_limited`].
#[derive(Debug)]
pub enum LimitedGet {
    Body(Vec<u8>),
    /// An error status, with its reason phrase.
    Status(u16, String),
    /// The body is over the limit.
    TooLarge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConditionalGet {
    NotModified {
        etag: Option<String>,
    },
    Ok {
        etag: Option<String>,
        bytes: Vec<u8>,
    },
}

const MAX_RATE_LIMIT_RETRIES: u32 = 5;

/// A route as the debug log writes it: the path only. Queries can carry what the user typed (a search) or a
/// refresh token, and debug logs leave the machine in bug reports.
fn log_path(route: &str) -> &str {
    route.split_once('?').map_or(route, |(path, _)| path)
}

fn diagnostic_route(route: &str) -> &str {
    match route.split_once('?') {
        Some((path, query))
            if query
                .split('&')
                .any(|pair| pair.starts_with("refresh_token=")) =>
        {
            path
        }
        _ => route,
    }
}

fn transport_error(error: reqwest::Error, route: &str) -> Error {
    Error::Transport(if diagnostic_route(route) != route {
        error.without_url()
    } else {
        error
    })
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("{route} returned {status}")]
    Status {
        route: String,
        status: StatusCode,
        body: Value,
    },
    /// Every attempt was answered 429. `retry_after` is what the last answer asked to wait, in seconds as the body
    /// carries it (`retry_after`), so a caller can hold off as the official client does.
    #[error("{route} stayed rate limited")]
    RateLimited {
        route: String,
        retry_after: Option<f64>,
    },
    #[error("invalid header value")]
    Header,
}

pub struct Client {
    /// The connection pool. Replaced by a fresh one when a request had to be asked again on a new connection
    /// (`get_hedged`): the old pool's connection is the one that stopped answering.
    http: std::sync::RwLock<reqwest::Client>,
    /// Replaced when the account's token is renewed (`UPDATE_TOKEN`, module 631670 `KD`).
    token: std::sync::RwLock<Token>,
    identity: Identity,
    /// Called when Discord answers 401: the session is gone (`401` in `interceptResponse`, module 596339).
    unauthorized: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Called for a 4xx answer whose code says the account has limited access (module 30076: 340000 to 349999).
    limited_access: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Where routes are sent; a test points it at a local server.
    api_base: String,
}

/// Module 30076 `O`: a 4xx answer with a code from `USER_LIMITED_ACCESS_DEFAULT` (340000) to `USER_LIMITED_ACCESS_MAX`
/// (349999).
fn is_limited_access(status: StatusCode, body: &Value) -> bool {
    status.is_client_error()
        && body["code"]
            .as_u64()
            .is_some_and(|code| (340_000..=349_999).contains(&code))
}

/// The header set every request of the pinned client carries, shared with the login client (`dc-auth`).
pub fn fixed_headers(identity: &Identity) -> Result<HeaderMap, Error> {
    let mut headers = HeaderMap::new();
    let mut add = |name: &'static str, value: &str| -> Result<(), Error> {
        let value = HeaderValue::from_str(value).map_err(|_| Error::Header)?;
        headers.insert(HeaderName::from_static(name), value);
        Ok(())
    };
    add("accept", "*/*")?;
    add("accept-language", &identity.system.locale)?;
    add("origin", pin::WEBAPP_ORIGIN)?;
    add("referer", &format!("{}/channels/@me", pin::WEBAPP_ORIGIN))?;
    add("sec-ch-ua", pin::SEC_CH_UA)?;
    add("sec-ch-ua-mobile", "?0")?;
    add("sec-ch-ua-platform", "\"Windows\"")?;
    add("sec-fetch-dest", "empty")?;
    add("sec-fetch-mode", "cors")?;
    add("sec-fetch-site", "same-origin")?;
    add("x-debug-options", "bugReporterEnabled")?;
    add("x-discord-locale", &identity.system.locale)?;
    add("x-discord-timezone", &identity.system.timezone)?;
    Ok(headers)
}

/// The connection pool of a client. Every request to a host rides one HTTP/2 connection: one that stops answering (a
/// sleep, a changed network) is found by its pings and dropped, so the next requests open a new one instead of waiting
/// on it. The connect timeout covers the TCP connect and the TLS handshake (reqwest wraps the whole connector).
fn pool(identity: &Identity) -> Result<reqwest::Client, Error> {
    Ok(reqwest::Client::builder()
        .user_agent(pin::USER_AGENT)
        .default_headers(fixed_headers(identity)?)
        .tcp_nodelay(true)
        .pool_max_idle_per_host(32)
        .connect_timeout(Duration::from_secs(10))
        .http2_keep_alive_interval(Duration::from_secs(20))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        .build()?)
}

/// A `multipart/form-data` request (`Client::post_form`).
#[derive(Clone, Debug)]
pub struct Form {
    pub fields: Vec<(String, String)>,
    pub file_field: String,
    pub file_name: String,
    pub file_type: String,
    pub file: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Client {
    pub fn new(token: Token, identity: Identity) -> Result<Self, Error> {
        let http = pool(&identity)?;
        Ok(Self {
            http: std::sync::RwLock::new(http),
            token: std::sync::RwLock::new(token),
            identity,
            unauthorized: None,
            limited_access: None,
            api_base: pin::API_BASE.to_owned(),
        })
    }

    fn http(&self) -> reqwest::Client {
        self.http.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The same client on a pool of its own: new connections, and a new name lookup for each.
    fn fresh(&self) -> Result<Self, Error> {
        Ok(Self {
            http: std::sync::RwLock::new(pool(&self.identity)?),
            token: std::sync::RwLock::new(Token::new(self.authorization())),
            identity: self.identity.clone(),
            unauthorized: self.unauthorized.clone(),
            limited_access: self.limited_access.clone(),
            api_base: self.api_base.clone(),
        })
    }

    /// `GET` for a request the screen waits on (the open channel's page at the start). With HTTP/2 every request to
    /// a host shares one connection, so a connection that stopped answering (or a connect that hangs, as one routed to
    /// a dead address does) would hold it until the connect timeout or the pings give up. When no answer has come
    /// after `after`, the same request is asked again on a new pool, and the first good answer wins; when the new
    /// pool's answers, the later requests use it too.
    pub async fn get_hedged(&self, route: &str, after: Duration) -> Result<Value, Error> {
        let first = self.get(route);
        tokio::pin!(first);
        tokio::select! {
            result = &mut first => return result,
            () = tokio::time::sleep(after) => {}
        }
        debug!(
            "GET {}: no answer after {} ms, asking again on a new connection",
            log_path(route),
            after.as_millis()
        );
        let Ok(fresh) = self.fresh() else {
            return first.await;
        };
        let second = fresh.get(route);
        tokio::pin!(second);
        tokio::select! {
            result = &mut first => match result {
                Ok(value) => Ok(value),
                Err(_) => {
                    let result = second.await;
                    if result.is_ok() {
                        self.adopt(fresh.http());
                    }
                    result
                }
            },
            result = &mut second => match result {
                Ok(value) => {
                    self.adopt(fresh.http());
                    Ok(value)
                }
                Err(_) => first.await,
            },
        }
    }

    /// The new pool answered where the old one did not: the requests after this one use it.
    fn adopt(&self, http: reqwest::Client) {
        *self.http.write().unwrap_or_else(|e| e.into_inner()) = http;
    }

    /// Sends every route to `base` instead of the API (for tests against a local server).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// Every request after this one carries the new token: `PATCH /users/@me` answers a password change with one
    /// (`UPDATE_TOKEN`, module 631670 `KD`) and the old one is dead.
    pub fn set_token(&self, token: Token) {
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = token;
    }

    fn authorization(&self) -> String {
        self.token
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .expose()
            .to_owned()
    }

    /// Runs `hook` whenever a request comes back 401 (not a step-up request), so the app can drop the stored login.
    pub fn on_unauthorized(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.unauthorized = Some(Arc::new(hook));
        self
    }

    /// Runs `hook` when a request comes back refused because the account has limited access (module 596339's
    /// `interceptResponse` opens the notice of module 218642 then).
    pub fn on_limited_access(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.limited_access = Some(Arc::new(hook));
        self
    }

    fn note_status(&self, status: StatusCode, body: &Value) {
        if is_limited_access(status, body)
            && let Some(hook) = &self.limited_access
        {
            hook();
        }
        // Only the plain answer to a token the server no longer accepts (`{"message": "401: Unauthorized",
        // "code": 0}`, or no code at all) ends the login. A 401 with any other code is about that one request (a
        // step-up, a route's own refusal) and must not throw away the stored login of the account.
        if status == StatusCode::UNAUTHORIZED
            && body["code"].as_u64().unwrap_or(0) == 0
            && let Some(hook) = &self.unauthorized
        {
            hook();
        }
    }

    async fn send(
        &self,
        method: &Method,
        route: &str,
        body: Option<&Value>,
    ) -> Result<Response, Error> {
        let (route, reason) = split_audit_reason(route);
        let url = format!("{}{route}", self.api_base);
        let url = dc_proto::replay::rewrite(&url).into_owned();
        let mut request = self
            .http()
            .request(method.clone(), url)
            .header("authorization", self.authorization())
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            );
        if let Some(reason) = reason {
            // `reason` of the bundle's request options (module 620632) travels as this header, percent-encoded.
            request = request.header("x-audit-log-reason", reason);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        request
            .send()
            .await
            .map_err(|error| transport_error(error, &route))
    }

    pub async fn request(
        &self,
        method: Method,
        route: &str,
        body: Option<&Value>,
    ) -> Result<Value, Error> {
        let diagnostic = diagnostic_route(route);
        let mut last_retry_after = None;
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let asked = std::time::Instant::now();
            let response = self.send(&method, route, body).await?;
            let status = response.status();
            let content_type = content_type(&response);
            debug!("{method} {}: {status}", log_path(route));
            if status == StatusCode::TOO_MANY_REQUESTS {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                if slowmode_rejection(&body) {
                    return Err(Error::Status {
                        route: diagnostic.to_owned(),
                        status,
                        body,
                    });
                }
                last_retry_after = body["retry_after"].as_f64();
                let wait = retry_after(&body);
                warn!(
                    "{} rate limited for {wait:?}, attempt {attempt}",
                    log_path(route)
                );
                if attempt == MAX_RATE_LIMIT_RETRIES {
                    // No attempt follows; the caller gets the wait instead.
                    break;
                }
                tokio::time::sleep(wait).await;
                continue;
            }
            let text = response
                .text()
                .await
                .map_err(|error| transport_error(error, route))?;
            dc_proto::replay::record_http(
                method.as_str(),
                &format!("{}{}", self.api_base, split_audit_reason(route).0),
                status.as_u16(),
                content_type.as_deref(),
                asked.elapsed(),
                text.as_bytes(),
            );
            let value = if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or(Value::Null)
            };
            if status.is_success() {
                return Ok(value);
            }
            self.note_status(status, &value);
            return Err(Error::Status {
                route: diagnostic.to_owned(),
                status,
                body: value,
            });
        }
        Err(Error::RateLimited {
            route: diagnostic.to_owned(),
            retry_after: last_retry_after,
        })
    }

    /// `GET` with an application's own OAuth2 access token as the bearer, for the Embedded App SDK's `AUTHENTICATE`
    /// (`GET /oauth2/@me`). The account's token is never sent: this is the activity's token, which the page handed over.
    pub async fn get_as_bearer(&self, route: &str, bearer: &str) -> Result<Value, Error> {
        let url = format!("{}{route}", self.api_base);
        let url = dc_proto::replay::rewrite(&url).into_owned();
        let response = self
            .http()
            .get(url)
            .header("authorization", format!("Bearer {bearer}"))
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            )
            .send()
            .await?;
        let status = response.status();
        debug!("GET {} as bearer: {status}", log_path(route));
        let text = response.text().await?;
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            Ok(value)
        } else {
            Err(Error::Status {
                route: route.to_owned(),
                status,
                body: value,
            })
        }
    }

    /// `POST` with a form: its text fields, then one file. A rate limited attempt is sent again, so the form is rebuilt
    /// each time.
    pub async fn post_form(&self, route: &str, form: &Form) -> Result<Value, Error> {
        let url = format!("{}{route}", self.api_base);
        let url = dc_proto::replay::rewrite(&url).into_owned();
        let mut last_retry_after = None;
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let file = reqwest::multipart::Part::bytes(form.file.clone())
                .file_name(form.file_name.clone())
                .mime_str(&form.file_type)?;
            let mut body = reqwest::multipart::Form::new();
            for (name, value) in &form.fields {
                body = body.text(name.clone(), value.clone());
            }
            let mut request = self
                .http()
                .post(&url)
                .header("authorization", self.authorization())
                .header(
                    "x-super-properties",
                    self.identity.super_properties_header(true),
                )
                .multipart(body.part(form.file_field.clone(), file));
            for (name, value) in &form.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            let response = request.send().await?;
            let status = response.status();
            debug!("POST {}: {status}", log_path(route));
            if status == StatusCode::TOO_MANY_REQUESTS {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                last_retry_after = body["retry_after"].as_f64();
                let wait = retry_after(&body);
                warn!(
                    "{} rate limited for {wait:?}, attempt {attempt}",
                    log_path(route)
                );
                if attempt == MAX_RATE_LIMIT_RETRIES {
                    break;
                }
                tokio::time::sleep(wait).await;
                continue;
            }
            let text = response.text().await?;
            let value = if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or(Value::Null)
            };
            if status.is_success() {
                return Ok(value);
            }
            return Err(Error::Status {
                route: route.to_owned(),
                status,
                body: value,
            });
        }
        Err(Error::RateLimited {
            route: route.to_owned(),
            retry_after: last_retry_after,
        })
    }

    /// `POST` several files as one `multipart/form-data` body, each as a part named after its file (the official
    /// debug log upload, `attachments` of `uploadDebugFiles`). One attempt: a rate limited upload fails, as there.
    pub async fn post_files(
        &self,
        route: &str,
        files: &[(String, Vec<u8>)],
    ) -> Result<Value, Error> {
        let url = format!("{}{route}", self.api_base);
        let url = dc_proto::replay::rewrite(&url).into_owned();
        let mut body = reqwest::multipart::Form::new();
        for (name, bytes) in files {
            let part = reqwest::multipart::Part::bytes(bytes.clone())
                .file_name(name.clone())
                .mime_str("application/octet-stream")?;
            body = body.part(name.clone(), part);
        }
        let response = self
            .http()
            .post(&url)
            .header("authorization", self.authorization())
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            )
            .multipart(body)
            .send()
            .await?;
        let status = response.status();
        debug!("POST {}: {status}", log_path(route));
        let text = response.text().await?;
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::Null)
        };
        if status.is_success() {
            return Ok(value);
        }
        Err(Error::Status {
            route: route.to_owned(),
            status,
            body: value,
        })
    }

    /// A public asset: no authorization, so tokens never reach a CDN.
    pub async fn fetch_public(&self, url: &str) -> Result<Vec<u8>, Error> {
        let asked = std::time::Instant::now();
        let response = self
            .http()
            .get(dc_proto::replay::rewrite(url).as_ref())
            .send()
            .await?;
        let status = response.status();
        let content_type = content_type(&response);
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                dc_proto::replay::record_http(
                    "GET",
                    url,
                    status.as_u16(),
                    content_type.as_deref(),
                    asked.elapsed(),
                    &[],
                );
                return Err(error.into());
            }
        };
        let bytes = response.bytes().await?.to_vec();
        dc_proto::replay::record_http(
            "GET",
            url,
            status.as_u16(),
            content_type.as_deref(),
            asked.elapsed(),
            &bytes,
        );
        Ok(bytes)
    }

    /// A public JSON POST: no authorization, so tokens never reach the other host.
    pub async fn post_public_json(&self, url: &str, body: &Value) -> Result<Vec<u8>, Error> {
        let response = self
            .http()
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Public GET (no authorization) reading at most `limit` bytes of the body: an error status, or a body over the
    /// limit (by its length header or as it streams), comes back as [`LimitedGet`] instead of the bytes.
    pub async fn fetch_public_limited(&self, url: &str, limit: usize) -> Result<LimitedGet, Error> {
        let mut response = self.http().get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Ok(LimitedGet::Status(
                status.as_u16(),
                status.canonical_reason().unwrap_or_default().to_owned(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Ok(LimitedGet::TooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > limit {
                return Ok(LimitedGet::TooLarge);
            }
        }
        Ok(LimitedGet::Body(bytes))
    }

    /// Public CDN GET with optional `If-None-Match` validation (no authorization headers sent).
    pub async fn fetch_public_conditional(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<ConditionalGet, Error> {
        let asked = std::time::Instant::now();
        let mut req = self.http().get(dc_proto::replay::rewrite(url).as_ref());
        if let Some(tag) = etag {
            req = req.header(reqwest::header::IF_NONE_MATCH, tag);
        }
        let response = req.send().await?;
        let next_etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if response.status() == StatusCode::NOT_MODIFIED {
            dc_proto::replay::record_http("GET", url, 304, None, asked.elapsed(), &[]);
            return Ok(ConditionalGet::NotModified {
                etag: next_etag.or_else(|| etag.map(str::to_owned)),
            });
        }
        let status = response.status();
        let content_type = content_type(&response);
        let response = response.error_for_status()?;
        let bytes = response.bytes().await?.to_vec();
        dc_proto::replay::record_http(
            "GET",
            url,
            status.as_u16(),
            content_type.as_deref(),
            asked.elapsed(),
            &bytes,
        );
        Ok(ConditionalGet::Ok {
            etag: next_etag,
            bytes,
        })
    }

    /// Puts a file at the signed URL `POST /channels/{id}/attachments` handed out. The URL is the grant, so no
    /// authorization goes with it.
    pub async fn put_upload(&self, url: &str, bytes: Vec<u8>) -> Result<(), Error> {
        self.http()
            .put(dc_proto::replay::rewrite(url).as_ref())
            .header("content-type", "application/octet-stream")
            .body(bytes)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get(&self, route: &str) -> Result<Value, Error> {
        self.request(Method::GET, route, None).await
    }

    pub async fn post(&self, route: &str, body: &Value) -> Result<Value, Error> {
        self.request(Method::POST, route, Some(body)).await
    }
}

/// The answer's `content-type`, for the replay recorder.
fn content_type(response: &Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn retry_after(body: &Value) -> Duration {
    Duration::from_secs_f64(body["retry_after"].as_f64().unwrap_or(1.0).clamp(0.0, 60.0))
}

// Module 138399 handles this in the composer; retrying it here would send a held message later.
fn slowmode_rejection(body: &Value) -> bool {
    body["code"].as_u64() == Some(20016)
}

/// The query parameter that carries a moderation reason through a route string; `send` turns it into the
/// `X-Audit-Log-Reason` header the official client sets for `reason` (kick, ban, timeout and the like).
pub const AUDIT_REASON_PARAM: &str = "__audit_reason";

/// `text` percent-encoded the way `encodeURIComponent` does it.
pub fn percent_encode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// `route` with `reason` attached as the audit reason, percent-encoded. An empty reason adds nothing.
pub fn with_audit_reason(route: &str, reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        return route.to_owned();
    }
    let joiner = if route.contains('?') { '&' } else { '?' };
    format!(
        "{route}{joiner}{AUDIT_REASON_PARAM}={}",
        percent_encode(reason)
    )
}

/// Takes the audit reason back out of a route: the route as it goes on the wire, and the header value.
fn split_audit_reason(route: &str) -> (String, Option<String>) {
    let Some((path, query)) = route.split_once('?') else {
        return (route.to_owned(), None);
    };
    let mut kept = Vec::new();
    let mut reason = None;
    for pair in query.split('&') {
        match pair
            .strip_prefix(AUDIT_REASON_PARAM)
            .and_then(|rest| rest.strip_prefix('='))
        {
            Some(value) => reason = Some(value.to_owned()),
            None => kept.push(pair),
        }
    }
    if kept.is_empty() {
        (path.to_owned(), reason)
    } else {
        (format!("{path}?{}", kept.join("&")), reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::identity::{Launch, System};
    use serde_json::json;

    #[test]
    fn an_audit_reason_rides_the_route_and_leaves_as_a_header() {
        let route = with_audit_reason("/guilds/1/bans/2", "spam & more");
        assert_eq!(route, "/guilds/1/bans/2?__audit_reason=spam%20%26%20more");
        assert_eq!(
            split_audit_reason(&route),
            (
                "/guilds/1/bans/2".to_owned(),
                Some("spam%20%26%20more".to_owned())
            )
        );
        assert_eq!(with_audit_reason("/a", "  "), "/a");
        let with_query = with_audit_reason("/guilds/1/members/2?x=1", "r");
        assert_eq!(
            split_audit_reason(&with_query),
            ("/guilds/1/members/2?x=1".to_owned(), Some("r".to_owned()))
        );
        assert_eq!(split_audit_reason("/a"), ("/a".to_owned(), None));
    }

    fn identity() -> Identity {
        Identity {
            system: System {
                os: "Windows",
                os_version: "10.0.26200".into(),
                os_sdk_version: "26200".into(),
                os_build_revision: "9445".into(),
                arch: "x64",
                locale: "en-US".into(),
                timezone: "Europe/Istanbul".into(),
            },
            launch: Launch::new(),
        }
    }

    #[test]
    fn fixed_headers_match_the_recording() {
        let headers = fixed_headers(&identity()).unwrap();
        assert_eq!(headers["x-debug-options"], "bugReporterEnabled");
        assert_eq!(headers["x-discord-timezone"], "Europe/Istanbul");
        assert_eq!(headers["origin"], "https://discordapp.com");
        assert_eq!(headers["referer"], "https://discordapp.com/channels/@me");
    }

    #[test]
    fn inventory_refresh_queries_are_removed_from_diagnostics() {
        let route = "/content-inventory/users/@me?feature=inbox&refresh_token=fixture-cursor&for_game_profile=false";
        assert_eq!(diagnostic_route(route), "/content-inventory/users/@me");
        assert_eq!(diagnostic_route("/messages?limit=50"), "/messages?limit=50");
        let error = reqwest::Client::new()
            .get("https://example.invalid/")
            .header("invalid\nheader", "value")
            .build()
            .unwrap_err()
            .with_url(reqwest::Url::parse(&format!("https://example.invalid{route}")).unwrap());
        let safe = transport_error(error, route);
        assert!(!format!("{safe:?}").contains("fixture-cursor"));
        assert!(!format!("{safe}").contains("fixture-cursor"));
    }

    #[test]
    fn limited_access_is_a_4xx_with_a_code_in_its_range() {
        assert!(is_limited_access(
            StatusCode::FORBIDDEN,
            &json!({"code": 340000})
        ));
        assert!(is_limited_access(
            StatusCode::BAD_REQUEST,
            &json!({"code": 349999})
        ));
        assert!(!is_limited_access(
            StatusCode::FORBIDDEN,
            &json!({"code": 350000})
        ));
        assert!(!is_limited_access(
            StatusCode::FORBIDDEN,
            &json!({"code": 50013})
        ));
        assert!(!is_limited_access(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"code": 340001})
        ));
    }

    #[test]
    fn unauthorized_hook_ignores_step_up_requests() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        let client = Client::new(Token::new("t"), identity())
            .unwrap()
            .on_unauthorized(move || {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        client.note_status(StatusCode::UNAUTHORIZED, &json!({"code": 60003}));
        client.note_status(StatusCode::UNAUTHORIZED, &json!({"code": 50014}));
        client.note_status(StatusCode::FORBIDDEN, &json!({}));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        client.note_status(
            StatusCode::UNAUTHORIZED,
            &json!({"message": "401: Unauthorized", "code": 0}),
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn retry_after_is_bounded() {
        assert_eq!(
            retry_after(&json!({"retry_after": 2.5})),
            Duration::from_millis(2500)
        );
        assert_eq!(
            retry_after(&json!({"retry_after": 9000})),
            Duration::from_secs(60)
        );
        assert_eq!(retry_after(&json!({})), Duration::from_secs(1));
    }

    #[test]
    fn slowmode_is_not_an_automatic_http_retry() {
        assert!(slowmode_rejection(&json!({"code":20016,"retry_after":5})));
        assert!(!slowmode_rejection(&json!({"retry_after":5,"global":true})));
        assert!(!slowmode_rejection(&json!({"code":20028,"retry_after":5})));
    }

    #[test]
    fn a_renewed_token_replaces_the_one_requests_carry() {
        let client = Client::new(Token::new("fixture-old"), Identity::new()).unwrap();
        assert_eq!(client.authorization(), "fixture-old");
        client.set_token(Token::new("fixture-new"));
        assert_eq!(client.authorization(), "fixture-new");
    }

    #[test]
    fn client_debug_cannot_show_the_token() {
        assert!(!format!("{:?}", Token::new("secret")).contains("secret"));
    }

    /// A local server that answers every request 429: the error that ends the retries carries the last wait.
    #[tokio::test]
    async fn a_page_held_by_a_connection_that_never_answers_is_asked_again_on_a_new_one() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // The first connection is accepted and never answered (as one routed to a dead address); the next ones are.
        let server = tokio::spawn(async move {
            let mut held = Vec::new();
            let mut connections = 0;
            while let Ok((mut stream, _)) = listener.accept().await {
                connections += 1;
                if connections == 1 {
                    held.push(stream);
                    continue;
                }
                // Every request on the connection is answered (the client keeps it for the next one).
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let mut seen = Vec::new();
                        while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(read) => seen.extend_from_slice(&buffer[..read]),
                            }
                        }
                        let body = r#"[{"id":"1"}]"#;
                        let answer = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if stream.write_all(answer.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let client = Client::new(Token::new("fixture"), Identity::new())
            .unwrap()
            .with_api_base(format!("http://{address}"));
        let started = std::time::Instant::now();
        let page = client.get_hedged("/channels/1/messages?limit=50", Duration::from_millis(300));
        let page = tokio::time::timeout(Duration::from_secs(10), page)
            .await
            .expect("the page answers within the test's bound")
            .unwrap();
        assert_eq!(page[0]["id"], "1");
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(300) && took < Duration::from_secs(3),
            "answered after the hedge, not after a timeout: {took:?}"
        );
        // The pool that answered is the one the next requests use: they never wait on the silent connection.
        let again = std::time::Instant::now();
        let page = tokio::time::timeout(Duration::from_secs(3), client.get("/channels/1/messages"))
            .await
            .expect("the next request does not wait on the silent connection")
            .unwrap();
        assert_eq!(page[0]["id"], "1");
        assert!(again.elapsed() < Duration::from_secs(2));
        server.abort();
    }

    #[tokio::test]
    async fn a_page_that_answers_in_time_is_asked_for_once() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = requests.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let counted = counted.clone();
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 4096];
                    loop {
                        let mut seen = Vec::new();
                        while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(read) => seen.extend_from_slice(&buffer[..read]),
                            }
                        }
                        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let body = "[]";
                        let answer = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if stream.write_all(answer.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let client = Client::new(Token::new("fixture"), Identity::new())
            .unwrap()
            .with_api_base(format!("http://{address}"));
        let page = client.get_hedged("/channels/1/messages?limit=50", Duration::from_secs(2));
        let page = tokio::time::timeout(Duration::from_secs(10), page)
            .await
            .expect("the page answers within the test's bound")
            .unwrap();
        assert!(page.is_array());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn a_route_that_stays_rate_limited_reports_the_last_retry_after() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut answered = 0;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut seen = Vec::new();
                let mut buffer = [0_u8; 4096];
                while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    seen.extend_from_slice(&buffer[..read]);
                }
                let body = r#"{"message":"You are being rate limited.","retry_after":0.01,"global":false}"#;
                let answer = format!(
                    "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(answer.as_bytes()).await.unwrap();
                answered += 1;
                if answered > MAX_RATE_LIMIT_RETRIES {
                    break;
                }
            }
            answered
        });
        let client = Client::new(Token::new("fixture"), Identity::new())
            .unwrap()
            .with_api_base(format!("http://{address}"));
        let error = client.get("/streams/call:1:2/preview").await.unwrap_err();
        let Error::RateLimited { retry_after, .. } = error else {
            panic!("{error:?}");
        };
        assert_eq!(retry_after, Some(0.01));
        assert_eq!(server.await.unwrap(), MAX_RATE_LIMIT_RETRIES + 1);
    }
}
