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

mod upload;
mod webhook;

pub use upload::Progress;

const MAX_RATE_LIMIT_RETRIES: u32 = 5;

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
    #[error("{route} stayed rate limited")]
    RateLimited { route: String },
    #[error("invalid header value")]
    Header,
}

/// 401 with this code is a step-up (MFA) request for a sensitive action, not a dead token.
const MFA_REQUIRED: u64 = 60003;

pub struct Client {
    http: reqwest::Client,
    /// Replaced when the account's token is renewed (`UPDATE_TOKEN`, module 631670 `KD`).
    token: std::sync::RwLock<Token>,
    identity: Identity,
    /// Called when Discord answers 401: the session is gone (`401` in `interceptResponse`, module 596339).
    unauthorized: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Where routes are sent; a test points it at a local server.
    api_base: String,
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
        let http = reqwest::Client::builder()
            .user_agent(pin::USER_AGENT)
            .default_headers(fixed_headers(&identity)?)
            .build()?;
        Ok(Self {
            http,
            token: std::sync::RwLock::new(token),
            identity,
            unauthorized: None,
            api_base: pin::API_BASE.to_owned(),
        })
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

    fn note_status(&self, status: StatusCode, body: &Value) {
        if status == StatusCode::UNAUTHORIZED
            && body["code"].as_u64() != Some(MFA_REQUIRED)
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
        let mut request = self
            .http
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
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let response = self.send(&method, route, body).await?;
            let status = response.status();
            debug!("{method} {diagnostic}: {status}");
            if status == StatusCode::TOO_MANY_REQUESTS {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                if slowmode_rejection(&body) {
                    return Err(Error::Status {
                        route: diagnostic.to_owned(),
                        status,
                        body,
                    });
                }
                let wait = retry_after(&body);
                warn!("{diagnostic} rate limited for {wait:?}, attempt {attempt}");
                tokio::time::sleep(wait).await;
                continue;
            }
            let text = response
                .text()
                .await
                .map_err(|error| transport_error(error, route))?;
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
        })
    }

    /// `GET` with an application's own OAuth2 access token as the bearer, for the Embedded App SDK's `AUTHENTICATE`
    /// (`GET /oauth2/@me`). The account's token is never sent: this is the activity's token, which the page handed over.
    pub async fn get_as_bearer(&self, route: &str, bearer: &str) -> Result<Value, Error> {
        let url = format!("{}{route}", self.api_base);
        let response = self
            .http
            .get(url)
            .header("authorization", format!("Bearer {bearer}"))
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            )
            .send()
            .await?;
        let status = response.status();
        debug!("GET {route} as bearer: {status}");
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
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let file = reqwest::multipart::Part::bytes(form.file.clone())
                .file_name(form.file_name.clone())
                .mime_str(&form.file_type)?;
            let mut body = reqwest::multipart::Form::new();
            for (name, value) in &form.fields {
                body = body.text(name.clone(), value.clone());
            }
            let mut request = self
                .http
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
            debug!("POST {route}: {status}");
            if status == StatusCode::TOO_MANY_REQUESTS {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                let wait = retry_after(&body);
                warn!("{route} rate limited for {wait:?}, attempt {attempt}");
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
        let mut body = reqwest::multipart::Form::new();
        for (name, bytes) in files {
            let part = reqwest::multipart::Part::bytes(bytes.clone())
                .file_name(name.clone())
                .mime_str("application/octet-stream")?;
            body = body.part(name.clone(), part);
        }
        let response = self
            .http
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
        debug!("POST {route}: {status}");
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
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Puts a file at the signed URL `POST /channels/{id}/attachments` handed out. The URL is the grant, so no
    /// authorization goes with it.
    pub async fn put_upload(&self, url: &str, bytes: Vec<u8>) -> Result<(), Error> {
        self.http
            .put(url)
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
    fn unauthorized_hook_ignores_step_up_requests() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = calls.clone();
        let client = Client::new(Token::new("t"), identity())
            .unwrap()
            .on_unauthorized(move || {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        client.note_status(StatusCode::UNAUTHORIZED, &json!({"code": 60003}));
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
}
