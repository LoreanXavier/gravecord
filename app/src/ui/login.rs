//! The login screen: the page the pinned client shows with no token. Measured in `design/dom/login/*.json` and
//! compared with `design/golden/windows/login/*@1.png`. Everything that talks to Discord is `dc-auth`; this file only
//! draws the states it reports and turns clicks and typing into its commands.
//!
//! Left out here, on purpose: the passkey sign in with no password (`BLOCKED.md`). The WebAuthn second factor runs the
//! system's prompt through `dc-webauthn`. The captcha is hCaptcha's own widget in a `dc-webhost` surface over the
//! dialog's slot.

use super::{color, editor_style, icon, window_controls};
use crate::editor::{Editor, EditorEvent};
use crate::login_art::ArtBytes;
use crate::ui::motion::Posed as _;
use dc_auth::errors::classify;
use dc_auth::flow::{Command, Event, login_event};
use dc_auth::qr::{self, Matrix};
use dc_auth::{
    CaptchaAnswer, CaptchaChallenge, Failure, FormErrors, MfaChallenge, MfaKind, MfaMethod,
    RemoteEvent,
};
use dc_proto::secrets::Token;
use dc_webhost::{CaptchaEvent, HostEvent, WebHost, captcha_event};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable as _, FontWeight, HighlightStyle, Image, ImageFormat, IntoElement, KeyDownEvent,
    ObjectFit, Pixels, Render, SharedString, StyledText, Window, WindowControlArea, canvas,
    deferred, div, fill, img, prelude::*, px, rgba, size,
};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use zeroize::Zeroizing;

/// What the screen reports to the shell.
pub enum LoginEvent {
    Success(Token),
}

impl EventEmitter<LoginEvent> for LoginView {}

/// The images of the page, decoded.
#[derive(Default)]
pub struct Art {
    artwork: Option<Arc<Image>>,
    logo: Option<Arc<Image>>,
    qr_logo: Option<Arc<Image>>,
    default_avatar: Option<Arc<Image>>,
    mascot: Option<Arc<Image>>,
}

