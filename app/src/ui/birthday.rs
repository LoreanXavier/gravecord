//! The date of birth input of module 201505 (month, day and year selects in the order of the locale's moment `L`
//! format, module 608998's locale map) and the age gate dialog of module 189282 that asks an account without a date
//! of birth (`nsfwAllowed == null`) for one. The new-user step of module 109766 (`W`) uses the same input; its shell is
//! drawn in `add_server.rs`.
//!
//! Submitting is module 955437 `n7`: `PATCH /users/@me` with `{"date_of_birth": "YYYY-MM-DD"}` (module 831167). The
//! request only goes out when the server said the account has no date of birth, the dialog was not staged by the
//! harness and the run is not a capture run ([`may_send`]); otherwise the body is only kept for the harness to print.
//!
//! The selects are `Te` of module 843282, which renders the select of module 691885 (`selectField_vWC4bk`, its
//! `popoutPosition` unused) with `maxOptionsVisible: 6`; they are drawn as `channel_settings.rs` draws that select
//! (40px, `--control-input-height-md`) and its `selectDropdown_vWC4bk` list. The placeholder is
//! `--input-placeholder-text-default`. Widths are `.day__vUhaG` 30%, `.month__vUhaG` 35%, `.year__vUhaG` 30%, spread
//! apart (`space-between`), 4px under them, then the Field's error (module 452027: a 16px CircleErrorIcon and
//! body-xs in `--text-feedback-critical`).

use super::guild_events::field_label;
use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::net::Request;
use crate::ui::motion::Transitions as _;
use chrono::{Datelike, Months, NaiveDate};
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, KeyDownEvent, ScrollHandle,
    SharedString, Window, deferred, div, prelude::*, px, relative,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// `getArticleURL(MVz.AGE_GATE)` (module 652215: article 360040724612).
const AGE_GATE_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360040724612";
/// The key a submit answers under; `modal` or `nuf` follows.
const SUBMIT_KEY: &str = "birthday-submit:";
/// `C` of module 323073 (`new Date("06/16/2020")`): accounts made after it see the invite card's age gate. The
/// official date is local midnight; UTC midnight here.
const CUTOFF_MS: u64 = 1_592_265_600_000;
/// The Discord epoch of snowflakes.
const DISCORD_EPOCH: u64 = 1_420_070_400_000;
/// An option of the list (`selectDropdown_vWC4bk` rows).
const OPTION_HEIGHT: f32 = 40.;
/// `maxVisibleItems: 6`.
const VISIBLE: usize = 6;
/// Type-ahead buffer reset window (`201505` `f`: `1000ms`).
const TYPEAHEAD_RESET: Duration = Duration::from_millis(1000);

/// One of the three selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Day,
    Month,
    Year,
}

/// Which input: the dialog of 189282 or the new-user step of 109766.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Modal,
    Nuf,
}

impl Owner {
    fn name(self) -> &'static str {
        match self {
            Owner::Modal => "modal",
            Owner::Nuf => "nuf",
        }
    }
}

/// `w_` of module 204925: where the dialog was opened from (the analytics `source`). Only the invite card opens it
/// here (GAP: the NSFW channel and server gates of `kK` in module 323073, Family Center's `showAgeGateModal` of
/// module 890623 and the deep link prompt are not wired); the harness opens the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub(crate) enum Source {
    NsfwChannel,
    NsfwServer,
    NsfwServerInvite,
    NsfwServerInviteEmbed,
    FamilyCenter,
    DeepLinkPrompt,
}

/// What the server said of the account's `nsfw_allowed`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Reported {
    /// No READY or USER_UPDATE carried the field yet (a cold start from the cache).
    #[default]
    NotSeen,
    /// `null`: the account has no date of birth.
    NoBirthday,
    Set(bool),
}

/// The field as a gateway user object carries it; `None` when the object leaves it out.
pub(crate) fn reported_from(field: Option<&Value>) -> Option<Reported> {
    match field? {
        Value::Null => Some(Reported::NoBirthday),
        Value::Bool(allowed) => Some(Reported::Set(*allowed)),
        _ => None,
    }
}

/// The submit goes out only for an account the server reported without a date of birth, from a dialog the harness
/// did not stage, outside capture runs (where a `gw` step can fake the server state as well).
pub(crate) fn may_send(reported: Reported, staged: bool, capture_run: bool) -> bool {
    reported == Reported::NoBirthday && !staged && !capture_run
}

/// `N` of module 963055 with `xq` and `zS` of module 323073: a Join on the invite card of a server whose
/// `nsfw_level` is EXPLICIT (1) or AGE_RESTRICTED (3) opens the dialog instead, for an account made after the cutoff
/// without a date of birth, unless it is already a member. A staged state stands for such an account.
pub(crate) fn invite_gate(
    created_ms: Option<u64>,
    reported: Reported,
    staged: bool,
    nsfw_level: u64,
    member: bool,
) -> bool {
    !member
        && matches!(nsfw_level, 1 | 3)
        && (staged
            || (reported == Reported::NoBirthday && created_ms.is_some_and(|ms| ms > CUTOFF_MS)))
}

/// moment's `localeData().longDateFormat("L")` for a Discord locale: the locales of module 608998 with their moment
/// locale's `L`; any other locale keeps moment's default `en`.
pub(crate) fn long_date_format(locale: &str) -> &'static str {
    match locale {
        "bg" => "D.MM.YYYY",
        "cs" | "da" | "de" | "fi" | "hr" | "no" | "pl" | "ro" | "ru" | "tr" | "uk" => "DD.MM.YYYY",
        "el" | "en-GB" | "es-ES" | "es-419" | "fr" | "hi" | "it" | "pt-BR" | "th" | "vi" => {
            "DD/MM/YYYY"
        }
        "hu" | "ko" => "YYYY.MM.DD.",
        "ja" | "zh-CN" | "zh-TW" => "YYYY/MM/DD",
        "lt" | "sv-SE" => "YYYY-MM-DD",
        "nl" => "DD-MM-YYYY",
        _ => "MM/DD/YYYY",
    }
}

/// `T` of module 201505: the selects in the order of `D`, `M` and `Y` in the format; day, month, year when one is
/// missing.
pub(crate) fn field_order(format: &str) -> [Part; 3] {
    let (day, month, year) = match (format.find('D'), format.find('M'), format.find('Y')) {
        (Some(day), Some(month), Some(year)) => (day, month, year),
        _ => (0, 1, 2),
    };
    let mut parts = [(day, Part::Day), (month, Part::Month), (year, Part::Year)];
    parts.sort_by_key(|(index, _)| *index);
    parts.map(|(_, part)| part)
}

