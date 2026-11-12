//! The login controller: what the screen asks for as `Command`s, what it shows as `Event`s. Every step of the pinned
//! client's flow lives here, not in the view: password login, the MFA ticket, the captcha resend, a disabled or
//! scheduled-for-deletion account, forgot password, and the QR session.
//!
//! The controller keeps what a later step needs and nothing else: the typed credentials while a captcha or a
//! "Restore Account" may resend them, and the MFA ticket. Both are dropped on `Reset`, on success and on drop; the
//! password is wiped with them.

use crate::api::{AuthApi, ForgotOutcome, LoginOutcome, LoginRequest, MfaChallenge, MfaKind};
use crate::errors::{CaptchaAnswer, CaptchaChallenge, Failure, FormErrors};
use crate::remote::{self, RemoteAuth, RemoteEvent};
use dc_proto::identity::Identity;
use dc_proto::secrets::Token;
use std::sync::Arc;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

pub enum Command {
    Login {
        login: String,
        password: Zeroizing<String>,
        undelete: bool,
    },
    /// The challenge was solved: resend what it interrupted.
    CaptchaSolved(CaptchaAnswer),
    CaptchaDismissed,
    Mfa {
        kind: MfaKind,
        code: String,
    },
    SendSms,
    Forgot {
        login: String,
    },
    /// "Restore Account": the login again, with `undelete`.
    RestoreAccount,
    StartQr,
    RestartQr,
    StopQr,
    /// "Return to Login", closing the MFA dialog: forgets the ticket and the typed credentials.
    Reset,
}

/// Names the command and nothing it carries: a typed login, password or code never reaches a log or a panic message
/// through `{:?}` (`Zeroizing<String>` would print its text).
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Login { .. } => "Command::Login(<redacted>)",
            Self::CaptchaSolved(_) => "Command::CaptchaSolved(<redacted>)",
            Self::CaptchaDismissed => "Command::CaptchaDismissed",
            Self::Mfa { .. } => "Command::Mfa(<redacted>)",
            Self::SendSms => "Command::SendSms",
            Self::Forgot { .. } => "Command::Forgot(<redacted>)",
            Self::RestoreAccount => "Command::RestoreAccount",
            Self::StartQr => "Command::StartQr",
            Self::RestartQr => "Command::RestartQr",
            Self::StopQr => "Command::StopQr",
            Self::Reset => "Command::Reset",
        })
    }
}

#[derive(Debug)]
pub enum Event {
    LoggingIn,
    /// Signed in, by password, MFA or QR.
    Success(Token),
    /// The form's own errors, under the fields they name.
    Fields(FormErrors),
    Mfa(MfaChallenge),
    MfaFailed(String),
    SmsSent {
        phone: String,
    },
    SmsFailed(String),
    AccountDisabled,
    ScheduledForDeletion,
    PhoneVerificationRequired,
    Captcha(CaptchaChallenge),
    Suspended,
    /// A rate limit or an answer with no field to show it under: the official form shows nothing, the button
    /// just stops loading and the typed login and password stay (see `login_event`).
    Quiet(Failure),
    ForgotSent {
        login: String,
    },
    ForgotOneTimeLogin,
    ForgotFields(FormErrors),
    Qr(RemoteEvent),
}

/// What the login form is told about a refused `POST /auth/login`. The pinned page (module 275941, `handleLogin`)
/// stores `p(error)` (module 15552) and its default form reads only `login`, `email` and `password` from it, so a
/// rate limit (`{message, retry_after}`), a dead network, a 5xx and an unknown refusal show nothing and leave the typed
/// login and password as they are; only the form's own field errors appear, under their fields.
pub fn login_event(failure: Failure) -> Event {
    match failure {
        Failure::Fields(errors) => Event::Fields(errors),
        Failure::Captcha(challenge) => Event::Captcha(challenge),
        Failure::AccountDisabled => Event::AccountDisabled,
        Failure::ScheduledForDeletion => Event::ScheduledForDeletion,
        Failure::PhoneVerificationRequired => Event::PhoneVerificationRequired,
        Failure::Suspended => Event::Suspended,
        other => Event::Quiet(other),
    }
}

pub struct Config {
    pub api_base: String,
    pub remote: remote::Config,
    pub identity: Identity,
}

impl Config {
    pub fn pinned() -> Self {
        Self {
            api_base: dc_proto::pin::API_BASE.to_owned(),
            remote: remote::Config::default(),
            identity: Identity::new(),
        }
    }
}