impl Art {
    pub fn from_bytes(bytes: ArtBytes) -> Self {
        let svg = |bytes: Option<Vec<u8>>| {
            bytes.map(|b| Arc::new(Image::from_bytes(ImageFormat::Svg, b)))
        };
        let png = |bytes: Option<Vec<u8>>| {
            bytes.map(|b| Arc::new(Image::from_bytes(ImageFormat::Png, b)))
        };
        Self {
            artwork: svg(bytes.artwork),
            logo: svg(bytes.logo),
            qr_logo: png(bytes.qr_logo),
            default_avatar: png(bytes.default_avatar),
            mascot: svg(bytes.mascot),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum QrState {
    /// Connecting, or between two sessions.
    Initializing,
    Code(Matrix),
    /// A phone scanned the code: the name it shows, and the avatar once fetched.
    Scanned {
        name: String,
        user: String,
        avatar_hash: Option<String>,
    },
    Finishing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    /// The security key slide: one button that starts the platform's WebAuthn dialog (`Z` in module 397404).
    WebAuthn,
    Totp,
    /// "Slow down! Are you sure?" before the backup code field.
    BackupWarning,
    Backup,
    Sms,
}

struct Mfa {
    challenge: MfaChallenge,
    step: Step,
    error: Option<String>,
    busy: bool,
    phone: Option<String>,
    /// Seconds left on the backup code warning.
    countdown: u32,
}

enum Dialog {
    None,
    Mfa(Mfa),
    ForgotSent(String),
    Captcha(CaptchaChallenge),
    Disabled,
    Deletion,
    /// Code 70007: the account wants a phone verification this client cannot do natively.
    PhoneVerification,
    Suspended,
}

pub struct LoginView {
    commands: UnboundedSender<Command>,
    email: Entity<Editor>,
    password: Entity<Editor>,
    code: Entity<Editor>,
    show_password: bool,
    login_error: Option<String>,
    password_error: Option<String>,
    busy: bool,
    qr: QrState,
    dialog: Dialog,
    art: Art,
    avatar: Option<Arc<Image>>,
    /// Bytes of the scanned user's avatar, fetched off the UI thread.
    avatar_requests: Option<UnboundedSender<(String, String)>>,
    /// The message the shell shows after a rejected token (the official "invalid token" wording is none: the page just
    /// shows the form), kept for tests.
    focus: FocusHandle,
    /// A focus change asked for outside a render, done on the next one.
    want_focus: Option<Wanted>,
    /// A `--login <state>` capture: the backup warning's countdown holds still.
    frozen: bool,
    /// The captcha widget's surface, up while the dialog is, and the bounds it was last placed at.
    captcha_host: Option<WebHost>,
    captcha_bounds: (f32, f32, f32, f32),
    captcha_failed: bool,
    captcha_poll: Option<gpui::Task<()>>,
    /// The Multi-Factor dialog a captcha interrupted, put back when the captcha is done.
    captcha_behind: Option<Mfa>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Email,
    Password,
    Code,
}

/// Requests for the avatar of the account that scanned the code, and the bytes that come back.
pub type AvatarChannels = (
    UnboundedSender<(String, String)>,
    UnboundedReceiver<Vec<u8>>,
);

const REQUIRED: &str = "This field is required";
/// `xSCvBf`: what the security key slide shows when the key step fails (module 397404 `Z`).
const WEBAUTHN_FAILED: &str = "An error occurred. Please try again.";

impl LoginView {
    pub fn new(
        commands: UnboundedSender<Command>,
        events: UnboundedReceiver<Event>,
        avatar_requests: Option<AvatarChannels>,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = |cx: &mut Context<Self>, masked: bool| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_keeps_text(true);
                editor.set_masked(masked);
                editor
            });
            cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| match event {
                EditorEvent::Submit(_) => this.submit_focused(cx),
                EditorEvent::Edited => cx.notify(),
                EditorEvent::Cancel => {}
            })
            .detach();
            editor
        };
        let email = field(cx, false);
        let password = field(cx, true);
        let code = field(cx, false);
        let (avatar_tx, avatar_rx) = match avatar_requests {
            Some((tx, rx)) => (Some(tx), Some(rx)),
            None => (None, None),
        };
        let this = Self {
            commands,
            email,
            password,
            code,
            show_password: false,
            login_error: None,
            password_error: None,
            busy: false,
            qr: QrState::Initializing,
            dialog: Dialog::None,
            art: Art::default(),
            avatar: None,
            avatar_requests: avatar_tx,
            focus: cx.focus_handle(),
            want_focus: Some(Wanted::Email),
            frozen: false,
            captcha_host: None,
            captcha_bounds: (0., 0., 0., 0.),
            captcha_failed: false,
            captcha_poll: None,
            captcha_behind: None,
        };
        this.send(Command::StartQr);
        cx.spawn(async move |this, cx| {
            let mut events = events;
            while let Some(event) = events.recv().await {
                if this.update(cx, |this, cx| this.apply(event, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
        if let Some(mut avatars) = avatar_rx {
            cx.spawn(async move |this, cx| {
                while let Some(bytes) = avatars.recv().await {
                    let image = Arc::new(Image::from_bytes(ImageFormat::Png, bytes));
                    if this
                        .update(cx, |this, cx| {
                            this.avatar = Some(image);
                            cx.notify();
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .detach();
        }
        this
    }

    pub fn set_art(&mut self, art: Art, cx: &mut Context<Self>) {
        self.art = art;
        cx.notify();
    }

    /// Where the keyboard starts: the email field.
    pub fn focus_first(&self, window: &mut Window, cx: &mut gpui::App) {
        window.focus(&self.email.focus_handle(cx), cx);
    }

    pub fn has_art(&self) -> bool {
        self.art.artwork.is_some()
    }

    /// A capture of one state with no network (`--login <state>`): the same events the controller would send, and
    /// typed test values.
    pub fn preview(&mut self, state: &str, cx: &mut Context<Self>) {
        self.frozen = true;
        let fill = |this: &mut Self, cx: &mut Context<Self>| {
            this.email
                .update(cx, |editor, _| editor.set_text("tester@example.invalid"));
            this.password
                .update(cx, |editor, _| editor.set_text("test-value-only"));
        };
        let challenge = |methods: Vec<MfaMethod>| MfaChallenge {
            ticket: "test-ticket".to_owned(),
            methods,
            login_instance_id: None,
        };
        let all = vec![MfaMethod::Totp, MfaMethod::Backup, MfaMethod::Sms];
        let invalid = || {
            let mut errors = FormErrors::default();
            errors.fields.insert(
                "login".to_owned(),
                "Login or password is invalid.".to_owned(),
            );
            errors.fields.insert(
                "password".to_owned(),
                "Login or password is invalid.".to_owned(),
            );
            errors
        };
        let sample = "aZ3-xQ_kLm9pR2sT7vW1yB4cD6eF8gH0jN5uOaZ3xQk".to_owned();
        match state {
            "qr-loading" => self.apply_qr(RemoteEvent::Initializing),
            "qr-ticket" => {
                self.apply_qr(RemoteEvent::Scanned(dc_auth::RemoteUser {
                    id: "123456789012345678".to_owned(),
                    discriminator: "0".to_owned(),
                    avatar: None,
                    username: "TestUser".to_owned(),
                }));
            }
            _ => self.apply_qr(RemoteEvent::Code {
                fingerprint: sample,
            }),
        }
        match state {
            "filled-error" => {
                fill(self, cx);
                self.apply(Event::Fields(invalid()), cx);
            }
            "logging-in" => {
                fill(self, cx);
                self.apply(Event::LoggingIn, cx);
            }
            // The answer of the login route, run through the real mapping (`classify`, `login_event`), as a person
            // sees it after the button stops loading: nothing under the fields for a rate limit, a dead network and a
            // 5xx, the typed login and password still in the boxes.
            "rate-limited" | "answer-429" => {
                fill(self, cx);
                self.preview_login_answer(
                    429,
                    serde_json::json!({"message": "You are being rate limited.", "retry_after": 2.5, "global": false}),
                    cx,
                );
            }
            "answer-network" => {
                fill(self, cx);
                self.apply(Event::LoggingIn, cx);
                self.apply(login_event(Failure::Network), cx);
            }
            "answer-500" => {
                fill(self, cx);
                self.preview_login_answer(500, serde_json::Value::Null, cx);
            }
            "answer-invalid-form" => {
                fill(self, cx);
                self.preview_login_answer(
                    400,
                    serde_json::json!({"code": 50035, "message": "Invalid Form Body", "errors": {
                        "login": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]},
                        "password": {"_errors": [{"code": "INVALID_LOGIN", "message": "Login or password is invalid."}]}}}),
                    cx,
                );
            }
            // A form error under the login only, as `BASE_TYPE_REQUIRED` answers for an empty field.
            "answer-login-field" => {
                fill(self, cx);
                self.preview_login_answer(
                    400,
                    serde_json::json!({"code": 50035, "message": "Invalid Form Body", "errors": {
                        "login": {"_errors": [{"code": "EMAIL_TYPE_INVALID_EMAIL", "message": "Not a well formed email address."}]}}}),
                    cx,
                );
            }
            // A failed captcha solution is a 400 with `captcha_key` again: the dialog asks once more.
            "answer-captcha-invalid" => {
                fill(self, cx);
                self.preview_login_answer(
                    400,
                    serde_json::json!({"captcha_key": ["invalid-response"], "captcha_service": "hcaptcha", "captcha_sitekey": "test"}),
                    cx,
                );
            }
            "answer-mfa-429" | "answer-mfa-500" | "answer-mfa-network" | "answer-mfa-sms-429" => {
                self.apply(Event::Mfa(challenge(all.clone())), cx);
                let (kind, route) = if state == "answer-mfa-sms-429" {
                    self.goto_step(Step::Sms, cx);
                    (MfaKind::Sms, "/auth/mfa/sms")
                } else {
                    (MfaKind::Totp, "/auth/mfa/totp")
                };
                let message = match state {
                    "answer-mfa-network" => dc_auth::api::NETWORK_MESSAGE.to_owned(),
                    "answer-mfa-500" => {
                        dc_auth::api::mfa_message(kind, route, 500, &serde_json::Value::Null)
                    }
                    _ => dc_auth::api::mfa_message(
                        kind,
                        route,
                        429,
                        &serde_json::json!({"message": "You are being rate limited.", "retry_after": 2.5}),
                    ),
                };
                self.code.update(cx, |editor, _| editor.set_text("123456"));
                self.want_focus = None;
                self.apply(Event::MfaFailed(message), cx);
            }
            "mfa-webauthn"
            | "mfa-webauthn-failed"
            | "mfa-webauthn-waiting"
            | "mfa-webauthn-only"
            | "mfa-select-webauthn" => {
                let key = MfaMethod::WebAuthn("{}".to_owned());
                let methods = if state == "mfa-webauthn-only" {
                    vec![key]
                } else {
                    vec![key, MfaMethod::Totp, MfaMethod::Backup, MfaMethod::Sms]
                };
                self.apply(Event::Mfa(challenge(methods)), cx);
                match state {
                    // The state after the prompt closed without a credential (the stub answers on a timer, which a
                    // frame taken at once would miss).
                    "mfa-webauthn-failed" => {
                        self.key_answered(Err(dc_webauthn::Error::Cancelled), cx)
                    }
                    // The prompt is open: the button waits.
                    "mfa-webauthn-waiting" => self.authenticate_with_key(0, cx),
                    "mfa-select-webauthn" => self.goto_step(Step::Select, cx),
                    _ => {}
                }
            }
            "password-shown" => {
                fill(self, cx);
                self.show_password = true;
            }
            "forgot-empty" => self.login_error = Some(REQUIRED.to_owned()),
            "forgot-sent" => {
                self.email
                    .update(cx, |editor, _| editor.set_text("tester@example.invalid"));
                self.apply(
                    Event::ForgotSent {
                        login: "tester@example.invalid".to_owned(),
                    },
                    cx,
                );
            }
            "captcha" => {
                fill(self, cx);
                self.apply(
                    Event::Captcha(CaptchaChallenge {
                        service: "hcaptcha".to_owned(),
                        sitekey: "test".to_owned(),
                        session_id: None,
                        rqdata: None,
                        rqtoken: None,
                        serve_invisible: false,
                    }),
                    cx,
                );
            }
            "account-disabled" => self.apply(Event::AccountDisabled, cx),
            "account-deletion" => self.apply(Event::ScheduledForDeletion, cx),
            "mfa-select" => {
                self.apply(Event::Mfa(challenge(all)), cx);
                self.goto_step(Step::Select, cx);
            }
            "mfa-totp" => self.apply(Event::Mfa(challenge(all)), cx),
            "mfa-totp-error" => {
                self.apply(Event::Mfa(challenge(all)), cx);
                self.apply(Event::MfaFailed("Invalid two-factor code".to_owned()), cx);
                // The golden was taken after typing a code and clicking Confirm: filled, and focus left the field.
                self.code.update(cx, |editor, _| editor.set_text("123456"));
                self.want_focus = None;
            }
            "mfa-sms" => {
                self.apply(Event::Mfa(challenge(all)), cx);
                self.goto_step(Step::Sms, cx);
                self.apply(
                    Event::SmsSent {
                        phone: "+** *** *** ** 34".to_owned(),
                    },
                    cx,
                );
            }
            "mfa-backup" => {
                self.apply(Event::Mfa(challenge(all)), cx);
                self.goto_step(Step::BackupWarning, cx);
                if let Dialog::Mfa(mfa) = &mut self.dialog {
                    mfa.countdown = 9;
                }
            }
            "mfa-backup-code" => {
                self.apply(Event::Mfa(challenge(all)), cx);
                self.goto_step(Step::Backup, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// A capture's answer of the login route, mapped the way the controller maps a real one. Nothing is sent.
    fn preview_login_answer(
        &mut self,
        status: u16,
        body: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.apply(Event::LoggingIn, cx);
        self.apply(login_event(classify(status, &body)), cx);
        // The golden of a rate limit was taken after typing the password and pressing Enter there.
        if self.want_focus == Some(Wanted::Email) {
            self.want_focus = Some(Wanted::Password);
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Typed credentials never outlive the form: wiped from the fields on success and on logout.
    pub fn clear_secrets(&mut self, cx: &mut Context<Self>) {
        for editor in [&self.password, &self.code] {
            editor.update(cx, |editor, _| editor.set_text(""));
        }
    }

    // ---- what the controller reports ----

    pub(crate) fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::LoggingIn => {
                self.busy = true;
                self.login_error = None;
                self.password_error = None;
            }
            Event::Success(token) => {
                self.busy = false;
                self.dialog = Dialog::None;
                self.clear_secrets(cx);
                cx.emit(LoginEvent::Success(token));
            }
            Event::Fields(errors) => {
                self.busy = false;
                self.show_field_errors(&errors);
            }
            Event::Mfa(challenge) => {
                self.busy = false;
                // The dialog opens on the slide of the first method, WebAuthn's included (`methods[0]?.type ?? "select"`).
                let step = match challenge.methods.first() {
                    Some(MfaMethod::WebAuthn(_)) => Step::WebAuthn,
                    Some(MfaMethod::Totp) => Step::Totp,
                    Some(MfaMethod::Backup) => Step::BackupWarning,
                    Some(MfaMethod::Sms) => Step::Sms,
                    None => Step::Select,
                };
                self.code.update(cx, |editor, _| editor.set_text(""));
                if step == Step::Sms {
                    self.send(Command::SendSms);
                }
                self.dialog = Dialog::Mfa(Mfa {
                    challenge,
                    step,
                    error: None,
                    busy: false,
                    phone: None,
                    countdown: BACKUP_WAIT,
                });
                if step == Step::BackupWarning {
                    self.start_countdown(cx);
                }
                self.want_focus = Some(Wanted::Code);
            }
            Event::MfaFailed(message) => {
                if let Dialog::Mfa(mfa) = &mut self.dialog {
                    mfa.busy = false;
                    mfa.error = Some(message);
                }
            }
            Event::SmsSent { phone } => {
                if let Dialog::Mfa(mfa) = &mut self.dialog {
                    mfa.phone = Some(phone);
                    mfa.error = None;
                }
            }
            Event::SmsFailed(message) => {
                if let Dialog::Mfa(mfa) = &mut self.dialog {
                    mfa.error = (!message.is_empty()).then_some(message);
                }
            }
            Event::AccountDisabled => {
                self.busy = false;
                self.dialog = Dialog::Disabled;
            }
            Event::ScheduledForDeletion => {
                self.busy = false;
                self.dialog = Dialog::Deletion;
            }
            Event::PhoneVerificationRequired => {
                self.busy = false;
                self.dialog = Dialog::PhoneVerification;
            }
            Event::Suspended => {
                self.busy = false;
                self.dialog = Dialog::Suspended;
            }
            Event::Captcha(challenge) => {
                self.busy = false;
                self.captcha_failed = false;
                if let Dialog::Mfa(mfa) = std::mem::replace(&mut self.dialog, Dialog::None) {
                    self.captcha_behind = Some(mfa);
                }
                self.dialog = Dialog::Captcha(challenge);
            }
            // The official form shows nothing for a rate limit or a dead network: the button stops loading.
            Event::Quiet(_) => {
                self.busy = false;
                if let Dialog::Mfa(mfa) = &mut self.dialog {
                    mfa.busy = false;
                }
            }
            Event::ForgotSent { login } => self.dialog = Dialog::ForgotSent(login),
            Event::ForgotOneTimeLogin => {
                let login = self.email.read(cx).text().trim().to_owned();
                self.dialog = Dialog::ForgotSent(login);
            }
            Event::ForgotFields(errors) => self.show_field_errors(&errors),
            Event::Qr(event) => self.apply_qr(event),
        }
        cx.notify();
    }

    fn show_field_errors(&mut self, errors: &FormErrors) {
        self.login_error = errors
            .field("login")
            .or_else(|| errors.field("email"))
            .map(str::to_owned);
        self.password_error = errors.field("password").map(str::to_owned);
        // New field errors move the keyboard to the password field, else to the login field (the `useEffect` on
        // `errors` in the form of module 275941). Errors the form has no field for change nothing.
        if self.password_error.is_some() {
            self.want_focus = Some(Wanted::Password);
        } else if self.login_error.is_some() {
            self.want_focus = Some(Wanted::Email);
        }
    }

    fn apply_qr(&mut self, event: RemoteEvent) {
        self.qr = match event {
            RemoteEvent::Initializing => QrState::Initializing,
            RemoteEvent::Code { fingerprint } => qr::matrix(&qr::login_url(&fingerprint))
                .map(QrState::Code)
                .unwrap_or(QrState::Initializing),
            RemoteEvent::Scanned(user) => {
                self.avatar = None;
                if let (Some(hash), Some(requests)) = (&user.avatar, &self.avatar_requests) {
                    let _ = requests.send((user.id.clone(), hash.clone()));
                }
                QrState::Scanned {
                    name: user.tag(),
                    user: user.id.clone(),
                    avatar_hash: user.avatar.clone(),
                }
            }
            RemoteEvent::Finishing => QrState::Finishing,
            // `LoginEvent::Success` follows from the controller's own `Event::Success`.
            RemoteEvent::Token(_) => self.qr.clone(),
        };
    }

    fn start_countdown(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep = this
                    .update(cx, |this, cx| {
                        let Dialog::Mfa(mfa) = &mut this.dialog else {
                            return false;
                        };
                        if mfa.step != Step::BackupWarning {
                            return false;
                        }
                        if this.frozen {
                            return false;
                        }
                        mfa.countdown = mfa.countdown.saturating_sub(1);
                        cx.notify();
                        mfa.countdown > 0
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
        .detach();
    }

    // ---- what the user does ----

    fn submit_focused(&mut self, cx: &mut Context<Self>) {
        match &self.dialog {
            Dialog::None => self.log_in(cx),
            Dialog::Mfa(mfa) if matches!(mfa.step, Step::Totp | Step::Backup | Step::Sms) => {
                self.confirm_mfa(cx)
            }
            _ => {}
        }
    }

    fn log_in(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let login = self.email.read(cx).text().trim().to_owned();
        let password = Zeroizing::new(self.password.read(cx).text().to_owned());
        self.login_error = login.is_empty().then(|| REQUIRED.to_owned());
        self.password_error = password.is_empty().then(|| REQUIRED.to_owned());
        if login.is_empty() || password.is_empty() {
            cx.notify();
            return;
        }
        self.busy = true;
        self.send(Command::Login {
            login,
            password,
            undelete: false,
        });
        cx.notify();
    }

    fn forgot_password(&mut self, cx: &mut Context<Self>) {
        let login = self.email.read(cx).text().trim().to_owned();
        if login.is_empty() {
            self.login_error = Some(REQUIRED.to_owned());
            cx.notify();
            return;
        }
        self.login_error = None;
        self.send(Command::Forgot { login });
        cx.notify();
    }

    fn confirm_mfa(&mut self, cx: &mut Context<Self>) {
        let code = self.code.read(cx).text().trim().to_owned();
        let Dialog::Mfa(mfa) = &mut self.dialog else {
            return;
        };
        if mfa.busy || code.is_empty() {
            return;
        }
        let kind = match mfa.step {
            Step::Sms => MfaKind::Sms,
            Step::Backup => MfaKind::Backup,
            _ => MfaKind::Totp,
        };
        mfa.busy = true;
        mfa.error = None;
        let _ = self.commands.send(Command::Mfa { kind, code });
        cx.notify();
    }

    /// "Authenticate with a passkey or security key" (`Z` in module 397404): the challenge of the method goes to the
    /// system's prompt over `window` (a handle, 0 for none), and its credential goes back as the code of
    /// `POST /auth/mfa/webauthn`. A prompt that ends without a credential shows the slide's one error sentence.
    fn authenticate_with_key(&mut self, window: isize, cx: &mut Context<Self>) {
        let Dialog::Mfa(mfa) = &mut self.dialog else {
            return;
        };
        if mfa.busy {
            return;
        }
        let challenge = mfa
            .challenge
            .methods
            .iter()
            .find_map(|method| match method {
                MfaMethod::WebAuthn(challenge) => Some(challenge.clone()),
                _ => None,
            });
        let Some(challenge) = challenge else {
            mfa.error = Some(WEBAUTHN_FAILED.to_owned());
            cx.notify();
            return;
        };
        mfa.busy = true;
        mfa.error = None;
        let answer = crate::webauthn::start(crate::webauthn::Kind::Get, window, challenge);
        cx.spawn(async move |this, cx| {
            let answer = answer.await.unwrap_or(Err(dc_webauthn::Error::Cancelled));
            this.update(cx, |this, cx| this.key_answered(answer, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// The prompt closed: its credential is posted as the second factor, anything else ends the attempt.
    fn key_answered(&mut self, answer: crate::webauthn::Answer, cx: &mut Context<Self>) {
        let Dialog::Mfa(mfa) = &mut self.dialog else {
            return;
        };
        if mfa.step != Step::WebAuthn {
            // The person went elsewhere while the prompt was open.
            mfa.busy = false;
            return;
        }
        match answer {
            Ok(credential) => {
                let _ = self.commands.send(Command::Mfa {
                    kind: MfaKind::WebAuthn,
                    code: credential,
                });
            }
            Err(_) => {
                mfa.busy = false;
                mfa.error = Some(WEBAUTHN_FAILED.to_owned());
            }
        }
        cx.notify();
    }

    fn goto_step(&mut self, step: Step, cx: &mut Context<Self>) {
        let Dialog::Mfa(mfa) = &mut self.dialog else {
            return;
        };
        mfa.step = step;
        mfa.error = None;
        mfa.countdown = BACKUP_WAIT;
        self.code.update(cx, |editor, _| editor.set_text(""));
        match step {
            Step::Sms => self.send(Command::SendSms),
            Step::BackupWarning => self.start_countdown(cx),
            _ => {}
        }
        self.want_focus = Some(Wanted::Code);
        cx.notify();
    }

    /// The widget's answers: a person solved it (the token goes back with the resend), it expired, or it failed.
    fn poll_captcha(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &self.captcha_host else {
            return;
        };
        for event in host.poll() {
            match captcha_event(&event) {
                Some(CaptchaEvent::Solved(token)) => {
                    let Dialog::Captcha(challenge) = &self.dialog else {
                        return;
                    };
                    let answer = CaptchaAnswer {
                        key: token,
                        rqtoken: challenge.rqtoken.clone(),
                        session_id: challenge.session_id.clone(),
                    };
                    self.captcha_host = None;
                    self.dialog = match self.captcha_behind.take() {
                        Some(mut mfa) => {
                            mfa.busy = true;
                            Dialog::Mfa(mfa)
                        }
                        None => {
                            self.busy = true;
                            Dialog::None
                        }
                    };
                    self.send(Command::CaptchaSolved(answer));
                    cx.notify();
                    return;
                }
                Some(CaptchaEvent::Failed) => {
                    self.captcha_failed = true;
                    cx.notify();
                }
                _ => {}
            }
            if let HostEvent::Failed(_) = event {
                self.captcha_failed = true;
                cx.notify();
            }
        }
    }

    /// Keeps the widget's surface matched to the dialog: made when the captcha dialog opens, placed over its slot,
    /// dropped when the dialog goes.
    fn sync_captcha(
        &mut self,
        window: &mut Window,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        let Dialog::Captcha(challenge) = &self.dialog else {
            self.captcha_host = None;
            self.captcha_poll = None;
            return;
        };
        // The dialog is 400 x 380 in the middle; the widget's 302 x 76 frame sits 49px in and 268.5px down.
        let bounds = (
            (width - 400.) / 2. + 49.,
            (height - 380.) / 2. + 268.5,
            302.,
            78.,
        );
        if let Some(host) = &self.captcha_host {
            if self.captcha_bounds != bounds {
                host.set_bounds(bounds);
                self.captcha_bounds = bounds;
            }
            return;
        }
        if self.captcha_failed {
            return;
        }
        let Some(dir) = crate::storage::data_dir().map(|dir| dir.join("captcha")) else {
            self.captcha_failed = true;
            return;
        };
        match WebHost::captcha(
            window,
            dir,
            bounds,
            &challenge.sitekey,
            challenge.rqdata.as_deref(),
        ) {
            Ok(host) => {
                self.captcha_host = Some(host);
                self.captcha_bounds = bounds;
                self.captcha_poll = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        if this.update(cx, |this, cx| this.poll_captcha(cx)).is_err() {
                            return;
                        }
                    }
                }));
            }
            Err(_) => self.captcha_failed = true,
        }
    }

    fn close_dialog(&mut self, cx: &mut Context<Self>) {
        if matches!(self.dialog, Dialog::Captcha(_)) {
            self.send(Command::CaptchaDismissed);
            self.captcha_host = None;
            self.dialog = match self.captcha_behind.take() {
                Some(mfa) => Dialog::Mfa(mfa),
                None => Dialog::None,
            };
            self.busy = false;
            cx.notify();
            return;
        } else {
            self.send(Command::Reset);
        }
        self.dialog = Dialog::None;
        self.busy = false;
        self.code.update(cx, |editor, _| editor.set_text(""));
        self.want_focus = Some(Wanted::Email);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "tab" && matches!(self.dialog, Dialog::None) {
            let to_password = self.email.focus_handle(cx).is_focused(window);
            let next = if to_password == !event.keystroke.modifiers.shift {
                self.password.focus_handle(cx)
            } else {
                self.email.focus_handle(cx)
            };
            window.focus(&next, cx);
            cx.stop_propagation();
        } else if event.keystroke.key == "escape" && !matches!(self.dialog, Dialog::None) {
            self.close_dialog(cx);
            cx.stop_propagation();
        }
    }
}

/// Seconds the backup code warning holds its button (`countdown` in the dialog).
const BACKUP_WAIT: u32 = 10;

// ---- drawing ----

pub(super) fn heading(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(24.))
        .line_height(px(32.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-strong"))
        .child(text.into())
}

pub(super) fn body(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .text_color(color("--text-default"))
        .child(text.into())
}

/// A paragraph with one bold run.
pub(super) fn rich(text: &str, bold: Range<usize>) -> StyledText {
    StyledText::new(text.to_owned()).with_highlights(vec![(
        bold,
        HighlightStyle {
            font_weight: Some(FontWeight(600.)),
            ..Default::default()
        },
    )])
}

/// `textButton`: a link-colored 14px button.
pub(super) fn text_button(id: &'static str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_size(px(14.))
        .line_height(px(18.))
        .text_color(color("--text-brand"))
        .hover(|style| style.underline())
        .child(label.to_owned())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Primary,
    Secondary,
}

fn button(id: &'static str, label: &str, look: Look) -> gpui::Stateful<gpui::Div> {
    button_with(id, label.to_owned().into_any_element(), look)
}

fn button_with(id: &'static str, content: AnyElement, look: Look) -> gpui::Stateful<gpui::Div> {
    let (background, border, text) = match look {
        Look::Primary => (
            color("--control-primary-background-default"),
            color("--control-primary-border-default"),
            color("--white"),
        ),
        Look::Secondary => (
            color("--control-secondary-background-default"),
            color("--border-muted"),
            color("--control-secondary-text-default"),
        ),
    };
    div()
        .id(id)
        .w_full()
        .h(px(40.))
        .flex_none()
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(text)
        .child(content)
}

/// The three dots of a button that waits (`spinner` in the official `Button`).
fn dots() -> AnyElement {
    let dot = |index: usize, diameter: f32| {
        div()
            .size(px(diameter))
            .rounded_full()
            .bg(color("--white"))
            .with_animation(
                SharedString::from(format!("login-dot-{index}")),
                Animation::new(Duration::from_millis(1200)).repeat(),
                move |element, delta| {
                    let phase = (delta + 1. - index as f32 * 0.25) % 1.;
                    element.opacity(0.35 + 0.65 * (1. - phase))
                },
            )
    };
    div()
        .flex()
        .items_center()
        .gap(px(1.))
        .child(dot(0, 6.))
        .child(dot(1, 5.))
        .child(dot(2, 4.))
        .into_any_element()
}

/// The message row under a field: a 16px icon and 12px text in the critical color.
fn field_error(message: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(icon("login_error", 16., "--text-feedback-critical"))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-feedback-critical"))
                .child(message.to_owned()),
        )
}

pub(super) fn label(text: &str, required: bool) -> gpui::Div {
    div()
        .h(px(20.))
        .flex()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text.to_owned()),
        )
        .when(required, |this| {
            this.child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-feedback-critical"))
                    .child("*"),
            )
        })
}

impl LoginView {
    fn background(&self, width: f32, height: f32) -> AnyElement {
        // `preserveAspectRatio="xMinYMin slice"` on a 1440 x 900 viewBox: scaled to cover, anchored top left.
        let scale = (width / 1440.).max(height / 900.);
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .children(self.art.artwork.clone().map(|artwork| {
                img(artwork)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(1440. * scale))
                    .h(px(900. * scale))
                    .object_fit(ObjectFit::Fill)
            }))
            .children(
                (if crate::branding::selected() == crate::branding::IconSet::Gravecord {
                    Some(crate::branding::login_logo())
                } else {
                    self.art.logo.clone()
                })
                .map(|logo| {
                    img(logo)
                        .absolute()
                        .left(px(48.))
                        .top(px(48.))
                        .w(px(124.))
                        .h(px(24.))
                }),
            )
            .into_any_element()
    }

    fn input_box(
        &self,
        editor: &Entity<Editor>,
        height: f32,
        pad: f32,
        focused: bool,
        error: bool,
        focus: &str,
    ) -> gpui::Div {
        let border = if error {
            color("--border-feedback-critical")
        } else if focused {
            color(focus)
        } else {
            color("--input-border-default")
        };
        div()
            .relative()
            .w_full()
            .h(px(height))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .px(px(pad))
            .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
    }

    fn login_card(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let email_focused = self.email.focus_handle(cx).is_focused(window);
        let password_focused = self.password.focus_handle(cx).is_focused(window);
        let email_error = self.login_error.clone();
        let password_error = self.password_error.clone();
        let show = self.show_password;
        self.password
            .update(cx, |editor, _| editor.set_masked(!show));

        let email_field = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label("Email or Phone Number", true))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.input_box(
                        &self.email,
                        44.,
                        11.,
                        email_focused,
                        email_error.is_some(),
                        "--text-link",
                    ))
                    .children(email_error.as_deref().map(field_error)),
            );
        let password_field = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label("Password", true))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        self.input_box(
                            &self.password,
                            40.,
                            12.,
                            password_focused,
                            password_error.is_some(),
                            "--background-brand",
                        )
                        .child(
                            div()
                                .id("login-eye")
                                .absolute()
                                .right(px(3.))
                                .top(px(3.))
                                .size(px(32.))
                                .rounded(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(icon(
                                    if show { "eye_slash" } else { "login_eye" },
                                    16.,
                                    "--text-strong",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_password = !this.show_password;
                                    cx.notify();
                                })),
                        ),
                    )
                    .children(password_error.as_deref().map(field_error)),
            );

        let log_in = button_with(
            "login-submit",
            if self.busy {
                dots()
            } else {
                "Log In".into_any_element()
            },
            Look::Primary,
        )
        .on_click(cx.listener(|this, _, _, cx| this.log_in(cx)));

        let main = div()
            .w(px(416.))
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(heading("Welcome back!"))
                    .child(body("We're so excited to see you again!")),
            )
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .flex_col()
                    .child(email_field.mb(px(20.)))
                    .child(password_field)
                    .child(
                        div().mt(px(4.)).mb(px(20.)).h(px(19.)).flex().child(
                            text_button("login-forgot", "Forgot your password?")
                                .on_click(cx.listener(|this, _, _, cx| this.forgot_password(cx))),
                        ),
                    )
                    .child(div().mb(px(8.)).child(log_in))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(19.))
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child("Need an account?"),
                            )
                            .child(
                                text_button("login-register", "Register").on_click(|_, _, cx| {
                                    cx.open_url("https://discord.com/register")
                                }),
                            ),
                    ),
            );

