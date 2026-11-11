//! The dialogs behind the Account page buttons, with their fields, checks and requests. Read from the official
//! modules: Edit username (`667792`), Edit email (`162282`), Update your password (`398177`), Add / Edit phone
//! (`615715`, `938401`, `660740`, `557722`), the password confirmation (`662758`) and Disable / Delete account
//! (`158935` `b3`, `631670` `U_`).
//!
//! Nothing here keeps a password: the editors hold it until the request is built, and the harness refuses every
//! account write (`net.rs`), so a test script fills the fields with placeholder text and stops at the answer the
//! refusal gives.

use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::{Root, color, editor_style, icon, modal};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::{Value, json};

pub(in crate::ui) const FORM_KEY: &str = "settings-account-form";
/// The availability check of the username field.
pub(in crate::ui) const USERNAME_KEY: &str = "settings-account-username-check";
/// `debounce(..., 800)` of the check (module 576326).
const USERNAME_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(800);
/// `NC.d.USER_SETTINGS_UPDATE` (module 557722).
const PHONE_REASON: &str = "user_settings_update";
/// `INVALID_PASSWORD` (module 652215 `t02`).
const INVALID_PASSWORD: u64 = 50018;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Step {
    Username,
    Password,
    /// Module 162282 `I`: the old address is verified first (an account with a verified email).
    EmailSend,
    EmailCode,
    EmailReason,
    EmailScam,
    EmailAddress,
    EmailDone,
    PhoneNumber,
    PhoneCode,
    PhonePassword,
    PhoneManage,
    PhoneRemove,
    Remove,
}

/// A request in the shape the official client sends it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Call {
    pub method: &'static str,
    pub route: &'static str,
    pub body: Option<Value>,
}

impl Call {
    fn request(&self) -> Request {
        Request::Rest {
            key: Some(FORM_KEY.to_owned()),
            method: self.method.parse().unwrap_or(reqwest::Method::POST),
            route: self.route.to_owned(),
            body: self.body.clone(),
        }
    }
}

/// `KD` of module 631670 (`PATCH /users/@me`): the Edit username dialog sends username, password and, for a legacy
/// tag the account may edit, the discriminator.
pub(in crate::ui) fn username_call(
    username: &str,
    discriminator: Option<&str>,
    password: &str,
) -> Call {
    let mut body = json!({"username": username, "password": password});
    if let Some(tag) = discriminator {
        body["discriminator"] = json!(tag);
    }
    Call {
        method: "PATCH",
        route: "/users/@me",
        body: Some(body),
    }
}

pub(in crate::ui) fn password_call(current: &str, new: &str) -> Call {
    Call {
        method: "PATCH",
        route: "/users/@me",
        body: Some(json!({"password": current, "new_password": new})),
    }
}

/// `O()` of module 162282: `PUT /users/@me/email` mails the code to the old address.
pub(in crate::ui) fn email_send_call() -> Call {
    Call {
        method: "PUT",
        route: "/users/@me/email",
        body: None,
    }
}

pub(in crate::ui) fn email_verify_call(code: &str) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/email/verify-code",
        body: Some(json!({"code": code})),
    }
}

/// `{email, emailToken, password}` through `_L`: the token is null on an account that had no verified address.
pub(in crate::ui) fn email_change_call(email: &str, token: Option<&str>, password: &str) -> Call {
    Call {
        method: "PATCH",
        route: "/users/@me",
        body: Some(json!({"email": email, "email_token": token, "password": password})),
    }
}

pub(in crate::ui) fn phone_begin_call(phone: &str) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/phone",
        body: Some(json!({"phone": phone, "change_phone_reason": PHONE_REASON})),
    }
}

pub(in crate::ui) fn phone_verify_call(phone: &str, code: &str) -> Call {
    Call {
        method: "POST",
        route: "/phone-verifications/verify",
        body: Some(json!({"phone": phone, "code": code})),
    }
}

pub(in crate::ui) fn phone_add_call(token: &str, password: &str) -> Call {
    Call {
        method: "POST",
        route: "/users/@me/phone",
        body: Some(
            json!({"phone_token": token, "password": password, "change_phone_reason": PHONE_REASON}),
        ),
    }
}

pub(in crate::ui) fn phone_remove_call(password: &str) -> Call {
    Call {
        method: "DELETE",
        route: "/users/@me/phone",
        body: Some(json!({"password": password, "change_phone_reason": PHONE_REASON})),
    }
}

/// `U_` of module 631670.
pub(in crate::ui) fn remove_call(password: &str, delete: bool) -> Call {
    Call {
        method: "POST",
        route: if delete {
            "/users/@me/delete"
        } else {
            "/users/@me/disable"
        },
        body: Some(json!({"password": password})),
    }
}

/// The username rules of `attemptUsername` (module 493527), in its order: characters, repeated dots, length.
pub(in crate::ui) fn username_problem(username: &str) -> Option<&'static str> {
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        Some("Please only use numbers, letters, underscores _ , or periods.")
    } else if username.contains("..") {
        Some("Username cannot contain repeating dots.")
    } else if username.chars().count() < 2 || username.chars().count() > 32 {
        Some("This must be 2-32 characters.")
    } else {
        None
    }
}

