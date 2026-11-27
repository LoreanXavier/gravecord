//! The auth REST calls, from `login`, `loginMFAv2`, `forgotPassword` and the remote auth screen of the pinned build
//! (modules 830215 and 785161; routes in module 652215: `LOGIN`, `LOGIN_MFA`, `LOGIN_SMS_SEND`, `FORGOT_PASSWORD`,
//! `REMOTE_AUTH_LOGIN`, `LOGOUT`). The headers are the pinned identity's.
//!
//! No call here carries an `authorization` header except `logout`, and no log line carries a body.

use crate::errors::{CaptchaAnswer, Failure, classify};
use dc_proto::identity::Identity;
use dc_proto::pin;
use dc_proto::secrets::Token;
use reqwest::Method;
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
#[derive(Clone, PartialEq)]
pub struct MfaChallenge {
    pub ticket: String,
    pub methods: Vec<MfaMethod>,
    pub login_instance_id: Option<String>,
}

/// The ticket is a bearer of the half-finished login: it stays out of `{:?}`.
impl std::fmt::Debug for MfaChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MfaChallenge")
            .field(
                "methods",
                &self
                    .methods
                    .iter()
                    .map(MfaMethod::route)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
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

/// The page the logged-out client shows (`/login`), the `Referer` of its requests.
const LOGIN_PAGE: &str = "/login";

pub struct AuthApi {
    http: wreq::Client,
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
        // The Discord client's handshake, header order and cookies (`dc_http::chromium`), with the header set every
        // request of the page carries.
        let http = dc_http::chromium::builder()
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

    /// The system language the requests carry, for the QR login's socket.
    pub(crate) fn locale(&self) -> &str {
        &self.identity.system.locale
    }

    /// A request of the logged-out page (module 596339's `prepareRequest`): `X-Super-Properties`, `X-Installation-ID`
    /// once known, the page's `Origin` (not on a `GET`) and the login page as its `Referer`.
    fn request(&self, method: Method, url: String) -> wreq::RequestBuilder {
        let mut request = self
            .http
            .request(method.clone(), url)
            .header(
                "x-super-properties",
                self.identity.super_properties_header(true),
            )
            .header("referer", format!("{}{LOGIN_PAGE}", pin::WEBAPP_ORIGIN));
        if let Some(installation) = dc_http::installation_id() {
            request = request.header("x-installation-id", installation);
        }
        if let Some(origin) = dc_http::origin(&method) {
            request = request.header("origin", origin);
        }
        request
    }

    /// The installation id of this install (`installation_id_v3`): the logged-out page asks `GET /experiments` for
    /// it with the fingerprint, then `GET /apex/experiments?surface=2` when that gave none
    /// (`fetchInstallationExperiments`, module 620233). Known ids are kept ([`dc_http::set_installation_id`]).
    pub async fn installation(&self) -> Option<String> {
        if let Some(known) = dc_http::installation_id() {
            return Some(known);
        }
        let _ = self.fingerprint().await;
        if dc_http::installation_id().is_none()
            && let Ok(answer) = self
                .request(
                    Method::GET,
                    format!("{}/apex/experiments?surface=2", self.base),
                )
                .send()
                .await
            && let Ok(body) = answer.json::<Value>().await
            && let Some(id) = body["installation"].as_str()
        {
            dc_http::set_installation_id(id);
        }
        dc_http::installation_id()
    }

    /// `X-Fingerprint` comes from `GET /experiments`, as the logged-out client gets it (`ee` in module 46653). A failure
    /// only means the header is left out.
    async fn fingerprint(&self) -> Option<String> {
        let mut cached = self.fingerprint.lock().await;
        if cached.is_none() {
            let answer = self
                .request(
                    Method::GET,
                    format!("{}/experiments?with_guild_experiments=true", self.base),
                )
                .send()
                .await
                .ok()?;
            let body: Value = answer.json().await.ok()?;
            if let Some(id) = body["installation"].as_str() {
                dc_http::set_installation_id(id);
            }
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
            .request(Method::POST, format!("{}{route}", self.base))
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
        let (status, answer) = match self.post(&route, &body, captcha, None).await {
            Ok(answer) => answer,
            // The dialog of the pinned client shows the request error's own text under the field.
            Err(Failure::Network) => return Err(Failure::MfaInvalid(NETWORK_MESSAGE.to_owned())),
            Err(other) => return Err(other),
        };
        if (200..300).contains(&status) {
            return token_of(&answer).ok_or(Failure::Other {
                status,
                message: None,
            });
        }
        // A captcha demand is the HTTP layer's (400 only) and a suspended token the action's; everything else is
        // the sentence the dialog shows under the code field.
        if status != 429 {
            match classify(status, &answer) {
                Failure::Captcha(challenge) => return Err(Failure::Captcha(challenge)),
                Failure::Suspended => return Err(Failure::Suspended),
                _ => {}
            }
        }
        Err(Failure::MfaInvalid(mfa_message(
            kind, &route, status, &answer,
        )))
    }

    /// `POST /auth/mfa/sms/send`: the masked number the code went to.
    pub async fn send_sms(&self, challenge: &MfaChallenge) -> Result<String, Failure> {
        let body = json!({"ticket": challenge.ticket});
        let (status, answer) = match self.post("/auth/mfa/sms/send", &body, None, None).await {
            Ok(answer) => answer,
            Err(Failure::Network) => {
                return Err(Failure::Other {
                    status: 0,
                    message: Some(NETWORK_MESSAGE.to_owned()),
                });
            }
            Err(other) => return Err(other),
        };
        if (200..300).contains(&status) {
            return Ok(answer["phone"].as_str().unwrap_or_default().to_owned());
        }
        // `rejectWithError: false`: the rejection is the raw answer, so the dialog shows `body.message` (nothing when
        // the answer has none), a rate limit's included.
        if status != 429
            && let Failure::Captcha(challenge) = classify(status, &answer)
        {
            return Err(Failure::Captcha(challenge));
        }
        Err(Failure::Other {
            status,
            message: answer["message"].as_str().map(str::to_owned),
        })
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

/// The text of a request that never got an answer: superagent's `crossDomainError` (module 333405), which the
/// pinned client's dialogs show as `e.message`. The line break is collapsed the way a web page's text collapses it.
pub const NETWORK_MESSAGE: &str = "Request has been terminated Possible causes: the network is offline, Origin is not allowed by Access-Control-Allow-Origin, the page is being unloaded, etc.";

/// What the Multi-Factor dialog shows under the code field after a refused `/auth/mfa/{type}`. `loginMFAv2` (module
/// 830215) rethrows code 60008 as `Error(body.message)` and every other refusal as the `HTTPResponseError` (module
/// 620632), whose `message` is `METHOD url [status]`. The authenticator and backup slides (module 397404) show
/// `e.body?.message ?? e.message`; the SMS slide shows `e.message ?? e.body?.message`, which is always the former.
pub fn mfa_message(kind: MfaKind, route: &str, status: u16, body: &Value) -> String {
    let message = body["message"].as_str();
    if body["code"].as_u64() == Some(crate::errors::CODE_MFA_INVALID_CODE) {
        return message.unwrap_or_default().to_owned();
    }
    // The routes hold no digits, so the `\d+` -> `xxx` rewrite of the message changes nothing here.
    let request_error = format!("POST {route} [{status}]");
    match (kind, message) {
        (MfaKind::Sms, _) | (_, None) => request_error,
        (_, Some(message)) => message.to_owned(),
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
    fn a_wrong_code_shows_the_servers_sentence() {
        let body = json!({"code": 60008, "message": "Invalid two-factor code"});
        for kind in [MfaKind::Totp, MfaKind::Backup, MfaKind::Sms] {
            assert_eq!(
                mfa_message(kind, "/auth/mfa/totp", 400, &body),
                "Invalid two-factor code"
            );
        }
    }

    #[test]
    fn a_rate_limited_code_shows_the_message_except_for_sms() {
        let body = json!({"message": "You are being rate limited.", "retry_after": 2.5});
        assert_eq!(
            mfa_message(MfaKind::Totp, "/auth/mfa/totp", 429, &body),
            "You are being rate limited."
        );
        assert_eq!(
            mfa_message(MfaKind::Backup, "/auth/mfa/backup", 429, &body),
            "You are being rate limited."
        );
        // The SMS slide reads `e.message` first, and the request error's message is never empty.
        assert_eq!(
            mfa_message(MfaKind::Sms, "/auth/mfa/sms", 429, &body),
            "POST /auth/mfa/sms [429]"
        );
    }

    #[test]
    fn an_answer_without_a_message_shows_the_request_error() {
        assert_eq!(
            mfa_message(MfaKind::Totp, "/auth/mfa/totp", 502, &Value::Null),
            "POST /auth/mfa/totp [502]"
        );
        assert_eq!(
            mfa_message(MfaKind::Backup, "/auth/mfa/backup", 500, &json!({"x": 1})),
            "POST /auth/mfa/backup [500]"
        );
    }

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