        div()
            .id("login-card")
            .occlude()
            .w(px(784.))
            .p(px(32.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.2),
                offset: gpui::point(px(0.), px(2.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .gap(px(64.))
            .child(main)
            .child(self.qr_panel(cx))
            .into_any_element()
    }

    fn qr_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let column = div()
            .w(px(240.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .justify_center();
        match &self.qr {
            QrState::Scanned { name, .. } => {
                let avatar = self
                    .avatar
                    .clone()
                    .or_else(|| self.art.default_avatar.clone());
                column
                    .child(self.scanned_avatar(avatar))
                    .child(div().mt(px(32.)).child(heading("Check your phone!")))
                    .child(
                        div()
                            .mt(px(7.))
                            .child(body(format!("Logging in as {name}"))),
                    )
                    .child(div().mt(px(18.)).child(
                        text_button("login-start-over", "Not me, start over").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.qr = QrState::Initializing;
                                this.send(Command::RestartQr);
                                cx.notify();
                            }),
                        ),
                    ))
                    .into_any_element()
            }
            state => {
                let matrix = match state {
                    QrState::Code(matrix) => Some(matrix.clone()),
                    _ => None,
                };
                let loading = matrix.is_none();
                let code = div()
                    .relative()
                    .size(px(176.))
                    .flex_none()
                    .rounded(px(4.))
                    .when(loading, |this| {
                        this.flex()
                            .items_center()
                            .justify_center()
                            .child(wandering_cubes())
                    })
                    .when_some(matrix, |this, matrix| {
                        this.bg(rgba(0xffffffff))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(8.))
                                    .top(px(8.))
                                    .size(px(160.))
                                    .child(draw_matrix(matrix)),
                            )
                            .children(
                                (if crate::branding::selected()
                                    == crate::branding::IconSet::Gravecord
                                {
                                    Some(crate::branding::qr_logo())
                                } else {
                                    self.art.qr_logo.clone()
                                })
                                .map(|logo| {
                                    img(logo)
                                        .absolute()
                                        .left(px(63.))
                                        .top(px(63.))
                                        .size(px(50.))
                                }),
                            )
                    });
                column
                    .child(code)
                    .child(div().mt(px(32.)).child(heading("Log in with QR Code")))
                    .child(
                        div()
                            .mt(px(8.))
                            .w(px(240.))
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(rich(
                                "Scan this with the Discord mobile app to log in instantly.",
                                19..37,
                            )),
                    )
                    .child(
                        div()
                            .w(px(256.))
                            .p(px(8.))
                            .flex()
                            .justify_center()
                            .child(text_button("login-passkey", "Or sign in with a passkey")),
                    )
                    .into_any_element()
            }
        }
    }

    /// The avatar of the account that scanned the code, with the "online via mobile" badge cut out of its corner.
    fn scanned_avatar(&self, avatar: Option<Arc<Image>>) -> AnyElement {
        div()
            .relative()
            .size(px(120.))
            .flex_none()
            .children(avatar.map(|avatar| {
                img(avatar)
                    .size(px(120.))
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
            }))
            // The cut-out in the card's own color, then the phone.
            .child(
                div()
                    .absolute()
                    .left(px(80.))
                    .top(px(68.))
                    .w(px(40.))
                    .h(px(52.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-high")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(88.))
                    .top(px(76.))
                    .w(px(24.))
                    .h(px(36.))
                    .rounded(px(4.))
                    .bg(rgba(0x45a366ff))
                    .child(
                        div()
                            .absolute()
                            .left(px(3.))
                            .top(px(7.))
                            .size(px(18.))
                            .bg(color("--background-surface-high")),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .top(px(28.))
                            .size(px(4.))
                            .rounded_full()
                            .bg(color("--background-surface-high")),
                    ),
            )
            .into_any_element()
    }
}

/// Draws the modules as filled runs. No quiet zone: the card around it is the white.
/// The `wanderingCubes` spinner (module 193433's `spinner-wandering-cubes`): two 10px cubes in a 32px box, 1.8s,
/// the second half a turn behind. Each key frame is eased in and out (`ease-in-out`).
pub(super) fn wandering_cubes() -> impl IntoElement {
    // (percent, x, y, turns' degrees, scale)
    const FRAMES: [(f32, f32, f32, f32, f32); 5] = [
        (0., 0., 0., 0., 1.),
        (0.25, 22., 0., -90., 0.5),
        (0.5, 22., 22., -180., 1.),
        (0.75, 0., 22., -270., 0.5),
        (1., 0., 0., -360., 1.),
    ];
    static START: std::sync::LazyLock<std::time::Instant> =
        std::sync::LazyLock::new(std::time::Instant::now);
    let ease = |t: f32| {
        // cubic-bezier(.42, 0, .58, 1), solved for x by bisection.
        let bezier = |u: f32, a: f32, b: f32| {
            3. * (1. - u) * (1. - u) * u * a + 3. * (1. - u) * u * u * b + u * u * u
        };
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..24 {
            let mid = (low + high) / 2.;
            if bezier(mid, 0.42, 0.58) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        bezier((low + high) / 2., 0., 1.)
    };
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let elapsed = START.elapsed().as_secs_f32();
            let origin = bounds.origin;
            for delay in [0f32, 0.9] {
                let phase = ((elapsed + delay) % 1.8) / 1.8;
                let at = FRAMES
                    .iter()
                    .rposition(|frame| frame.0 <= phase)
                    .unwrap_or(0)
                    .min(3);
                let (from, to) = (FRAMES[at], FRAMES[at + 1]);
                let t = ease((phase - from.0) / (to.0 - from.0));
                let lerp = |a: f32, b: f32| a + (b - a) * t;
                let (x, y, degrees, scale) = (
                    lerp(from.1, to.1),
                    lerp(from.2, to.2),
                    lerp(from.3, to.3),
                    lerp(from.4, to.4),
                );
                let (sin, cos) = degrees.to_radians().sin_cos();
                // The cube's corners around its center (5, 5), turned and scaled, then moved.
                let corner = |cx: f32, cy: f32| {
                    let (cx, cy) = (cx * scale, cy * scale);
                    gpui::point(
                        origin.x + px(5. + x + cx * cos - cy * sin),
                        origin.y + px(5. + y + cx * sin + cy * cos),
                    )
                };
                let mut path = gpui::PathBuilder::fill();
                path.move_to(corner(-5., -5.));
                path.line_to(corner(5., -5.));
                path.line_to(corner(5., 5.));
                path.line_to(corner(-5., 5.));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, color("--brand-400"));
                }
            }
            window.request_animation_frame();
        },
    )
    .size(px(32.))
    .flex_none()
}

