//! The dialogs behind the Account page buttons, with their fields, checks and requests. Read from the official
//! modules: Edit username (`667792`), Edit email (`162282`), Update your password (`398177`), Add / Edit phone
//! (`615715`, `938401`, `660740`, `557722`), the password confirmation (`662758`) and Disable / Delete account
//! (`158935` `b3`, `631670` `U_`).
//!
//! Nothing here keeps a password: the editors hold it until the request is built, and the harness refuses every
//! account write (`net.rs`), so a test script fills the fields with placeholder text and stops at the answer the
//! refusal gives.

use super::mfa;
use super::model::Choice;
use super::phone_countries::COUNTRIES;
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::{Root, color, editor_style, icon, modal};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, div, prelude::*, px,
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
    /// Module 333737: the secret, its QR code and the field for the first code.
    MfaEnable,
    /// `bn`: the confirmation before the authenticator app goes.
    MfaRemove,
    /// Module 662758 for `disableSMS`.
    MfaSmsPassword,
    /// Module 662758 for `sendMFABackupCodesVerificationKeyEmail`, then module 518142 for the emailed key.
    MfaCodesPassword,
    MfaCodesKey,
    /// Module 549404: the codes of a new authenticator app.
    MfaCodes,
    /// Module 245329 `INIT`: the register dialog that starts the system's prompt for a new security key or passkey.
    KeyRegister,
    /// Module 245329 `NAME`: the name of the key the prompt just made.
    KeyName,
    /// Module 428726: renames a key.
    KeyRename,
    /// The question before the last passkey goes (module 41000, `J/x6ns` over `IMKkcn`).
    KeyDelete,
}

/// A request in the shape the official client sends it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Call {
    pub method: &'static str,
    pub route: &'static str,
    pub body: Option<Value>,
}

impl Call {
    /// The same request under another answer key.
    pub(in crate::ui) fn request_in(&self, key: &str) -> Request {
        Request::Rest {
            key: Some(key.to_owned()),
            method: self.method.parse().unwrap_or(reqwest::Method::POST),
            route: self.route.to_owned(),
            body: self.body.clone(),
        }
    }

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
        Some(dc_intl::tr(
            "Please only use numbers, letters, underscores _ , or periods.",
        ))
    } else if username.contains("..") {
        Some(dc_intl::tr("Username cannot contain repeating dots."))
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
            Some(true) => Verdict::Problem(dc_intl::tr(USERNAME_TAKEN).to_owned()),
            Some(false) => Verdict::Available,
            None => return None,
        });
    }
    match answer["status"].as_u64() {
        Some(429) => Some(Verdict::Problem(
            dc_intl::tr(USERNAME_RATE_LIMIT).to_owned(),
        )),
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
        .then_some(dc_intl::tr("Your new password cannot be empty."));
    let second = (new != confirm).then_some(dc_intl::tr("Passwords do not match!"));
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

/// Module 181658 `aTVNes`: what the phone dialog shows when the failure has no message of its own.
const PHONE_FAILURE: &str = "Oops! You've caught an ultra rare error. This is probably our fault, so please try again or check our status page.";

/// The mask of the country dialing code: digits only.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// `${code}${number}` of module 938401: the number the route takes.
pub(in crate::ui) fn phone_number(code: &str, number: &str) -> String {
    format!("+{}{}", digits(code), digits(number))
}

/// `c.A.find(name === "United States")` of module 938401: the option the select starts on.
fn default_country() -> usize {
    COUNTRIES
        .iter()
        .position(|country| country.alpha2 == "US")
        .unwrap_or(0)
}

/// The two regional-indicator letters of the option's flag (`String.fromCodePoint(...alpha2.map(127397 + code))`);
/// Windows has no flag glyphs and draws the pair as the two letters.
fn flag_letters(alpha2: &str) -> String {
    alpha2.to_ascii_uppercase()
}

/// How well a name answers a query: the ranks of the match-sorter the select filters with (module 873200: its
/// default keys are `label`), best first. `None` when the letters do not even occur in order.
fn match_rank(name: &str, query: &str) -> Option<u8> {
    let name = name.to_lowercase();
    let query = query.to_lowercase();
    if name == query {
        return Some(6);
    }
    if name.starts_with(&query) {
        return Some(5);
    }
    if name.contains(&format!(" {query}")) {
        return Some(4);
    }
    if name.contains(&query) {
        return Some(3);
    }
    let initials: String = name
        .split([' ', '-'])
        .filter_map(|word| word.chars().next())
        .collect();
    if initials.contains(&query) {
        return Some(2);
    }
    let mut rest = name.chars();
    query
        .chars()
        .all(|letter| rest.any(|have| have == letter))
        .then_some(1)
}

/// The options a query leaves (all of them for an empty one): by rank, then by name, as match-sorter orders ties.
fn country_matches(query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..COUNTRIES.len()).collect();
    }
    let mut ranked: Vec<(u8, usize)> = COUNTRIES
        .iter()
        .enumerate()
        .filter_map(|(index, country)| Some((match_rank(country.name, query)?, index)))
        .collect();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            COUNTRIES[a.1]
                .name
                .to_lowercase()
                .cmp(&COUNTRIES[b.1].name.to_lowercase())
        })
    });
    ranked.into_iter().map(|(_, index)| index).collect()
}

/// The dialing code shown in front of the number box (`leading: P?.code`, as the option lists it).
fn country_prefix(index: usize) -> String {
    COUNTRIES[index].code.to_owned()
}

/// The active border is two pixels wide in the captures: the 1px border and a 1px ring inside it.
fn focus_ring() -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .rounded(px(7.))
        .border_1()
        .border_color(color("--input-border-active"))
}

/// Where the open flag of the country list lives: a key that ends in `/open`, so a press outside the list and Escape
/// close it like any select list.
const COUNTRY_OPEN: &str = "phone_country/open";
/// `maxOptionsVisible` (module 890497): the list shows five rows of 40px (`listBoxItem_b5PEJH`).
const COUNTRY_ROWS: f32 = 5.;
const COUNTRY_ROW: f32 = 40.;

