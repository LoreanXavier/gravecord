//! What Discord answers a failed auth request with, read the way the official client does (`Wl` in module 620632 and
//! `p` in module 15552): field errors by name, else a message. Nothing here keeps a password, a token or a ticket.

use serde_json::Value;
use std::collections::BTreeMap;

/// `ACCOUNT_SCHEDULED_FOR_DELETION`, `ACCOUNT_DISABLED`, `PHONE_VERIFICATION_REQUIRED`, `MFA_INVALID_CODE`
/// (`t02` in `CONSTANTS.md`).
pub const CODE_ACCOUNT_SCHEDULED_FOR_DELETION: u64 = 20011;
pub const CODE_ACCOUNT_DISABLED: u64 = 20013;
pub const CODE_MFA_INVALID_CODE: u64 = 60008;
pub const CODE_PHONE_VERIFICATION_REQUIRED: u64 = 70007;

/// The fields a form shows an error under, first message of each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormErrors {
    pub fields: BTreeMap<String, String>,
    pub message: Option<String>,
    pub code: Option<u64>,
    pub retry_after: Option<f64>,
}

impl FormErrors {
    pub fn parse(body: &Value) -> Self {
        let mut errors = Self {
            message: body["message"].as_str().map(str::to_owned),
            code: body["code"].as_u64(),
            retry_after: body["retry_after"].as_f64(),
            ..Self::default()
        };
        // Current shape: {"errors": {"login": {"_errors": [{"code", "message"}]}}}, nested objects included.
        if let Some(object) = body["errors"].as_object() {
            for (name, value) in object {
                if let Some(message) = first_message(value) {
                    errors.fields.insert(name.clone(), message);
                }
            }
        }
        // The old shape (`oldFormErrors`): {"email": ["Not a well formed email address."]}.
        if errors.fields.is_empty()
            && let Some(object) = body.as_object()
        {
            for (name, value) in object {
                if let Some(first) = value.as_array().and_then(|list| list.first())
                    && let Some(message) = first.as_str()
                {
                    errors.fields.insert(name.clone(), message.to_owned());
                }
            }
        }
        errors
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

fn first_message(value: &Value) -> Option<String> {
    if let Some(list) = value["_errors"].as_array()
        && let Some(message) = list.first().and_then(|entry| entry["message"].as_str())
    {
        return Some(message.to_owned());
    }
    value
        .as_object()?
        .values()
        .find_map(first_message)
        .or_else(|| value.as_array()?.first()?.as_str().map(str::to_owned))
}

/// The hCaptcha challenge of a 400 with `captcha_key` (`extractCaptchaPropsFromResponse`, module 295562).
#[derive(Clone, Debug, PartialEq)]
pub struct CaptchaChallenge {
    pub service: String,
    pub sitekey: String,
    pub session_id: Option<String>,
    pub rqdata: Option<String>,
    pub rqtoken: Option<String>,
    pub serve_invisible: bool,
}

impl CaptchaChallenge {
    pub fn parse(body: &Value) -> Option<Self> {
        body["captcha_key"].as_array()?;
        let text = |name: &str| body[name].as_str().map(str::to_owned);
        Some(Self {
            service: text("captcha_service").unwrap_or_default(),
            sitekey: text("captcha_sitekey").unwrap_or_default(),
            session_id: text("captcha_session_id"),
            rqdata: text("captcha_rqdata"),
            rqtoken: text("captcha_rqtoken"),
            serve_invisible: body["should_serve_invisible"].as_bool().unwrap_or(false),
        })
    }
}

/// A solved challenge, sent back as `X-Captcha-Key`, `X-Captcha-Rqtoken` and `X-Captcha-Session-Id`.
#[derive(Clone)]
pub struct CaptchaAnswer {
    pub key: String,
    pub rqtoken: Option<String>,
    pub session_id: Option<String>,
}

impl std::fmt::Debug for CaptchaAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaptchaAnswer(<redacted>)")
    }
}