/// The QR component of module 414121 as the two-factor dialog uses it: `side` square on white, `padding` all round
/// (`qrCodeContainer_d4-PpI`: `--space-8`) and corners of `radius` (`--radius-xs`, 4).
pub(in crate::ui) fn qr_code_card(
    text: &str,
    side: f32,
    padding: f32,
    radius: f32,
) -> Option<AnyElement> {
    let matrix = qr::matrix(text)?;
    Some(
        div()
            .flex_none()
            .size(px(side))
            .p(px(padding))
            .rounded(px(radius))
            .bg(rgba(0xffffffff))
            .child(draw_matrix(matrix))
            .into_any_element(),
    )
}

pub(super) fn draw_matrix(matrix: Matrix) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let side = f32::from(bounds.size.width) / matrix.size as f32;
            for y in 0..matrix.size {
                let mut x = 0;
                while x < matrix.size {
                    if !matrix.is_dark(x, y) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < matrix.size && matrix.is_dark(x, y) {
                        x += 1;
                    }
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin
                                + gpui::point(px(start as f32 * side), px(y as f32 * side)),
                            size(px((x - start) as f32 * side), px(side)),
                        ),
                        rgba(0x000000ff),
                    ));
                }
            }
        },
    )
    .size_full()
}

/// `authBox` as a dialog: 480 wide on the card color.
fn dialog_box() -> gpui::Stateful<gpui::Div> {
    div()
        .id("login-dialog")
        .occlude()
        .w(px(480.))
        .rounded(px(8.))
        .bg(color("--background-surface-high"))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.2),
            offset: gpui::point(px(0.), px(2.)),
            blur_radius: px(10.),
            spread_radius: px(0.),
            inset: false,
        }])
}