/// What the three selects hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// One is still empty: nothing to submit, no error.
    Incomplete,
    /// All three, but no such day (`moment("31/2/2000", "DD/MM/YYYY").isValid()` is false): "Please enter a valid
    /// date of birth".
    Invalid,
    Valid(NaiveDate),
}

pub(crate) fn entry(day: Option<u32>, month: Option<u32>, year: Option<i32>) -> Entry {
    match (day, month, year) {
        (Some(day), Some(month), Some(year)) => {
            NaiveDate::from_ymd_opt(year, month, day).map_or(Entry::Invalid, Entry::Valid)
        }
        _ => Entry::Incomplete,
    }
}

/// `moment().diff(birthday, "years")`: `tR` of module 536637 (moment's `monthDiff`), whole months truncated to years.
/// When today's day of the month is before the birthday's, `tR` swaps its ends: the birthday moved on by the months,
/// clamped to the month's end, is the anchor, so a 29 February birthday is a year older on 28 February of common
/// years. Otherwise the anchor is today moved back, which never falls before the birthday's day.
pub(crate) fn age(birthday: NaiveDate, today: NaiveDate) -> i32 {
    let months =
        (today.year() - birthday.year()) * 12 + today.month() as i32 - birthday.month() as i32;
    let whole = if today.day() >= birthday.day() {
        months
    } else {
        let anchor = if months >= 0 {
            birthday.checked_add_months(Months::new(months as u32))
        } else {
            birthday.checked_sub_months(Months::new(months.unsigned_abs()))
        }
        .unwrap_or(birthday);
        // Today (past midnight) on or after the anchor adds a fraction; before it, a fraction short.
        if today >= anchor { months } else { months - 1 }
    };
    if whole >= 0 {
        whole / 12
    } else {
        -((-whole - 1) / 12)
    }
}

/// The request body (module 831167: `YYYY-MM-DD` in the `en` locale).
pub(crate) fn submit_body(birthday: NaiveDate) -> Value {
    json!({ "date_of_birth": birthday.format("%Y-%m-%d").to_string() })
}

/// What a refused submit means (the `catch` of 189282 `x` and 109766 `W`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// `body.date_of_birth`: the server found the account under age.
    Underage,
    /// `body.username`: "Your username is invalid..." (`TGg/2k`).
    Username,
    Message(String),
}

pub(crate) fn failure(body: &Value) -> Failure {
    if !body["date_of_birth"].is_null() {
        Failure::Underage
    } else if !body["username"].is_null() {
        Failure::Username
    } else {
        Failure::Message(body["message"].as_str().unwrap_or_default().to_owned())
    }
}