/// `PgfBSx`, `mCrAUb` and `T15lqn`: what the availability check says.
pub(in crate::ui) const USERNAME_AVAILABLE: &str = "Username is available. Nice!";
const USERNAME_TAKEN: &str =
    "Username is unavailable. Try adding numbers, letters, underscores _ , or periods.";
const USERNAME_RATE_LIMIT: &str = "Too fast! Wait a bit and try again.";

#[derive(Debug, PartialEq)]
pub(in crate::ui) enum Verdict {
    Available,
    Problem(String),
}

/// `UNIQUE_USERNAME_ATTEMPT_SUCCESS` / `_FAILURE` as the validation hook reads them: `{taken}` says yes or no; a 429
/// is the rate limit; another status under 500 (not 401) shows the server's own message; the rest shows nothing.
pub(in crate::ui) fn username_verdict(answer: &Value) -> Option<Verdict> {
    if answer["refused"].as_bool() == Some(true) {
        return None;
    }
    if answer["ok"].as_bool() == Some(true) {
        return Some(match answer["body"]["taken"].as_bool() {
            Some(true) => Verdict::Problem(USERNAME_TAKEN.to_owned()),
            Some(false) => Verdict::Available,
            None => return None,
        });
    }
    match answer["status"].as_u64() {
        Some(429) => Some(Verdict::Problem(USERNAME_RATE_LIMIT.to_owned())),
        Some(status) if (1..500).contains(&status) && status != 401 => {
            let message = answer["body"]["message"].as_str()?;
            Some(Verdict::Problem(message.to_owned()))
        }
        _ => None,
    }
}

/// Module 398177: an empty new password, then a confirmation that differs.
pub(in crate::ui) fn password_problems(
    new: &str,
    confirm: &str,
) -> (Option<&'static str>, Option<&'static str>) {
    let first = new
        .is_empty()
        .then_some("Your new password cannot be empty.");
    let second = (new != confirm).then_some("Passwords do not match!");
    (first, second)
}

/// The first message the server gave for `field`: `{field: [..]}` (`oldFormErrors`) or `{errors: {field:
/// {_errors: [{message}]}}}`.
pub(in crate::ui) fn field_error(body: &Value, field: &str) -> Option<String> {
    body[field][0]
        .as_str()
        .or_else(|| body["errors"][field]["_errors"][0]["message"].as_str())
        .map(str::to_owned)
}

/// The mask of the country dialing code: digits only.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// `${code}${number}` of module 938401: the number the route takes.
pub(in crate::ui) fn phone_number(code: &str, number: &str) -> String {
    format!("+{}{}", digits(code), digits(number))
}

pub(in crate::ui) struct Field {
    name: &'static str,
    label: &'static str,
    editor: Entity<Editor>,
    _sub: Subscription,
    /// `revealable`: the eye button toggles the bullets.
    revealable: bool,
    shown: bool,
    error: Option<String>,
    /// The positive message under the box (`successMessage` of module 452027).
    success: Option<String>,
    helper: Option<&'static str>,
    leading: Option<&'static str>,
    width: Option<f32>,
}

pub(in crate::ui) struct Form {
    pub step: Step,
    delete: bool,
    fields: Vec<Field>,
    notice: Option<String>,
    working: bool,
    email_token: Option<String>,
    reason: Option<usize>,
    phone: String,
    phone_token: Option<String>,
    new_email: String,
    /// Set by a refused or failed request of the step, for the frame and the log.
    pub last: Option<String>,
    /// The first field takes the focus on the next frame.
    focus_pending: bool,
    /// The debounce of the username check, and the name the request in flight asked about.
    check_wait: Option<gpui::Task<()>>,
    check_asked: String,
}

/// The reasons of module 162282 in `FL` order, with the strings `naBTFO`, `LQ0RUP`, `oOqQjw`, `p38n1b`.
const REASONS: [&str; 4] = [
    "A Discord employee asked me to",
    "Someone asked me to",
    "I recently created a new email",
    "Something else",
];

impl Form {
    fn text(&self, name: &str, cx: &gpui::App) -> String {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.editor.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    fn set_error(&mut self, name: &str, message: Option<String>) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.name == name) {
            field.error = message;
        }
    }
}

