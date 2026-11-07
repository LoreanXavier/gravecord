//! REST client with the pinned header set from `TRAFFIC/identify.json`.

use dc_proto::identity::Identity;
use dc_proto::pin;
use dc_proto::secrets::Token;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Response, StatusCode};
use serde_json::Value;
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

pub struct Client {
    http: reqwest::Client,
    token: Token,
    identity: Identity,
}

fn fixed_headers(identity: &Identity) -> Result<HeaderMap, Error> {
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
        })
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