/// Scrolls the list the least that shows `row` fully, as `scrollIntoView` does.
fn country_scroll_to(handle: &ScrollHandle, row: usize) {
    let view = COUNTRY_ROW * COUNTRY_ROWS;
    let now = -f32::from(handle.offset().y);
    let top = row as f32 * COUNTRY_ROW;
    let to = if top < now {
        top
    } else if top + COUNTRY_ROW > now + view {
        top + COUNTRY_ROW - view
    } else {
        now
    };
    handle.set_offset(gpui::point(px(0.), px(-to)));
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
    leading: Option<SharedString>,
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
    mfa: mfa::Draft,
    /// The Add phone dialog's country select (module 938401 `f`): the picked option of `COUNTRIES`, the search box of
    /// its open list, the list's scroll and the row the arrow keys are on (in the filtered list).
    country: usize,
    country_search: Option<(Entity<Editor>, Subscription)>,
    country_scroll: ScrollHandle,
    country_active: Option<usize>,
    /// The search box takes the focus on the next frame (the list just opened).
    country_focus: bool,
    /// The `phone` state of module 938401 `j`: empty until the box or the select changes once, then
    /// `${code}${digits}`; the Send button stays dimmed while it is empty.
    phone_typed: String,
    /// The field that holds the focus, drawn with the active border.
    focused: Option<&'static str>,
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
            // Enter submits the form from the field's text: it must not empty the box first.
            editor.set_keeps_text(true);
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

    fn country_is_open(&self) -> bool {
        matches!(
            self.setting_choices.get(COUNTRY_OPEN),
            Some(Choice::On(true))
        )
    }

    /// The search box of the open country list (`comboBoxInput_vWC4bk`): typing narrows the options, Enter takes the
    /// row the arrow keys are on.
    fn country_search_editor(&self, cx: &mut Context<Self>) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor
        });
        let sub = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    if let Some(form) = this.settings_extra.form.as_mut() {
                        // The first match is the row Enter takes.
                        let typed = !Self::country_query(form, cx).is_empty();
                        form.country_active = typed.then_some(0);
                        form.country_scroll.set_offset(gpui::point(px(0.), px(0.)));
                    }
                }
                EditorEvent::Submit(query) => this.country_pick_active(query, cx),
                EditorEvent::Cancel => {
                    this.setting_choices.insert(COUNTRY_OPEN, Choice::On(false));
                }
            }
            cx.notify();
        });
        (editor, sub)
    }

    /// The select field or its chevron: the list opens on the picked option, or closes.
    fn country_toggle(&mut self, cx: &mut Context<Self>) {
        if self.country_is_open() {
            self.setting_choices.insert(COUNTRY_OPEN, Choice::On(false));
        } else if let Some(form) = self.settings_extra.form.as_mut() {
            if let Some((search, _)) = form.country_search.as_ref() {
                search.update(cx, |editor, _| editor.set_text(""));
            }
            form.country_active = None;
            form.country_focus = true;
            country_scroll_to(&form.country_scroll, form.country);
            self.setting_choices.insert(COUNTRY_OPEN, Choice::On(true));
        }
        cx.notify();
    }

    /// An option was taken (`S` of module 938401): the dialing code changes, the number box takes the focus.
    fn country_pick(&mut self, option: usize, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        form.country = option;
        if let Some(phone) = form.fields.iter_mut().find(|f| f.name == "phone") {
            phone.leading = Some(country_prefix(option).into());
        }
        form.phone_typed = phone_number(COUNTRIES[option].code, &form.text("phone", cx));
        form.focus_pending = true;
        self.setting_choices.insert(COUNTRY_OPEN, Choice::On(false));
        cx.notify();
    }

    /// The query in the search box.
    fn country_query(form: &Form, cx: &gpui::App) -> String {
        form.country_search
            .as_ref()
            .map(|(search, _)| search.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    /// Enter in the search box; `query` is what the box held (the editor empties itself on Enter).
    fn country_pick_active(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_ref() else {
            return;
        };
        let matches = country_matches(query);
        if let Some(option) = form
            .country_active
            .and_then(|row| matches.get(row).copied())
        {
            self.country_pick(option, cx);
        }
    }

    /// Arrow keys on the open list move the active row; the list follows it.
    fn country_arrow(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        let matches = country_matches(&Self::country_query(form, cx));
        if matches.is_empty() {
            return;
        }
        // The first arrow starts from the picked option's row.
        let base = form
            .country_active
            .or_else(|| matches.iter().position(|&option| option == form.country));
        let row = match (down, base) {
            (true, Some(base)) => (base + 1).min(matches.len() - 1),
            (false, Some(base)) => base.saturating_sub(1),
            (_, None) => 0,
        };
        form.country_active = Some(row);
        country_scroll_to(&form.country_scroll, row);
        cx.notify();
    }

    /// `f` of module 938401: the "Country Code" select beside the number box, and the note under them.
    fn phone_number_body(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let note = "Your phone number can be used to verify one Discord account at a time and is only used for verification and login.";
        let bold = "one Discord account";
        let start = note.find(bold).unwrap_or(0);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .items_start()
                    .child(
                        div()
                            .w(px(192.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Country Code")),
                            )
                            .child(self.country_select(form, cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).children(
                            form.fields
                                .first()
                                .map(|field| self.account_field_element(field, 0, cx)),
                        ),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(gpui::StyledText::new(note).with_highlights([(
                        start..start + bold.len(),
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(600.)),
                            ..Default::default()
                        },
                    )])),
            )
            .into_any_element()
    }

    /// The flag cell of an option (`leading_o2hjBM`, 32 square): on Windows the flag is the two letters.
    /// `closed`: the picked option in the field is drawn lower case and a little larger (golden of the closed field).
    fn country_flag(alpha2: &str, closed: bool) -> gpui::Div {
        div()
            .flex_none()
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .pt(px(2.))
            .text_size(px(if closed { 12. } else { 11. }))
            .font_weight(FontWeight(if closed { 500. } else { 600. }))
            .text_color(color("--text-subtle"))
            .child(if closed {
                flag_letters(alpha2).to_lowercase()
            } else {
                flag_letters(alpha2)
            })
    }

    /// The searchable select of module 890497 (`selectField_vWC4bk`): the picked option with its flag, or the search
    /// box while the list is open; the list shows five 40px rows.
    fn country_select(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let open = self.country_is_open();
        let current = &COUNTRIES[form.country];
        let query = Self::country_query(form, cx);
        let chevron = super::mask_icon("settings_select_chevron", 20., "--text-strong")
            .when(open, |this| {
                this.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
            });
        let field = div()
            .id("phone-country-field")
            .w(px(192.))
            .h(px(40.))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(if open {
                color("--input-border-active")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.country_toggle(cx)))
            .child(match (open, form.country_search.as_ref()) {
                (true, Some((search, _))) => div()
                    .relative()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .pl(px(12.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w(px(0.)).child(search.clone()))
                    // The box starts with the picked name in it; typing replaces it.
                    .when(query.is_empty(), |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(12.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-strong"))
                                .child(current.name),
                        )
                    })
                    .into_any_element(),
                _ => div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(Self::country_flag(current.alpha2, true))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(current.name),
                    )
                    .into_any_element(),
            })
            .child(
                div()
                    .flex_none()
                    .w(px(36.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(chevron),
            )
            .when(open, |this| this.child(focus_ring()));
        let list = open.then(|| {
            let matches = country_matches(&query);
            let rows: Vec<AnyElement> = matches
                .iter()
                .enumerate()
                .map(|(row, &option)| {
                    let country = &COUNTRIES[option];
                    let selected = option == form.country;
                    div()
                        .id(SharedString::from(format!("phone-country-{option}")))
                        .flex_none()
                        .w(px(166.))
                        .h(px(COUNTRY_ROW))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(form.country_active == Some(row), |this| {
                            this.bg(color("--background-mod-normal"))
                        })
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .flex()
                        .items_center()
                        .child(Self::country_flag(country.alpha2, false))
                        // The name has 98px beside the empty check cell, 94 beside the check (dump of the open list).
                        .child(
                            div()
                                .flex_none()
                                .ml(px(4.))
                                .w(px(if selected { 94. } else { 98. }))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-strong"))
                                .child(country.name),
                        )
                        .child(
                            div()
                                .flex_none()
                                .ml(px(8.))
                                .w(px(if selected { 28. } else { 24. }))
                                .p(px(4.))
                                .when(selected, |this| {
                                    this.child(icon("select_check", 20., "--text-strong"))
                                }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.country_pick(option, cx)))
                        .into_any_element()
                })
                .collect();
            gpui::deferred(
                crate::ui::keyboard::outside(
                    div().id("phone-country-list"),
                    crate::ui::keyboard::Layer::SettingsSelect,
                    cx,
                )
                .occlude()
                .absolute()
                .top(px(44.))
                .left_0()
                .w(px(192.))
                .p(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.16),
                    offset: gpui::point(px(0.), px(2.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .child(
                    div()
                        .relative()
                        .w(px(182.))
                        .child(
                            div()
                                .id("phone-country-rows")
                                .w(px(182.))
                                .max_h(px(COUNTRY_ROW * COUNTRY_ROWS))
                                .overflow_y_scroll()
                                .track_scroll(&form.country_scroll)
                                .child(div().flex().flex_col().children(rows)),
                        )
                        .child(super::auto_scrollbar(form.country_scroll.clone())),
                ),
            )
            .with_priority(2)
        });
        div()
            .relative()
            .flex_none()
            .w(px(192.))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if !this.country_is_open() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" => this.country_arrow(true, cx),
                    "up" => this.country_arrow(false, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(field)
            .children(list)
            .into_any_element()
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
            mfa: mfa::Draft::default(),
            country: default_country(),
            country_search: None,
            country_scroll: ScrollHandle::new(),
            country_active: None,
            country_focus: false,
            phone_typed: String::new(),
            focused: None,
        };
        let verified = self.stores.account.verified && self.stores.account.email.is_some();
        let has_phone = self.stores.account.phone.is_some();
        let step = match kind {
            "email" if verified => Step::EmailSend,
            "email" => Step::EmailAddress,
            "phone" if has_phone => Step::PhoneManage,
            "phone" => Step::PhoneNumber,
            "password" => Step::Password,
            "mfa-enable" => Step::MfaEnable,
            "mfa-remove" => Step::MfaRemove,
            "mfa-sms-remove" => Step::MfaSmsPassword,
            "mfa-codes" => Step::MfaCodesPassword,
            "mfa-key-register" => Step::KeyRegister,
            "mfa-key-rename" => Step::KeyRename,
            "mfa-key-delete" => Step::KeyDelete,
            "disable" | "delete" => {
                form.delete = kind == "delete";
                // `b3`: an owner of servers is told to transfer them first.
                if self.owns_servers() {
                    self.settings_extra.dialog = Some(super::actions::Dialog::Alert {
                        title: dc_intl::tr("You own servers!").to_owned(),
                        body: dc_intl::tr("In order to delete or disable your account you must first transfer ownership of all servers that you own.").to_owned(),
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
        let mut search = None;
        match step {
            Step::Username => {
                let unique = user.discriminator == "0" || user.discriminator.is_empty();
                let mut name = self.account_field("username", dc_intl::tr("Username"), false, cx);
                name.editor.update(cx, |e, _| e.set_text(&user.username));
                name.helper = unique.then_some(dc_intl::tr(
                    "Please only use numbers, letters, underscores _ , or periods.",
                ));
                name.width = (!unique).then_some(1.);
                fields.push(name);
                if !unique {
                    let mut tag = self.account_field(
                        "discriminator",
                        dc_intl::tr("Discriminator"),
                        false,
                        cx,
                    );
                    tag.editor
                        .update(cx, |e, _| e.set_text(&user.discriminator));
                    tag.leading = Some("#".into());
                    tag.width = Some(96.);
                    let _ = premium;
                    fields.push(tag);
                }
                fields.push(self.account_field(
                    "password",
                    dc_intl::tr("Current Password"),
                    true,
                    cx,
                ));
            }
            Step::Password => {
                fields.push(self.account_field(
                    "password",
                    dc_intl::tr("Current Password"),
                    true,
                    cx,
                ));
                fields.push(self.account_field(
                    "new_password",
                    dc_intl::tr("New Password"),
                    true,
                    cx,
                ));
                fields.push(self.account_field(
                    "confirm",
                    dc_intl::tr("Confirm New Password"),
                    true,
                    cx,
                ));
            }
            Step::EmailCode => {
                fields.push(self.account_field("code", dc_intl::tr("Verification Code"), false, cx))
            }
            Step::EmailAddress => {
                fields.push(self.account_field("email", dc_intl::tr("Email"), false, cx));
                fields.push(self.account_field(
                    "password",
                    dc_intl::tr("Current Password"),
                    true,
                    cx,
                ));
            }
            Step::PhoneNumber => {
                // `f` of module 938401: the country select, then the number box with the dialing code in front.
                let country = self
                    .settings_extra
                    .form
                    .as_ref()
                    .map_or_else(default_country, |form| form.country);
                let mut phone = self.account_field("phone", dc_intl::tr("Phone Number"), false, cx);
                phone.leading = Some(country_prefix(country).into());
                fields.push(phone);
                search = Some(self.country_search_editor(cx));
                self.setting_choices.insert(COUNTRY_OPEN, Choice::On(false));
            }
            Step::PhoneCode => fields.push(self.account_field(
                "code",
                dc_intl::tr("Verify Your Number"),
                false,
                cx,
            )),
            Step::PhonePassword | Step::PhoneRemove | Step::Remove => {
                fields.push(self.account_field("password", dc_intl::tr("Password"), true, cx));
            }
            Step::MfaSmsPassword | Step::MfaCodesPassword => {
                // `CIGa+7`.
                fields.push(self.account_field("password", dc_intl::tr("Password"), true, cx));
            }
            Step::MfaEnable => {
                // `HZPBOd` is the hidden label, `000 000` the placeholder, `maxLength: 7`.
                let field = self.account_field("code", "", false, cx);
                field
                    .editor
                    .update(cx, |e, _| e.set_placeholder("000 000".to_owned()));
                fields.push(field);
                if let Some(form) = self.settings_extra.form.as_mut() {
                    form.mfa.secret = mfa::new_secret();
                }
            }
            Step::MfaCodesKey => {
                // `TjGb4Q`, `maxLength: 8`.
                fields.push(self.account_field("code", dc_intl::tr("Verification key"), false, cx));
            }
            Step::MfaRemove | Step::MfaCodes | Step::KeyRegister | Step::KeyDelete => {}
            Step::KeyName => {
                // `Jzd+z/` over the box, which starts with `I/sJtJ`.
                let field = self.account_field(
                    "name",
                    dc_intl::tr("Choose a name for your new key"),
                    false,
                    cx,
                );
                field
                    .editor
                    .update(cx, |e, _| e.set_text(dc_intl::tr(mfa::KEY_DEFAULT_NAME)));
                fields.push(field);
            }
            Step::KeyRename => {
                // `7yEgJY` over the box; `placeholder: credential.name`, `maxLength: 32`.
                let field = self.account_field("name", dc_intl::tr("New Key Name"), false, cx);
                let current = self
                    .settings_extra
                    .form
                    .as_ref()
                    .map(|form| form.mfa.key_name.clone())
                    .unwrap_or_default();
                field.editor.update(cx, |e, _| e.set_placeholder(current));
                fields.push(field);
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
            form.country_search = search;
            form.phone_typed = String::new();
            form.notice = None;
            form.working = false;
        }
        cx.notify();
    }

    fn account_form_edited(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.step == Step::PhoneNumber && name == "phone" {
            // `y` of module 938401: `${code}${digits}` into the `phone` state that Send waits for.
            form.phone_typed = phone_number(COUNTRIES[form.country].code, &form.text("phone", cx));
        }
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
                    field.success = Some(dc_intl::tr(USERNAME_AVAILABLE).to_owned());
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
        if matches!(
            form.step,
            Step::KeyRegister | Step::KeyName | Step::KeyRename | Step::KeyDelete
        ) {
            let name = text("name");
            self.key_submit(name.trim().to_owned(), cx);
            return;
        }
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
                // Send is disabled while the `phone` state of module 938401 is empty.
                if form.phone_typed.is_empty() {
                    return;
                }
                form.phone = phone_number(COUNTRIES[form.country].code, &text("phone"));
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
            Step::MfaEnable => {
                let code: String = text("code").chars().filter(char::is_ascii_digit).collect();
                if code.is_empty() {
                    return;
                }
                mfa::totp_enable_call(&code, &form.mfa.secret)
            }
            Step::MfaRemove => mfa::totp_disable_call(),
            Step::MfaSmsPassword => {
                if text("password").is_empty() {
                    return;
                }
                mfa::sms_disable_call(&text("password"))
            }
            Step::MfaCodesPassword => {
                if text("password").is_empty() {
                    return;
                }
                form.mfa.password = text("password");
                mfa::backup_key_call(&form.mfa.password)
            }
            Step::MfaCodesKey => {
                if text("code").trim().is_empty() {
                    return;
                }
                form.mfa.key = text("code").trim().to_owned();
                mfa::backup_codes_call(&form.mfa.key, &form.mfa.view_nonce, false)
            }
            Step::EmailReason
            | Step::EmailScam
            | Step::EmailDone
            | Step::PhoneManage
            | Step::MfaCodes
            | Step::KeyRegister
            | Step::KeyName
            | Step::KeyRename
            | Step::KeyDelete => return,
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
        if matches!(
            form.step,
            Step::KeyRegister | Step::KeyName | Step::KeyRename | Step::KeyDelete
        ) {
            self.key_fetched(ok, answer, cx);
            return;
        }
        if !ok {
            form.last = Some(if body["refused"].as_bool() == Some(true) {
                "refused in this run".to_owned()
            } else {
                format!("status {}", body["status"])
            });
            if body["refused"].as_bool() == Some(true) {
                // A refused call never reaches the server, so the answer carries no body: module 615715 hands the
                // thrown error to `getAnyErrorMessage()` (module 319400), which falls back to the message of
                // module 181658 (`aTVNes`). A server answer shows its own field message or `message` instead
                // (invalid number, rate limit: the text is the server's, handled below).
                if form.step == Step::PhoneNumber {
                    form.set_error("phone", Some(PHONE_FAILURE.to_owned()));
                }
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
                    .unwrap_or(dc_intl::tr("Please try again"))
                    .to_owned();
                self.settings_extra.form = None;
                self.settings_extra.dialog = Some(super::actions::Dialog::Alert {
                    title: dc_intl::tr("Error Deleting Your Account").to_owned(),
                    body: message,
                });
                return;
            }
            if let Some(message) = answer["message"].as_str()
                && form.fields.iter().all(|f| f.error.is_none())
            {
                match step {
                    Step::PhoneCode
                    | Step::PhoneNumber
                    | Step::EmailCode
                    | Step::MfaEnable
                    | Step::MfaCodesKey
                    | Step::MfaSmsPassword
                    | Step::MfaCodesPassword => {
                        let name = match step {
                            Step::PhoneNumber => "phone",
                            Step::MfaSmsPassword | Step::MfaCodesPassword => "password",
                            _ => "code",
                        };
                        form.set_error(name, Some(message.to_owned()));
                    }
                    _ => form.notice = Some(message.to_owned()),
                }
            } else if matches!(step, Step::PhoneNumber | Step::PhoneCode)
                && form.fields.iter().all(|f| f.error.is_none())
            {
                // No field error and no `message` in the body (a 5xx page, say): module 181658's own text.
                form.set_error(
                    if step == Step::PhoneNumber {
                        "phone"
                    } else {
                        "code"
                    },
                    Some(PHONE_FAILURE.to_owned()),
                );
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
            // `MFA_ENABLE_SUCCESS`: the account has the authenticator app and its first backup codes.
            Step::MfaEnable => {
                self.stores.account.mfa_enabled = true;
                form.mfa.codes = mfa::codes_of(answer);
                let key = String::new();
                let mut stored = answer.clone();
                stored["key"] = json!(key);
                self.fetched.insert(mfa::CODES_KEY.to_owned(), stored);
                next = Some(Step::MfaCodes);
            }
            Step::MfaRemove => {
                self.stores.account.mfa_enabled = false;
                self.fetched.remove(mfa::CODES_KEY);
                self.settings_extra.form = None;
            }
            Step::MfaSmsPassword => {
                if let Some(user) = self.stores.current_user.as_mut() {
                    user.flags &= !mfa::SMS_FLAG;
                }
                self.settings_extra.form = None;
            }
            Step::MfaCodesPassword => {
                form.mfa.set_nonces(answer);
                next = Some(Step::MfaCodesKey);
            }
            Step::MfaCodesKey => {
                if answer["backup_codes"].is_array() {
                    // `MFA_VIEW_BACKUP_CODES`: the codes and the key that asked for them stay for the panel.
                    let mut stored = answer.clone();
                    stored["key"] = json!(form.mfa.key);
                    stored["regenerate_nonce"] = json!(form.mfa.regenerate_nonce);
                    self.fetched.insert(mfa::CODES_KEY.to_owned(), stored);
                    self.settings_extra.form = None;
                } else {
                    // The answer of "Resend it": a new key with new nonces.
                    form.mfa.set_nonces(answer);
                    form.mfa.resent = true;
                }
            }
            Step::EmailReason
            | Step::EmailScam
            | Step::EmailDone
            | Step::PhoneManage
            | Step::MfaCodes
            | Step::KeyRegister
            | Step::KeyName
            | Step::KeyRename
            | Step::KeyDelete => {}
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
        self.connection_dialog_focus(window, cx);
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        // The field with the focus gets the active border on the next frame.
        let focused = form
            .fields
            .iter()
            .find(|f| gpui::Focusable::focus_handle(f.editor.read(cx), cx).is_focused(window))
            .map(|f| f.name);
        if focused != form.focused {
            form.focused = focused;
            cx.notify();
        }
        if std::mem::take(&mut form.country_focus)
            && let Some((search, _)) = form.country_search.as_ref()
        {
            window.focus(&gpui::Focusable::focus_handle(search.read(cx), cx), cx);
            return;
        }
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
            modal::button(
                id,
                dc_intl::tr("Cancel"),
                modal::Tone::Secondary,
                |this, _, _| {
                    this.settings_extra.form = None;
                },
            )
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
                    dc_intl::tr("Change your username"),
                    Some(dc_intl::tr("Enter a new username and your existing password.").to_owned()),
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Done"), busy(modal::Tone::Primary))],
                ),
                Step::Password => (
                    dc_intl::tr("Update your password"),
                    Some(dc_intl::tr("Enter your current password and a new password.").to_owned()),
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Done"), busy(modal::Tone::Primary))],
                ),
                Step::EmailSend => (
                    dc_intl::tr("Verify email address"),
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
                        primary("account-ok", dc_intl::tr("Send Verification Code"), busy(modal::Tone::Primary)),
                    ],
                ),
                Step::EmailCode => (
                    dc_intl::tr("Enter code"),
                    Some(dc_intl::tr("Check your email: we sent you a verification code. Enter it here to verify you\u{2019}re really you.").to_owned()),
                    Vec::new(),
                    vec![primary("account-ok", dc_intl::tr("Next"), busy(modal::Tone::Primary))],
                ),
                Step::EmailReason => (
                    dc_intl::tr("Why are you changing your email?"),
                    None,
                    vec![self.email_reasons(form.reason, cx)],
                    vec![
                        cancel("account-cancel"),
                        if form.reason.is_some() {
                            modal::button("account-ok", dc_intl::tr("Continue"), modal::Tone::Primary, |this, _, cx| {
                                // `u.qr`: the first two reasons get the scam alert.
                                let next = match this.settings_extra.form.as_ref().and_then(|f| f.reason) {
                                    Some(0 | 1) => Step::EmailScam,
                                    _ => Step::EmailAddress,
                                };
                                this.account_form_step(next, cx);
                            })
                        } else {
                            modal::button("account-ok", dc_intl::tr("Continue"), modal::Tone::PrimaryDisabled, |_, _, _| {})
                        },
                    ],
                ),
                Step::EmailScam => (
                    dc_intl::tr("Potential scam alert!"),
                    None,
                    vec![div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(text("This sounds very similar to common scams going around! Changing your email address can give someone else access to your account - learn more about trending scams here.", false))
                        .child(text(dc_intl::tr("Discord will never ask you to change your email - report anyone who asks you to do this."), false))
                        .into_any_element()],
                    vec![
                        go("account-risk", dc_intl::tr("I know the risks, continue"), modal::Tone::Secondary, Step::EmailAddress),
                        cancel("account-cancel"),
                    ],
                ),
                Step::EmailAddress => (
                    dc_intl::tr("Enter an email address"),
                    Some(dc_intl::tr("Enter a new email address and your existing password.").to_owned()),
                    Vec::new(),
                    vec![
                        if form.email_token.is_some() {
                            go("account-back", dc_intl::tr("Back"), modal::Tone::Secondary, Step::EmailReason)
                        } else {
                            cancel("account-cancel")
                        },
                        primary("account-ok", dc_intl::tr("Done"), busy(modal::Tone::Primary)),
                    ],
                ),
                Step::EmailDone => (
                    dc_intl::tr("Confirm your new email"),
                    None,
                    vec![div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(text(
                            &format!("To finish, we sent a verification email to: {}", form.new_email),
                            false,
                        ))
                        .child(text(dc_intl::tr("Didn't get the email? Make sure to check your spam folder."), false))
                        .into_any_element()],
                    vec![modal::button("account-ok", dc_intl::tr("Okay"), modal::Tone::Primary, |this, _, _| {
                        this.settings_extra.form = None;
                    })],
                ),
                Step::PhoneNumber => (
                    dc_intl::tr("Enter a phone number"),
                    Some(dc_intl::tr("You will receive a text message with a verification code.").to_owned()),
                    vec![self.phone_number_body(form, cx)],
                    vec![primary(
                        "account-ok",
                        dc_intl::tr("Send"),
                        if form.phone_typed.is_empty() {
                            modal::Tone::PrimaryDisabled
                        } else {
                            busy(modal::Tone::Primary)
                        },
                    )],
                ),
                Step::PhoneCode => (
                    dc_intl::tr("Verify your phone number"),
                    Some(dc_intl::tr("Enter the 6 digit code we sent to your phone.").to_owned()),
                    Vec::new(),
                    vec![modal::button("account-resend", dc_intl::tr("Resend Code"), modal::Tone::Secondary, |this, _, cx| {
                        let Some(form) = this.settings_extra.form.as_ref() else {
                            return;
                        };
                        let _ = this.requests.send(phone_begin_call(&form.phone).request());
                        cx.notify();
                    })],
                ),
                Step::PhonePassword => (
                    dc_intl::tr("Enter your password to confirm changes"),
                    None,
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Confirm"), busy(modal::Tone::Primary))],
                ),
                Step::PhoneManage => (
                    dc_intl::tr("Edit your phone number"),
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
                            buttons.push(go("account-remove", dc_intl::tr("Remove"), modal::Tone::CriticalSecondary, Step::PhoneRemove));
                        }
                        buttons.push(go("account-edit", dc_intl::tr("Edit"), modal::Tone::Primary, Step::PhoneNumber));
                        buttons
                    },
                ),
                Step::PhoneRemove => (
                    dc_intl::tr("Remove phone number"),
                    None,
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Remove"), modal::Tone::Critical)],
                ),
                Step::Remove => (
                    if form.delete { dc_intl::tr("Delete account") } else { dc_intl::tr("Disable account") },
                    None,
                    vec![self.warning_notice(if form.delete {
                        dc_intl::tr("Are you sure that you want to delete your account? This will immediately log you out of your account and you will not be able to log in again.")
                    } else {
                        dc_intl::tr("Are you sure that you want to disable your account? This will immediately log you out and make your account inaccessible to anyone.")
                    })],
                    vec![
                        cancel("account-cancel"),
                        primary(
                            "account-ok",
                            if form.delete { dc_intl::tr("Delete Account") } else { dc_intl::tr("Disable Account") },
                            modal::Tone::Critical,
                        ),
                    ],
                ),
                // Module 333737: the three steps, the last with the code field and its Activate button.
                Step::MfaEnable => (
                    dc_intl::tr("Enable Authenticator App"),
                    Some(dc_intl::tr("Make your account safer in 3 easy steps:").to_owned()),
                    vec![self.mfa_enable_body(form, cx)],
                    Vec::new(),
                ),
                // `bn`: `D+aE7g` over `EA4ZEk`, a critical `Remove`.
                Step::MfaRemove => (
                    dc_intl::tr("Remove Authenticator App"),
                    // `xlA7Tl` or `EA4ZEk` by the security keys of the account (module 705147).
                    Some(
                        mfa::remove_text(
                            self.fetched
                                .get(mfa::CREDENTIALS_KEY)
                                .and_then(Value::as_array)
                                .map_or(0, Vec::len),
                        )
                        .to_owned(),
                    ),
                    Vec::new(),
                    vec![
                        cancel("account-cancel"),
                        primary("account-ok", dc_intl::tr("Remove"), busy(modal::Tone::Critical)),
                    ],
                ),
                // `KLWnit` over `W0/Duf` in the warning notice of module 662758.
                Step::MfaSmsPassword => (
                    dc_intl::tr("Remove SMS Authentication"),
                    None,
                    vec![self.warning_notice(
                        dc_intl::tr("This removes the ability to log in with an authentication code through SMS. Are you sure you want to remove this?"),
                    )],
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Confirm"), busy(modal::Tone::Primary))],
                ),
                Step::MfaCodesPassword => (
                    dc_intl::tr("Enter Your Password"),
                    None,
                    Vec::new(),
                    vec![cancel("account-cancel"), primary("account-ok", dc_intl::tr("Next"), busy(modal::Tone::Primary))],
                ),
                // Module 518142.
                Step::MfaCodesKey => (
                    dc_intl::tr("Enter Verification Key"),
                    None,
                    vec![text(
                        dc_intl::tr("As an added security measure, please enter the key we just sent to your email address. You should see it in a minute or two."),
                        false,
                    )
                    .into_any_element()],
                    vec![
                        cancel("account-cancel"),
                        primary("account-ok", dc_intl::tr("Submit"), busy(modal::Tone::Primary)),
                    ],
                ),
                // Module 549404 `renderTotp`.
                Step::MfaCodes => (
                    dc_intl::tr("2FA is activated! \u{1f389}"),
                    Some("Here is something you should do!".to_owned()),
                    vec![self.mfa_codes_body(form, cx)],
                    Vec::new(),
                ),
                // Module 245329 `INIT`: `vrOCCk` (`wePEBF` while the prompt is open) over `Lh5vTW` (`aVMiX3`), the
                // critical notice of a failed attempt above, and `oibaQa` as the one button.
                Step::KeyRegister => (
                    if form.mfa.waiting {
                        dc_intl::tr("Interact with your security key")
                    } else {
                        dc_intl::tr("Register a Security Key")
                    },
                    None,
                    {
                        let mut parts = Vec::new();
                        if let Some(error) = form.mfa.error.as_deref() {
                            parts.push(self.critical_notice(error));
                        }
                        parts.push(
                            text(
                                if form.mfa.waiting {
                                    dc_intl::tr("You may need to tap your device or confirm a prompt.")
                                } else {
                                    dc_intl::tr("When you are ready to authenticate, press the button below.")
                                },
                                false,
                            )
                            .into_any_element(),
                        );
                        parts
                    },
                    vec![modal::button(
                        "account-ok",
                        dc_intl::t!("oibaQa"),
                        if form.working || form.mfa.waiting || form.mfa.challenge.is_empty() {
                            modal::Tone::PrimaryLoading
                        } else {
                            modal::Tone::Primary
                        },
                        |this, window, cx| {
                            let parent = crate::webauthn::parent(window);
                            this.key_begin(parent, cx)
                        },
                    )],
                ),
                // Module 245329 `NAME`: `cY/IOu`, the box under `Jzd+z/` and `5dyZ1S`.
                Step::KeyName => (
                    dc_intl::tr("Name your security key"),
                    None,
                    Vec::new(),
                    vec![primary(
                        "account-ok",
                        dc_intl::tr("Finish"),
                        if form
                            .fields
                            .iter()
                            .all(|f| f.name != "name" || f.editor.read(cx).text().trim().is_empty())
                        {
                            modal::Tone::PrimaryDisabled
                        } else if form.working {
                            modal::Tone::PrimaryLoading
                        } else {
                            modal::Tone::Primary
                        },
                    )],
                ),
                // Module 428726: `YjuMsf`, the `7yEgJY` box (at most 32 characters) and `ETE/oC` / `i4jeWR`.
                Step::KeyRename => (
                    dc_intl::tr("Rename your security key"),
                    None,
                    Vec::new(),
                    vec![
                        cancel("account-cancel"),
                        primary(
                            "account-ok",
                            dc_intl::tr("Done"),
                            {
                                let typed = form
                                    .fields
                                    .iter()
                                    .find(|f| f.name == "name")
                                    .map_or(0, |f| f.editor.read(cx).text().chars().count());
                                if typed == 0 || typed > 32 {
                                    modal::Tone::PrimaryDisabled
                                } else {
                                    busy(modal::Tone::Primary)
                                }
                            },
                        ),
                    ],
                ),
                // Module 41000: `J/x6ns` over `IMKkcn`, a critical `+xgS+L`.
                Step::KeyDelete => (
                    dc_intl::tr("This is your last Passkey"),
                    Some(dc_intl::tr(mfa::LAST_KEY_TEXT).to_owned()),
                    Vec::new(),
                    vec![
                        cancel("account-cancel"),
                        primary("account-ok", dc_intl::tr("Delete"), busy(modal::Tone::Critical)),
                    ],
                ),
            };
        // Fields: a column, or the username and its tag in one row.
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut inline: Vec<AnyElement> = Vec::new();
        for (index, field) in form
            .fields
            .iter()
            .enumerate()
            .filter(|_| !matches!(form.step, Step::MfaEnable | Step::PhoneNumber))
        {
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
        if form.step == Step::MfaCodesKey {
            if form.mfa.resent {
                // `j4qu8n`.
                rows.push(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-feedback-positive"))
                        .child(dc_intl::tr(
                            "Verification email resent, please check your email.",
                        ))
                        .into_any_element(),
                );
            }
            rows.push(
                div()
                    .id("mfa-resend-key")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-link"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.settings_extra.form.as_ref() {
                            let _ = this.requests.send(
                                mfa::backup_key_call(&form.mfa.password).request_in(FORM_KEY),
                            );
                        }
                        cx.notify();
                    }))
                    .child(dc_intl::t!("PZgmxv"))
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
                    .child(dc_intl::tr(
                        "Didn\u{2019}t receive a code or it expired? Resend it.",
                    ))
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
        // Module 549404 `handleCloseModal`: until the codes were downloaded the close button asks first.
        if form.step == Step::MfaCodes && form.mfa.skip_ask {
            let back = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
                if let Some(form) = this.settings_extra.form.as_mut() {
                    form.mfa.skip_ask = false;
                }
            };
            return Some(modal::render(
                modal::Spec {
                    id: "settings-mfa-skip",
                    title: SharedString::from(dc_intl::tr("Continue without Backup")),
                    subtitle: None,
                    body: Some(text(dc_intl::tr(mfa::SKIP_BACKUP_TEXT), false).into_any_element()),
                    buttons: vec![
                        modal::button(
                            "mfa-skip-cancel",
                            dc_intl::tr("Cancel"),
                            modal::Tone::Secondary,
                            back,
                        ),
                        modal::button(
                            "mfa-skip-ok",
                            dc_intl::t!("MwSEoz"),
                            modal::Tone::Critical,
                            |this, _, _| {
                                // `MFA_SEEN_BACKUP_CODE_PROMPT`, then the dialog closes.
                                mfa::mark_prompt_seen();
                                this.settings_extra.form = None;
                            },
                        ),
                    ],
                    close: Box::new(back),
                },
                cx,
            ));
        }
        let subtitle_element = if form.step == Step::MfaCodes {
            // `/Nhi8R`: "Here is something you <b>should do</b>!".
            let text = "Here is something you should do!";
            let start = "Here is something you ".len();
            Some(
                gpui::StyledText::new(text)
                    .with_highlights([(
                        start..start + "should do".len(),
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(600.)),
                            ..Default::default()
                        },
                    )])
                    .into_any_element(),
            )
        } else {
            subtitle.map(|s| div().child(s).into_any_element())
        };
        let spec = modal::Spec {
            id: "settings-account-dialog",
            title: SharedString::from(title.to_owned()),
            subtitle: subtitle_element,
            body,
            buttons,
            close: Box::new(|this, _, _| this.settings_extra.close_form()),
        };
        Some(if form.step == Step::PhoneCode {
            modal::render_small(spec, cx)
        } else {
            modal::render(spec, cx)
        })
    }

    /// `b` of module 333737 with `section_uV72M-`: the image column (156 wide, the image centred in it), 20px gap and
    /// 20px below; beside it a column with the heading (`heading-xs/semibold`, 14/18, `--text-strong`), the text and
    /// what goes under it.
    fn mfa_section(
        &self,
        image: AnyElement,
        label: &'static str,
        text: AnyElement,
        children: Vec<AnyElement>,
    ) -> AnyElement {
        div()
            .flex()
            .gap(px(20.))
            .mb(px(20.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w(px(156.))
                    .items_center()
                    .justify_center()
                    .child(image),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .child(text)
                    .children(children),
            )
            .into_any_element()
    }

    /// `d.c` with `divider_uV72M-`: a 1px `--border-subtle` rule with 20px above and below; the section's own 20px
    /// above it is the same margin (they collapse), so only the 20px below is added.
    fn mfa_rule(&self) -> AnyElement {
        div()
            .mb(px(20.))
            .h(px(1.))
            .w_full()
            .bg(color("--border-subtle"))
            .into_any_element()
    }

    /// Body text (`body-md/normal`, 16/20, `--text-default`) whose linked runs open their address.
    fn mfa_links(&self, id: &'static str, parts: &[(&str, Option<&'static str>)]) -> AnyElement {
        let mut text = String::new();
        let mut highlights = Vec::new();
        let mut urls = Vec::new();
        for (part, url) in parts {
            let start = text.len();
            text.push_str(part);
            if let Some(url) = url {
                highlights.push((
                    start..text.len(),
                    gpui::HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..Default::default()
                    },
                ));
                urls.push((start..text.len(), *url));
            }
        }
        let ranges: Vec<_> = urls.iter().map(|(range, _)| range.clone()).collect();
        div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(
                gpui::InteractiveText::new(
                    SharedString::from(id),
                    gpui::StyledText::new(text).with_highlights(highlights),
                )
                .on_click(ranges, move |at, _, cx| {
                    if let Some((_, url)) = urls.get(at) {
                        cx.open_url(url);
                    }
                }),
            )
            .into_any_element()
    }

    /// A `sm` button of the design system (32 high): `family` is the token family (`connected`, `secondary`).
    fn mfa_small_button(
        id: &'static str,
        label: &'static str,
        family: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_none()
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(&format!("--control-{family}-border-default")))
            .bg(color(&format!("--control-{family}-background-default")))
            .hover(|style| {
                style
                    .bg(color(&format!("--control-{family}-background-hover")))
                    .border_color(color(&format!("--control-{family}-border-hover")))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color(&format!("--control-{family}-text-default")))
            .whitespace_nowrap()
            .on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
            .child(label)
            .into_any_element()
    }

    /// Module 333737 `render`: download an app, scan the code, log in with it. Images: step one's picture
    /// (`c0a09bd4b521cbd2.png`, 100x100) and step three's (`2e11aacc339bafc1.svg`) are not in the snapshot's assets,
    /// so their places stay empty.
    fn mfa_enable_body(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let body = |text: &str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(text.to_owned())
                .into_any_element()
        };
        let email = self.stores.account.email.clone().unwrap_or_default();
        // `g.Ay` without a size: 128, level M, `qrCodeContainer_d4-PpI` (`--space-8` padding, `--radius-xs`).
        let qr = crate::ui::login::qr_code_card(
            &mfa::otpauth_url(&email, &form.mfa.secret),
            128.,
            8.,
            4.,
        )
        .unwrap_or_else(|| div().size(px(128.)).into_any_element());
        let field = form
            .fields
            .first()
            .map(|field| self.account_field_element(field, 0, cx));
        let activate = div()
            .id("mfa-activate")
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.account_form_submit(cx)))
            .child(super::button(
                "mfa-activate-button",
                dc_intl::tr("Activate"),
                if form.working {
                    super::ButtonKind::Secondary
                } else {
                    super::ButtonKind::Primary
                },
            ));
        div()
            .flex()
            .flex_col()
            .child(self.mfa_section(
                div().size(px(100.)).into_any_element(),
                dc_intl::tr("Download an authenticator app"),
                self.mfa_links(
                    "mfa-apps",
                    &[
                        ("Download and install ", None),
                        (dc_intl::tr("Authy"), Some("https://www.authy.com/")),
                        (" or ", None),
                        (
                            dc_intl::tr("Google Authenticator"),
                            Some("https://support.google.com/accounts/answer/1066447?hl=en"),
                        ),
                        (" for your phone or tablet.", None),
                    ],
                ),
                Vec::new(),
            ))
            .child(self.mfa_rule())
            .child(self.mfa_section(
                qr,
                dc_intl::tr("Scan the QR code"),
                body(dc_intl::t!("hFeBkl")),
                // `topSpacing_uV72M-`: 20 above the manual key, whose value is `secret_uV72M-` (`--font-code`).
                vec![div()
                    .mt(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("2FA Key (Manual entry)")),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_family("Consolas")
                            .text_color(color("--text-default"))
                            .child(form.mfa.secret.clone()),
                    )
                    .into_any_element()],
            ))
            .child(self.mfa_rule())
            .child(self.mfa_section(
                div().size(px(100.)).into_any_element(),
                dc_intl::tr("Log in with your code"),
                body(dc_intl::tr(
                    "Enter the 6-digit verification code generated.",
                )),
                // `inputContainer_uV72M-`: the field and the button in a row, 8 apart and 8 below the text.
                vec![div()
                    .mt(px(8.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(0.)).children(field))
                    .child(activate)
                    .into_any_element()],
            ))
            .into_any_element()
    }

    /// Module 549404 `renderTotp`: the passkey section (only while the account has no security key), a divider, and
    /// the backup codes download. `f` is `modalSection_XAorGN` (column) with a `heading-sm/semibold` label, the text
    /// in `sectionBody_XAorGN` (`--text-muted`, 20 below) and the button under it. The dialog has no footer
    /// (`actions: []`): it closes through its close button, which asks first until the codes were downloaded.
    fn mfa_codes_body(&self, form: &Form, cx: &mut Context<Self>) -> AnyElement {
        let email = self.stores.account.email.clone().unwrap_or_default();
        let contents = mfa::backup_file(&email, &form.mfa.codes);
        let has_keys = self
            .fetched
            .get(mfa::CREDENTIALS_KEY)
            .and_then(Value::as_array)
            .is_some_and(|list| !list.is_empty());
        let label = |text: &'static str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let muted = || {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .mb(px(20.))
                .text_color(color("--text-muted"))
        };
        let passkey = (!has_keys).then(|| {
            div()
                .flex()
                .flex_col()
                .child(label(dc_intl::tr("Add a Passkey")))
                .child(muted().child(
                    dc_intl::tr("Skip passwords entirely. Sign in with your face, fingerprint, device PIN, or password manager for the ultimate convenience and security."),
                ))
                .child(muted().child(self.mfa_links(
                    "mfa-passkey-learn",
                    &[(
                        "Learn more about Passkeys",
                        Some("https://support.discord.com/hc/en-us/articles/219576828"),
                    )],
                )))
                .child(div().flex().child(Self::mfa_small_button(
                    "mfa-add-passkey",
                    dc_intl::tr("Add a Passkey"),
                    "connected",
                    cx,
                    // `handleAddPasskey`: the dialog closes and the key registration starts (module 665671).
                    |this, cx| {
                        this.settings_extra.form = None;
                        this.security_key_register(cx);
                    },
                )))
                .into_any_element()
        });
        let download = Self::mfa_small_button(
            "mfa-download-codes",
            dc_intl::tr("Download Backup Codes"),
            "connected",
            cx,
            move |this, cx| {
                this.mfa_save_codes(&contents, cx);
                // `onDownload`: `MFA_SEEN_BACKUP_CODE_PROMPT`.
                mfa::mark_prompt_seen();
            },
        );
        div()
            .flex()
            .flex_col()
            .children(passkey)
            .children((!has_keys).then(|| self.mfa_rule()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(label(dc_intl::tr("Download Backup Codes")))
                    .child(muted().child("Without your backup codes, you are at risk of losing your account permanently if you lose access to your authenticator! Save these somewhere safe on and off your device to reduce the risk of losing your account!"))
                    .child(div().flex().child(download)),
            )
            .into_any_element()
    }

    /// A critical notice (`inlinenotice` of type `critical`), as the register dialog shows a failed attempt.
    fn critical_notice(&self, message: &str) -> AnyElement {
        div()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-critical"))
            .bg(color("--background-feedback-mod-critical"))
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-strong"))
            .child(message.to_owned())
            .into_any_element()
    }

    /// "Add Security Key" (module 665671): an empty POST asks for the ticket and the challenge, and the register
    /// dialog opens on them. The dialog shows at once with its button waiting; a refused request closes it with the
    /// critical toast `xSCvBf` (the official client opens it only once the answer is in).
    pub(in crate::ui) fn security_key_register(&mut self, cx: &mut Context<Self>) {
        self.open_account_form("mfa-key-register", cx);
        if let Some(form) = self.settings_extra.form.as_mut() {
            form.working = true;
        }
        let _ = self.requests.send(mfa::key_start_request(FORM_KEY));
        cx.notify();
    }

    /// The register dialog's button: the challenge goes to the system's prompt over the window `parent`.
    pub(in crate::ui) fn key_begin(&mut self, parent: isize, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.step != Step::KeyRegister
            || form.mfa.waiting
            || form.working
            || form.mfa.challenge.is_empty()
        {
            return;
        }
        form.mfa.waiting = true;
        form.mfa.error = None;
        let answer = crate::webauthn::start(
            crate::webauthn::Kind::Create,
            parent,
            form.mfa.challenge.clone(),
        );
        cx.spawn(async move |this, cx| {
            let answer = answer.await.unwrap_or(Err(dc_webauthn::Error::Cancelled));
            this.update(cx, |this, cx| this.key_prompt_answered(answer, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// The prompt closed (`H` of module 245329): the credential moves the dialog to the name step, anything else
    /// leaves `xSCvBf` on the first one.
    fn key_prompt_answered(&mut self, answer: crate::webauthn::Answer, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        // A dialog closed (and maybe opened again) while the prompt was open is not the one that asked.
        if form.step != Step::KeyRegister || !form.mfa.waiting {
            return;
        }
        form.mfa.waiting = false;
        match answer {
            Ok(credential) => {
                form.mfa.credential = credential;
                self.account_form_step(Step::KeyName, cx);
            }
            Err(_) => form.mfa.error = Some(dc_intl::tr(mfa::KEY_PROMPT_FAILED).to_owned()),
        }
        cx.notify();
    }

    /// The primary button of the key dialogs.
    fn key_submit(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        if form.working {
            return;
        }
        match form.step {
            Step::KeyName => {
                if name.is_empty() {
                    return;
                }
                form.mfa.key_name = name.clone();
                form.working = true;
                let _ = self.requests.send(mfa::key_register_request(
                    FORM_KEY,
                    &name,
                    &form.mfa.ticket,
                    &form.mfa.credential,
                ));
            }
            Step::KeyRename => {
                let length = name.chars().count();
                if length == 0 || length > 32 {
                    return;
                }
                form.working = true;
                let _ =
                    self.requests
                        .send(mfa::key_rename_request(FORM_KEY, &form.mfa.key_id, &name));
                form.mfa.key_name = name;
            }
            Step::KeyDelete => {
                form.working = true;
                let _ = self
                    .requests
                    .send(mfa::key_delete_request(FORM_KEY, &form.mfa.key_id));
            }
            _ => {}
        }
        cx.notify();
    }

    /// The answer of the request a key dialog sent.
    fn key_fetched(&mut self, ok: bool, answer: &Value, cx: &mut Context<Self>) {
        let Some(form) = self.settings_extra.form.as_mut() else {
            return;
        };
        let message = answer["message"]
            .as_str()
            .unwrap_or(dc_intl::tr(mfa::KEY_REGISTER_FAILED))
            .to_owned();
        match form.step {
            Step::KeyRegister => {
                let ticket = answer["ticket"].as_str();
                let challenge = answer["challenge"].as_str();
                match (ok, ticket, challenge) {
                    (true, Some(ticket), Some(challenge)) => {
                        form.mfa.ticket = ticket.to_owned();
                        form.mfa.challenge = challenge.to_owned();
                    }
                    _ => {
                        // `xSCvBf` as a critical toast, and no dialog.
                        self.settings_extra.form = None;
                        self.show_app_toast(dc_intl::tr(mfa::KEY_PROMPT_FAILED), cx);
                    }
                }
            }
            Step::KeyName if !ok => {
                // The register request failed: back to the first step with `fEptJP`, the draft kept.
                form.mfa.error = Some(dc_intl::tr(mfa::KEY_REGISTER_FAILED).to_owned());
                self.account_form_step(Step::KeyRegister, cx);
            }
            Step::KeyName => {
                // `AUTHENTICATOR_CREATE` and `MFA_ENABLE_SUCCESS`: the key joins the list, two-factor is on and the
                // answer's backup codes are the account's codes now.
                let mut entry = answer.clone();
                if let Some(object) = entry.as_object_mut() {
                    object.remove("backup_codes");
                }
                self.key_listed(entry);
                self.stores.account.mfa_enabled = true;
                if answer["backup_codes"].is_array() {
                    self.fetched.insert(
                        mfa::CODES_KEY.to_owned(),
                        json!({"backup_codes": answer["backup_codes"], "key": ""}),
                    );
                }
                self.settings_extra.form = None;
            }
            Step::KeyRename if !ok => form.set_error("name", Some(message)),
            Step::KeyRename => {
                let id = form.mfa.key_id.clone();
                let name = answer["name"]
                    .as_str()
                    .map_or_else(|| form.mfa.key_name.clone(), str::to_owned);
                self.key_renamed(&id, &name);
                self.settings_extra.form = None;
            }
            Step::KeyDelete if !ok => form.notice = Some(message),
            Step::KeyDelete => {
                let id = form.mfa.key_id.clone();
                self.key_forget(&id);
                // The question only comes up when no authenticator app stands behind the passkey.
                self.stores.account.mfa_enabled = false;
                self.settings_extra.form = None;
            }
            _ => {}
        }
        cx.notify();
    }

    /// `AUTHENTICATOR_CREATE`: the store keeps a key once.
    fn key_listed(&mut self, entry: Value) {
        let id = mfa::key_id_of(&entry);
        let list = self
            .fetched
            .entry(mfa::CREDENTIALS_KEY.to_owned())
            .or_insert_with(|| json!([]));
        if let Some(list) = list.as_array_mut()
            && !list.iter().any(|known| mfa::key_id_of(known) == id)
        {
            list.push(entry);
        }
    }

    /// `AUTHENTICATOR_UPDATE`: the name the server kept.
    fn key_renamed(&mut self, id: &str, name: &str) {
        if let Some(list) = self
            .fetched
            .get_mut(mfa::CREDENTIALS_KEY)
            .and_then(Value::as_array_mut)
        {
            for entry in list.iter_mut().filter(|e| mfa::key_id_of(e) == id) {
                entry["name"] = json!(name);
            }
        }
    }

    /// `AUTHENTICATOR_DELETE`.
    pub(in crate::ui) fn key_forget(&mut self, id: &str) {
        if let Some(list) = self
            .fetched
            .get_mut(mfa::CREDENTIALS_KEY)
            .and_then(Value::as_array_mut)
        {
            list.retain(|entry| mfa::key_id_of(entry) != id);
        }
    }

    /// The options menu's "Edit" (module 41000 opens module 428726).
    #[cfg_attr(
        not(feature = "capture"),
        expect(
            dead_code,
            reason = "called by the per-key options menu, which is not wired yet"
        )
    )]
    pub(in crate::ui) fn security_key_rename(
        &mut self,
        id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.open_account_form("mfa-key-rename", cx);
        if let Some(form) = self.settings_extra.form.as_mut() {
            form.mfa.key_id = id;
            form.mfa.key_name = name;
        }
        self.account_form_step(Step::KeyRename, cx);
    }

    /// The options menu's "Delete": the last passkey of an account asks first, any other goes at once.
    #[cfg_attr(
        not(feature = "capture"),
        expect(
            dead_code,
            reason = "called by the per-key options menu, which is not wired yet"
        )
    )]
    pub(in crate::ui) fn security_key_delete(&mut self, id: String, cx: &mut Context<Self>) {
        let keys = self
            .fetched
            .get(mfa::CREDENTIALS_KEY)
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        // Whether an authenticator app stands behind the keys is not known here (READY's `authenticator_types` is
        // not kept), so a single key always asks.
        if mfa::last_key_needs_question(keys, false) {
            self.open_account_form("mfa-key-delete", cx);
            if let Some(form) = self.settings_extra.form.as_mut() {
                form.mfa.key_id = id;
            }
        } else {
            let key = format!("{}delete:{id}", mfa::KEY_PREFIX);
            let _ = self.requests.send(mfa::key_delete_request(&key, &id));
        }
        cx.notify();
    }

    /// Capture only, the harness's `mfa <state>` steps: every dialog of the two-factor flow with the answers of the
    /// service injected, so nothing is sent and no password is typed. The codes are made-up placeholders.
    /// `enable`, `remove`, `sms-remove`, `codes-password`, `codes-key`, `codes-key-resent`, `activated`,
    /// `activated-seen`, `panel` (the nested panel with codes and an authenticator), `panel-sms`, `keys` (one security
    /// key listed, which changes the removal text and hides the passkey section).
    #[cfg(feature = "capture")]
    pub(crate) fn script_mfa(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let placeholder_codes = json!({"backup_codes": [
            {"code": "ab12cd34", "consumed": false},
            {"code": "ef56gh78", "consumed": false},
            {"code": "ij90kl12", "consumed": true},
            {"code": "mn34op56", "consumed": false},
        ]});
        match words {
            ["enable"] => self.open_account_form("mfa-enable", cx),
            ["remove"] => self.open_account_form("mfa-remove", cx),
            ["sms-remove"] => self.open_account_form("mfa-sms-remove", cx),
            ["codes-password"] => self.open_account_form("mfa-codes", cx),
            ["codes-key"] | ["codes-key-resent"] => {
                self.open_account_form("mfa-codes", cx);
                self.account_form_step(Step::MfaCodesKey, cx);
                if let Some(form) = self.settings_extra.form.as_mut() {
                    form.mfa.resent = words[0] == "codes-key-resent";
                }
            }
            ["activated"] | ["activated-seen"] => {
                self.open_account_form("mfa-enable", cx);
                self.stores.account.mfa_enabled = true;
                if let Some(form) = self.settings_extra.form.as_mut() {
                    form.mfa.codes = mfa::codes_of(&placeholder_codes);
                }
                self.account_form_step(Step::MfaCodes, cx);
                if words[0] == "activated-seen" {
                    mfa::mark_prompt_seen();
                } else {
                    mfa::forget_prompt_seen();
                }
            }
            ["keys"] => {
                self.fetched.insert(
                    mfa::CREDENTIALS_KEY.to_owned(),
                    json!([{"id": "100", "type": 2, "name": "Placeholder key", "last_used": null}]),
                );
            }
            // Two keys, one used a while ago: the list with its options buttons.
            ["keys2"] => {
                let used = (chrono::Utc::now() - chrono::Duration::days(3)).to_rfc3339();
                self.fetched.insert(
                    mfa::CREDENTIALS_KEY.to_owned(),
                    json!([
                        {"id": "100", "type": 2, "name": "Placeholder key", "last_used": null},
                        {"id": "101", "type": 2, "name": "Second placeholder", "last_used": used}
                    ]),
                );
                self.stores.account.mfa_enabled = true;
                self.stores.account.verified = true;
                self.settings_extra.panel = Some(super::account_panels::Panel::Mfa);
            }
            // The register dialog on a made-up challenge: nothing is asked of the service.
            ["key-start"] | ["key-wait"] | ["key-error"] | ["key-name"] => {
                self.open_account_form("mfa-key-register", cx);
                if let Some(form) = self.settings_extra.form.as_mut() {
                    form.mfa.ticket = "placeholder-ticket".to_owned();
                    form.mfa.challenge = json!({"publicKey": {
                        "challenge": "Zm9vYmFy",
                        "rp": {"id": "discord.com", "name": "Discord"},
                        "user": {"id": "AQID", "name": "tester", "displayName": "tester"},
                        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
                    }})
                    .to_string();
                    form.working = false;
                    if words[0] == "key-error" {
                        form.mfa.error = Some(dc_intl::tr(mfa::KEY_PROMPT_FAILED).to_owned());
                    }
                }
                match words[0] {
                    "key-wait" => self.key_begin(0, cx),
                    "key-name" => {
                        if let Some(form) = self.settings_extra.form.as_mut() {
                            form.mfa.credential = "{}".to_owned();
                        }
                        self.account_form_step(Step::KeyName, cx);
                    }
                    _ => {}
                }
            }
            ["key-rename"] => {
                self.security_key_rename("100".to_owned(), "Placeholder key".to_owned(), cx);
            }
            ["key-delete"] => {
                self.fetched.insert(
                    mfa::CREDENTIALS_KEY.to_owned(),
                    json!([{"id": "100", "type": 2, "name": "Placeholder key", "last_used": null}]),
                );
                self.security_key_delete("100".to_owned(), cx);
            }
            ["panel"] | ["panel-sms"] => {
                self.stores.account.mfa_enabled = true;
                self.stores.account.verified = true;
                if words[0] == "panel-sms" {
                    if let Some(user) = self.stores.current_user.as_mut() {
                        user.flags |= mfa::SMS_FLAG;
                    }
                    self.stores.account.phone = Some("+15555550100".to_owned());
                }
                self.fetched
                    .insert(mfa::CODES_KEY.to_owned(), placeholder_codes);
                self.settings_extra.panel = Some(super::account_panels::Panel::Mfa);
            }
            _ => return "usage: mfa enable|remove|sms-remove|codes-password|codes-key|activated|keys|panel".to_owned(),
        }
        cx.notify();
        "ok".to_owned()
    }

    /// `getDownloadFileContents`: the codes are offered as `discord_backup_codes.txt` through the save dialog, which
    /// opens in the Downloads folder (the same path the other downloads take).
    pub(in crate::ui) fn mfa_save_codes(&mut self, contents: &str, cx: &mut Context<Self>) {
        // Harness runs have no one to answer the dialog: they write straight into the export folder.
        #[cfg(feature = "capture")]
        if let Some(folder) = std::env::var_os("GRAVECORD_EXPORT_DIR").map(std::path::PathBuf::from)
        {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("discord_backup_codes.txt"), contents);
            cx.notify();
            return;
        }
        let directory = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Downloads"))
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some("discord_backup_codes.txt"));
        let contents = contents.to_owned();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = chosen.await {
                let _ = std::fs::write(path, contents);
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    pub(in crate::ui) fn warning_notice(&self, message: &'static str) -> AnyElement {
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
                            .child(dc_intl::tr(label)),
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
        let focused = self
            .settings_extra
            .form
            .as_ref()
            .is_some_and(|form| form.focused == Some(field.name));
        let border = if field.error.is_some() {
            color("--border-feedback-critical")
        } else if focused {
            // `:focus-within` of the input wrapper (module 95477), the active border.
            color("--input-border-active")
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
            .when(!field.label.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(field.label),
                )
            })
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    // The number box keeps its dialing code 8px in (`leadingText`, dump of the Add phone dialog).
                    .when(name == "phone", |this| this.pl(px(8.)))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(focused && field.error.is_none(), |this| {
                        this.relative().child(focus_ring())
                    })
                    .children(field.leading.clone().map(|lead| {
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
                        // A long message wraps inside the field's width instead of running past the dialog.
                        div()
                            .flex_1()
                            .min_w(px(0.))
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

impl super::actions::State {
    /// The close button, a press on the scrim or Escape on the form: the "2FA is activated" dialog asks first until
    /// the codes were downloaded (module 549404 `handleCloseModal`) and Escape on that question goes back to the
    /// dialog; every other step just closes.
    pub(in crate::ui) fn close_form(&mut self) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if form.step != Step::MfaCodes {
            self.form = None;
        } else if form.mfa.skip_ask {
            form.mfa.skip_ask = false;
        } else if mfa::prompt_seen() {
            self.form = None;
        } else {
            form.mfa.skip_ask = true;
        }
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
    fn the_country_list_starts_on_the_united_states_and_filters_like_the_select() {
        let all = country_matches("");
        assert_eq!(COUNTRIES[default_country()].name, "United States");
        assert_eq!(all.len(), COUNTRIES.len());
        // Names that begin with the query come before names that only contain it.
        let kor: Vec<&str> = country_matches("kor")
            .into_iter()
            .map(|i| COUNTRIES[i].name)
            .collect();
        assert_eq!(&kor[..2], ["North Korea", "South Korea"]);
        assert!(country_matches("zzzz").is_empty());
        // A country with a long dialing code keeps its digits on the wire.
        assert_eq!(phone_number("+1 684", "555 0100"), "+16845550100");
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