impl Root {
    fn account_field(
        &self,
        name: &'static str,
        label: &'static str,
        secret: bool,
        cx: &mut Context<Self>,
    ) -> Field {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_masked(secret);
            editor
        });
        // Tab walks the form's inputs: the window's tab order, as the browser's.
        let _ = gpui::Focusable::focus_handle(editor.read(cx), cx).tab_stop(true);
        let sub = cx.subscribe(&editor, move |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(_) => this.account_form_submit(cx),
                EditorEvent::Cancel => this.settings_extra.form = None,
                EditorEvent::Edited => this.account_form_edited(name, cx),
            }
            cx.notify();
        });
        Field {
            name,
            label,
            editor,
            _sub: sub,
            revealable: secret,
            shown: false,
            error: None,
            success: None,
            helper: None,
            leading: None,
            width: None,
        }
    }

    /// An Account page button: the first step of its dialog.
    pub(in crate::ui) fn open_account_form(&mut self, kind: &str, cx: &mut Context<Self>) {
        let mut form = Form {
            step: Step::Username,
            delete: false,
            fields: Vec::new(),
            notice: None,
            working: false,
            email_token: None,
            reason: None,
            phone: String::new(),
            phone_token: None,
            new_email: String::new(),
            last: None,
            focus_pending: false,
            check_wait: None,
            check_asked: String::new(),
        };
        let verified = self.stores.account.verified && self.stores.account.email.is_some();
        let has_phone = self.stores.account.phone.is_some();
        let step = match kind {
            "email" if verified => Step::EmailSend,
            "email" => Step::EmailAddress,
            "phone" if has_phone => Step::PhoneManage,
            "phone" => Step::PhoneNumber,
            "password" => Step::Password,
            "disable" | "delete" => {
                form.delete = kind == "delete";
                // `b3`: an owner of servers is told to transfer them first.
                if self.owns_servers() {
                    self.settings_extra.dialog = Some(super::actions::Dialog::Alert {
                        title: "You own servers!".to_owned(),
                        body: "In order to delete or disable your account you must first transfer ownership of all servers that you own.".to_owned(),
                    });
                    cx.notify();
                    return;
                }
                Step::Remove
            }
            _ => Step::Username,
        };
        self.settings_extra.form = Some(form);
        self.account_form_step(step, cx);
    }

    fn owns_servers(&self) -> bool {
        let Some(me) = self.stores.current_user.as_ref().map(|u| u.id) else {
            return false;
        };
        self.stores
            .guilds
            .iter()
            .any(|guild| guild.owner_id == Some(me))
    }

    /// Moves the form to `step`: new fields, the first one focused.
    pub(in crate::ui) fn account_form_step(&mut self, step: Step, cx: &mut Context<Self>) {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let premium = self.stores.account.premium_type.is_some();
        let mut fields: Vec<Field> = Vec::new();
        match step {
            Step::Username => {
                let unique = user.discriminator == "0" || user.discriminator.is_empty();
                let mut name = self.account_field("username", "Username", false, cx);
                name.editor.update(cx, |e, _| e.set_text(&user.username));
                name.helper = unique
                    .then_some("Please only use numbers, letters, underscores _ , or periods.");
                name.width = (!unique).then_some(1.);
                fields.push(name);
                if !unique {
                    let mut tag = self.account_field("discriminator", "Discriminator", false, cx);
                    tag.editor
                        .update(cx, |e, _| e.set_text(&user.discriminator));
                    tag.leading = Some("#");
                    tag.width = Some(96.);
                    let _ = premium;
                    fields.push(tag);
                }
                fields.push(self.account_field("password", "Current Password", true, cx));
            }
            Step::Password => {
                fields.push(self.account_field("password", "Current Password", true, cx));
                fields.push(self.account_field("new_password", "New Password", true, cx));
                fields.push(self.account_field("confirm", "Confirm New Password", true, cx));
            }
            Step::EmailCode => {
                fields.push(self.account_field("code", "Verification Code", false, cx))
            }
            Step::EmailAddress => {
                fields.push(self.account_field("email", "Email", false, cx));
                fields.push(self.account_field("password", "Current Password", true, cx));
            }
            Step::PhoneNumber => {
                let mut code = self.account_field("country", "Country Code", false, cx);
                code.editor.update(cx, |e, _| e.set_text("1"));
                code.leading = Some("+");
                code.width = Some(120.);
                fields.push(code);
                fields.push(self.account_field("phone", "Phone Number", false, cx));
            }
            Step::PhoneCode => {
                fields.push(self.account_field("code", "Verify Your Number", false, cx))
            }
            Step::PhonePassword | Step::PhoneRemove | Step::Remove => {
                fields.push(self.account_field("password", "Password", true, cx));
            }
            Step::EmailSend
            | Step::EmailReason
            | Step::EmailScam
            | Step::EmailDone
            | Step::PhoneManage => {}
        }
        if let Some(form) = self.settings_extra.form.as_mut() {
            form.focus_pending = !fields.is_empty();
            form.step = step;
            form.fields = fields;
            form.notice = None;
            form.working = false;
        }
        cx.notify();
    }

    fn account_form_edited(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        // The six digits of the phone code go in as soon as they are all typed (`h.A` `onSubmit`).
        let complete = form.step == Step::PhoneCode
            && name == "code"
            && digits(&form.text("code", cx)).len() >= 6
            && !form.working;
        form.set_error(name, None);
        if let Some(field) = form.fields.iter_mut().find(|f| f.name == name) {
            field.success = None;
        }
        if form.step == Step::Username && name == "username" {
            // `useUsernameValidation` (module 576326): after 800 ms without typing, a name that is not empty and
            // not the current one is checked.
            let typed = form.text("username", cx);
            let current = self
                .stores
                .current_user
                .as_ref()
                .map(|user| user.username.clone())
                .unwrap_or_default();
            let form = self.settings_extra.form.as_mut().expect("form");
            form.check_wait = None;
            if !typed.is_empty() && typed != current {
                form.check_wait = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(USERNAME_DEBOUNCE).await;
                    this.update(cx, |root, cx| root.username_check(typed, cx))
                        .ok();
                }));
            }
        }
        if complete {
            self.account_form_submit(cx);
        }
    }

    /// The debounce ran out: the rules of `attemptUsername` first, then `POST /users/@me/pomelo-attempt`.
    fn username_check(&mut self, typed: String, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.step != Step::Username || form.text("username", cx) != typed {
            return;
        }
        if let Some(problem) = username_problem(&typed) {
            form.set_error("username", Some(problem.to_owned()));
            cx.notify();
            return;
        }
        form.check_asked = typed.clone();
        let _ = self.requests.send(Request::Rest {
            key: Some(USERNAME_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: "/users/@me/pomelo-attempt".to_owned(),
            body: Some(json!({"username": typed})),
        });
    }

    /// The answer of the availability check, for the name still in the box.
    pub(in crate::ui) fn username_check_answered(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.step != Step::Username || form.text("username", cx) != form.check_asked {
            return;
        }
        match username_verdict(body) {
            Some(Verdict::Available) => {
                if let Some(field) = form.fields.iter_mut().find(|f| f.name == "username") {
                    field.error = None;
                    field.success = Some(USERNAME_AVAILABLE.to_owned());
                }
            }
            Some(Verdict::Problem(message)) => form.set_error("username", Some(message)),
            None => {}
        }
        cx.notify();
    }

    /// The primary button or Enter.
    pub(in crate::ui) fn account_form_submit(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.working {
            return;
        }
        let user = self.stores.current_user.clone().unwrap_or_default();
        let values: Vec<(&'static str, String)> = form
            .fields
            .iter()
            .map(|f| (f.name, f.editor.read(cx).text().to_owned()))
            .collect();
        let text = |name: &str| {
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let call = match form.step {
            Step::Username => {
                let name = text("username");
                if let Some(problem) = username_problem(&name) {
                    form.set_error("username", Some(problem.to_owned()));
                    return;
                }
                let tag = form
                    .fields
                    .iter()
                    .any(|f| f.name == "discriminator")
                    .then(|| text("discriminator"));
                username_call(&name, tag.as_deref(), &text("password"))
            }
            Step::Password => {
                let (first, second) = password_problems(&text("new_password"), &text("confirm"));
                form.set_error("new_password", first.map(str::to_owned));
                form.set_error("confirm", second.map(str::to_owned));
                if first.is_some() || second.is_some() {
                    return;
                }
                password_call(&text("password"), &text("new_password"))
            }
            Step::EmailSend => email_send_call(),
            Step::EmailCode => email_verify_call(text("code").trim()),
            Step::EmailAddress => {
                form.new_email = text("email");
                email_change_call(
                    &form.new_email,
                    form.email_token.as_deref(),
                    &text("password"),
                )
            }
            Step::PhoneNumber => {
                form.phone = phone_number(&text("country"), &text("phone"));
                if digits(&text("phone")).is_empty() {
                    return;
                }
                phone_begin_call(&form.phone)
            }
            Step::PhoneCode => phone_verify_call(&form.phone, digits(&text("code")).as_str()),
            Step::PhonePassword => phone_add_call(
                form.phone_token.as_deref().unwrap_or_default(),
                &text("password"),
            ),
            Step::PhoneRemove => phone_remove_call(&text("password")),
            Step::Remove => {
                if text("password").is_empty() {
                    return;
                }
                remove_call(&text("password"), form.delete)
            }
            Step::EmailReason | Step::EmailScam | Step::EmailDone | Step::PhoneManage => return,
        };
        let _ = user;
        form.working = true;
        let _ = self.requests.send(call.request());
        cx.notify();
    }

    /// The answer of the request the form sent.
    pub(in crate::ui) fn account_form_fetched(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        form.working = false;
        let ok = body["ok"].as_bool() == Some(true);
        let answer = &body["body"];
        if !ok {
            form.last = Some(if body["refused"].as_bool() == Some(true) {
                "refused in this run".to_owned()
            } else {
                format!("status {}", body["status"])
            });
            if body["refused"].as_bool() == Some(true) {
                return;
            }
            let wrong_password = answer["code"].as_u64() == Some(INVALID_PASSWORD);
            let step = form.step;
            for name in [
                "username",
                "discriminator",
                "email",
                "password",
                "new_password",
                "code",
                "phone",
            ] {
                if let Some(message) = field_error(answer, name) {
                    form.set_error(name, Some(message));
                }
            }
            if step == Step::Remove && !wrong_password {
                // `b2`: any other refusal is an alert of its own.
                let message = answer["message"]
                    .as_str()
                    .unwrap_or("Please try again")
                    .to_owned();
                self.settings_extra.form = None;
                self.settings_extra.dialog = Some(super::actions::Dialog::Alert {
                    title: "Error Deleting Your Account".to_owned(),
                    body: message,
                });
                return;
            }
            if let Some(message) = answer["message"].as_str()
                && form.fields.iter().all(|f| f.error.is_none())
            {
                match step {
                    Step::PhoneCode | Step::PhoneNumber | Step::EmailCode => {
                        let name = if step == Step::PhoneNumber {
                            "phone"
                        } else {
                            "code"
                        };
                        form.set_error(name, Some(message.to_owned()));
                    }
                    _ => form.notice = Some(message.to_owned()),
                }
            }
            return;
        }
        let mut next = None;
        match form.step {
            Step::Username | Step::Password => {
                if let Some(user) = self.stores.current_user.as_mut() {
                    if let Some(name) = answer["username"].as_str() {
                        user.username = name.to_owned();
                    }
                    if let Some(tag) = answer["discriminator"].as_str() {
                        user.discriminator = tag.to_owned();
                    }
                }
                self.settings_extra.form = None;
            }
            Step::EmailSend => {
                next = Some(Step::EmailCode);
            }
            Step::EmailCode => {
                form.email_token = answer["token"].as_str().map(str::to_owned);
                next = Some(Step::EmailReason);
            }
            Step::EmailAddress => {
                let email = form.new_email.clone();
                if let Some(address) = answer["email"].as_str() {
                    self.stores.account.email = Some(address.to_owned());
                    self.stores.account.verified = answer["verified"].as_bool().unwrap_or(false);
                }
                form.new_email = email;
                next = Some(Step::EmailDone);
            }
            Step::PhoneNumber => next = Some(Step::PhoneCode),
            Step::PhoneCode => {
                form.phone_token = answer["token"].as_str().map(str::to_owned);
                next = Some(Step::PhonePassword);
            }
            Step::PhonePassword => {
                self.stores.account.phone = Some(form.phone.clone());
                self.settings_extra.form = None;
            }
            Step::PhoneRemove => {
                self.stores.account.phone = None;
                self.settings_extra.form = None;
            }
            // Disabling or deleting logs the account out (`U_`: `logoutInternal`).
            Step::Remove => {
                self.settings_extra.form = None;
                cx.emit(crate::ui::logout::RootEvent::LogOut);
            }
            Step::EmailReason | Step::EmailScam | Step::EmailDone | Step::PhoneManage => {}
        }
        if let Some(step) = next {
            self.account_form_step(step, cx);
        }
        cx.notify();
    }

    /// Render hook: a new step's first field takes the focus.
    pub(in crate::ui) fn account_form_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if !std::mem::take(&mut form.focus_pending) {
            return;
        }
        if let Some(first) = form.fields.first() {
            window.focus(
                &gpui::Focusable::focus_handle(first.editor.read(cx), cx),
                cx,
            );
        }
    }

    /// The dialog of the open form.
    pub(in crate::ui) fn account_form_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.settings_extra.form.as_ref()?;
        let account = &self.stores.account;
        let text = |value: &str, strong: bool| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .when(strong, |this| this.font_weight(FontWeight(600.)))
                .text_color(color("--text-default"))
                .child(value.to_owned())
        };
        let cancel = |id: &'static str| {
            modal::button(id, "Cancel", modal::Tone::Secondary, |this, _, _| {
                this.settings_extra.form = None;
            })
        };
        let primary = |id: &'static str, label: &'static str, tone: modal::Tone| {
            modal::button(id, label, tone, |this, _, cx| this.account_form_submit(cx))
        };
        let go = |id: &'static str, label: &'static str, tone: modal::Tone, step: Step| {
            modal::button(id, label, tone, move |this, _, cx| {
                this.account_form_step(step, cx)
            })
        };
        let busy = |tone: modal::Tone| {
            if form.working && tone == modal::Tone::Primary {
                modal::Tone::PrimaryDisabled
            } else {
                tone
            }
        };
        let (title, subtitle, mut body, buttons): (&str, Option<String>, Vec<AnyElement>, Vec<modal::Button>) =
            match form.step {
                Step::Username => (
                    "Change your username",
                    Some("Enter a new username and your existing password.".to_owned()),
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", "Done", busy(modal::Tone::Primary))],
                ),
                Step::Password => (
                    "Update your password",
                    Some("Enter your current password and a new password.".to_owned()),
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", "Done", busy(modal::Tone::Primary))],
                ),
                Step::EmailSend => (
                    "Verify email address",
                    None,
                    vec![
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(text(
                                &format!(
                                    "We\u{2019}ll need to verify your old email address, {}, in order to change it.",
                                    account.email.clone().unwrap_or_default()
                                ),
                                false,
                            ))
                            .child(text(
                                "Lost access to your email? Please contact your email provider to regain access.",
                                false,
                            ))
                            .into_any_element(),
                    ],
                    vec![
                        cancel("account-cancel"),
                        primary("account-ok", "Send Verification Code", busy(modal::Tone::Primary)),
                    ],
                ),
                Step::EmailCode => (
                    "Enter code",
                    Some("Check your email: we sent you a verification code. Enter it here to verify you\u{2019}re really you.".to_owned()),
                    Vec::new(),
                    vec![primary("account-ok", "Next", busy(modal::Tone::Primary))],
                ),
                Step::EmailReason => (
                    "Why are you changing your email?",
                    None,
                    vec![self.email_reasons(form.reason, cx)],
                    vec![
                        cancel("account-cancel"),
                        if form.reason.is_some() {
                            modal::button("account-ok", "Continue", modal::Tone::Primary, |this, _, cx| {
                                // `u.qr`: the first two reasons get the scam alert.
                                let next = match this.settings_extra.form.as_ref().and_then(|f| f.reason) {
                                    Some(0 | 1) => Step::EmailScam,
                                    _ => Step::EmailAddress,
                                };
                                this.account_form_step(next, cx);
                            })
                        } else {
                            modal::button("account-ok", "Continue", modal::Tone::PrimaryDisabled, |_, _, _| {})
                        },
                    ],
                ),
                Step::EmailScam => (
                    "Potential scam alert!",
                    None,
                    vec![div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(text("This sounds very similar to common scams going around! Changing your email address can give someone else access to your account - learn more about trending scams here.", false))
                        .child(text("Discord will never ask you to change your email - report anyone who asks you to do this.", false))
                        .into_any_element()],
                    vec![
                        go("account-risk", "I know the risks, continue", modal::Tone::Secondary, Step::EmailAddress),
                        cancel("account-cancel"),
                    ],
                ),
                Step::EmailAddress => (
                    "Enter an email address",
                    Some("Enter a new email address and your existing password.".to_owned()),
                    Vec::new(),
                    vec![
                        if form.email_token.is_some() {
                            go("account-back", "Back", modal::Tone::Secondary, Step::EmailReason)
                        } else {
                            cancel("account-cancel")
                        },
                        primary("account-ok", "Done", busy(modal::Tone::Primary)),
                    ],
                ),
                Step::EmailDone => (
                    "Confirm your new email",
                    None,
                    vec![div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(text(
                            &format!("To finish, we sent a verification email to: {}", form.new_email),
                            false,
                        ))
                        .child(text("Didn't get the email? Make sure to check your spam folder.", false))
                        .into_any_element()],
                    vec![modal::button("account-ok", "Okay", modal::Tone::Primary, |this, _, _| {
                        this.settings_extra.form = None;
                    })],
                ),
                Step::PhoneNumber => (
                    "Enter a phone number",
                    Some("You will receive a text message with a verification code.".to_owned()),
                    vec![text(
                        "Your phone number can be used to verify one Discord account at a time and is only used for verification and login.",
                        false,
                    )
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .into_any_element()],
                    vec![primary("account-ok", "Send", busy(modal::Tone::Primary))],
                ),
                Step::PhoneCode => (
                    "Verify your phone number",
                    Some("Enter the 6 digit code we sent to your phone.".to_owned()),
                    Vec::new(),
                    vec![modal::button("account-resend", "Resend Code", modal::Tone::Secondary, |this, _, cx| {
                        let Some(form) = this.settings_extra.form.as_ref() else {
                            return;
                        };
                        let _ = this.requests.send(phone_begin_call(&form.phone).request());
                        cx.notify();
                    })],
                ),
                Step::PhonePassword => (
                    "Enter your password to confirm changes",
                    None,
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", "Confirm", busy(modal::Tone::Primary))],
                ),
                Step::PhoneManage => (
                    "Edit your phone number",
                    None,
                    vec![div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(text("Your current phone number is", false))
                        .child(text(account.phone.as_deref().unwrap_or(""), true))
                        .into_any_element()],
                    {
                        let mut buttons = Vec::new();
                        if account.email.is_some() {
                            buttons.push(go("account-remove", "Remove", modal::Tone::CriticalSecondary, Step::PhoneRemove));
                        }
                        buttons.push(go("account-edit", "Edit", modal::Tone::Primary, Step::PhoneNumber));
                        buttons
                    },
                ),
                Step::PhoneRemove => (
                    "Remove phone number",
                    None,
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", "Remove", modal::Tone::Critical)],
                ),
                Step::Remove => (
                    if form.delete { "Delete account" } else { "Disable account" },
                    None,
                    vec![self.warning_notice(if form.delete {
                        "Are you sure that you want to delete your account? This will immediately log you out of your account and you will not be able to log in again."
                    } else {
                        "Are you sure that you want to disable your account? This will immediately log you out and make your account inaccessible to anyone."
                    })],
                    vec![
                        cancel("account-cancel"),
                        primary(
                            "account-ok",
                            if form.delete { "Delete Account" } else { "Disable Account" },
                            modal::Tone::Critical,
                        ),
                    ],
                ),
            };
        // Fields: a column, or the username and its tag in one row.
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut inline: Vec<AnyElement> = Vec::new();
        for (index, field) in form.fields.iter().enumerate() {
            let element = self.account_field_element(field, index, cx);
            if field.width.is_some() && matches!(form.step, Step::Username | Step::PhoneNumber) {
                inline.push(element);
            } else {
                if !inline.is_empty() {
                    rows.push(
                        div()
                            .flex()
                            .gap(px(4.))
                            .items_end()
                            .children(std::mem::take(&mut inline))
                            .into_any_element(),
                    );
                }
                rows.push(element);
            }
        }
        if !inline.is_empty() {
            let row = div()
                .flex()
                .gap(px(4.))
                .items_start()
                .children(inline)
                .into_any_element();
            rows.insert(0, row);
        }
        if let Some(notice) = form.notice.as_ref() {
            rows.push(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-feedback-critical"))
                    .child(notice.clone())
                    .into_any_element(),
            );
        }
        if form.step == Step::EmailCode {
            rows.push(
                div()
                    .id("account-resend-email")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-link"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.requests.send(email_send_call().request());
                        cx.notify();
                    }))
                    .child("Didn\u{2019}t receive a code or it expired? Resend it.")
                    .into_any_element(),
            );
        }
        if !rows.is_empty() {
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .children(rows)
                    .into_any_element(),
            );
        }
        let body = (!body.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .children(body)
                .into_any_element()
        });
        let spec = modal::Spec {
            id: "settings-account-dialog",
            title: SharedString::from(title.to_owned()),
            subtitle: subtitle.map(|s| div().child(s).into_any_element()),
            body,
            buttons,
            close: Box::new(|this, _, _| this.settings_extra.form = None),
        };
        Some(if form.step == Step::PhoneCode {
            modal::render_small(spec, cx)
        } else {
            modal::render(spec, cx)
        })
    }

    fn warning_notice(&self, message: &'static str) -> AnyElement {
        div()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-warning"))
            .bg(color("--background-feedback-mod-warning"))
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-strong"))
            .child(message)
            .into_any_element()
    }

    fn email_reasons(&self, picked: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(REASONS.iter().enumerate().map(|(index, label)| {
                div()
                    .id(SharedString::from(format!("account-reason-{index}")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(32.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(form) = this.settings_extra.form.as_mut() {
                            form.reason = Some(index);
                        }
                        cx.notify();
                    }))
                    .child(crate::ui::create_channel::radio(picked == Some(index)))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(*label),
                    )
            }))
            .into_any_element()
    }

    /// One labelled input (`95477.k` inside `452027.D`): label 16/20 medium, the 40px box, the error under it at 12/16.
    fn account_field_element(
        &self,
        field: &Field,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = if field.error.is_some() {
            color("--border-feedback-critical")
        } else {
            color("--input-border-default")
        };
        let name = field.name;
        let eye = field.revealable.then(|| {
            let shown = field.shown;
            div()
                .id(SharedString::from(format!("account-eye-{name}")))
                .cursor_pointer()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = this.settings_extra.form.as_mut()
                        && let Some(field) = form.fields.iter_mut().find(|f| f.name == name)
                    {
                        field.shown = !field.shown;
                        let masked = !field.shown;
                        field.editor.update(cx, |e, _| e.set_masked(masked));
                    }
                    cx.notify();
                }))
                .child(icon(
                    if shown { "eye_slash" } else { "login_eye" },
                    20.,
                    "--icon-strong",
                ))
        });
        let _ = index;
        div()
            .when_some(field.width, |this, width| {
                if width > 1. {
                    this.w(px(width)).flex_none()
                } else {
                    this.flex_1().min_w(px(0.))
                }
            })
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(field.label),
            )
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .children(field.leading.map(|lead| {
                        div()
                            .text_color(color("--input-placeholder-text-default"))
                            .child(lead)
                    }))
                    .child(div().flex_1().min_w(px(0.)).child(field.editor.clone()))
                    .children(eye),
            )
            .children(field.error.as_ref().map(|message| {
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
                            .child(message.clone()),
                    )
            }))
            .children(
                field
                    .success
                    .as_ref()
                    .filter(|_| field.error.is_none())
                    .map(|message| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("circle_check", 16., "--text-feedback-positive"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-feedback-positive"))
                                    .child(message.clone()),
                            )
                    }),
            )
            .children(
                field
                    .helper
                    .filter(|_| field.error.is_none() && field.success.is_none())
                    .map(|help| {
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(help)
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_username_check_reads_its_answers_like_the_bundle() {
        let taken = json!({"ok": true, "status": 200, "body": {"taken": true}});
        let free = json!({"ok": true, "status": 200, "body": {"taken": false}});
        assert_eq!(username_verdict(&free), Some(Verdict::Available));
        assert_eq!(
            username_verdict(&taken),
            Some(Verdict::Problem(USERNAME_TAKEN.to_owned()))
        );
        let limited = json!({"ok": false, "status": 429, "body": {"retry_after": 3}});
        assert_eq!(
            username_verdict(&limited),
            Some(Verdict::Problem(USERNAME_RATE_LIMIT.to_owned()))
        );
        let refused = json!({"ok": false, "status": 400, "body": {"message": "Nope"}});
        assert_eq!(
            username_verdict(&refused),
            Some(Verdict::Problem("Nope".to_owned()))
        );
        // A dead token and a server fault say nothing; a harness refusal neither.
        assert_eq!(
            username_verdict(&json!({"ok": false, "status": 401, "body": {"message": "x"}})),
            None
        );
        assert_eq!(
            username_verdict(&json!({"ok": false, "status": 503, "body": null})),
            None
        );
        assert_eq!(
            username_verdict(&json!({"ok": false, "status": 0, "refused": true})),
            None
        );
    }

    #[test]
    fn the_username_rules_use_the_bundles_strings() {
        assert_eq!(username_problem("a"), Some("This must be 2-32 characters."));
        assert_eq!(
            username_problem("a..b"),
            Some("Username cannot contain repeating dots.")
        );
        assert_eq!(
            username_problem("a b"),
            Some("Please only use numbers, letters, underscores _ , or periods.")
        );
        assert_eq!(username_problem("fine_name.2"), None);
    }

    #[test]
    fn username_change_posts_username_and_password_to_me() {
        let call = username_call("someone", None, "pw");
        assert_eq!((call.method, call.route), ("PATCH", "/users/@me"));
        assert_eq!(
            call.body,
            Some(json!({"username": "someone", "password": "pw"}))
        );
        let tagged = username_call("someone", Some("1234"), "pw");
        assert_eq!(tagged.body.unwrap()["discriminator"], "1234");
    }

    #[test]
    fn password_change_sends_new_password_beside_the_current() {
        assert_eq!(
            password_call("old", "new").body,
            Some(json!({"password": "old", "new_password": "new"}))
        );
    }

    #[test]
    fn email_change_flow_requests() {
        assert_eq!(
            (email_send_call().method, email_send_call().route),
            ("PUT", "/users/@me/email")
        );
        assert_eq!(
            email_verify_call("123456").route,
            "/users/@me/email/verify-code"
        );
        let call = email_change_call("a@b.c", Some("tok"), "pw");
        assert_eq!(
            call.body,
            Some(json!({"email": "a@b.c", "email_token": "tok", "password": "pw"}))
        );
        assert_eq!(
            email_change_call("a@b.c", None, "pw").body.unwrap()["email_token"],
            Value::Null
        );
    }

    #[test]
    fn phone_flow_requests() {
        assert_eq!(phone_number("1", "(555) 010-9999"), "+15550109999");
        let begin = phone_begin_call("+15550109999");
        assert_eq!(begin.route, "/users/@me/phone");
        assert_eq!(
            begin.body.unwrap()["change_phone_reason"],
            "user_settings_update"
        );
        assert_eq!(
            phone_verify_call("+1", "123456").route,
            "/phone-verifications/verify"
        );
        assert_eq!(phone_add_call("t", "pw").body.unwrap()["phone_token"], "t");
        assert_eq!(phone_remove_call("pw").method, "DELETE");
    }

    #[test]
    fn disable_and_delete_post_the_password_to_their_routes() {
        assert_eq!(remove_call("pw", false).route, "/users/@me/disable");
        assert_eq!(remove_call("pw", true).route, "/users/@me/delete");
        assert_eq!(
            remove_call("pw", true).body,
            Some(json!({"password": "pw"}))
        );
    }

    #[test]
    fn username_rules_follow_attempt_username() {
        assert_eq!(username_problem("ok_name.1"), None);
        assert!(
            username_problem("no spaces")
                .unwrap()
                .starts_with("Please only use")
        );
        assert_eq!(
            username_problem("a..b"),
            Some("Username cannot contain repeating dots.")
        );
        assert!(username_problem("a").unwrap().contains("2-32"));
        assert!(username_problem(&"a".repeat(33)).is_some());
    }

    #[test]
    fn password_checks_come_in_the_official_order() {
        assert_eq!(
            password_problems("", "").0,
            Some("Your new password cannot be empty.")
        );
        assert_eq!(
            password_problems("a", "b").1,
            Some("Passwords do not match!")
        );
        assert_eq!(password_problems("a", "a"), (None, None));
    }

    #[test]
    fn server_messages_are_read_from_both_error_shapes() {
        let old = json!({"password": ["Password does not match."]});
        assert_eq!(
            field_error(&old, "password").as_deref(),
            Some("Password does not match.")
        );
        let new = json!({"errors": {"email": {"_errors": [{"message": "Not a well formed email address."}]}}});
        assert_eq!(
            field_error(&new, "email").as_deref(),
            Some("Not a well formed email address.")
        );
        assert_eq!(field_error(&old, "username"), None);
    }

    #[test]
    fn requests_use_the_form_key() {
        let Request::Rest { key, route, .. } = email_send_call().request() else {
            panic!("not a rest request");
        };
        assert_eq!(key.as_deref(), Some(FORM_KEY));
        assert_eq!(route, "/users/@me/email");
    }
}