/// Why an auth request did not give a token or a next step.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// Shown under the fields. `INVALID_LOGIN` arrives here.
    Fields(FormErrors),
    Captcha(CaptchaChallenge),
    AccountDisabled,
    ScheduledForDeletion,
    /// Code 70007: the account asks for a verification by phone from a new location.
    PhoneVerificationRequired,
    /// A suspended account: the token is only good for the standing page.
    Suspended,
    RateLimited {
        retry_after: f64,
    },
    /// The MFA code was wrong; the server's own sentence.
    MfaInvalid(String),
    /// Any other answer, with the server's message when it sent one.
    Other {
        status: u16,
        message: Option<String>,
    },
    /// No answer at all.
    Network,
}

/// Maps a login answer to its failure the way `login` in module 830215 does.
pub fn classify(status: u16, body: &Value) -> Failure {
    if status == 429 {
        return Failure::RateLimited {
            retry_after: body["retry_after"]
                .as_f64()
                .unwrap_or(1.0)
                .clamp(0.0, 3600.0),
        };
    }
    if body["suspended_user_token"].is_string() {
        return Failure::Suspended;
    }
    if status == 400
        && let Some(challenge) = CaptchaChallenge::parse(body)
    {
        return Failure::Captcha(challenge);
    }
    match body["code"].as_u64() {
        Some(CODE_ACCOUNT_SCHEDULED_FOR_DELETION) => return Failure::ScheduledForDeletion,
        Some(CODE_ACCOUNT_DISABLED) => return Failure::AccountDisabled,
        Some(CODE_PHONE_VERIFICATION_REQUIRED) => return Failure::PhoneVerificationRequired,
        _ => {}
    }
    let errors = FormErrors::parse(body);
    if !errors.is_empty() {
        return Failure::Fields(errors);
    }
    Failure::Other {
        status,
        message: errors.message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_login_shows_under_both_fields() {
        let body = json!({"code": 50035, "message": "Invalid Form Body", "errors": {
            "login": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]},
            "password": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]}}});
        let Failure::Fields(errors) = classify(400, &body) else {
            panic!("not field errors");
        };
        assert_eq!(errors.field("login"), Some("Login or password is invalid."));
        assert_eq!(
            errors.field("password"),
            Some("Login or password is invalid.")
        );
    }

    #[test]
    fn old_form_errors_are_read_too() {
        let errors = FormErrors::parse(&json!({"email": ["Not a well formed email address."]}));
        assert_eq!(
            errors.field("email"),
            Some("Not a well formed email address.")
        );
    }

    #[test]
    fn account_states_come_from_the_code() {
        assert_eq!(
            classify(400, &json!({"code": 20013})),
            Failure::AccountDisabled
        );
        assert_eq!(
            classify(400, &json!({"code": 20011})),
            Failure::ScheduledForDeletion
        );
        assert_eq!(
            classify(400, &json!({"code": 70007})),
            Failure::PhoneVerificationRequired
        );
    }

    #[test]
    fn captcha_needs_the_captcha_key_list() {
        let body = json!({"captcha_key": ["captcha-required"], "captcha_service": "hcaptcha",
            "captcha_sitekey": "k", "captcha_rqtoken": "r", "captcha_session_id": "s"});
        let Failure::Captcha(challenge) = classify(400, &body) else {
            panic!("not a captcha");
        };
        assert_eq!(challenge.service, "hcaptcha");
        assert_eq!(challenge.rqtoken.as_deref(), Some("r"));
        assert!(!challenge.serve_invisible);
        assert!(matches!(
            classify(400, &json!({"captcha_key": "x"})),
            Failure::Other { .. }
        ));
    }

    #[test]
    fn rate_limit_carries_the_wait() {
        assert_eq!(
            classify(
                429,
                &json!({"message": "You are being rate limited.", "retry_after": 2.5})
            ),
            Failure::RateLimited { retry_after: 2.5 }
        );
    }

    #[test]
    fn answers_do_not_print_a_captcha_solution() {
        let answer = CaptchaAnswer {
            key: "P1_solution".into(),
            rqtoken: None,
            session_id: None,
        };
        assert!(!format!("{answer:?}").contains("solution"));
    }
}
