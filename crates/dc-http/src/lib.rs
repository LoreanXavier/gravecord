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

const MAX_RATE_LIMIT_RETRIES: u32 = 5;

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
    token: Token,
    identity: Identity,
    /// Called when Discord answers 401: the session is gone (`401` in `interceptResponse`, module 596339).
    unauthorized: Option<Arc<dyn Fn() + Send + Sync>>,
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
            token,
            identity,
            unauthorized: None,
        })
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
        let url = format!("{}{route}", pin::API_BASE);
        let mut request = self
            .http
            .request(method.clone(), url)
            .header("authorization", self.token.expose())
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            );
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?)
    }

    pub async fn request(
        &self,
        method: Method,
        route: &str,
        body: Option<&Value>,
    ) -> Result<Value, Error> {
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let response = self.send(&method, route, body).await?;
            let status = response.status();
            debug!("{method} {route}: {status}");
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
            self.note_status(status, &value);
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

    /// `GET` with an application's own OAuth2 access token as the bearer, for the Embedded App SDK's `AUTHENTICATE`
    /// (`GET /oauth2/@me`). The account's token is never sent: this is the activity's token, which the page handed over.
    pub async fn get_as_bearer(&self, route: &str, bearer: &str) -> Result<Value, Error> {
        let url = format!("{}{route}", pin::API_BASE);
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
        let url = format!("{}{route}", pin::API_BASE);
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
                .header("authorization", self.token.expose())
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
        let url = format!("{}{route}", pin::API_BASE);
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
            .header("authorization", self.token.expose())
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

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::identity::{Launch, System};
    use serde_json::json;

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
    fn client_debug_cannot_show_the_token() {
        assert!(!format!("{:?}", Token::new("secret")).contains("secret"));
    }
}
