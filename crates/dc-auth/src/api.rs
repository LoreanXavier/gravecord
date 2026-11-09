//! The auth REST calls, from `login`, `loginMFAv2`, `forgotPassword` and the remote auth screen of the pinned build
//! (modules 830215 and 785161; routes in module 652215: `LOGIN`, `LOGIN_MFA`, `LOGIN_SMS_SEND`, `FORGOT_PASSWORD`,
//! `REMOTE_AUTH_LOGIN`, `LOGOUT`). The headers are the pinned identity's.
//!
//! No call here carries an `authorization` header except `logout`, and no log line carries a body.

use crate::errors::{CaptchaAnswer, Failure, classify};
use dc_proto::identity::Identity;
use dc_proto::pin;
use dc_proto::secrets::Token;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

/// What the form holds. The password is wiped when this is dropped.
pub struct LoginRequest {
    pub login: String,
    pub password: Zeroizing<String>,
    pub undelete: bool,
}

/// `ea()` in module 46653: the methods the account offers, in the order the dialog starts with.
#[derive(Clone, Debug, PartialEq)]
pub struct MfaChallenge {
    pub ticket: String,
    pub methods: Vec<MfaMethod>,
    pub login_instance_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MfaMethod {
    /// The WebAuthn challenge, a JSON document.
    WebAuthn(String),
    Totp,
    Backup,
    Sms,
}

impl MfaMethod {
    /// The route segment of `LOGIN_MFA(type)`.
    pub fn route(&self) -> &'static str {
        match self {
            Self::WebAuthn(_) => "webauthn",
            Self::Totp => "totp",
            Self::Backup => "backup",
            Self::Sms => "sms",
        }
    }
}

/// How a `/auth/mfa/{type}` call is addressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MfaKind {
    Totp,
    Sms,
    Backup,
    WebAuthn,
}

impl MfaKind {
    fn route(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Sms => "sms",
            Self::Backup => "backup",
            Self::WebAuthn => "webauthn",
        }
    }
}