/// Starts the controller on the current tokio runtime.
pub fn spawn(
    config: Config,
) -> Result<
    (
        mpsc::UnboundedSender<Command>,
        mpsc::UnboundedReceiver<Event>,
    ),
    dc_http::Error,
> {
    let api = Arc::new(AuthApi::new(config.api_base, config.identity)?);
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let controller = Controller {
        api,
        remote_config: config.remote,
        events: event_tx,
        credentials: None,
        challenge: None,
        interrupted_mfa: None,
        remote: None,
    };
    tokio::spawn(controller.run(command_rx));
    Ok((command_tx, event_rx))
}

struct Controller {
    api: Arc<AuthApi>,
    remote_config: remote::Config,
    events: mpsc::UnboundedSender<Event>,
    /// Kept for the captcha resend and "Restore Account".
    credentials: Option<LoginRequest>,
    challenge: Option<MfaChallenge>,
    /// The second factor step a captcha interrupted.
    interrupted_mfa: Option<(MfaKind, String)>,
    remote: Option<RemoteAuth>,
}

impl Controller {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        while let Some(command) = commands.recv().await {
            self.handle(command).await;
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Login {
                login,
                password,
                undelete,
            } => {
                self.credentials = Some(LoginRequest {
                    login,
                    password,
                    undelete,
                });
                self.login(None).await;
            }
            Command::CaptchaSolved(answer) => {
                // Resends whichever request the challenge interrupted, as the official HTTP layer does.
                match self.interrupted_mfa.take() {
                    Some((kind, code)) => self.mfa(kind, &code, Some(&answer)).await,
                    None => self.login(Some(answer)).await,
                }
            }
            Command::CaptchaDismissed => self.interrupted_mfa = None,
            Command::Mfa { kind, code } => self.mfa(kind, &code, None).await,
            Command::SendSms => self.sms().await,
            Command::Forgot { login } => self.forgot(login).await,
            Command::RestoreAccount => {
                if let Some(credentials) = &mut self.credentials {
                    credentials.undelete = true;
                    self.login(None).await;
                }
            }
            Command::StartQr => self.start_qr(),
            Command::RestartQr => match &self.remote {
                Some(remote) => remote.restart(),
                None => self.start_qr(),
            },
            Command::StopQr => self.remote = None,
            Command::Reset => {
                self.credentials = None;
                self.challenge = None;
                self.interrupted_mfa = None;
            }
        }
    }

    fn start_qr(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = remote::Config {
            url: self.remote_config.url.clone(),
            origin: self.remote_config.origin.clone(),
        };
        self.remote = Some(RemoteAuth::spawn(self.api.clone(), config, tx));
        let events = self.events.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let out = match event {
                    RemoteEvent::Token(token) => Event::Success(token),
                    other => Event::Qr(other),
                };
                if events.send(out).is_err() {
                    break;
                }
            }
        });
    }

    async fn login(&mut self, captcha: Option<CaptchaAnswer>) {
        let Some(credentials) = &self.credentials else {
            return;
        };
        self.emit(Event::LoggingIn);
        match self.api.login(credentials, captcha.as_ref()).await {
            Ok(LoginOutcome::Token(token)) => self.succeed(token),
            Ok(LoginOutcome::Mfa(challenge)) => {
                // The typed password is not needed past this point.
                self.credentials = None;
                self.challenge = Some(challenge.clone());
                self.emit(Event::Mfa(challenge));
            }
            Err(failure) => self.fail_login(failure),
        }
    }

    fn fail_login(&mut self, failure: Failure) {
        // Only a captcha, a disabled or scheduled account and a phone verification can resend the typed credentials;
        // after any other answer the password is dropped (and wiped) at once.
        if !matches!(
            failure,
            Failure::Captcha(_)
                | Failure::AccountDisabled
                | Failure::ScheduledForDeletion
                | Failure::PhoneVerificationRequired
        ) {
            self.credentials = None;
        }
        self.emit(login_event(failure));
    }

    fn succeed(&mut self, token: Token) {
        self.credentials = None;
        self.challenge = None;
        self.interrupted_mfa = None;
        self.remote = None;
        self.emit(Event::Success(token));
    }

    async fn mfa(&mut self, kind: MfaKind, code: &str, captcha: Option<&CaptchaAnswer>) {
        let Some(challenge) = self.challenge.clone() else {
            return;
        };
        match self.api.mfa(kind, code, &challenge, captcha).await {
            Ok(token) => self.succeed(token),
            Err(Failure::MfaInvalid(message)) => self.emit(Event::MfaFailed(message)),
            Err(Failure::Captcha(challenge)) => {
                self.interrupted_mfa = Some((kind, code.to_owned()));
                self.emit(Event::Captcha(challenge));
            }
            Err(Failure::Suspended) => self.emit(Event::Suspended),
            Err(other) => self.emit(Event::Quiet(other)),
        }
    }

    async fn sms(&mut self) {
        let Some(challenge) = self.challenge.clone() else {
            return;
        };
        match self.api.send_sms(&challenge).await {
            Ok(phone) => self.emit(Event::SmsSent { phone }),
            Err(Failure::Fields(errors)) => {
                self.emit(Event::SmsFailed(errors.message.unwrap_or_default()))
            }
            Err(Failure::Other { message, .. }) => {
                self.emit(Event::SmsFailed(message.unwrap_or_default()))
            }
            Err(other) => self.emit(Event::Quiet(other)),
        }
    }

    async fn forgot(&mut self, login: String) {
        match self.api.forgot_password(&login).await {
            Ok(ForgotOutcome::OneTimeLogin) => self.emit(Event::ForgotOneTimeLogin),
            Ok(ForgotOutcome::Sent) => self.emit(Event::ForgotSent { login }),
            Err(Failure::Fields(errors)) => self.emit(Event::ForgotFields(errors)),
            Err(other) => self.emit(Event::Quiet(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::classify;
    use serde_json::{Value, json};

    fn event(status: u16, body: Value) -> Event {
        login_event(classify(status, &body))
    }

    #[test]
    fn commands_and_challenges_print_no_secret() {
        let login = Command::Login {
            login: "who@example.invalid".into(),
            password: Zeroizing::new("hunter2-value".into()),
            undelete: false,
        };
        let mfa = Command::Mfa {
            kind: MfaKind::Totp,
            code: "135790".into(),
        };
        let challenge = MfaChallenge {
            ticket: "ticket-value".into(),
            methods: vec![],
            login_instance_id: None,
        };
        let text = format!(
            "{login:?} {mfa:?} {challenge:?} {:?}",
            Event::Mfa(challenge.clone())
        );
        for secret in ["who@example", "hunter2", "135790", "ticket-value"] {
            assert!(!text.contains(secret), "{secret} reached a Debug print");
        }
    }

    #[test]
    fn a_rate_limit_shows_nothing() {
        let body =
            json!({"message": "You are being rate limited.", "retry_after": 2.5, "global": false});
        assert!(matches!(
            event(429, body),
            Event::Quiet(Failure::RateLimited { retry_after }) if retry_after == 2.5
        ));
    }

    #[test]
    fn a_server_error_or_a_page_instead_of_json_shows_nothing() {
        assert!(matches!(
            event(500, Value::Null),
            Event::Quiet(Failure::Other { status: 500, .. })
        ));
        assert!(matches!(
            event(502, json!({"message": "Bad Gateway", "code": 0})),
            Event::Quiet(Failure::Other { status: 502, .. })
        ));
        assert!(matches!(
            login_event(Failure::Network),
            Event::Quiet(Failure::Network)
        ));
    }

    #[test]
    fn an_invalid_form_body_shows_under_its_fields() {
        let body = json!({"code": 50035, "message": "Invalid Form Body", "errors": {
            "login": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]},
            "password": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]}}});
        let Event::Fields(errors) = event(400, body) else {
            panic!("not field errors");
        };
        assert_eq!(errors.field("login"), Some("Login or password is invalid."));
        assert_eq!(
            errors.field("password"),
            Some("Login or password is invalid.")
        );
    }

    #[test]
    fn a_captcha_answer_that_is_not_a_demand_is_not_a_challenge() {
        // A failed solution comes back as a 400 whose `captcha_key` lists "invalid-response" and the new challenge
        // fields; the HTTP layer of the pinned client (module 596339) asks again for any 400 with `captcha_key`.
        let body = json!({"captcha_key": ["invalid-response"], "captcha_service": "hcaptcha", "captcha_sitekey": "k"});
        assert!(matches!(event(400, body), Event::Captcha(_)));
        // Without the list it is only an unknown refusal, and the form shows nothing.
        assert!(matches!(
            event(400, json!({"captcha_key": "invalid-response"})),
            Event::Quiet(Failure::Other { .. })
        ));
    }
}
