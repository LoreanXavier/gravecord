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

#[derive(Debug)]
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
    /// just stops loading. The sentence is for logs and tests.
    Quiet(Failure),
    ForgotSent {
        login: String,
    },
    ForgotOneTimeLogin,
    ForgotFields(FormErrors),
    Qr(RemoteEvent),
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
        match failure {
            Failure::Fields(errors) => self.emit(Event::Fields(errors)),
            Failure::Captcha(challenge) => self.emit(Event::Captcha(challenge)),
            Failure::AccountDisabled => self.emit(Event::AccountDisabled),
            Failure::ScheduledForDeletion => self.emit(Event::ScheduledForDeletion),
            Failure::PhoneVerificationRequired => self.emit(Event::PhoneVerificationRequired),
            Failure::Suspended => {
                self.credentials = None;
                self.emit(Event::Suspended);
            }
            other => {
                self.credentials = None;
                self.emit(Event::Quiet(other));
            }
        }
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