pub enum LoginOutcome {
    Token(Token),
    Mfa(MfaChallenge),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ForgotOutcome {
    /// Instructions were mailed to the address typed.
    Sent,
    /// The account signs in from a mailed link instead (`ONE_TIME_LOGIN`).
    OneTimeLogin,
}

pub struct AuthApi {
    http: reqwest::Client,
    base: String,
    identity: Identity,
    fingerprint: Mutex<Option<String>>,
}

fn token_of(body: &Value) -> Option<Token> {
    body["token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(Token::new)
}

impl AuthApi {
    /// `base` is the REST root, `pin::API_BASE` outside tests.
    pub fn new(base: impl Into<String>, identity: Identity) -> Result<Self, dc_http::Error> {
        let http = reqwest::Client::builder()
            .user_agent(pin::USER_AGENT)
            .default_headers(dc_http::fixed_headers(&identity)?)
            .build()?;
        Ok(Self {
            http,
            base: base.into().trim_end_matches('/').to_owned(),
            identity,
            fingerprint: Mutex::new(None),
        })
    }

    pub fn pinned(identity: Identity) -> Result<Self, dc_http::Error> {
        Self::new(pin::API_BASE, identity)
    }

    /// `X-Fingerprint` comes from `GET /experiments`, as the logged-out client gets it (`ee` in module 46653). A failure
    /// only means the header is left out.
    async fn fingerprint(&self) -> Option<String> {
        let mut cached = self.fingerprint.lock().await;
        if cached.is_none() {
            let answer = self
                .http
                .get(format!(
                    "{}/experiments?with_guild_experiments=true",
                    self.base
                ))
                .header(
                    "x-super-properties",
                    self.identity.super_properties_header(true),
                )
                .send()
                .await
                .ok()?;
            let body: Value = answer.json().await.ok()?;
            *cached = body["fingerprint"].as_str().map(str::to_owned);
        }
        cached.clone()
    }

    async fn post(
        &self,
        route: &str,
        body: &Value,
        captcha: Option<&CaptchaAnswer>,
        token: Option<&Token>,
    ) -> Result<(u16, Value), Failure> {
        let mut headers = HeaderMap::new();
        let mut put = |name: &'static str, value: &str| {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                headers.insert(name, value);
            }
        };
        if let Some(fingerprint) = self.fingerprint().await {
            put("x-fingerprint", &fingerprint);
        }
        if let Some(answer) = captcha {
            put("x-captcha-key", &answer.key);
            if let Some(rqtoken) = &answer.rqtoken {
                put("x-captcha-rqtoken", rqtoken);
            }
            if let Some(session) = &answer.session_id {
                put("x-captcha-session-id", session);
            }
        }
        if let Some(token) = token {
            put("authorization", token.expose());
        }
        let answer = self
            .http
            .post(format!("{}{route}", self.base))
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            )
            .headers(headers)
            .json(body)
            .send()
            .await
            .map_err(|_| Failure::Network)?;
        let status = answer.status().as_u16();
        let text = answer.text().await.map_err(|_| Failure::Network)?;
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::Null)
        };
        Ok((status, value))
    }

    /// `POST /auth/login`.
    pub async fn login(
        &self,
        request: &LoginRequest,
        captcha: Option<&CaptchaAnswer>,
    ) -> Result<LoginOutcome, Failure> {
        let body = json!({
            "login": request.login,
            "password": request.password.as_str(),
            "undelete": request.undelete,
            "login_source": null,
            "gift_code_sku_id": null,
        });
        let (status, answer) = self.post("/auth/login", &body, captcha, None).await?;
        if !(200..300).contains(&status) {
            return Err(classify(status, &answer));
        }
        if answer["mfa"].as_bool().unwrap_or(false) {
            return Ok(LoginOutcome::Mfa(challenge(&answer)));
        }
        token_of(&answer)
            .map(LoginOutcome::Token)
            .ok_or(Failure::Other {
                status,
                message: None,
            })
    }

    /// `POST /auth/mfa/{type}` with the ticket the login gave.
    pub async fn mfa(
        &self,
        kind: MfaKind,
        code: &str,
        challenge: &MfaChallenge,
        captcha: Option<&CaptchaAnswer>,
    ) -> Result<Token, Failure> {
        let body = json!({
            "code": code,
            "ticket": challenge.ticket,
            "login_source": null,
            "gift_code_sku_id": null,
            "login_instance_id": challenge.login_instance_id,
        });
        let route = format!("/auth/mfa/{}", kind.route());
        let (status, answer) = self.post(&route, &body, captcha, None).await?;
        if (200..300).contains(&status) {
            return token_of(&answer).ok_or(Failure::Other {
                status,
                message: None,
            });
        }
        // The dialog shows whatever sentence the server sent (`e.body?.message ?? e.message`).
        match classify(status, &answer) {
            Failure::Captcha(challenge) => Err(Failure::Captcha(challenge)),
            Failure::RateLimited { retry_after } => Err(Failure::RateLimited { retry_after }),
            Failure::Suspended => Err(Failure::Suspended),
            Failure::Network => Err(Failure::Network),
            _ => Err(Failure::MfaInvalid(
                answer["message"].as_str().unwrap_or_default().to_owned(),
            )),
        }
    }

    /// `POST /auth/mfa/sms/send`: the masked number the code went to.
    pub async fn send_sms(&self, challenge: &MfaChallenge) -> Result<String, Failure> {
        let body = json!({"ticket": challenge.ticket});
        let (status, answer) = self.post("/auth/mfa/sms/send", &body, None, None).await?;
        if (200..300).contains(&status) {
            return Ok(answer["phone"].as_str().unwrap_or_default().to_owned());
        }
        Err(classify(status, &answer))
    }

    /// `POST /auth/forgot`.
    pub async fn forgot_password(&self, login: &str) -> Result<ForgotOutcome, Failure> {
        let (status, answer) = self
            .post("/auth/forgot", &json!({"login": login}), None, None)
            .await?;
        if (200..300).contains(&status) {
            return Ok(if answer["method"].as_str() == Some("one_time_login") {
                ForgotOutcome::OneTimeLogin
            } else {
                ForgotOutcome::Sent
            });
        }
        Err(classify(status, &answer))
    }

    /// `POST /users/@me/remote-auth/login`: the encrypted token the scanned QR code was approved with.
    pub async fn remote_auth_login(&self, ticket: &str) -> Result<String, Failure> {
        let (status, answer) = self
            .post(
                "/users/@me/remote-auth/login",
                &json!({"ticket": ticket}),
                None,
                None,
            )
            .await?;
        if (200..300).contains(&status) {
            return answer["encrypted_token"]
                .as_str()
                .map(str::to_owned)
                .ok_or(Failure::Other {
                    status,
                    message: None,
                });
        }
        Err(classify(status, &answer))
    }

    /// `POST /auth/logout`, with the token being retired. Best effort: the local token is dropped either way.
    pub async fn logout(&self, token: &Token) -> Result<(), Failure> {
        let body =
            json!({"provider": null, "token": null, "voip_provider": null, "voip_token": null});
        let (status, answer) = self.post("/auth/logout", &body, None, Some(token)).await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(classify(status, &answer))
    }
}

fn challenge(answer: &Value) -> MfaChallenge {
    let mut methods = Vec::new();
    if let Some(webauthn) = answer["webauthn"].as_str() {
        methods.push(MfaMethod::WebAuthn(webauthn.to_owned()));
    }
    if answer["totp"].as_bool().unwrap_or(false) {
        methods.push(MfaMethod::Totp);
    }
    if answer["backup"].as_bool().unwrap_or(false) {
        methods.push(MfaMethod::Backup);
    }
    if answer["sms"].as_bool().unwrap_or(false) {
        methods.push(MfaMethod::Sms);
    }
    MfaChallenge {
        ticket: answer["ticket"].as_str().unwrap_or_default().to_owned(),
        methods,
        login_instance_id: answer["login_instance_id"].as_str().map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_keep_the_dialogs_order() {
        let answer = json!({"mfa": true, "ticket": "t", "sms": true, "backup": true, "totp": true,
            "webauthn": "{\"challenge\":1}", "login_instance_id": "i"});
        let found = challenge(&answer);
        assert_eq!(
            found
                .methods
                .iter()
                .map(MfaMethod::route)
                .collect::<Vec<_>>(),
            ["webauthn", "totp", "backup", "sms"]
        );
        assert_eq!(found.login_instance_id.as_deref(), Some("i"));
    }

    #[test]
    fn an_empty_token_is_no_token() {
        assert!(token_of(&json!({"token": ""})).is_none());
        assert!(token_of(&json!({"token": null})).is_none());
        assert!(token_of(&json!({"token": "a.b.c"})).is_some());
    }
}