impl LoginView {
    fn close_button(&self, id: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .top(px(12.))
            .right(px(12.))
            .size(px(40.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon("login_close", 20., "--control-icon-only-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx)))
            .into_any_element()
    }

    fn mfa_dialog(&self, mfa: &Mfa, cx: &mut Context<Self>) -> AnyElement {
        let has = |wanted: &MfaMethod| mfa.challenge.methods.iter().any(|method| method == wanted);
        let title = heading("Multi-Factor Authentication");
        // "Verify with something else" only exists when the account has more than one method (`y` in module 397404).
        let several = mfa.challenge.methods.len() > 1;
        let verify_else = button("mfa-else", "Verify with something else", Look::Secondary)
            .on_click(cx.listener(|this, _, _, cx| this.goto_step(Step::Select, cx)));
        let confirm_enabled = !mfa.busy && !self.code.read(cx).text().trim().is_empty();
        let confirm = button_with(
            "mfa-confirm",
            if mfa.busy {
                dots()
            } else {
                "Confirm".into_any_element()
            },
            Look::Primary,
        )
        .when(!confirm_enabled, |this| this.opacity(0.5))
        .on_click(cx.listener(|this, _, _, cx| this.confirm_mfa(cx)));
        let code_field =
            |text: &str, placeholder: &str, editor: &Entity<Editor>, cx: &mut Context<Self>| {
                editor.update(cx, |editor, _| {
                    editor.set_placeholder(placeholder.to_owned())
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(text.to_owned()),
                    )
                    .child(self.input_box(editor, 40., 12., true, false, "--background-brand"))
            };
        let error = mfa.error.clone().map(|message| {
            div()
                .mt(px(8.))
                .text_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-feedback-critical"))
                .child(message)
        });
        let sub = |text: &str| {
            div()
                .mt(px(8.))
                .text_size(px(20.))
                .line_height(px(24.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text.to_owned())
        };

        let inner: AnyElement = match mfa.step {
            Step::Select => {
                let item = |id: &'static str, text: &str, step: Step, cx: &mut Context<Self>| {
                    div()
                        .id(id)
                        .h(px(50.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-low"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(text.to_owned())
                        .child(icon("login_arrow", 20., "--text-muted"))
                        .on_click(cx.listener(move |this, _, _, cx| this.goto_step(step, cx)))
                };
                div()
                    .px(px(24.))
                    .pt(px(24.))
                    .pb(px(33.))
                    .child(title)
                    .child(sub("This extra step shows it's really you."))
                    .child(
                        div()
                            .mt(px(25.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .when(
                                mfa.challenge
                                    .methods
                                    .iter()
                                    .any(|method| matches!(method, MfaMethod::WebAuthn(_))),
                                |this| {
                                    this.child(item(
                                        "mfa-pick-webauthn",
                                        "Use a passkey or security key",
                                        Step::WebAuthn,
                                        cx,
                                    ))
                                },
                            )
                            .when(has(&MfaMethod::Totp), |this| {
                                this.child(item(
                                    "mfa-pick-totp",
                                    "Use your authenticator app",
                                    Step::Totp,
                                    cx,
                                ))
                            })
                            .when(has(&MfaMethod::Backup), |this| {
                                this.child(item(
                                    "mfa-pick-backup",
                                    "Use a backup code",
                                    Step::BackupWarning,
                                    cx,
                                ))
                            })
                            .when(has(&MfaMethod::Sms), |this| {
                                this.child(item(
                                    "mfa-pick-sms",
                                    "Use a code sent to your phone",
                                    Step::Sms,
                                    cx,
                                ))
                            }),
                    )
                    .into_any_element()
            }
            Step::BackupWarning => {
                let waiting = mfa.countdown > 0;
                let para = |text: StyledText, top: f32, height: f32| {
                    div()
                        .mt(px(top))
                        .h(px(height))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(text)
                };
                let last = if waiting {
                    let head = "If you do not have access to any other methods, the button below will activate in ";
                    let seconds = mfa.countdown.to_string();
                    rich(
                        &format!("{head}{seconds} seconds."),
                        head.len()..head.len() + seconds.len(),
                    )
                } else {
                    StyledText::new(
                        "If you do not have access to any other methods, use the other button below to proceed.",
                    )
                };
                let sure = button_with(
                    "mfa-sure",
                    if waiting {
                        dots()
                    } else {
                        "I'm sure, use a backup code".into_any_element()
                    },
                    Look::Primary,
                )
                .when(waiting, |this| this.opacity(0.5))
                .on_click(cx.listener(|this, _, _, cx| {
                    if matches!(&this.dialog, Dialog::Mfa(mfa) if mfa.countdown == 0) {
                        this.goto_step(Step::Backup, cx);
                    }
                }));
                div()
                    .px(px(24.))
                    .pt(px(24.))
                    .pb(px(24.))
                    .child(title)
                    .child(sub("Slow down! Are you sure?"))
                    .child(para(
                        StyledText::new("Backup codes can only be used one time each and are for emergency access to your account!"),
                        23.,
                        41.,
                    ))
                    .child(para(
                        StyledText::new("If you have access to any of your other methods, select \u{201c}Verify with something else\u{201d} to go back."),
                        13.,
                        40.,
                    ))
                    .child(para(last, 0., 41.))
                    .child(
                        div()
                            .mt(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(several.then_some(verify_else))
                            .child(sure),
                    )
                    .into_any_element()
            }
            Step::WebAuthn => {
                // The button waits (`loading`) while the system prompt is open.
                let authenticate = button_with(
                    "mfa-webauthn",
                    if mfa.busy {
                        dots()
                    } else {
                        "Authenticate with a passkey or security key".into_any_element()
                    },
                    Look::Primary,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let parent = crate::webauthn::parent(window);
                    this.authenticate_with_key(parent, cx)
                }));
                div()
                    .child(div().px(px(24.)).pt(px(24.)).child(title))
                    .children(error.map(|error| div().px(px(24.)).pt(px(16.)).child(error)))
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(24.))
                            .pb(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(several.then_some(verify_else))
                            .child(authenticate),
                    )
                    .into_any_element()
            }
            step => {
                let (text, placeholder, header) = match step {
                    Step::Backup => ("Enter Discord Backup Code", "8-digit backup code", None),
                    Step::Sms => (
                        "Enter Discord SMS Code",
                        "6-digit authentication code",
                        Some(mfa.phone.clone()),
                    ),
                    _ => (
                        "Enter Discord Auth Code",
                        "6-digit authentication code",
                        None,
                    ),
                };
                let field = code_field(text, placeholder, &self.code, cx);
                let field = if step == Step::Sms {
                    // The input and its "Resend SMS?" button share a row.
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(text.to_owned()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(div().flex_1().child(self.input_box(
                                    &self.code,
                                    40.,
                                    12.,
                                    true,
                                    false,
                                    "--background-brand",
                                )))
                                .child(
                                    button("mfa-resend", "Resend SMS?", Look::Secondary)
                                        .w(px(125.62))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.send(Command::SendSms);
                                            cx.notify();
                                        })),
                                ),
                        )
                } else {
                    field
                };
                div()
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(24.))
                            .child(title)
                            .children(header.map(|phone| {
                                let text = match phone {
                                    Some(phone) => format!("We've sent a message to {phone}.  Please enter the code you received."),
                                    None => "Sending a message to your phone.".to_owned(),
                                };
                                div()
                                    .mt(px(8.))
                                    .text_center()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(text)
                            })),
                    )
                    .child(div().px(px(24.)).pt(px(24.)).pb(px(24.)).child(field).children(error))
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(several.then_some(verify_else))
                            .child(confirm),
                    )
                    .into_any_element()
            }
        };
        dialog_box()
            .relative()
            .child(inner)
            .child(self.close_button("mfa-close", cx))
            .into_any_element()
    }