/// The `date_of_birth` error of a refused submit as module 784578 shows it (React joins an array's strings).
pub(crate) fn underage_message(body: &Value) -> Option<String> {
    let text = match &body["date_of_birth"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .concat(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// The id of module 784578's dialog; closing it any way goes back to the login (`c`).
pub(super) const UNDERAGE_ID: &str = "existing-user-age-gate-underage";

/// "Discord is for ages 13 and up" (module 784578 on the dialog of 189213): the server's message (else `WqEH4D`),
/// then "You have **30 days** to verify your age...", Learn More and Back to Login.
pub(super) fn underage_alert(message: Option<String>) -> super::chat_notices::Alert {
    use super::chat_notices::{Alert, AlertButton, Then};
    let message = message.unwrap_or_else(|| dc_intl::t("WqEH4D").to_owned());
    let subtitle = dc_intl::format_runs(
        "3TZUIw",
        &[
            ("underageMessage", message.as_str().into()),
            ("days", dc_intl::Arg::Num(30.)),
        ],
    )
    .into_iter()
    .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
    .collect();
    Alert {
        id: UNDERAGE_ID,
        title: dc_intl::t("NR/zrG").to_owned(),
        subtitle: Some(subtitle),
        content: None,
        buttons: vec![
            AlertButton {
                id: "age-gate-underage-learn-more",
                label: dc_intl::t("hvVgAZ").to_owned(),
                tone: super::modal::Tone::Secondary,
                then: Then::OpenArticle {
                    url: AGE_GATE_ARTICLE,
                },
            },
            AlertButton {
                id: "age-gate-underage-back",
                label: dc_intl::t("szzQ6H").to_owned(),
                tone: super::modal::Tone::Primary,
                then: Then::BackToLogin,
            },
        ],
    }
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn month_label_for(locale: &str, month: u32) -> &'static str {
    dc_intl::month_names(locale)
        .get(month.wrapping_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

fn month_label(month: u32) -> &'static str {
    month_label_for(dc_intl::locale(), month)
}

/// First option matching a lowercased alphanumeric type-ahead prefix (`201505` `f`).
fn typeahead_match(part: Part, year: i32, buffer: &str) -> Option<i32> {
    if buffer.is_empty() {
        return None;
    }
    options(part, year)
        .into_iter()
        .find(|(_, label)| label.to_lowercase().starts_with(buffer))
        .map(|(value, _)| value)
}

/// `popoutPosition: "top"` (`201505`): prefer above the select when it fits, else flip below.
fn popout_top(bounds_top: f32, bounds_bottom: f32, height: f32, window_height: f32) -> f32 {
    let above = bounds_top - (4. + height);
    if above >= 0. {
        above
    } else {
        (bounds_bottom + 4.).min((window_height - height).max(0.))
    }
}

/// The options of a select: 1 to 31, the months, this year back 100 years.
fn options(part: Part, year: i32) -> Vec<(i32, String)> {
    match part {
        Part::Day => (1..=31).map(|day| (day, day.to_string())).collect(),
        Part::Month => (1..=12)
            .map(|month| (month, month_label(month as u32).to_owned()))
            .collect(),
        Part::Year => (0..=100)
            .map(|back| (year - back, (year - back).to_string()))
            .collect(),
    }
}

fn part_slot(part: Part) -> usize {
    match part {
        Part::Day => 0,
        Part::Month => 1,
        Part::Year => 2,
    }
}

fn anchor_key(owner: Owner, part: Part) -> &'static str {
    match (owner, part) {
        (Owner::Modal, Part::Day) => "birthday-modal-day",
        (Owner::Modal, Part::Month) => "birthday-modal-month",
        (Owner::Modal, Part::Year) => "birthday-modal-year",
        (Owner::Nuf, Part::Day) => "birthday-nuf-day",
        (Owner::Nuf, Part::Month) => "birthday-nuf-month",
        (Owner::Nuf, Part::Year) => "birthday-nuf-year",
    }
}

/// The three selects of an input.
#[derive(Default)]
pub(crate) struct Input {
    pub day: Option<u32>,
    pub month: Option<u32>,
    pub year: Option<i32>,
    open: Option<Part>,
    scroll: ScrollHandle,
    focus: RefCell<Option<[FocusHandle; 3]>>,
    typeahead: String,
    typeahead_at: Option<Instant>,
}

impl Input {
    pub(crate) fn entry(&self) -> Entry {
        entry(self.day, self.month, self.year)
    }

    fn focus_handle(&self, part: Part, cx: &mut Context<Root>) -> FocusHandle {
        let mut guard = self.focus.borrow_mut();
        let handles =
            guard.get_or_insert_with(|| [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()]);
        handles[part_slot(part)].clone()
    }

    fn value(&self, part: Part) -> Option<i32> {
        match part {
            Part::Day => self.day.map(|day| day as i32),
            Part::Month => self.month.map(|month| month as i32),
            Part::Year => self.year,
        }
    }

    fn set(&mut self, part: Part, value: Option<i32>) {
        match part {
            Part::Day => self.day = value.and_then(|day| u32::try_from(day).ok()),
            Part::Month => self.month = value.and_then(|month| u32::try_from(month).ok()),
            Part::Year => self.year = value,
        }
    }
}

/// The open dialog of 189282.
pub(crate) struct Modal {
    source: Source,
    input: Input,
    /// `G`: the server's refusal under the input.
    error: Option<String>,
    /// `M`: the submit is in flight.
    submitting: bool,
    /// `L === 1`: under 18, the "You submitted that you're N years old" step.
    confirm: bool,
    submit_focus: FocusHandle,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) reported: Reported,
    /// Capture only: the harness shows the "no date of birth" UI on an account that has one. Nothing is sent.
    pub(crate) staged: bool,
    modal: Option<Modal>,
    /// The new-user step's input (109766 `W`).
    pub(crate) nuf: Input,
    nuf_error: Option<String>,
    nuf_submitting: bool,
    nuf_submit_focus: RefCell<Option<FocusHandle>>,
    /// The last submit's body and whether it went out (held in capture runs and staged states); the harness prints it.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    last: Option<(Owner, Value, bool)>,
}

impl Root {
    /// READY's `user.nsfw_allowed` and USER_UPDATE's (only when it carries the field): the state 189282 and 109766
    /// read (`getCurrentUser().nsfwAllowed`). Once the account has a date of birth the new-user step moves on
    /// (`W`'s effect).
    pub(super) fn birthday_dispatch(&mut self, name: &str, data: &Value) {
        let field = match name {
            "READY" => data["user"].get("nsfw_allowed"),
            "USER_UPDATE" => data.get("nsfw_allowed"),
            _ => return,
        };
        if let Some(reported) = reported_from(field) {
            self.birthday.reported = reported;
        }
        if !self.birthday.staged && matches!(self.birthday.reported, Reported::Set(_)) {
            self.add_server_age_gate_done();
        }
    }

    /// The new-user step starts empty.
    #[cfg(feature = "capture")]
    fn birthday_nuf_reset(&mut self) {
        self.birthday.nuf = Input::default();
        self.birthday.nuf_error = None;
        self.birthday.nuf_submitting = false;
    }

    /// `AGE_GATE_MODAL_OPEN` (module 612200 `yO`) as module 596339 mounts 189282 for a date-of-birth source.
    /// GAP: `zt` of module 323073 picks the age assurance flow (module 361016) instead when `AGE_GATED_SPACES` is on
    /// (`T.r()`); this client never does.
    pub(super) fn open_birthday_modal(&mut self, source: Source, cx: &mut Context<Self>) {
        self.birthday.modal = Some(Modal {
            source,
            input: Input::default(),
            error: None,
            submitting: false,
            confirm: false,
            submit_focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// The dialog of 189282 is open (counted with the other dialogs for focus and the system bar).
    pub(in crate::ui) fn birthday_modal_open(&self) -> bool {
        self.birthday.modal.is_some()
    }

    /// `N` of module 963055 for this account.
    pub(super) fn birthday_invite_gate(&self, nsfw_level: u64, member: bool) -> bool {
        let created = self
            .stores
            .current_user
            .as_ref()
            .map(|user| (user.id >> 22) + DISCORD_EPOCH);
        invite_gate(
            created,
            self.birthday.reported,
            self.birthday.staged,
            nsfw_level,
            member,
        )
    }

    /// `U` of 189282: the dialog closes (`AGE_GATE_MODAL_CLOSE`), returning to safety from NSFW
    /// sources or navigating to Family Center when appropriate.
    fn birthday_close(&mut self, cx: &mut Context<Self>) {
        let source = self.birthday.modal.as_ref().map(|m| m.source);
        self.birthday.modal = None;
        match source {
            Some(Source::NsfwChannel | Source::NsfwServer | Source::NsfwServerInvite) => {
                // `nsfwReturnToSafety`: step back to Home (DMs) when leaving an age-gated space.
                self.select(0, None);
            }
            Some(Source::FamilyCenter) => {
                self.open_settings_section("Family Center", cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Escape closes an open list first, then the dialog. True when something closed.
    pub(super) fn birthday_escape(&mut self) -> bool {
        if let Some(modal) = self.birthday.modal.as_mut()
            && modal.input.open.take().is_some()
        {
            return true;
        }
        // The new-user step's list only counts while that step is on screen.
        let nuf_shown = self.birthday_input_of(Owner::Nuf).is_some();
        if nuf_shown && self.birthday.nuf.open.take().is_some() {
            return true;
        }
        self.birthday.modal.take().is_some()
    }

    fn birthday_input_of(&self, owner: Owner) -> Option<&Input> {
        match owner {
            Owner::Modal => self.birthday.modal.as_ref().map(|modal| &modal.input),
            Owner::Nuf => self
                .add_server
                .as_ref()
                .is_some_and(|add| add.step == super::add_server::Step::AgeGate)
                .then_some(&self.birthday.nuf),
        }
    }

    fn birthday_input_mut(&mut self, owner: Owner) -> Option<&mut Input> {
        match owner {
            Owner::Modal => self.birthday.modal.as_mut().map(|modal| &mut modal.input),
            Owner::Nuf => Some(&mut self.birthday.nuf),
        }
    }

    fn birthday_toggle(&mut self, owner: Owner, part: Part, cx: &mut Context<Self>) {
        let other = match owner {
            Owner::Modal => Owner::Nuf,
            Owner::Nuf => Owner::Modal,
        };
        if let Some(input) = self.birthday_input_mut(other) {
            input.open = None;
        }
        let year = today().year();
        let Some(input) = self.birthday_input_mut(owner) else {
            return;
        };
        if input.open == Some(part) {
            input.open = None;
        } else {
            input.open = Some(part);
            // The picked option is in view, as low in the list as it can be.
            let picked = input.value(part).map(|value| match part {
                Part::Day | Part::Month => value - 1,
                Part::Year => year - value,
            });
            let offset = picked.map_or(0., |index| {
                ((index as f32 - (VISIBLE as f32 - 1.)) * OPTION_HEIGHT).max(0.)
            });
            input.scroll.set_offset(gpui::point(px(0.), px(-offset)));
        }
        cx.notify();
    }

    fn birthday_pick(
        &mut self,
        owner: Owner,
        part: Part,
        value: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.birthday_input_mut(owner) {
            input.set(part, Some(value));
            input.open = None;
        }
        // Module 201505 auto-advance & `onPopulated`: focus the next select in locale order, or the submit button once
        // the last select is populated.
        let order = field_order(long_date_format(dc_intl::locale()));
        if let Some(idx) = order.iter().position(|p| *p == part) {
            if let Some(&next_part) = order.get(idx + 1) {
                if let Some(handle) = self
                    .birthday_input_of(owner)
                    .map(|input| input.focus_handle(next_part, cx))
                {
                    window.focus(&handle, cx);
                }
            } else {
                let submit_handle = match owner {
                    Owner::Modal => self
                        .birthday
                        .modal
                        .as_ref()
                        .map(|modal| modal.submit_focus.clone()),
                    Owner::Nuf => Some(
                        self.birthday
                            .nuf_submit_focus
                            .borrow_mut()
                            .get_or_insert_with(|| cx.focus_handle())
                            .clone(),
                    ),
                };
                if let Some(handle) = submit_handle {
                    window.focus(&handle, cx);
                }
            }
        }
        cx.notify();
    }

    fn birthday_key_down(
        &mut self,
        owner: Owner,
        part: Part,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        match key {
            "enter" | "space" => {
                self.birthday_toggle(owner, part, cx);
                cx.stop_propagation();
            }
            "up" | "down" => {
                let opts = options(part, today().year());
                let cur = self
                    .birthday_input_of(owner)
                    .and_then(|input| input.value(part));
                let pos = cur.and_then(|v| opts.iter().position(|(ov, _)| *ov == v));
                let next_idx = match (key, pos) {
                    ("down", Some(i)) => (i + 1).min(opts.len().saturating_sub(1)),
                    ("down", None) => 0,
                    ("up", Some(i)) => i.saturating_sub(1),
                    ("up", None) => opts.len().saturating_sub(1),
                    _ => 0,
                };
                if let Some((val, _)) = opts.get(next_idx)
                    && let Some(input) = self.birthday_input_mut(owner)
                {
                    input.set(part, Some(*val));
                    cx.notify();
                }
                cx.stop_propagation();
            }
            _ => {
                // Module 201505 `f`: alphanumeric type-ahead with 1000ms reset window.
                if event.keystroke.modifiers.control
                    || event.keystroke.modifiers.alt
                    || event.keystroke.modifiers.platform
                {
                    return;
                }
                let ch = event
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|s| s.chars().count() == 1)
                    .and_then(|s| s.chars().next())
                    .filter(|c| c.is_ascii_alphanumeric());
                let Some(ch) = ch else {
                    return;
                };
                let now = Instant::now();
                let buf = {
                    let Some(input) = self.birthday_input_mut(owner) else {
                        return;
                    };
                    if input
                        .typeahead_at
                        .is_none_or(|prev| now.duration_since(prev) > TYPEAHEAD_RESET)
                    {
                        input.typeahead.clear();
                    }
                    input.typeahead.push(ch.to_ascii_lowercase());
                    input.typeahead_at = Some(now);
                    input.typeahead.clone()
                };
                if let Some(matched) = typeahead_match(part, today().year(), &buf) {
                    if let Some(input) = self.birthday_input_mut(owner) {
                        input.set(part, Some(matched));
                    }
                    let _ = window;
                    cx.notify();
                }
                cx.stop_propagation();
            }
        }
    }

    /// `j` of 189282: under 18 asks again first, else `x`.
    fn birthday_modal_submit(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.birthday.modal.as_mut() else {
            return;
        };
        if modal.submitting {
            return;
        }
        let Entry::Valid(birthday) = modal.input.entry() else {
            return;
        };
        if !modal.confirm && age(birthday, today()) < 18 {
            modal.confirm = true;
            cx.notify();
            return;
        }
        self.birthday_send(Owner::Modal, birthday, cx);
    }

    /// The Next of 109766 `W`.
    pub(super) fn birthday_nuf_next(&mut self, cx: &mut Context<Self>) {
        if self.birthday.nuf_submitting {
            return;
        }
        let Entry::Valid(birthday) = self.birthday.nuf.entry() else {
            return;
        };
        self.birthday_send(Owner::Nuf, birthday, cx);
    }

    /// `n7` of module 955437, behind [`may_send`].
    fn birthday_send(&mut self, owner: Owner, birthday: NaiveDate, cx: &mut Context<Self>) {
        let body = submit_body(birthday);
        let send = may_send(
            self.birthday.reported,
            self.birthday.staged,
            crate::storage::is_capture_run(),
        );
        self.birthday.last = Some((owner, body.clone(), send));
        if !send {
            cx.notify();
            return;
        }
        match owner {
            Owner::Modal => {
                if let Some(modal) = self.birthday.modal.as_mut() {
                    modal.error = None;
                    modal.submitting = true;
                }
            }
            Owner::Nuf => self.birthday.nuf_submitting = true,
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SUBMIT_KEY}{}", owner.name())),
            method: reqwest::Method::PATCH,
            route: "/users/@me".to_owned(),
            body: Some(body),
        });
        cx.notify();
    }

    /// The answer of a submit. Success takes the user's `nsfw_allowed` (`CURRENT_USER_UPDATE`) and closes the dialog
    /// or moves the new-user flow on; a refusal is [`failure`].
    pub(super) fn birthday_answered(
        &mut self,
        key: &str,
        report: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(owner) = key.strip_prefix(SUBMIT_KEY) else {
            return false;
        };
        let owner = if owner == "nuf" {
            Owner::Nuf
        } else {
            Owner::Modal
        };
        let body = &report["body"];
        if report["ok"].as_bool() == Some(true) {
            if !self.birthday.staged
                && let Some(reported) = reported_from(body.get("nsfw_allowed"))
            {
                self.birthday.reported = reported;
            }
            match owner {
                Owner::Modal => {
                    let was_family = self
                        .birthday
                        .modal
                        .as_ref()
                        .is_some_and(|m| m.source == Source::FamilyCenter);
                    self.birthday.modal = None;
                    // `EXISTING_USER_AGE_GATE_SUCCESS` confirmation toast (module 189282).
                    self.put_app_toast(dc_intl::t("6tahin").to_owned(), true, cx);
                    if was_family {
                        self.open_settings_section("Family Center", cx);
                    }
                }
                Owner::Nuf => {
                    self.birthday.nuf_submitting = false;
                    self.add_server_age_gate_done();
                }
            }
            cx.notify();
            return true;
        }
        let failed = failure(body);
        let message = match &failed {
            Failure::Underage => None,
            Failure::Username => Some(dc_intl::t("TGg/2k").to_owned()),
            Failure::Message(text) => Some(text.clone()),
        };
        match owner {
            Owner::Modal => match message {
                // `AGE_GATE_FAILURE_MODAL_OPEN` (module 612200 `MP`) puts module 784578's underage dialog in its
                // place, with the server's `date_of_birth` message.
                None => {
                    self.birthday.modal = None;
                    self.chat_extra.alert = Some(underage_alert(underage_message(body)));
                }
                Some(message) => {
                    if let Some(modal) = self.birthday.modal.as_mut() {
                        modal.error = Some(message);
                        modal.submitting = false;
                        modal.confirm = false;
                    }
                }
            },
            Owner::Nuf => {
                self.birthday.nuf_submitting = false;
                match message {
                    // GAP: `AGE_GATE_PREVENT_UNDERAGE_REGISTRATION` and `AGE_GATE_LOGOUT_UNDERAGE_NEW_USER` (the
                    // account is logged out); only the dialog closes here.
                    None => self.add_server = None,
                    Some(message) => self.birthday.nuf_error = Some(message),
                }
            }
        }
        cx.notify();
        true
    }

    /// `c` of module 784578: `AGE_GATE_MODAL_CLOSE`, then the login (`transitionTo(LOGIN)`, the session ends here).
    pub(super) fn back_to_login(&mut self, cx: &mut Context<Self>) {
        self.birthday.modal = None;
        self.leave_voice(cx);
        cx.emit(super::logout::RootEvent::LogOut);
    }

    /// The dialog closed by Escape: [`Self::back_to_login`] on the next frame.
    pub(super) fn back_to_login_later(&mut self) {
        self.chat_extra.back_to_login = true;
    }

    /// A message whose `$link` run opens the age gate article.
    fn birthday_link_text(id: &'static str, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let mut text = String::new();
        let mut links = Vec::new();
        let mut highlights = Vec::new();
        for (run, tag) in dc_intl::format_runs(key, &[("helpURL", AGE_GATE_ARTICLE.into())]) {
            let start = text.len();
            text.push_str(&run);
            if tag.as_deref() == Some("$link") {
                links.push(start..text.len());
                highlights.push((
                    start..text.len(),
                    gpui::HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..Default::default()
                    },
                ));
            }
        }
        let open = cx.listener(|this, _: &usize, _, cx| this.follow_link(AGE_GATE_ARTICLE, "", cx));
        div()
            .child(
                gpui::InteractiveText::new(
                    id,
                    gpui::StyledText::new(text).with_highlights(highlights),
                )
                .on_click(links, move |index, window, cx| open(&index, window, cx)),
            )
            .into_any_element()
    }

    /// One select of module 843282 / 201505: min-widths (`calc(2ch + 70px)` / `calc(4ch + 70px)`), focus handle +
    /// focus ring, and alphanumeric type-ahead + keyboard navigation.
    fn birthday_select(
        &self,
        owner: Owner,
        part: Part,
        input: &Input,
        window: Option<&Window>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = anchor_key(owner, part);
        let open = input.open == Some(part);
        let focus_handle = input.focus_handle(part, cx);
        let focused = window.is_some_and(|w| focus_handle.is_focused(w));
        let (width, min_width, placeholder) = match part {
            Part::Day => (0.30, Some(86.), dc_intl::t("Voklre")),
            Part::Month => (0.35, None, dc_intl::t("UDlN8W")),
            Part::Year => (0.30, Some(102.), dc_intl::t("ZWr5WA")),
        };
        let value: Option<String> = match part {
            Part::Day => input.day.map(|day| day.to_string()),
            Part::Month => input.month.map(|month| month_label(month).to_owned()),
            Part::Year => input.year.map(|year| year.to_string()),
        };
        let shown = value.is_some();
        let click_focus = focus_handle.clone();
        // As `channel_settings.rs` draws the same `selectField_vWC4bk`.
        div()
            .id(key)
            .relative()
            .track_focus(&focus_handle)
            .w(relative(width))
            .when_some(min_width, |d, mw| d.min_w(px(mw)))
            .h(px(40.))
            .rounded(px(8.))
            .when(focused && !open, |d| {
                d.border_2().border_color(color("--border-focus"))
            })
            .when(!focused || open, |d| {
                d.border_1().border_color(color(if open {
                    "--input-border-active"
                } else {
                    "--input-border-default"
                }))
            })
            .focus(|style| style.border_color(color("--border-focus")))
            .rule(crate::ui::motion::Rule::TextField)
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(self.anchor_probe(key))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .pl(px(12.))
                    .text_size(px(16.))
                    .text_color(color(if shown {
                        "--text-default"
                    } else {
                        "--input-placeholder-text-default"
                    }))
                    .child(value.unwrap_or_else(|| placeholder.to_owned())),
            )
            .child(
                div().w(px(36.)).pr(px(4.)).flex().justify_center().child(
                    gpui::svg()
                        .path("icons/combo_chevron.svg")
                        .size(px(20.))
                        .text_color(color("--text-strong"))
                        .with_transformation(crate::ui::motion::flip(
                            crate::ui::motion::key(key),
                            open,
                            crate::ui::motion::css(200., crate::ui::motion::Ease::EaseOut)
                                .full_motion(),
                        )),
                ),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                this.birthday_key_down(owner, part, event, window, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&click_focus, cx);
                this.birthday_toggle(owner, part, cx);
            }))
            .into_any_element()
    }

    /// Module 201505: the "Date of Birth" field (its `label` prop is never read), the selects in the locale's order and
    /// the error: an impossible day first, else `error`.
    fn birthday_input(
        &self,
        owner: Owner,
        required: bool,
        error: Option<&str>,
        window: Option<&Window>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(input) = self.birthday_input_of(owner) else {
            return div().into_any_element();
        };
        let order = field_order(long_date_format(dc_intl::locale()));
        let shown_error: Option<String> = if input.entry() == Entry::Invalid {
            Some(dc_intl::t("udnqh6").to_owned())
        } else {
            error.filter(|text| !text.is_empty()).map(str::to_owned)
        };
        let selects: Vec<AnyElement> = order
            .iter()
            .map(|part| self.birthday_select(owner, *part, input, window, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .child(field_label(dc_intl::t("xNpFJ6"), required))
            .child(
                div()
                    .w_full()
                    .mb(px(4.))
                    .flex()
                    .justify_between()
                    .children(selects),
            )
            .children(shown_error.map(|message| {
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
                            .child(message),
                    )
            }))
            .into_any_element()
    }

    /// The list of the open select, over everything (`popoutPosition: "top"`, below when there is no room above).
    fn birthday_popover(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let window_height = window.viewport_size().height;
        let (owner, input) = [Owner::Modal, Owner::Nuf].into_iter().find_map(|owner| {
            let input = self.birthday_input_of(owner)?;
            input.open.map(|_| (owner, input))
        })?;
        let part = input.open?;
        let key = anchor_key(owner, part);
        let inner = self.tip_anchors.borrow().get(key).copied()?;
        // The probe sits inside the 1px border.
        let bounds = gpui::Bounds::new(
            gpui::point(inner.left() - px(1.), inner.top() - px(1.)),
            gpui::size(inner.size.width + px(2.), inner.size.height + px(2.)),
        );
        let all = options(part, today().year());
        // `selectDropdown_vWC4bk`: `maxOptionsVisible: 6`, `popoutPosition: "top"` (prefers above the select when it
        // fits, flipping below only when insufficient room above).
        let height = (all.len().min(VISIBLE) as f32 * OPTION_HEIGHT + 8.) + 2.;
        let top = px(popout_top(
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
            height,
            f32::from(window_height),
        ));
        let picked = input.value(part);
        let items: Vec<AnyElement> = all
            .into_iter()
            .enumerate()
            .map(|(index, (value, label))| {
                let chosen = picked == Some(value);
                div()
                    .id(SharedString::from(format!("{key}-option-{index}")))
                    .flex_none()
                    .h(px(OPTION_HEIGHT))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .pl(px(8.))
                            .text_size(px(16.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .when(chosen, |row| {
                        row.child(div().px(px(4.)).child(icon(
                            "select_check",
                            20.,
                            "--text-strong",
                        )))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.birthday_pick(owner, part, value, window, cx)
                    }))
                    .into_any_element()
            })
            .collect();
        let list = div()
            .id("birthday-popout")
            .occlude()
            .w(bounds.size.width)
            .h(px(height))
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
            .overflow_hidden()
            .child(
                div()
                    .id("birthday-popout-list")
                    .size_full()
                    .p(px(4.))
                    .pr(px(20.))
                    .overflow_y_scroll()
                    .track_scroll(&input.scroll)
                    .flex()
                    .flex_col()
                    .children(items),
            );
        Some(
            deferred(
                div()
                    .id("birthday-popover-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(input) = this.birthday_input_mut(owner) {
                            input.open = None;
                        }
                        cx.notify();
                    }))
                    .child(div().absolute().left(bounds.left()).top(top).child(list)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The dialog of 189282 on the design system modal (module 189213: `graphic` and `gradientColor` reach module
    /// 224640, which ignores them): size md with the input as its body, or, for the under-18 question, size sm
    /// without one. Then the open select's list.
    pub(super) fn birthday_layers(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut layers = Vec::new();
        if let Some(open) = self.birthday.modal.as_ref() {
            match (open.confirm, open.input.entry()) {
                (true, Entry::Valid(birthday)) => {
                    let years = age(birthday, today());
                    let spec = modal::Spec {
                        id: "age-gate-confirm",
                        title: dc_intl::t!("wumolR", age = years).into(),
                        subtitle: Some(
                            div()
                                .child(dc_intl::t!("n3QjDE", helpURL = AGE_GATE_ARTICLE))
                                .into_any_element(),
                        ),
                        body: None,
                        buttons: vec![
                            modal::button(
                                "age-gate-confirm-back",
                                dc_intl::t("cfYCrb"),
                                Tone::Secondary,
                                |this, _, cx| {
                                    if let Some(modal) = this.birthday.modal.as_mut() {
                                        modal.confirm = false;
                                    }
                                    cx.notify();
                                },
                            ),
                            modal::button(
                                "age-gate-confirm-age",
                                dc_intl::t("6tahin"),
                                Tone::Primary,
                                |this, _, cx| this.birthday_modal_submit(cx),
                            ),
                        ],
                        close: Box::new(|this, _, cx| this.birthday_close(cx)),
                    };
                    layers.push(modal::render_small(spec, cx));
                }
                (_, entry) => {
                    let title = match open.source {
                        Source::NsfwServer
                        | Source::NsfwServerInvite
                        | Source::NsfwServerInviteEmbed => dc_intl::t("vAymlG"),
                        Source::FamilyCenter => dc_intl::t("M7mt7m"),
                        Source::NsfwChannel | Source::DeepLinkPrompt => dc_intl::t("F8otRo"),
                    };
                    let subtitle = match open.source {
                        Source::FamilyCenter => {
                            div().child(dc_intl::t("mhUrKS")).into_any_element()
                        }
                        Source::DeepLinkPrompt => {
                            Self::birthday_link_text("age-gate-subtitle", "iyhg2s", cx)
                        }
                        _ => Self::birthday_link_text("age-gate-subtitle", "n3QjDE", cx),
                    };
                    let body = self.birthday_input(
                        Owner::Modal,
                        false,
                        open.error.as_deref(),
                        Some(window),
                        cx,
                    );
                    let tone = if open.submitting {
                        Tone::PrimaryLoading
                    } else if matches!(entry, Entry::Valid(_)) {
                        Tone::Primary
                    } else {
                        Tone::PrimaryDisabled
                    };
                    let mut submit_btn = modal::button(
                        "age-gate-submit",
                        dc_intl::t("uBFuok"),
                        tone,
                        |this, _, cx| this.birthday_modal_submit(cx),
                    );
                    submit_btn.focus = Some(open.submit_focus.clone());
                    let spec = modal::Spec {
                        id: "age-gate",
                        title: title.into(),
                        subtitle: Some(subtitle),
                        body: Some(body),
                        buttons: vec![
                            modal::button(
                                "age-gate-back",
                                dc_intl::t("1MrpWO"),
                                Tone::Secondary,
                                |this, _, cx| this.birthday_close(cx),
                            ),
                            submit_btn,
                        ],
                        close: Box::new(|this, _, cx| this.birthday_close(cx)),
                    };
                    layers.push(modal::render(spec, cx));
                }
            }
        }
        layers.extend(self.birthday_popover(window, cx));
        layers
    }

    /// The new-user step of 109766 `W` (`content_gW5-F4`): 464px high on `--background-base-low`, 16px at the
    /// sides, centred: the Clyde mark (56 by 40, brand), "Welcome to Discord!" (heading-xl, bold, 20px above), the
    /// line with its help link (body-md, 8px above and 24px under), the required input (16px under), and Next across
    /// the bottom (`footer_gW5-F4` absolute, 16px around a large button: 44px).
    /// GAP: the Clyde mark (module 111159) is not among the assets; its 56 by 40 box stays empty.
    pub(super) fn birthday_nuf_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let ready = matches!(self.birthday.nuf.entry(), Entry::Valid(_));
        let submitting = self.birthday.nuf_submitting;
        let input = self.birthday_input(
            Owner::Nuf,
            true,
            self.birthday.nuf_error.as_deref(),
            None,
            cx,
        );
        let nuf_submit_focus = self
            .birthday
            .nuf_submit_focus
            .borrow_mut()
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let next = div()
            .id("birthday-nuf-next")
            .track_focus(&nuf_submit_focus)
            .relative()
            .w_full()
            .h(px(44.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-primary-border-default"))
            .bg(color("--control-primary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-primary-text-default"))
            .when(!ready, |this| this.opacity(0.5))
            .when(ready && !submitting, |this| this.cursor_pointer())
            .when(submitting, |this| {
                this.child(super::components::dots("birthday-nuf-next", "--white"))
            })
            .when(!submitting, |this| this.child(dc_intl::t("PDTjLN")))
            .on_click(cx.listener(|this, _, _, cx| this.birthday_nuf_next(cx)));
        div()
            .relative()
            .w(px(440.))
            .h(px(464.))
            .bg(color("--background-base-low"))
            .px(px(16.))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div().flex().justify_center().child(
                    div()
                        .w(px(56.))
                        .h(px(40.))
                        .rounded(px(12.))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("sb_discord", 28., "--white")),
                ),
            )
            .child(
                div()
                    .mt(px(20.))
                    .w_full()
                    .text_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("n7i7sv")),
            )
            .child(
                div()
                    .mt(px(8.))
                    .mb(px(24.))
                    .w_full()
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(Self::birthday_link_text(
                        "birthday-nuf-description",
                        "fa8kW1",
                        cx,
                    )),
            )
            .child(div().mb(px(16.)).child(input))
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .p(px(16.))
                    .child(next),
            )
            .into_any_element()
    }

    /// Capture only (`birthday ...`): the states of this file without touching the account.
    /// * `stage`: the account counts as one without a date of birth, in memory; nothing is ever sent from here on.
    /// * `open <channel|server|invite|invite-embed|family-center|deep-link>`: stages and opens 189282.
    /// * `nuf`: stages and opens the Add a Server dialog, which starts with 109766's birthday step.
    /// * `set <modal|nuf> <day> <month> <year>` (0 leaves a select empty), `pick <modal|nuf> <day|month|year>` opens
    ///   a select's list, `confirm` shows the under-18 question, `loading <modal|nuf>` the submit in flight.
    /// * `submit <modal|nuf>`: Submit/Next as a click runs it; the body is held and printed.
    /// * `answer <modal|nuf> ok|underage|username|message <text>`: the submit's answer, as the server would send it.
    /// * `state`: what is staged, reported, open and held.
    #[cfg(feature = "capture")]
    pub(crate) fn script_birthday(
        &mut self,
        words: &[&str],
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> String {
        let owner_of = |word: &str| {
            if word == "nuf" {
                Owner::Nuf
            } else {
                Owner::Modal
            }
        };
        match words {
            ["stage"] => self.birthday.staged = true,
            ["open", source] => {
                let source = match *source {
                    "channel" => Source::NsfwChannel,
                    "server" => Source::NsfwServer,
                    "invite" => Source::NsfwServerInvite,
                    "invite-embed" => Source::NsfwServerInviteEmbed,
                    "family-center" => Source::FamilyCenter,
                    "deep-link" => Source::DeepLinkPrompt,
                    _ => return "usage: birthday open channel|server|invite|invite-embed|family-center|deep-link".to_owned(),
                };
                self.birthday.staged = true;
                self.open_birthday_modal(source, cx);
            }
            ["nuf"] => {
                self.birthday.staged = true;
                self.open_add_server(window, cx);
                self.birthday_nuf_reset();
                if let Some(add) = self.add_server.as_mut() {
                    add.step = super::add_server::Step::AgeGate;
                }
            }
            ["set", owner, day, month, year] => {
                let number = |text: &str| text.parse::<i32>().ok().filter(|value| *value != 0);
                if let Some(input) = self.birthday_input_mut(owner_of(owner)) {
                    input.set(Part::Day, number(day));
                    input.set(Part::Month, number(month));
                    input.set(Part::Year, number(year));
                    input.open = None;
                }
            }
            ["pick", owner, part] => {
                let part = match *part {
                    "day" => Part::Day,
                    "month" => Part::Month,
                    _ => Part::Year,
                };
                self.birthday_toggle(owner_of(owner), part, cx);
            }
            ["confirm"] => {
                if let Some(modal) = self.birthday.modal.as_mut() {
                    modal.confirm = true;
                }
            }
            ["loading", owner] => match owner_of(owner) {
                Owner::Modal => {
                    if let Some(modal) = self.birthday.modal.as_mut() {
                        modal.submitting = true;
                    }
                }
                Owner::Nuf => self.birthday.nuf_submitting = true,
            },
            ["submit", owner] => match owner_of(owner) {
                Owner::Modal => self.birthday_modal_submit(cx),
                Owner::Nuf => self.birthday_nuf_next(cx),
            },
            ["answer", owner, kind, rest @ ..] => {
                let report = match *kind {
                    "ok" => json!({"ok": true, "status": 200, "body": {"nsfw_allowed": true}}),
                    "underage" => json!({"ok": false, "status": 400, "body": {"date_of_birth": ["underage"]}}),
                    "username" => json!({"ok": false, "status": 400, "body": {"username": ["invalid"]}}),
                    _ => json!({"ok": false, "status": 400, "body": {"message": rest.join(" ")}}),
                };
                self.birthday_answered(&format!("{SUBMIT_KEY}{owner}"), &report, cx);
            }
            ["state"] => {}
            _ => return "usage: birthday stage | open <source> | nuf | set <modal|nuf> <d> <m> <y> | pick <modal|nuf> <part> | confirm | loading <modal|nuf> | submit <modal|nuf> | answer <modal|nuf> <kind> | state".to_owned(),
        }
        cx.notify();
        let modal = self.birthday.modal.as_ref().map(|modal| {
            format!(
                "{:?} {:?} confirm={} submitting={} error={:?}",
                modal.source,
                modal.input.entry(),
                modal.confirm,
                modal.submitting,
                modal.error
            )
        });
        format!(
            "birthday: staged={} reported={:?} modal={modal:?} nuf={:?} nuf_error={:?} add_server_step={:?} last={:?}",
            self.birthday.staged,
            self.birthday.reported,
            self.birthday.nuf.entry(),
            self.birthday.nuf_error,
            self.add_server.as_ref().map(|add| add.step),
            self.birthday.last
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_underage_dialog_reads_the_servers_message_like_module_784578() {
        assert_eq!(
            underage_message(&serde_json::json!({"date_of_birth": ["You are too young."]}))
                .as_deref(),
            Some("You are too young.")
        );
        assert_eq!(underage_message(&serde_json::json!({"message": "x"})), None);
        let alert = underage_alert(None);
        assert_eq!(alert.title, "Discord is for ages 13 and up");
        let subtitle = alert.subtitle.expect("a subtitle");
        let text: String = subtitle.iter().map(|(run, _)| run.as_str()).collect();
        assert!(
            text.starts_with("You must be at least 13 years old to use Discord. You have 30 days")
        );
        assert!(subtitle.iter().any(|(run, bold)| *bold && run == "30 days"));
        assert_eq!(alert.buttons.len(), 2);
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn the_selects_follow_the_locale_format() {
        use Part::{Day, Month, Year};
        assert_eq!(field_order(long_date_format("en-US")), [Month, Day, Year]);
        assert_eq!(field_order(long_date_format("en-GB")), [Day, Month, Year]);
        assert_eq!(field_order(long_date_format("de")), [Day, Month, Year]);
        assert_eq!(field_order(long_date_format("bg")), [Day, Month, Year]);
        assert_eq!(field_order(long_date_format("ja")), [Year, Month, Day]);
        assert_eq!(field_order(long_date_format("hu")), [Year, Month, Day]);
        assert_eq!(field_order(long_date_format("sv-SE")), [Year, Month, Day]);
        // A locale module 608998 does not list keeps moment's `en`.
        assert_eq!(field_order(long_date_format("id")), [Month, Day, Year]);
        // A format without one of the letters falls back to day, month, year.
        assert_eq!(field_order("YYYY"), [Day, Month, Year]);
    }

    #[test]
    fn only_real_days_are_birthdays() {
        assert_eq!(entry(None, Some(2), Some(2000)), Entry::Incomplete);
        assert_eq!(entry(Some(30), Some(2), Some(2000)), Entry::Invalid);
        assert_eq!(entry(Some(29), Some(2), Some(2001)), Entry::Invalid);
        assert_eq!(entry(Some(31), Some(4), Some(2000)), Entry::Invalid);
        assert_eq!(
            entry(Some(29), Some(2), Some(2000)),
            Entry::Valid(date(2000, 2, 29))
        );
    }

    #[test]
    fn age_counts_whole_years_as_moment_does() {
        // The day before and the day of the 18th birthday.
        assert_eq!(age(date(2008, 5, 11), date(2026, 5, 10)), 17);
        assert_eq!(age(date(2008, 5, 10), date(2026, 5, 10)), 18);
        // A 29 February birthday turns 18 on 28 February of a common year (moment's clamped anchor).
        assert_eq!(age(date(2008, 2, 29), date(2026, 2, 27)), 17);
        assert_eq!(age(date(2008, 2, 29), date(2026, 2, 28)), 18);
        assert_eq!(age(date(2008, 2, 29), date(2026, 3, 1)), 18);
        // A 31st birthday in a 30-day month counts on the 30th.
        assert_eq!(age(date(2008, 3, 31), date(2026, 4, 30)), 18);
        // Months near the birthday.
        assert_eq!(age(date(2000, 12, 31), date(2026, 1, 1)), 25);
        assert_eq!(age(date(2026, 11, 20), date(2026, 11, 18)), 0);
    }

    #[test]
    fn the_body_is_the_iso_day() {
        assert_eq!(
            submit_body(date(2001, 3, 9)),
            json!({"date_of_birth": "2001-03-09"})
        );
    }

    #[test]
    fn the_request_goes_out_only_for_a_reported_account_outside_captures() {
        assert!(may_send(Reported::NoBirthday, false, false));
        assert!(!may_send(Reported::NoBirthday, true, false));
        assert!(!may_send(Reported::NoBirthday, false, true));
        assert!(!may_send(Reported::NotSeen, false, false));
        assert!(!may_send(Reported::Set(true), false, false));
        assert!(!may_send(Reported::Set(false), false, false));
    }

    #[test]
    fn the_gateway_field_is_three_states() {
        assert_eq!(reported_from(None), None);
        assert_eq!(
            reported_from(Some(&Value::Null)),
            Some(Reported::NoBirthday)
        );
        assert_eq!(reported_from(Some(&json!(true))), Some(Reported::Set(true)));
        assert_eq!(
            reported_from(Some(&json!(false))),
            Some(Reported::Set(false))
        );
    }

    #[test]
    fn the_invite_card_asks_new_accounts_on_age_restricted_servers() {
        let after = Some(CUTOFF_MS + 1);
        let before = Some(CUTOFF_MS - 1);
        assert!(invite_gate(after, Reported::NoBirthday, false, 3, false));
        assert!(invite_gate(after, Reported::NoBirthday, false, 1, false));
        assert!(!invite_gate(after, Reported::NoBirthday, false, 2, false));
        assert!(!invite_gate(after, Reported::NoBirthday, false, 3, true));
        assert!(!invite_gate(before, Reported::NoBirthday, false, 3, false));
        assert!(!invite_gate(after, Reported::Set(true), false, 3, false));
        assert!(invite_gate(None, Reported::Set(true), true, 3, false));
    }

    #[test]
    fn refusals_read_like_the_official_catch() {
        assert_eq!(failure(&json!({"date_of_birth": ["x"]})), Failure::Underage);
        assert_eq!(failure(&json!({"username": ["x"]})), Failure::Username);
        assert_eq!(
            failure(&json!({"message": "Nope"})),
            Failure::Message("Nope".to_owned())
        );
    }

    #[test]
    fn the_lists_hold_the_official_options() {
        assert_eq!(options(Part::Day, 2026).len(), 31);
        assert_eq!(options(Part::Month, 2026)[8].1, "September");
        let years = options(Part::Year, 2026);
        assert_eq!(years.len(), 101);
        assert_eq!(years[0].0, 2026);
        assert_eq!(years[100].0, 1926);
    }

    #[test]
    fn localized_months_typeahead_and_top_popout_match_201505() {
        assert_eq!(month_label_for("en-US", 1), "January");
        assert_eq!(month_label_for("tr", 1), "Ocak");
        assert_eq!(month_label_for("de", 3), "März");
        assert_eq!(month_label_for("ja", 12), "12月");

        assert_eq!(typeahead_match(Part::Month, 2026, "sep"), Some(9));
        assert_eq!(typeahead_match(Part::Day, 2026, "24"), Some(24));
        assert_eq!(typeahead_match(Part::Year, 2026, "1999"), Some(1999));

        // popoutPosition="top": prefers above when room exists, falls back below when cramped at top.
        assert_eq!(popout_top(300., 340., 250., 720.), 46.);
        assert_eq!(popout_top(100., 140., 250., 720.), 144.);
    }
}