    /// "Account Disabled" and "Account Scheduled for Deletion": a 480 card with the way back and the way to restore.
    fn account_dialog(&self, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let (title, text) = if disabled {
            ("Account Disabled", "You cannot use it while it's disabled.")
        } else {
            (
                "Account Scheduled for Deletion",
                "Your account is scheduled to self-destruct soon.",
            )
        };
        dialog_box()
            .p(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .child(heading(title))
            .child(div().mt(px(7.)).child(body(text)))
            .child(
                div().mt(px(20.)).w_full().child(
                    button("account-return", "Return to Login", Look::Primary)
                        .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx))),
                ),
            )
            .child(
                div()
                    .mt(px(7.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child("Change your mind?")
                    .child(text_button("account-restore", "Restore Account").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.dialog = Dialog::None;
                            this.send(Command::RestoreAccount);
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }

    /// A message dialog over the card: title, text and one button, on the 400 wide `size-sm` modal.
    fn notice(
        &self,
        id: &'static str,
        title: &str,
        text: AnyElement,
        ok: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .occlude()
            .w(px(400.))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .relative()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(24.))
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title.to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .child(text),
                    ),
            )
            .child(
                div().pt(px(16.)).px(px(24.)).pb(px(8.)).flex().child(
                    button("notice-ok", ok, Look::Primary)
                        .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx))),
                ),
            )
            .child(
                div()
                    .id("notice-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(40.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon("login_close", 20., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx))),
            )
            .into_any_element()
    }

    /// "Wait! Are you human?": the 400 x 380 dialog. The widget itself is hCaptcha's own page in a surface that sits over
    /// the slot (`sync_captcha`); only what it cannot show (a failure to load) is drawn here.
    fn captcha_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("captcha-dialog")
            .occlude()
            .relative()
            .w(px(400.))
            .h(px(380.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(160.))
                    .bg(gpui::linear_gradient(
                        160.,
                        gpui::linear_color_stop(rgba(0x333764ff), 0.),
                        gpui::linear_color_stop(color("--background-surface-high"), 1.),
                    )),
            )
            .children(self.art.mascot.clone().map(|mascot| {
                img(mascot)
                    .absolute()
                    .left(px(99.86))
                    .top(px(43.5))
                    .w(px(198.28))
                    .h(px(108.))
            }))
            .child(
                div()
                    .absolute()
                    .top(px(175.5))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(heading("Wait! Are you human?")),
            )
            .child(
                div()
                    .absolute()
                    .top(px(214.5))
                    .w_full()
                    .flex()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child("Please confirm you\u{2019}re not a robot."),
            )
            .child(
                div()
                    .absolute()
                    .left(px(48.))
                    .top(px(267.5))
                    .w(px(302.))
                    .h(px(76.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(16.))
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .when(self.captcha_failed, |this| {
                        this.child("The captcha could not load. Close this and try again, or sign in with the QR code.")
                    }),
            )
            .child(
                div()
                    .id("captcha-close")
                    .absolute()
                    .top(px(7.))
                    .right(px(7.))
                    .size(px(40.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon("login_close", 20., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx))),
            )
            .into_any_element()
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.want_focus.take() {
            Some(Wanted::Email) => window.focus(&self.email.focus_handle(cx), cx),
            Some(Wanted::Password) => window.focus(&self.password.focus_handle(cx), cx),
            Some(Wanted::Code) => window.focus(&self.code.focus_handle(cx), cx),
            None => {}
        }
        let (width, height) = (
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height),
        );
        self.sync_captcha(window, width, height, cx);
        let (center, overlay): (AnyElement, Option<AnyElement>) = match &self.dialog {
            Dialog::None => (self.login_card(window, cx), None),
            Dialog::Mfa(mfa) => (self.mfa_dialog(mfa, cx), None),
            Dialog::Disabled => (self.account_dialog(true, cx), None),
            Dialog::Deletion => (self.account_dialog(false, cx), None),
            Dialog::ForgotSent(login) => {
                let head = "We sent instructions to change your password to ";
                let text = format!("{head}{login}, please check both your inbox and spam folder.");
                let text = div().child(rich(&text, head.len()..head.len() + login.len()));
                let note = self.notice(
                    "forgot-sent",
                    "Instructions Sent",
                    text.into_any_element(),
                    "Okay",
                    cx,
                );
                (self.login_card(window, cx), Some(note))
            }
            Dialog::Captcha(_) => (self.login_card(window, cx), Some(self.captcha_dialog(cx))),
            Dialog::PhoneVerification => {
                let note = self.notice(
                    "phone-verification",
                    "Verify your account",
                    div()
                        .child("This account asks for a phone verification. Sign in with the QR code on the page's right side instead.")
                        .into_any_element(),
                    "Okay",
                    cx,
                );
                (self.login_card(window, cx), Some(note))
            }
            Dialog::Suspended => {
                let note = self.notice(
                    "suspended",
                    "Account Suspended",
                    div()
                        .child("This account is suspended. Its standing page is on discord.com.")
                        .into_any_element(),
                    "Okay",
                    cx,
                );
                (self.login_card(window, cx), Some(note))
            }
        };
        div()
            .id("login")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family(super::SANS)
            .font_features(super::kerning())
            .text_size(px(16.))
            .line_height(px(22.))
            .text_color(color("--text-default"))
            .child(self.background(width, height))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(center),
            )
            .children(overlay.map(|dialog| {
                let pose = crate::ui::motion::modal_pose("login-dialog");
                deferred(
                    div()
                        .id("login-scrim")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(pose.dim(rgba(0x000000b8)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| this.close_dialog(cx)))
                        .child(div().posed(pose).child(dialog)),
                )
                .with_priority(1)
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(32.))
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(div().absolute().top_0().right_0().child(window_controls()))
    }
}
