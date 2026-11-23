//! The Family Center row of the Home sidebar (`ID` of module 596339), its Disable Family Center
//! dialog (module 371158), and the teen Guardian QR Share Code widget (module 509914 / 207049).
//!
//! The row shows while the account's `familyCenterEnabledV2` privacy setting is on and the account may see NSFW
//! content (`shouldShowFamilyCenterRow`: module 253932 `dm` and module 945276, `nsfwAllowed`). Its close button (`Ib`,
//! `XSmallIcon`, "Close") opens the dialog: "Disable Family Center", the sentence whose link opens User Settings >
//! Family Center, Cancel and the critical Disable Family Center, which turns the setting off and goes to Friends.

use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::time::{Duration, Instant};

/// The privacy setting `familyCenterEnabledV2` (the Family Center panel's "Enable Family Center tab").
const ENABLED: &str = "family_center_enabled";
/// `GET /family-center/@me/link-code` (module 509914 / 899847).
pub(super) const LINK_CODE_ROUTE: &str = "/family-center/@me/link-code";
pub(super) const LINK_CODE_KEY: &str = "family_center:link_code";
/// Link code refresh cadence: 5 minutes (`300s`, module 509914).
pub(super) const LINK_CODE_TTL_SECS: u64 = 300;

/// Module 41527 (`191627.bo`): Family Center connection error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestConnectionFailure {
    GenericError = 0,
    IneligibleForFamilyCenter = 1,
    PendingRequestExists = 2,
    MaxConnectionsReached = 3,
}

impl RequestConnectionFailure {
    pub(crate) fn from_api_code(code: u64) -> Self {
        match code {
            290_000 => Self::IneligibleForFamilyCenter,
            290_005 => Self::PendingRequestExists,
            290_002 => Self::MaxConnectionsReached,
            _ => Self::GenericError,
        }
    }
}

/// Module 41527 (`h`): 8-state parent/guardian Request Connection modal state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RequestConnectionPhase {
    PrereqLoading,
    ConfirmAge,
    Verifying,
    MustBeAdult,
    InvalidCode,
    Request,
    Sent,
    Error(RequestConnectionFailure),
}

/// Module 41527: active Parent/Guardian Request Connection modal state.
#[derive(Clone, Debug)]
pub(crate) struct RequestConnectionModal {
    pub teen_id: String,
    pub link_code: String,
    pub teen_username: Option<String>,
    pub guardian_email: Option<String>,
    pub phase: RequestConnectionPhase,
    pub submitting: bool,
}

/// Module 58144: active Teen Accept Link modal state.
#[derive(Clone, Debug)]
pub(crate) struct AcceptLinkModal {
    pub other_user_id: String,
    pub other_user_name: String,
    pub submitting: bool,
    pub has_error: bool,
}

/// Module 179950 (`FamilyCenterDisconnectLinkModal`): two-checkbox disconnect confirmation state.
#[derive(Clone, Debug)]
pub(crate) struct DisconnectLinkModal {
    pub other_user_id: String,
    pub other_user_name: String,
    pub is_adult: bool,
    pub requires_consent: bool,
    pub ack1: bool,
    pub ack2: bool,
    pub submitting: bool,
    pub has_error: bool,
}

impl DisconnectLinkModal {
    pub(crate) fn can_disconnect(&self) -> bool {
        self.ack1 && self.ack2 && !self.submitting
    }
}

/// Module 362890 (`219893`): hour/minute pair for teen restricted downtime schedule rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DowntimeTime {
    pub hours: u8,
    pub minutes: u8,
}

impl DowntimeTime {
    pub(crate) fn total_minutes(self) -> u16 {
        u16::from(self.hours) * 60 + u16::from(self.minutes)
    }

    pub(crate) fn format_12h(self) -> String {
        let suffix = if self.hours >= 12 { "PM" } else { "AM" };
        let h12 = match self.hours % 12 {
            0 => 12,
            h => h,
        };
        format!("{h12}:{:02} {suffix}", self.minutes)
    }

    /// Module 362890: auto-computed end time `(start + 540) % 1440` (+9 hours).
    pub(crate) fn plus_nine_hours(self) -> Self {
        let m = (self.total_minutes() + 540) % 1440;
        Self {
            hours: (m / 60) as u8,
            minutes: (m % 60) as u8,
        }
    }
}

/// Module 362890 (`T`): parses typed `H:MM` or `H:M` (1-digit minute × 10) with AM/PM candidates.
#[allow(dead_code)]
pub(crate) fn parse_typed_downtime_time(raw: &str) -> Vec<DowntimeTime> {
    let trimmed = raw.trim();
    let Some((h_s, m_s)) = trimmed.split_once(':') else {
        return Vec::new();
    };
    if h_s.is_empty() || h_s.len() > 2 || m_s.is_empty() || m_s.len() > 2 {
        return Vec::new();
    }
    let (Ok(h), Ok(m_raw)) = (h_s.parse::<u8>(), m_s.parse::<u8>()) else {
        return Vec::new();
    };
    let m = if m_s.len() == 1 { m_raw * 10 } else { m_raw };
    if h >= 24 || m >= 60 {
        return Vec::new();
    }
    let mut out = vec![DowntimeTime {
        hours: h,
        minutes: m,
    }];
    if h == 12 {
        out.push(DowntimeTime {
            hours: 0,
            minutes: m,
        });
    } else if (1..12).contains(&h) {
        out.push(DowntimeTime {
            hours: h + 12,
            minutes: m,
        });
    }
    out
}

/// Module 362890 (`219893.a3`): formats whole-hour (`hFDcmZ`) or half-hour (`wcrXLM`) duration.
pub(crate) fn downtime_duration_label(start: DowntimeTime, end: DowntimeTime) -> String {
    let s = start.total_minutes();
    let e = end.total_minutes();
    let dur = if e > s { e - s } else { 1440 - s + e };
    let hours = dur / 60;
    if dur % 60 == 30 {
        dc_intl::format("wcrXLM", &[("hours", f64::from(hours).into())])
    } else {
        dc_intl::format("hFDcmZ", &[("hours", f64::from(hours).into())])
    }
}

/// Module 362890 (`vX7xid` same-day vs `R87Y2K` overnight): live schedule preview string.
pub(crate) fn downtime_schedule_preview(start: DowntimeTime, end: DowntimeTime) -> String {
    let overnight = start.total_minutes() > end.total_minutes();
    let key = if overnight { "R87Y2K" } else { "vX7xid" };
    let s = start.format_12h();
    let e = end.format_12h();
    dc_intl::format(
        key,
        &[
            ("startTime", s.as_str().into()),
            ("endTime", e.as_str().into()),
        ],
    )
}

/// Official day numbers (`76573.ob`: Sun=7, Mon=1..Sat=6) in UI Sun..Sat order.
pub(crate) const DOWNTIME_DAYS: [(u8, &str, &str); 7] = [
    (7, "Sun", "Sunday"),
    (1, "Mon", "Monday"),
    (2, "Tue", "Tuesday"),
    (3, "Wed", "Wednesday"),
    (4, "Thu", "Thursday"),
    (5, "Fri", "Friday"),
    (6, "Sat", "Saturday"),
];

/// Module 362890: existing restricted-schedule rule used for edit mode & overlap detection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DowntimeRule {
    pub rule_id: String,
    pub start: DowntimeTime,
    pub end: DowntimeTime,
    pub days: Vec<u8>,
    pub enabled: bool,
}

/// Module 362890: returns conflicting `"{dayLabel}  {start} – {end}"` lines when selected days
/// overlap another existing rule (`rule.rule_id != editing_rule_id`).
pub(crate) fn downtime_overlap_entries(
    selected_days: &[u8],
    existing_rules: &[DowntimeRule],
    editing_rule_id: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    for &(day_num, _, full_label) in &DOWNTIME_DAYS {
        if !selected_days.contains(&day_num) {
            continue;
        }
        for rule in existing_rules {
            if editing_rule_id == Some(rule.rule_id.as_str()) {
                continue;
            }
            if rule.days.contains(&day_num) {
                out.push(format!(
                    "{full_label}  {} – {}",
                    rule.start.format_12h(),
                    rule.end.format_12h()
                ));
            }
        }
    }
    out
}

/// Module 362890 (`95396`): `POST /family-center/{teen_id}/restricted-schedule/rule`.
pub(crate) fn create_downtime_rule_request(
    teen_id: &str,
    start: DowntimeTime,
    end: DowntimeTime,
    days: &[u8],
    enabled: bool,
) -> Request {
    Request::Rest {
        key: Some(format!("family-center:downtime-create:{teen_id}")),
        method: reqwest::Method::POST,
        route: format!("/family-center/{teen_id}/restricted-schedule/rule"),
        body: Some(serde_json::json!({
            "label": "",
            "start_time": { "hours": start.hours, "minutes": start.minutes, "seconds": 0, "nanos": 0 },
            "end_time": { "hours": end.hours, "minutes": end.minutes, "seconds": 0, "nanos": 0 },
            "days": days,
            "enabled": enabled,
        })),
    }
}

/// Module 362890 (`95396`): `PATCH /family-center/{teen_id}/restricted-schedule/rules/{rule_id}`.
pub(crate) fn update_downtime_rule_request(
    teen_id: &str,
    rule_id: &str,
    start: DowntimeTime,
    end: DowntimeTime,
    days: &[u8],
    enabled: bool,
) -> Request {
    Request::Rest {
        key: Some(format!("family-center:downtime-update:{teen_id}:{rule_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/family-center/{teen_id}/restricted-schedule/rules/{rule_id}"),
        body: Some(serde_json::json!({
            "label": "",
            "start_time": { "hours": start.hours, "minutes": start.minutes, "seconds": 0, "nanos": 0 },
            "end_time": { "hours": end.hours, "minutes": end.minutes, "seconds": 0, "nanos": 0 },
            "days": days,
            "enabled": enabled,
        })),
    }
}

/// Module 362890 (`95396`): `DELETE /family-center/{teen_id}/restricted-schedule/rules/{rule_id}`.
pub(crate) fn delete_downtime_rule_request(teen_id: &str, rule_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("family-center:downtime-delete:{teen_id}:{rule_id}")),
        method: reqwest::Method::DELETE,
        route: format!("/family-center/{teen_id}/restricted-schedule/rules/{rule_id}"),
        body: None,
    }
}

/// Module 362890 (`FamilyCenterDowntimeScheduleModal`): Create/Edit teen screen downtime modal state.
#[derive(Clone, Debug)]
pub(crate) struct DowntimeScheduleModal {
    pub teen_id: String,
    pub editing_rule_id: Option<String>,
    pub enabled: bool,
    pub start: DowntimeTime,
    pub end: DowntimeTime,
    pub selected_days: Vec<u8>,
    pub existing_rules: Vec<DowntimeRule>,
    pub submitting: bool,
}

/// Module 320742 (`FamilyCenterQRCodeModal`): decoy URL shown on blurred QR before reveal (`191627.EC`).
pub(crate) const TEEN_QR_DECOY_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
/// Module 320742: official Family Center learn-more article URL (`LyyCsE`).
pub(crate) const TEEN_QR_HELP_URL: &str = "https://support.discord.com/hc/articles/14155060633623";

/// Module 320742: teen-only blurred QR code modal state (`FamilyCenterQRCodeModal`).
#[derive(Clone, Debug, Default)]
pub(crate) struct TeenQrCodeModal {
    pub revealed: bool,
    pub loading: bool,
}

/// Module 320742: returns the QR payload URL (decoy until revealed with a valid link code).
pub(crate) fn teen_qr_payload_url(
    revealed: bool,
    user_id: dc_state::Id,
    link_code: Option<&str>,
) -> String {
    if revealed && let Some(code) = link_code.filter(|c| !c.is_empty()) {
        family_center_share_url(user_id, code)
    } else {
        TEEN_QR_DECOY_URL.to_owned()
    }
}

/// Module 270915 (`Change spending cap`): Family Center teen monthly spending cap modal state.
#[derive(Clone, Debug)]
pub(crate) struct SpendingCapModal {
    pub teen_id: String,
    pub currency_symbol: String,
    pub exponent: u32,
    pub amount_input: String,
    pub existing_cap_minor: Option<u64>,
    pub monthly_spend_minor: u64,
    pub renew_date_label: String,
    pub submitting: bool,
}

/// Module 270915 (`361157.cU`): sanitizes currency input to digits (and single decimal with up to `exponent` places).
pub(crate) fn sanitize_currency_input(raw: &str, exponent: u32) -> String {
    let mut out = String::new();
    let mut seen_dot = false;
    let mut frac_digits = 0u32;
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            if seen_dot {
                if frac_digits < exponent {
                    out.push(ch);
                    frac_digits += 1;
                }
            } else {
                out.push(ch);
            }
        } else if ch == '.' && exponent > 0 && !seen_dot {
            seen_dot = true;
            out.push('.');
        }
    }
    out
}

/// Module 270915: parses sanitized major-unit currency text to minor units (e.g. `"25.50"` w/ exp 2 -> `Some(2550)`).
pub(crate) fn parse_currency_minor_units(input: &str, exponent: u32) -> Option<u64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let val: f64 = trimmed.parse().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    let scale = 10_f64.powi(exponent as i32);
    Some((val * scale).round() as u64)
}

/// Formats minor units with currency symbol (e.g. `2550` w/ `"$"`, exp 2 -> `"$25.50"`).
pub(crate) fn format_minor_currency(symbol: &str, minor: u64, exponent: u32) -> String {
    if exponent == 0 {
        return format!("{symbol}{minor}");
    }
    let div = 10_u64.pow(exponent);
    format!(
        "{symbol}{}.{:0width$}",
        minor / div,
        minor % div,
        width = exponent as usize
    )
}

impl SpendingCapModal {
    pub(crate) fn is_clearing_cap(&self) -> bool {
        self.amount_input.trim().is_empty() && self.existing_cap_minor.is_some()
    }

    pub(crate) fn parsed_minor(&self) -> Option<u64> {
        let clean = sanitize_currency_input(&self.amount_input, self.exponent);
        parse_currency_minor_units(&clean, self.exponent)
    }

    pub(crate) fn is_overspending(&self) -> bool {
        self.monthly_spend_minor > 0
            && self
                .parsed_minor()
                .is_some_and(|cap| cap <= self.monthly_spend_minor)
    }

    pub(crate) fn can_save(&self) -> bool {
        !self.submitting && (self.is_clearing_cap() || self.parsed_minor().is_some())
    }
}

/// Module 270915 (`Rsh.FAMILY_CENTER_TEEN_SETTINGS`): `PATCH /family-center/{teenId}/settings-proto`.
pub(crate) fn family_center_spending_cap_request(
    teen_id: &str,
    settings_b64: &str,
) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("family-center:spending-cap:{teen_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/family-center/{teen_id}/settings-proto"),
        body: Some(serde_json::json!({ "settings": settings_b64 })),
    }
}

/// Module 41527: `GET /family-center/connection-prerequisites?teen_id={teen_id}&link_code={link_code}`.
#[allow(dead_code)]
pub(crate) fn family_center_prereqs_request(teen_id: &str, link_code: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("family-center:prereqs:{teen_id}")),
        method: reqwest::Method::GET,
        route: format!(
            "/family-center/connection-prerequisites?teen_id={teen_id}&link_code={link_code}"
        ),
        body: None,
    }
}

/// Module 41527: `POST /users/@me/linked-users` (`{ recipient_id, code }`).
pub(crate) fn family_center_send_link_request(
    teen_id: &str,
    link_code: &str,
) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("family-center:send-link:{teen_id}")),
        method: reqwest::Method::POST,
        route: "/users/@me/linked-users".to_owned(),
        body: Some(serde_json::json!({
            "recipient_id": teen_id,
            "code": link_code,
        })),
    }
}

/// Module 58144 (`899847.nt`): `PATCH /users/@me/linked-users` (`{ linked_user_id, link_status: 2 }`).
pub(crate) fn family_center_accept_link_request(other_user_id: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("family-center:accept-link:{other_user_id}")),
        method: reqwest::Method::PATCH,
        route: "/users/@me/linked-users".to_owned(),
        body: Some(serde_json::json!({
            "linked_user_id": other_user_id,
            "link_status": 2,
        })),
    }
}

/// Module 179950 (`899847.nt`, `Ef.INACTIVE = 3`): `PATCH /users/@me/linked-users` (`{ linked_user_id, link_status: 3 }`).
pub(crate) fn family_center_disconnect_link_request(other_user_id: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("family-center:disconnect-link:{other_user_id}")),
        method: reqwest::Method::PATCH,
        route: "/users/@me/linked-users".to_owned(),
        body: Some(serde_json::json!({
            "linked_user_id": other_user_id,
            "link_status": 3,
        })),
    }
}

fn fc_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() && s != key {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

#[derive(Default)]
pub(super) struct State {
    /// The Disable Family Center dialog is open.
    pub disable: bool,
    /// Module 207049 / 509914: Guardian QR Share Code modal is open.
    pub share_modal: bool,
    /// Module 41527: Parent/Guardian Request Connection modal.
    pub request_connection: Option<RequestConnectionModal>,
    /// Module 58144: Teen Accept Link modal.
    pub accept_link: Option<AcceptLinkModal>,
    /// Module 179950: Family Center Disconnect Link modal.
    pub disconnect_link: Option<DisconnectLinkModal>,
    /// Module 270915: Family Center Teen Spending Cap modal.
    pub spending_cap: Option<SpendingCapModal>,
    /// Module 320742: Family Center Teen QR Code modal (`FamilyCenterQRCodeModal`).
    pub teen_qr_modal: Option<TeenQrCodeModal>,
    /// Module 362890: Family Center Teen Screen Downtime Schedule Editor modal.
    pub downtime_schedule: Option<Box<DowntimeScheduleModal>>,
    /// Cached link code (override or fetched from `GET /family-center/@me/link-code`).
    pub link_code: Option<String>,
    /// Timestamp when the current link code was acquired (for 5-minute `M:SS` countdown).
    pub link_code_fetched_at: Option<Instant>,
    /// Shows `"+5kSoW"` (`"Link Copied!"`) until this instant after clicking Copy Link.
    pub copied_until: Option<Instant>,
    /// Guard so only one 1-second countdown refresh timer is running at a time.
    pub ticker_active: bool,
}

/// Official Family Center guardian invite URL (`https://discord.com/feature/family-center/my-family/{userId}/{linkCode}`).
pub(super) fn family_center_share_url(user_id: dc_state::Id, link_code: &str) -> String {
    format!("https://discord.com/feature/family-center/my-family/{user_id}/{link_code}")
}

/// Formats remaining seconds until link code rotation as `M:SS` (e.g. `4:59`).
pub(super) fn format_link_code_countdown(remaining_secs: u64) -> String {
    format!("{}:{:02}", remaining_secs / 60, remaining_secs % 60)
}

/// `shouldShowFamilyCenterRow` of module 596339: the setting is on and the account may see NSFW content.
pub(super) fn row_shown(enabled: bool, nsfw_allowed: bool) -> bool {
    enabled && nsfw_allowed
}

impl Root {
    pub(super) fn family_center_row_shown(&self) -> bool {
        row_shown(
            self.pref_on(ENABLED, false),
            self.stores.account.nsfw_allowed,
        )
    }

    /// The close button of the row (`Ib`): the dialog of module 371158.
    pub(super) fn open_disable_family_center(&mut self, cx: &mut Context<Self>) {
        self.family_center.disable = true;
        cx.notify();
    }

    /// Disable Family Center: `dm.updateSetting(false)`, then Friends (`pX(FRIENDS)`) and the dialog closes.
    fn disable_family_center(&mut self, cx: &mut Context<Self>) {
        self.family_center.disable = false;
        self.set_family_center_enabled(false);
        self.select_home(None);
        cx.notify();
    }

    /// Module 371158 on the dialog of module 189213 (`sm`: its only child is the error line, empty until a failure).
    pub(super) fn disable_family_center_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.family_center.disable {
            return None;
        }
        // `CrZZ+p` with its `openSettingsHook` drawn as an anchor.
        let runs = dc_intl::format_runs("CrZZ+p", &[]);
        let mut text = String::new();
        let mut link = 0..0;
        for (run, hook) in runs {
            let start = text.len();
            text.push_str(&run);
            if hook.as_deref() == Some("openSettingsHook") {
                link = start..text.len();
            }
        }
        let styled = gpui::StyledText::new(text).with_highlights([(
            link.clone(),
            gpui::HighlightStyle {
                color: Some(color("--text-link").into()),
                ..Default::default()
            },
        )]);
        let subtitle = div()
            .child(
                gpui::InteractiveText::new(
                    SharedString::from("disable-family-center-subtitle"),
                    styled,
                )
                .on_click(vec![link], {
                    let open = cx.listener(|this, _: &usize, _, cx| {
                        this.family_center.disable = false;
                        this.open_family_center_panel(cx);
                    });
                    move |index, window, cx| open(&index, window, cx)
                }),
            )
            .into_any_element();
        let spec = modal::Spec {
            id: "disable-family-center",
            title: dc_intl::t("VGB30H").into(),
            subtitle: Some(subtitle),
            body: None,
            buttons: vec![
                modal::button(
                    "disable-family-center-cancel",
                    dc_intl::t("ETE/oC"),
                    Tone::Secondary,
                    |this, _, _| this.family_center.disable = false,
                ),
                modal::button(
                    "disable-family-center-confirm",
                    dc_intl::t("cSb7U0"),
                    Tone::Critical,
                    |this, _, cx| this.disable_family_center(cx),
                ),
            ],
            close: Box::new(|this, _, _| this.family_center.disable = false),
        };
        Some(modal::render_small(spec, cx))
    }

    /// Request a fresh link code (`GET /family-center/@me/link-code`) and start the 1s countdown ticker.
    pub(super) fn request_family_center_link_code(&mut self, cx: &mut Context<Self>) {
        self.family_center.link_code_fetched_at = Some(Instant::now());
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(LINK_CODE_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: LINK_CODE_ROUTE.to_owned(),
            body: None,
        });
        if !self.family_center.ticker_active {
            self.family_center.ticker_active = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let keep_going = this
                        .update(cx, |root, cx| {
                            let active = root.family_center.share_modal
                                || root.settings_panel_is_family_center();
                            if !active {
                                root.family_center.ticker_active = false;
                                return false;
                            }
                            if let Some(fetched_at) = root.family_center.link_code_fetched_at
                                && fetched_at.elapsed().as_secs() >= LINK_CODE_TTL_SECS
                            {
                                root.family_center.link_code_fetched_at = Some(Instant::now());
                                let _ = root.requests.send(crate::net::Request::Rest {
                                    key: Some(LINK_CODE_KEY.to_owned()),
                                    method: reqwest::Method::GET,
                                    route: LINK_CODE_ROUTE.to_owned(),
                                    body: None,
                                });
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep_going {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn settings_panel_is_family_center(&self) -> bool {
        self.settings.is_some()
    }

    fn active_family_link_code(&self) -> Option<String> {
        self.family_center.link_code.clone().or_else(|| {
            self.fetched
                .get(LINK_CODE_KEY)
                .and_then(|v| v["link_code"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
    }

    /// Module 509914 `r`: Teen Guardian QR Share Code card (`card_PK0nhg`):
    /// - `"Share code with guardian"` (`pojgfk`)
    /// - `"New code in:"` (`RfkLDs`) + `M:SS` live countdown
    /// - 200x200 QR code via `login::qr_code_card` with `.aria_label("Mi60fm")`
    /// - `"or share link"` (`TGr8Ws`) divider + read-only URL box + Copy Link (`WqhZss` -> `+5kSoW`)
    pub(super) fn family_center_share_code_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let user_id = self.stores.current_user.as_ref().map(|u| u.id).unwrap_or(0);
        let code = self
            .active_family_link_code()
            .unwrap_or_else(|| "LINK-CODE-LOADING".to_owned());
        let url = family_center_share_url(user_id, &code);
        let elapsed = self
            .family_center
            .link_code_fetched_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let remaining = LINK_CODE_TTL_SECS.saturating_sub(elapsed);
        let countdown = format_link_code_countdown(remaining);
        let copied = self
            .family_center
            .copied_until
            .is_some_and(|until| Instant::now() < until);
        let copy_url = url.clone();
        let qr = super::login::qr_code_card(&url, 200., 8., 4.);

        div()
            .id("family-center-share-code-card")
            .p(px(24.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("pojgfk")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("RfkLDs"))
                            .child(
                                div()
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(countdown),
                            ),
                    ),
            )
            .child(
                div()
                    .id("family-center-qr-wrap")
                    .aria_label(dc_intl::t("Mi60fm"))
                    .p(px(8.))
                    .rounded(px(12.))
                    .bg(rgba(0xffffffff))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(qr),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("TGr8Ws")),
                    )
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle"))),
            )
            .child(
                div()
                    .w_full()
                    .h(px(44.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .text_color(color("--text-default"))
                            .truncate()
                            .child(url),
                    )
                    .child(
                        div()
                            .id("family-center-copy-link")
                            .flex_none()
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(if copied {
                                color("--status-positive")
                            } else {
                                color("--control-primary-background-default")
                            })
                            .hover(|s| {
                                s.bg(if copied {
                                    color("--status-positive")
                                } else {
                                    color("--control-primary-background-hover")
                                })
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(if copied {
                                dc_intl::t("+5kSoW")
                            } else {
                                dc_intl::t("WqhZss")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    copy_url.clone(),
                                ));
                                this.family_center.copied_until =
                                    Some(Instant::now() + Duration::from_secs(1));
                                this.put_app_toast(dc_intl::t("+5kSoW").to_owned(), true, cx);
                                cx.spawn(async move |weak, cx| {
                                    cx.background_executor().timer(Duration::from_secs(1)).await;
                                    let _ = weak.update(cx, |root, cx| {
                                        root.family_center.copied_until = None;
                                        cx.notify();
                                    });
                                })
                                .detach();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Module 207049 modal shell wrapping module 509914 Guardian QR Share Code (`ITlV6p` / `F4GT2S`).
    pub(super) fn family_center_share_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.family_center.share_modal {
            return None;
        }
        let body = self.family_center_share_code_card(cx);
        let dialog = div()
            .id("family-center-share-modal")
            .occlude()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--modal-background"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("ITlV6p")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("F4GT2S")),
                            ),
                    )
                    .child(
                        div()
                            .id("family-center-share-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.family_center.share_modal = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().px(px(20.)).pb(px(20.)).child(body));
        Some(
            deferred(
                div()
                    .id("family-center-share-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.family_center.share_modal = false;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(94)
            .into_any_element(),
        )
    }

    /// The row's close button (`closeButton_zln_tS`, the private channel row's): 18px, 8px from the end, at 0.7 while
    /// the row is hovered.
    pub(super) fn family_center_close_button(
        &self,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("family-center-close")
            .aria_label(dc_intl::t("cpT0Cq"))
            .mr(px(8.))
            .size(px(18.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.)
            .group_hover(group, |style| style.opacity(0.7))
            .hover(|style| style.opacity(1.))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.open_disable_family_center(cx);
            }))
            .child(super::icon("friend_ignore", 16., "--text-muted"))
    }
}

#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_family_center(&mut self, which: &str, cx: &mut Context<Self>) -> String {
        match which {
            "on" => self.set_family_center_enabled(true),
            // The fixture account may not see NSFW content; a stand-in so the row can be framed.
            "adult" => self.stores.account.nsfw_allowed = true,
            "home" => self.select_home(None),
            "disable" => self.family_center.disable = true,
            "teen" => {
                self.stores.account.nsfw_allowed = false;
                self.family_center.link_code = Some("GUARDIAN-QR-2026".to_owned());
                self.family_center.link_code_fetched_at = Some(Instant::now());
            }
            "share" => {
                self.stores.account.nsfw_allowed = false;
                self.family_center.link_code = Some("GUARDIAN-QR-2026".to_owned());
                self.family_center.link_code_fetched_at = Some(Instant::now());
                self.family_center.share_modal = true;
            }
            "share-close" => self.family_center.share_modal = false,
            _ => return format!("family-center: unknown step {which}"),
        }
        cx.notify();
        format!(
            "family-center: row {} dialog {}",
            self.family_center_row_shown(),
            self.family_center.disable
        )
    }
}

impl Root {
    pub(crate) fn close_family_center_modals(&mut self) -> bool {
        let a = self.family_center.request_connection.take().is_some();
        let b = self.family_center.accept_link.take().is_some();
        let c = self.family_center.disconnect_link.take().is_some();
        let d = self.family_center.spending_cap.take().is_some();
        let e = self.family_center.teen_qr_modal.take().is_some();
        let f = self.family_center.downtime_schedule.take().is_some();
        a || b || c || d || e || f
    }

    /// Handles REST responses for `POST /users/@me/linked-users` (module 41527) and
    /// `PATCH /users/@me/linked-users` (modules 58144 & 179950), mapping API error codes.
    pub(super) fn handle_family_center_fetched(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if key.starts_with("family-center:send-link:") {
            if let Some(m) = self.family_center.request_connection.as_mut() {
                m.submitting = false;
                if let Some(code) = body["code"].as_u64() {
                    m.phase = RequestConnectionPhase::Error(
                        RequestConnectionFailure::from_api_code(code),
                    );
                } else {
                    m.phase = RequestConnectionPhase::Sent;
                }
                cx.notify();
            }
            return true;
        }
        if key.starts_with("family-center:accept-link:") {
            if let Some(m) = self.family_center.accept_link.as_mut() {
                m.submitting = false;
                if body["code"].as_u64().is_some() {
                    m.has_error = true;
                } else {
                    self.family_center.accept_link = None;
                }
                cx.notify();
            }
            return true;
        }
        if key.starts_with("family-center:disconnect-link:") {
            if let Some(m) = self.family_center.disconnect_link.as_mut() {
                m.submitting = false;
                if body["code"].as_u64().is_some() {
                    m.has_error = true;
                    // Official 179950 catch handler resets both acknowledgement checkboxes on error.
                    m.ack1 = false;
                    m.ack2 = false;
                } else {
                    self.family_center.disconnect_link = None;
                }
                cx.notify();
            }
            return true;
        }
        if key.starts_with("family-center:spending-cap:") {
            if let Some(m) = self.family_center.spending_cap.as_mut() {
                m.submitting = false;
                if body["code"].as_u64().is_some() {
                    self.show_app_toast(dc_intl::t("Wu8BK2"), cx);
                } else {
                    self.family_center.spending_cap = None;
                    self.show_success_toast(dc_intl::t("2WKfG1").to_owned(), cx);
                }
                cx.notify();
            }
            return true;
        }
        false
    }

    /// Module 41527 (`FamilyCenterRequestConnectionModal`, CSS `936223`):
    /// 8-state Parent/Guardian connection flow (`PREREQ_LOADING`, `CONFIRM_AGE`, `VERIFYING`,
    /// `MUST_BE_ADULT`, `INVALID_CODE`, `REQUEST`, `SENT`, `ERROR`).
    pub(in crate::ui) fn family_center_request_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.request_connection.as_ref()?;
        let teen_name = modal
            .teen_username
            .clone()
            .unwrap_or_else(|| modal.teen_id.clone());
        let teen_initial = teen_name
            .chars()
            .next()
            .unwrap_or('T')
            .to_ascii_uppercase()
            .to_string();
        let guardian_initial = self
            .stores
            .current_user
            .as_ref()
            .and_then(|u| u.username.chars().next())
            .unwrap_or('G')
            .to_ascii_uppercase()
            .to_string();
        let teen_id = modal.teen_id.clone();
        let link_code = modal.link_code.clone();
        let submitting = modal.submitting;

        let dual_avatars = div()
            .px(px(10.))
            .py(px(8.))
            .rounded_full()
            .bg(color("--background-surface-higher"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(color("--control-primary-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(18.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--white"))
                    .child(guardian_initial),
            )
            .child(
                div()
                    .mx(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(div().size(px(4.)).rounded_full().bg(color("--icon-muted")))
                    .child(div().size(px(4.)).rounded_full().bg(color("--icon-muted")))
                    .child(div().size(px(4.)).rounded_full().bg(color("--icon-muted"))),
            )
            .child(
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(rgba(0x35ed7e55))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(18.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(teen_initial),
            );

        let content: AnyElement = match modal.phase {
            RequestConnectionPhase::PrereqLoading | RequestConnectionPhase::Verifying => div()
                .min_h(px(160.))
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("ZTNur7")),
                )
                .into_any_element(),
            RequestConnectionPhase::InvalidCode => div()
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_center()
                        .text_size(px(20.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(fc_copy("ewSb6o", "This link has expired")),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(fc_copy(
                            "jcUN2F",
                            "Guardian connection links expire after 1 hour. Ask your teen to send you a new link, or scan the QR code in their Family Center.",
                        )),
                )
                .child(
                    div()
                        .id("fc-invalid-dismiss")
                        .mt(px(8.))
                        .h(px(38.))
                        .px(px(20.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("WAI6xu"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.family_center.request_connection = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            RequestConnectionPhase::ConfirmAge => div()
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_center()
                        .text_size(px(20.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::format(
                            "pQQMJ7",
                            &[("username", teen_name.as_str().into())],
                        )),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(fc_copy(
                            "BQFHXW",
                            "You must be 18 or older to be a connected guardian",
                        )),
                )
                .child(
                    div()
                        .id("fc-confirm-age-cta")
                        .mt(px(8.))
                        .h(px(38.))
                        .px(px(20.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(fc_copy("3oUE4o", "Confirm Your Age"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.family_center.request_connection = None;
                            this.open_age_verification_modal(
                                super::age_verification_modal::AgeVerificationEntryPoint::FamilyCenterConnection,
                                cx,
                            );
                        })),
                )
                .into_any_element(),
            RequestConnectionPhase::MustBeAdult => div()
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_center()
                        .text_size(px(20.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(fc_copy(
                            "BQFHXW",
                            "You must be 18 or older to be a connected guardian",
                        )),
                )
                .child(
                    div()
                        .id("fc-must-be-adult-ok")
                        .mt(px(8.))
                        .h(px(38.))
                        .px(px(20.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("NX+WJN"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.family_center.request_connection = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            RequestConnectionPhase::Sent => {
                let email = modal
                    .guardian_email
                    .as_deref()
                    .unwrap_or("your email");
                let body = dc_intl::format("dVtWId", &[("email", email.into())]);
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(56.))
                            .rounded_full()
                            .bg(rgba(0x35ed7e33))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("circle_check", 28., "--status-positive")),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(fc_copy("EpwfZl", "Connection request sent!")),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(body),
                    )
                    .child(
                        div()
                            .id("fc-sent-close")
                            .mt(px(8.))
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("cpT0Cq"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.family_center.request_connection = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            RequestConnectionPhase::Error(fail) => {
                let (title, body) = match fail {
                    RequestConnectionFailure::IneligibleForFamilyCenter => (
                        fc_copy("W9JLJh", "You must be over 18 to connect with this teen as a parent"),
                        fc_copy("Uss2dn", "Please try again later."),
                    ),
                    RequestConnectionFailure::PendingRequestExists => (
                        fc_copy("tu0MRv", "Connection request already sent!"),
                        fc_copy("Xyjnwn", "Your teen can accept your request in the Family Center."),
                    ),
                    RequestConnectionFailure::MaxConnectionsReached => (
                        fc_copy("bS5x94", "You've reached your connection limit!"),
                        fc_copy("Uss2dn", "Please try again later."),
                    ),
                    RequestConnectionFailure::GenericError => (
                        fc_copy("nGX8Co", "Something went wrong!"),
                        fc_copy("Uss2dn", "Please try again later."),
                    ),
                };
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_center()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(14.))
                            .text_color(color("--text-muted"))
                            .child(body),
                    )
                    .child(
                        div()
                            .id("fc-error-dismiss")
                            .mt(px(8.))
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("WAI6xu"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.family_center.request_connection = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            RequestConnectionPhase::Request => {
                let consent = dc_intl::format(
                    "yiUJNU",
                    &[("username", teen_name.as_str().into())],
                );
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(28.))
                            .pt(px(28.))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(dual_avatars)
                            .child(
                                div()
                                    .mt(px(8.))
                                    .text_center()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(fc_copy("sMmIbm", "Connect with your Teen")),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(teen_name),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .px(px(12.))
                                    .text_center()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(consent),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("fc-request-cancel")
                                    .h(px(38.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("ETE/oC"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !submitting {
                                            this.family_center.request_connection = None;
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("fc-request-send")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(submitting, |d| d.opacity(0.6))
                                    .when(!submitting, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(fc_copy("ISg34l", "Send Connection Request"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) =
                                            this.family_center.request_connection.as_mut()
                                        {
                                            if m.submitting {
                                                return;
                                            }
                                            m.submitting = true;
                                            let _ = this.requests.send(
                                                family_center_send_link_request(
                                                    &teen_id, &link_code,
                                                ),
                                            );
                                            m.submitting = false;
                                            m.phase = RequestConnectionPhase::Sent;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        Some(
            deferred(
                div()
                    .id("family-center-request-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !submitting {
                                this.family_center.request_connection = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-request-modal")
                            .w(px(460.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(content),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 58144 (`FamilyCenterAcceptLinkModal`, CSS `67992`):
    /// Teen confirmation modal accepting a pending parent/guardian connection request (`PATCH /users/@me/linked-users`).
    pub(in crate::ui) fn family_center_accept_link_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.accept_link.as_ref()?;
        let other_id = modal.other_user_id.clone();
        let other_name = modal.other_user_name.clone();
        let other_initial = other_name
            .chars()
            .next()
            .unwrap_or('P')
            .to_ascii_uppercase()
            .to_string();
        let teen_initial = self
            .stores
            .current_user
            .as_ref()
            .and_then(|u| u.username.chars().next())
            .unwrap_or('Y')
            .to_ascii_uppercase()
            .to_string();
        let submitting = modal.submitting;
        let has_error = modal.has_error;
        let disclaimer = dc_intl::format("snlFqR", &[("username", other_name.as_str().into())]);

        Some(
            deferred(
                div()
                    .id("family-center-accept-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !submitting {
                                this.family_center.accept_link = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-accept-modal")
                            .w(px(460.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .aria_label(SharedString::from(fc_copy(
                                "rlNJwZ",
                                "Confirm parent connection with:",
                            )))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            // Optional critical alert banner (`pegSM5`)
                            .when(has_error, |card| {
                                card.child(
                                    div()
                                        .px(px(16.))
                                        .py(px(10.))
                                        .bg(color("--input-background-error-default"))
                                        .border_b_1()
                                        .border_color(color("--input-border-error-default"))
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(fc_copy(
                                            "pegSM5",
                                            "There was an error accepting the request. Please try again later.",
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .px(px(10.))
                                            .py(px(8.))
                                            .rounded_full()
                                            .bg(color("--background-surface-higher"))
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .size(px(48.))
                                                    .rounded_full()
                                                    .bg(color("--control-primary-background-default"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .child(teen_initial),
                                            )
                                            .child(icon(
                                                "attachment_link",
                                                20.,
                                                "--interactive-text-default",
                                            ))
                                            .child(
                                                div()
                                                    .size(px(48.))
                                                    .rounded_full()
                                                    .bg(rgba(0xe060a855))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(other_initial),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(fc_copy(
                                                "rlNJwZ",
                                                "Confirm parent connection with:",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                            .child(other_name),
                                    )
                                    .child(
                                        div()
                                            .mt(px(12.))
                                            .text_center()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(disclaimer),
                                    ),
                            )
                            // Full-width side-by-side footer (`actionsFullWidth: true`)
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("fc-accept-cancel")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .when(submitting, |d| d.opacity(0.5))
                                            .when(!submitting, |d| d.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !submitting {
                                                    this.family_center.accept_link = None;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fc-accept-confirm")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(submitting, |d| d.opacity(0.5))
                                            .when(!submitting, |d| d.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(fc_copy("wI/jo3", "Accept Request"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) =
                                                    this.family_center.accept_link.as_mut()
                                                {
                                                    if m.submitting {
                                                        return;
                                                    }
                                                    m.has_error = false;
                                                    m.submitting = true;
                                                }
                                                let _ = this.requests.send(
                                                    family_center_accept_link_request(&other_id),
                                                );
                                                this.family_center.accept_link = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 179950 (`FamilyCenterDisconnectLinkModal`, CSS `815298` / `756308`):
    /// Two-checkbox disconnect confirmation modal (`480px`, `o0JXuK`, adult/teen copy & consent warning).
    pub(in crate::ui) fn family_center_disconnect_link_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.disconnect_link.as_ref()?;
        let username = modal.other_user_name.clone();
        let other_id = modal.other_user_id.clone();
        let is_adult = modal.is_adult;
        let requires_consent = modal.requires_consent;
        let ack1 = modal.ack1;
        let ack2 = modal.ack2;
        let submitting = modal.submitting;
        let has_error = modal.has_error;
        let can_disconnect = modal.can_disconnect();

        let me_initial: String = self
            .stores
            .current_user
            .as_ref()
            .and_then(|u| u.display_name().chars().next())
            .unwrap_or('U')
            .to_uppercase()
            .collect();
        let other_initial: String = username
            .chars()
            .next()
            .unwrap_or('G')
            .to_uppercase()
            .collect();

        let title = dc_intl::format("o0JXuK", &[("username", username.as_str().into())]);
        let subtitle = if is_adult {
            dc_intl::t("WH+Gba").to_owned()
        } else {
            dc_intl::format("F2lccv", &[("username", username.as_str().into())])
        };
        let consent_notice = requires_consent.then(|| {
            if is_adult {
                dc_intl::format("Or6hgl", &[("username", username.as_str().into())])
            } else {
                dc_intl::t("hOEHFn").to_owned()
            }
        });
        let body_text = if is_adult {
            dc_intl::format("PlrZal", &[("username", username.as_str().into())])
        } else {
            dc_intl::format("XyRW4c", &[("username", username.as_str().into())])
        };
        let ack1_label = dc_intl::t(if is_adult { "PGQBnk" } else { "eiABQz" });
        let ack2_label = dc_intl::t(if is_adult { "0ki7+P" } else { "sCbKs4" });

        let ack_checkbox = |id: &'static str, checked: bool, label: &'static str, which: u8| {
            div()
                .id(id)
                .flex()
                .items_start()
                .gap(px(10.))
                .when(!submitting, |d| d.cursor_pointer())
                .when(submitting, |d| d.opacity(0.6))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = this.family_center.disconnect_link.as_mut() {
                        if m.submitting {
                            return;
                        }
                        if which == 1 {
                            m.ack1 = !m.ack1;
                        } else {
                            m.ack2 = !m.ack2;
                        }
                        m.has_error = false;
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .mt(px(1.))
                        .size(px(18.))
                        .flex_none()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color(if checked {
                            "--control-primary-background-default"
                        } else {
                            "--border-strong"
                        }))
                        .bg(if checked {
                            color("--control-primary-background-default")
                        } else {
                            rgba(0x00000000)
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(checked, |d| {
                            d.child(icon("checkmark_large", 12., "--white"))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(label),
                )
        };

        Some(
            deferred(
                div()
                    .id("family-center-disconnect-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !submitting {
                                this.family_center.disconnect_link = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-disconnect-modal")
                            .occlude()
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .when(has_error, |card| {
                                card.child(
                                    div()
                                        .px(px(16.))
                                        .py(px(10.))
                                        .bg(color("--input-background-error-default"))
                                        .border_b_1()
                                        .border_color(color("--input-border-error-default"))
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("krIDdy")),
                                )
                            })
                            .child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(20.))
                                    // `.header_5noEM3` + `.avatarPair_W7mUFH`
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(16.))
                                            .child(
                                                div()
                                                    .p(px(8.))
                                                    .rounded_full()
                                                    .bg(color("--background-surface-higher"))
                                                    .border_1()
                                                    .border_color(color("--border-muted"))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(12.))
                                                    .child(
                                                        div()
                                                            .size(px(48.))
                                                            .rounded_full()
                                                            .bg(color(
                                                                "--control-primary-background-default",
                                                            ))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(18.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--white"))
                                                            .child(me_initial),
                                                    )
                                                    .child(icon(
                                                        "modal_close",
                                                        18.,
                                                        "--text-feedback-critical",
                                                    ))
                                                    .child(
                                                        div()
                                                            .size(px(48.))
                                                            .rounded_full()
                                                            .bg(rgba(0xe060a855))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(18.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(other_initial),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .text_center()
                                                    .child(
                                                        div()
                                                            .text_size(px(18.))
                                                            .line_height(px(22.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(title),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-muted"))
                                                            .child(subtitle),
                                                    ),
                                            ),
                                    )
                                    .children(consent_notice.map(|notice| {
                                        div()
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--status-warning"))
                                            .bg(rgba(0xf0b2321a))
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-default"))
                                            .child(notice)
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(body_text),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.))
                                            .child(ack_checkbox("fc-disconnect-ack1", ack1, ack1_label, 1))
                                            .child(ack_checkbox("fc-disconnect-ack2", ack2, ack2_label, 2)),
                                    ),
                            )
                            // Full-width footer (`actionsFullWidth: true`)
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("fc-disconnect-cancel")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .when(!submitting, |d| d.cursor_pointer())
                                            .when(submitting, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("3ilveh"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !submitting {
                                                    this.family_center.disconnect_link = None;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fc-disconnect-confirm")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color("--control-critical-primary-background-default"))
                                            .when(can_disconnect, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(color(
                                                        "--control-critical-primary-background-hover",
                                                    ))
                                                })
                                            })
                                            .when(!can_disconnect, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("c5L+sl"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) =
                                                    this.family_center.disconnect_link.as_mut()
                                                {
                                                    if !m.can_disconnect() {
                                                        return;
                                                    }
                                                    m.has_error = false;
                                                    m.submitting = true;
                                                } else {
                                                    return;
                                                }
                                                let _ = this.requests.send(
                                                    family_center_disconnect_link_request(&other_id),
                                                );
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 270915 (`Change spending cap`, CSS `681364` `0zqV5Z`):
    /// Modal (`size="md"` 480px) to set or disable a connected teen's monthly purchase cap, with
    /// overspending warning state (`--background-feedback-mod-warning` / `--icon-feedback-warning`).
    pub(in crate::ui) fn family_center_spending_cap_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.spending_cap.as_ref()?;
        let submitting = modal.submitting;
        let is_clearing = modal.is_clearing_cap();
        let is_overspending = modal.is_overspending();
        let can_save = modal.can_save();
        let teen_id = modal.teen_id.clone();
        let symbol = modal.currency_symbol.clone();
        let input_text = modal.amount_input.clone();
        let renew_line = dc_intl::format(
            "IWdrfp",
            &[("date", modal.renew_date_label.as_str().into())],
        );
        let monthly_fmt = format_minor_currency(
            &modal.currency_symbol,
            modal.monthly_spend_minor,
            modal.exponent,
        );
        let monthly_note = (modal.monthly_spend_minor > 0).then(|| {
            if is_overspending {
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(icon("warning_triangle", 16., "--icon-feedback-warning"))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--icon-feedback-warning"))
                            .child(dc_intl::format(
                                "Tk6x4X",
                                &[
                                    ("amount", monthly_fmt.as_str().into()),
                                    ("date", modal.renew_date_label.as_str().into()),
                                ],
                            )),
                    )
                    .into_any_element()
            } else {
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::format(
                        "pfAlRY",
                        &[("amount", monthly_fmt.as_str().into())],
                    ))
                    .into_any_element()
            }
        });
        let primary_label = dc_intl::t(if is_clearing { "JZDGJ8" } else { "R3BPH+" });
        let primary_bg = if is_clearing {
            "--control-critical-primary-background-default"
        } else {
            "--control-primary-background-default"
        };
        let primary_hover = if is_clearing {
            "--control-critical-primary-background-hover"
        } else {
            "--control-primary-background-hover"
        };

        Some(
            deferred(
                div()
                    .id("family-center-spending-cap-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !submitting {
                                this.family_center.spending_cap = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-spending-cap-modal")
                            .occlude()
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("oBkFiL")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .text_color(color("--text-subtle"))
                                                    .child(dc_intl::t("IFguF2")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("fc-cap-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                16.,
                                                "--interactive-text-default",
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !submitting {
                                                    this.family_center.spending_cap = None;
                                                    cx.notify();
                                                }
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("gMeekL")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(renew_line),
                                    )
                                    .child(
                                        div()
                                            .id("fc-cap-input")
                                            .h(px(40.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color(if is_overspending {
                                                "--icon-feedback-warning"
                                            } else {
                                                "--input-border-default"
                                            }))
                                            .bg(color(if is_overspending {
                                                "--background-feedback-mod-warning"
                                            } else {
                                                "--input-background-default"
                                            }))
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .when(!input_text.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(15.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-muted"))
                                                        .child(symbol),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(15.))
                                                    .text_color(color(if input_text.is_empty() {
                                                        "--text-muted"
                                                    } else {
                                                        "--text-strong"
                                                    }))
                                                    .child(if input_text.is_empty() {
                                                        dc_intl::t("YEnpaj").to_owned()
                                                    } else {
                                                        input_text
                                                    }),
                                            ),
                                    )
                                    .children(monthly_note),
                            )
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .rounded_b(px(12.))
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("fc-cap-cancel")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .when(!submitting, |d| d.cursor_pointer())
                                            .when(submitting, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !submitting {
                                                    this.family_center.spending_cap = None;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fc-cap-save")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color(primary_bg))
                                            .when(can_save, |d| {
                                                d.cursor_pointer()
                                                    .hover(move |s| s.bg(color(primary_hover)))
                                            })
                                            .when(!can_save, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(primary_label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) =
                                                    this.family_center.spending_cap.as_mut()
                                                {
                                                    if !m.can_save() {
                                                        return;
                                                    }
                                                    m.submitting = true;
                                                } else {
                                                    return;
                                                }
                                                let _ = this.requests.send(
                                                    family_center_spending_cap_request(
                                                        &teen_id, "",
                                                    ),
                                                );
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 320742 (`FamilyCenterQRCodeModal`, CSS `804050` `DzIB9D`):
    /// Teen-only blurred QR modal (`iHYMAz` top warning banner, `AVTZaN` / `LyyCsE`, 272px QR card
    /// with decoy URL until revealed, `goDyoe` reminder, full-width Close `cpT0Cq` + Reveal/Hide QR).
    pub(in crate::ui) fn family_center_teen_qr_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.teen_qr_modal.as_ref()?;
        let revealed = modal.revealed;
        let loading = modal.loading;
        let user_id = self.stores.current_user.as_ref().map_or(100_001, |u| u.id);
        let qr_url =
            teen_qr_payload_url(revealed, user_id, self.family_center.link_code.as_deref());
        let qr_card = super::login::qr_code_card(&qr_url, 224., 8., 8.);
        let desc_runs = dc_intl::format_runs("LyyCsE", &[]);
        let mut desc_text = String::new();
        let mut desc_highlights = Vec::new();
        for (seg, hook) in desc_runs {
            let start = desc_text.len();
            desc_text.push_str(&seg);
            if hook.is_some() && start < desc_text.len() {
                desc_highlights.push((
                    start..desc_text.len(),
                    gpui::HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..gpui::HighlightStyle::default()
                    },
                ));
            }
        }
        let desc_line = div()
            .id("fc-teen-qr-desc")
            .w_full()
            .min_w_0()
            .text_center()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .cursor_pointer()
            .child(gpui::StyledText::new(desc_text).with_highlights(desc_highlights))
            .on_click(cx.listener(|_, _, _, cx| {
                cx.open_url(TEEN_QR_HELP_URL);
            }));

        Some(
            deferred(
                div()
                    .id("family-center-teen-qr-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.family_center.teen_qr_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-teen-qr-modal")
                            .occlude()
                            .w(px(460.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            // Top full-width warning notice banner (`.notice_DzIB9D`)
                            .child(
                                div()
                                    .w_full()
                                    .px(px(16.))
                                    .py(px(10.))
                                    .bg(color("--status-warning"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(6.))
                                    .child(icon("circle_info", 16., "--background-base-lowest"))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--background-base-lowest"))
                                            .child(dc_intl::t("iHYMAz")),
                                    ),
                            )
                            .child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .text_center()
                                            .child(dc_intl::t("AVTZaN")),
                                    )
                                    .child(desc_line)
                                    // QR code container (`.qrCodeContainer_DzIB9D`, obscured until revealed)
                                    .child(
                                        div()
                                            .id("fc-teen-qr-box")
                                            .relative()
                                            .mt(px(4.))
                                            .size(px(240.))
                                            .rounded(px(12.))
                                            .overflow_hidden()
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--white"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .opacity(if revealed { 1.0 } else { 0.14 })
                                                    .children(qr_card),
                                            )
                                            .when(!revealed, |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .inset_0()
                                                        .bg(rgba(0x111214cc))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(
                                                            div()
                                                                .px(px(14.))
                                                                .py(px(8.))
                                                                .rounded(px(8.))
                                                                .bg(color(
                                                                    "--background-surface-higher",
                                                                ))
                                                                .border_1()
                                                                .border_color(color(
                                                                    "--border-muted",
                                                                ))
                                                                .text_size(px(13.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color("--text-strong"))
                                                                .child(dc_intl::t("10dX6U")),
                                                        ),
                                                )
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) =
                                                    this.family_center.teen_qr_modal.as_mut()
                                                {
                                                    m.revealed = !m.revealed;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_center()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("goDyoe")),
                                    ),
                            )
                            // Full-width footer (`actionsFullWidth: true`)
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("fc-teen-qr-close")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("cpT0Cq"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.family_center.teen_qr_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fc-teen-qr-toggle")
                                            .flex_1()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .when(!loading, |d| d.cursor_pointer())
                                            .when(loading, |d| d.opacity(0.6))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t(if revealed {
                                                "wg2xwQ"
                                            } else {
                                                "10dX6U"
                                            }))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) =
                                                    this.family_center.teen_qr_modal.as_mut()
                                                {
                                                    m.revealed = !m.revealed;
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_teen_qr(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.family_center.teen_qr_modal = None;
            cx.notify();
            return "family teen-qr closed".to_owned();
        }
        if self.family_center.link_code.is_none() {
            self.family_center.link_code = Some("TEEN-QR-2026".to_owned());
        }
        self.family_center.teen_qr_modal = Some(TeenQrCodeModal {
            revealed: arg.trim() == "revealed",
            loading: false,
        });
        cx.notify();
        format!("family teen-qr opened ({})", arg.trim())
    }

    /// Renders module 362890 (`Schedule Screen Downtime`, CSS `39627` `*_lVGlUj`): teen downtime editor.
    pub(in crate::ui) fn family_center_downtime_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.family_center.downtime_schedule.as_deref()?;
        let is_edit = modal.editing_rule_id.is_some();
        let enabled = modal.enabled;
        let start = modal.start;
        let end = modal.end;
        let duration = downtime_duration_label(start, end);
        let preview = downtime_schedule_preview(start, end);
        let conflicts = downtime_overlap_entries(
            &modal.selected_days,
            &modal.existing_rules,
            modal.editing_rule_id.as_deref(),
        );
        let can_submit = !modal.selected_days.is_empty()
            && start.total_minutes() != end.total_minutes()
            && !modal.submitting;
        let teen_id = modal.teen_id.clone();
        let edit_id = modal.editing_rule_id.clone();
        let days_snapshot = modal.selected_days.clone();

        Some(
            deferred(
                div()
                    .id("family-center-downtime-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.family_center.downtime_schedule = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("family-center-downtime-dialog")
                            .occlude()
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .child(
                                div()
                                    .px(px(20.))
                                    .pt(px(20.))
                                    .pb(px(14.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("w/ISB8")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("AcJ4ke")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("fc-dt-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                16.,
                                                "--interactive-icon-default",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.family_center.downtime_schedule = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .p(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(18.))
                                    // Edit-mode Schedule Enabled switch (`30Owsd`)
                                    .when(is_edit, |col| {
                                        col.child(
                                            div()
                                                .id("fc-dt-enabled-toggle")
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(dc_intl::t("30Owsd")),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(40.))
                                                        .h(px(22.))
                                                        .p(px(2.))
                                                        .rounded_full()
                                                        .bg(color(if enabled {
                                                            "--control-primary-background-default"
                                                        } else {
                                                            "--background-mod-strong"
                                                        }))
                                                        .flex()
                                                        .items_center()
                                                        .when(enabled, |d| d.justify_end())
                                                        .child(
                                                            div()
                                                                .size(px(18.))
                                                                .rounded_full()
                                                                .bg(color("--white")),
                                                        ),
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) = this
                                                        .family_center
                                                        .downtime_schedule
                                                        .as_deref_mut()
                                                    {
                                                        m.enabled = !m.enabled;
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .h(px(1.))
                                                .w_full()
                                                .bg(color("--border-subtle")),
                                        )
                                    })
                                    // Downtime Hours (`37z4a2` + `n2mCrR` + duration `hFDcmZ`/`wcrXLM`)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("37z4a2")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .id("fc-dt-start")
                                                            .flex_1()
                                                            .h(px(40.))
                                                            .px(px(12.))
                                                            .rounded(px(8.))
                                                            .border_1()
                                                            .border_color(color(
                                                                "--input-border-default",
                                                            ))
                                                            .bg(color("--input-background-default"))
                                                            .cursor_pointer()
                                                            .flex()
                                                            .items_center()
                                                            .justify_between()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color("--text-strong"))
                                                            .child(start.format_12h())
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    if let Some(m) = this
                                                                        .family_center
                                                                        .downtime_schedule
                                                                        .as_deref_mut()
                                                                    {
                                                                        let next = (m
                                                                            .start
                                                                            .total_minutes()
                                                                            + 30)
                                                                            % 1440;
                                                                        m.start = DowntimeTime {
                                                                            hours: (next / 60) as u8,
                                                                            minutes: (next % 60)
                                                                                as u8,
                                                                        };
                                                                        m.end = m
                                                                            .start
                                                                            .plus_nine_hours();
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color("--text-muted"))
                                                            .child(dc_intl::t("n2mCrR")),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("fc-dt-end")
                                                            .flex_1()
                                                            .h(px(40.))
                                                            .px(px(12.))
                                                            .rounded(px(8.))
                                                            .border_1()
                                                            .border_color(color(
                                                                "--input-border-default",
                                                            ))
                                                            .bg(color("--input-background-default"))
                                                            .cursor_pointer()
                                                            .flex()
                                                            .items_center()
                                                            .justify_between()
                                                            .child(
                                                                div()
                                                                    .text_size(px(14.))
                                                                    .font_weight(FontWeight(500.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(end.format_12h()),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(12.))
                                                                    .text_color(color(
                                                                        "--text-subtle",
                                                                    ))
                                                                    .child(format!("• {duration}")),
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    if let Some(m) = this
                                                                        .family_center
                                                                        .downtime_schedule
                                                                        .as_deref_mut()
                                                                    {
                                                                        let next = (m
                                                                            .end
                                                                            .total_minutes()
                                                                            + 30)
                                                                            % 1440;
                                                                        m.end = DowntimeTime {
                                                                            hours: (next / 60) as u8,
                                                                            minutes: (next % 60)
                                                                                as u8,
                                                                        };
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    )
                                    .child(div().h(px(1.)).w_full().bg(color("--border-subtle")))
                                    // Repeat Days (`HaV0Sg`, `vX7xid`/`R87Y2K`, 7 day buttons, overlap `26A0Df`)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("HaV0Sg")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-muted"))
                                                            .child(preview),
                                                    ),
                                            )
                                            .child(
                                                div().flex().gap(px(6.)).children(
                                                    DOWNTIME_DAYS.into_iter().map(
                                                        |(day_num, short, _)| {
                                                            let sel = modal
                                                                .selected_days
                                                                .contains(&day_num);
                                                            div()
                                                                .id(SharedString::from(format!(
                                                                    "fc-dt-day-{day_num}"
                                                                )))
                                                                .flex_1()
                                                                .h(px(40.))
                                                                .rounded(px(6.))
                                                                .border_1()
                                                                .border_color(color(if sel {
                                                                    "--control-primary-background-default"
                                                                } else {
                                                                    "--control-secondary-border-default"
                                                                }))
                                                                .bg(if sel {
                                                                    rgba(0x5865f229)
                                                                } else {
                                                                    color("--control-secondary-background-default")
                                                                })
                                                                .cursor_pointer()
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .text_size(px(13.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color(
                                                                    "--control-secondary-text-default",
                                                                ))
                                                                .child(short)
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        if let Some(m) = this
                                                                            .family_center
                                                                            .downtime_schedule
                                                                            .as_deref_mut()
                                                                        {
                                                                            if let Some(pos) = m
                                                                                .selected_days
                                                                                .iter()
                                                                                .position(|&d| {
                                                                                    d == day_num
                                                                                })
                                                                            {
                                                                                m.selected_days
                                                                                    .remove(pos);
                                                                            } else {
                                                                                m.selected_days
                                                                                    .push(day_num);
                                                                            }
                                                                        }
                                                                        cx.notify();
                                                                    },
                                                                ))
                                                        },
                                                    ),
                                                ),
                                            )
                                            .when(!conflicts.is_empty(), |col| {
                                                col.child(
                                                    div()
                                                        .mt(px(6.))
                                                        .p(px(12.))
                                                        .rounded(px(8.))
                                                        .border_1()
                                                        .border_color(color("--status-warning"))
                                                        .bg(rgba(0xf0b2321a))
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(6.))
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_start()
                                                                .gap(px(8.))
                                                                .child(icon(
                                                                    "warning_triangle",
                                                                    16.,
                                                                    "--status-warning",
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .text_size(px(12.))
                                                                        .line_height(px(16.))
                                                                        .text_color(color(
                                                                            "--text-default",
                                                                        ))
                                                                        .child(dc_intl::t(
                                                                            "26A0Df",
                                                                        )),
                                                                ),
                                                        )
                                                        .children(conflicts.into_iter().map(
                                                            |line| {
                                                                div()
                                                                    .pl(px(24.))
                                                                    .text_size(px(12.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(format!("• {line}"))
                                                            },
                                                        )),
                                                )
                                            }),
                                    ),
                            )
                            // Footer: Create (`ETE/oC`, `pvcruO`) vs Edit (`d8pizZ`, `TDc9mW`)
                            .child(
                                div()
                                    .px(px(20.))
                                    .py(px(14.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("fc-dt-secondary")
                                            .h(px(36.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(if is_edit {
                                                rgba(0xda373c22)
                                            } else {
                                                color("--control-secondary-background-default")
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(if is_edit {
                                                color("--text-feedback-critical")
                                            } else {
                                                color("--control-secondary-text-default")
                                            })
                                            .child(dc_intl::t(if is_edit {
                                                "d8pizZ"
                                            } else {
                                                "ETE/oC"
                                            }))
                                            .on_click(cx.listener({
                                                let tid = teen_id.clone();
                                                let rid = edit_id.clone();
                                                move |this, _, _, cx| {
                                                    if let Some(r) = rid.as_deref() {
                                                        let _ = this.requests.send(
                                                            delete_downtime_rule_request(&tid, r),
                                                        );
                                                    }
                                                    this.family_center.downtime_schedule = None;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fc-dt-submit")
                                            .h(px(36.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(!can_submit, |d| d.opacity(0.5))
                                            .when(can_submit, |d| d.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t(if is_edit {
                                                "TDc9mW"
                                            } else {
                                                "pvcruO"
                                            }))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !can_submit {
                                                    return;
                                                }
                                                let req = if let Some(rid) = edit_id.as_deref() {
                                                    update_downtime_rule_request(
                                                        &teen_id,
                                                        rid,
                                                        start,
                                                        end,
                                                        &days_snapshot,
                                                        enabled,
                                                    )
                                                } else {
                                                    create_downtime_rule_request(
                                                        &teen_id,
                                                        start,
                                                        end,
                                                        &days_snapshot,
                                                        true,
                                                    )
                                                };
                                                let _ = this.requests.send(req);
                                                this.family_center.downtime_schedule = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_downtime(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.family_center.downtime_schedule = None;
            cx.notify();
            return "family downtime closed".to_owned();
        }
        let is_edit = arg.trim() == "edit";
        self.family_center.downtime_schedule = Some(Box::new(DowntimeScheduleModal {
            teen_id: "998877".to_owned(),
            editing_rule_id: is_edit.then(|| "rule-1".to_owned()),
            enabled: true,
            start: DowntimeTime {
                hours: 22,
                minutes: 0,
            },
            end: DowntimeTime {
                hours: 7,
                minutes: 0,
            },
            selected_days: vec![1, 2, 3, 4, 5],
            existing_rules: vec![DowntimeRule {
                rule_id: "rule-existing".to_owned(),
                start: DowntimeTime {
                    hours: 21,
                    minutes: 0,
                },
                end: DowntimeTime {
                    hours: 6,
                    minutes: 30,
                },
                days: vec![1, 3],
                enabled: true,
            }],
            submitting: false,
        }));
        cx.notify();
        format!("family downtime opened ({})", arg.trim())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_spending_cap(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.family_center.spending_cap = None;
            cx.notify();
            return "family spending-cap closed".to_owned();
        }
        let amount_input = match arg.trim() {
            "clear" => String::new(),
            "normal" => "50.00".to_owned(),
            _ => "20.00".to_owned(),
        };
        self.family_center.spending_cap = Some(SpendingCapModal {
            teen_id: "99887766".to_owned(),
            currency_symbol: "$".to_owned(),
            exponent: 2,
            amount_input,
            existing_cap_minor: Some(3000),
            monthly_spend_minor: 2500,
            renew_date_label: "12/1/26".to_owned(),
            submitting: false,
        });
        cx.notify();
        format!("family spending-cap opened ({})", arg.trim())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_disconnect_link(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.family_center.disconnect_link = None;
            cx.notify();
            return "family disconnect closed".to_owned();
        }
        let is_adult = arg.trim() != "teen";
        self.family_center.disconnect_link = Some(DisconnectLinkModal {
            other_user_id: "11223344".into(),
            other_user_name: if is_adult {
                "alex_teen".into()
            } else {
                "ParentGuardian".into()
            },
            is_adult,
            requires_consent: true,
            ack1: false,
            ack2: false,
            submitting: false,
            has_error: arg.trim() == "error",
        });
        cx.notify();
        format!("family disconnect opened (adult={is_adult})")
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_request_connection(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.family_center.request_connection = None;
            cx.notify();
            return "family request closed".to_owned();
        }
        let phase = match arg.trim() {
            "expired" => RequestConnectionPhase::InvalidCode,
            "confirm-age" => RequestConnectionPhase::ConfirmAge,
            "sent" => RequestConnectionPhase::Sent,
            "error" => {
                RequestConnectionPhase::Error(RequestConnectionFailure::MaxConnectionsReached)
            }
            _ => RequestConnectionPhase::Request,
        };
        self.family_center.request_connection = Some(RequestConnectionModal {
            teen_id: "99887766".into(),
            link_code: "DEMO-CODE".into(),
            teen_username: Some("alex_teen".into()),
            guardian_email: Some("guardian@example.com".into()),
            phase,
            submitting: false,
        });
        cx.notify();
        format!("family request opened ({phase:?})")
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_family_accept_link(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.family_center.accept_link = None;
            cx.notify();
            return "family accept closed".to_owned();
        }
        self.family_center.accept_link = Some(AcceptLinkModal {
            other_user_id: "11223344".into(),
            other_user_name: "ParentGuardian".into(),
            submitting: false,
            has_error: arg.trim() == "error",
        });
        cx.notify();
        "family accept opened".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_needs_the_setting_and_nsfw_access() {
        assert!(row_shown(true, true));
        assert!(!row_shown(true, false));
        assert!(!row_shown(false, true));
    }

    #[test]
    fn the_dialog_strings_are_the_official_ones() {
        assert_eq!(dc_intl::t("VGB30H"), "Disable Family Center");
        let runs = dc_intl::format_runs("CrZZ+p", &[]);
        assert!(
            runs.iter()
                .any(|(text, hook)| hook.as_deref() == Some("openSettingsHook")
                    && text == "User Settings > Family Center")
        );
    }

    #[test]
    fn guardian_share_url_countdown_and_strings_match_509914() {
        assert_eq!(
            family_center_share_url(42, "CODE123"),
            "https://discord.com/feature/family-center/my-family/42/CODE123"
        );
        assert_eq!(format_link_code_countdown(300), "5:00");
        assert_eq!(format_link_code_countdown(65), "1:05");
        assert_eq!(format_link_code_countdown(9), "0:09");
        for key in [
            "pojgfk", "RfkLDs", "Mi60fm", "TGr8Ws", "WqhZss", "+5kSoW", "ITlV6p", "F4GT2S",
        ] {
            assert!(dc_intl::has_key(key), "missing key {key} in 509914/207049");
        }
    }

    #[test]
    fn family_center_request_41527_and_accept_58144() {
        for key in [
            "oEAioF", "NX+WJN", "WAI6xu", "cpT0Cq", "ETE/oC", "GH11eI", "pQQMJ7", "3oUE4o",
            "BQFHXW", "ewSb6o", "jcUN2F", "sMmIbm", "yiUJNU", "ISg34l", "EpwfZl", "dVtWId",
            "rlNJwZ", "pegSM5", "snlFqR", "wI/jo3",
        ] {
            assert!(dc_intl::has_key(key), "missing 41527/58144 key {key}");
        }
        assert_eq!(
            RequestConnectionFailure::from_api_code(290_000),
            RequestConnectionFailure::IneligibleForFamilyCenter
        );
        assert_eq!(
            RequestConnectionFailure::from_api_code(290_005),
            RequestConnectionFailure::PendingRequestExists
        );
        assert_eq!(
            RequestConnectionFailure::from_api_code(290_002),
            RequestConnectionFailure::MaxConnectionsReached
        );
        assert!(matches!(
            family_center_prereqs_request("1", "ABC"),
            crate::net::Request::Rest { route, .. }
                if route == "/family-center/connection-prerequisites?teen_id=1&link_code=ABC"
        ));
        assert!(matches!(
            family_center_send_link_request("1", "ABC"),
            crate::net::Request::Rest { route, .. } if route == "/users/@me/linked-users"
        ));
        assert!(matches!(
            family_center_accept_link_request("2"),
            crate::net::Request::Rest { route, .. } if route == "/users/@me/linked-users"
        ));
    }

    #[test]
    fn family_center_disconnect_179950_intl_guard_and_request() {
        for key in [
            "o0JXuK", "WH+Gba", "F2lccv", "Or6hgl", "hOEHFn", "PlrZal", "XyRW4c", "PGQBnk",
            "eiABQz", "0ki7+P", "sCbKs4", "krIDdy", "3ilveh", "c5L+sl", "kFj4h1",
        ] {
            assert!(dc_intl::has_key(key), "missing 179950 key {key}");
        }
        let mut m = DisconnectLinkModal {
            other_user_id: "55".into(),
            other_user_name: "alex".into(),
            is_adult: true,
            requires_consent: true,
            ack1: false,
            ack2: false,
            submitting: false,
            has_error: false,
        };
        assert!(!m.can_disconnect());
        m.ack1 = true;
        assert!(!m.can_disconnect());
        m.ack2 = true;
        assert!(m.can_disconnect());
        m.submitting = true;
        assert!(!m.can_disconnect());
        assert!(matches!(
            family_center_disconnect_link_request("55"),
            crate::net::Request::Rest { route, body: Some(ref b), .. }
                if route == "/users/@me/linked-users"
                    && b["linked_user_id"] == "55"
                    && b["link_status"] == 3
        ));
    }

    #[test]
    fn family_center_spending_cap_270915_sanitizer_warning_and_request() {
        for key in [
            "oBkFiL", "IFguF2", "gMeekL", "IWdrfp", "YEnpaj", "Tk6x4X", "pfAlRY", "JZDGJ8",
            "2WKfG1", "Wu8BK2", "ETE/oC", "R3BPH+",
        ] {
            assert!(dc_intl::has_key(key), "missing 270915 key {key}");
        }
        assert_eq!(sanitize_currency_input("$12.345abc", 2), "12.34");
        assert_eq!(sanitize_currency_input("12.34", 0), "1234");
        assert_eq!(parse_currency_minor_units("25.50", 2), Some(2550));
        assert_eq!(format_minor_currency("$", 2550, 2), "$25.50");
        let m = SpendingCapModal {
            teen_id: "99".into(),
            currency_symbol: "$".into(),
            exponent: 2,
            amount_input: "20.00".into(),
            existing_cap_minor: Some(3000),
            monthly_spend_minor: 2500,
            renew_date_label: "12/1/26".into(),
            submitting: false,
        };
        assert!(m.is_overspending());
        assert!(!m.is_clearing_cap());
        assert!(m.can_save());
        assert!(matches!(
            family_center_spending_cap_request("99", "e30="),
            crate::net::Request::Rest { route, .. } if route == "/family-center/99/settings-proto"
        ));
    }

    #[test]
    fn family_center_teen_qr_320742_decoy_and_intl() {
        for key in [
            "iHYMAz", "AVTZaN", "LyyCsE", "goDyoe", "cpT0Cq", "wg2xwQ", "10dX6U",
        ] {
            assert!(dc_intl::has_key(key), "missing 320742 key {key}");
        }
        assert_eq!(
            teen_qr_payload_url(false, 42, Some("SECRET")),
            TEEN_QR_DECOY_URL
        );
        assert_eq!(teen_qr_payload_url(true, 42, None), TEEN_QR_DECOY_URL);
        assert_eq!(
            teen_qr_payload_url(true, 42, Some("SECRET")),
            "https://discord.com/feature/family-center/my-family/42/SECRET"
        );
    }

    #[test]
    fn family_center_downtime_362890_parser_overlap_routes_and_intl() {
        for key in [
            "w/ISB8", "AcJ4ke", "30Owsd", "37z4a2", "n2mCrR", "HaV0Sg", "vX7xid", "R87Y2K",
            "26A0Df", "Wu8BK2", "d8pizZ", "TDc9mW", "ETE/oC", "pvcruO", "hFDcmZ", "wcrXLM",
            "bPjqd1", "4dr9L9", "6lTTJ+",
        ] {
            assert!(dc_intl::has_key(key), "missing 362890 key {key}");
        }
        let parsed = parse_typed_downtime_time("9:3");
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            DowntimeTime {
                hours: 9,
                minutes: 30
            }
        );
        assert_eq!(
            parsed[1],
            DowntimeTime {
                hours: 21,
                minutes: 30
            }
        );
        assert!(parse_typed_downtime_time("25:00").is_empty());

        let start = DowntimeTime {
            hours: 22,
            minutes: 0,
        };
        let end = start.plus_nine_hours();
        assert_eq!(
            end,
            DowntimeTime {
                hours: 7,
                minutes: 0
            }
        );
        assert!(downtime_duration_label(start, end).contains('9'));
        assert!(downtime_schedule_preview(start, end).contains("next day"));

        let existing = vec![DowntimeRule {
            rule_id: "r1".into(),
            start: DowntimeTime {
                hours: 21,
                minutes: 0,
            },
            end: DowntimeTime {
                hours: 6,
                minutes: 30,
            },
            days: vec![1, 3],
            enabled: true,
        }];
        let ov = downtime_overlap_entries(&[1, 2, 3], &existing, None);
        assert_eq!(ov.len(), 2);
        assert!(downtime_overlap_entries(&[1, 3], &existing, Some("r1")).is_empty());

        assert!(matches!(
            create_downtime_rule_request("t1", start, end, &[1, 2], true),
            Request::Rest { method, route, .. }
                if method == reqwest::Method::POST
                    && route == "/family-center/t1/restricted-schedule/rule"
        ));
        assert!(matches!(
            update_downtime_rule_request("t1", "r1", start, end, &[1], false),
            Request::Rest { method, route, .. }
                if method == reqwest::Method::PATCH
                    && route == "/family-center/t1/restricted-schedule/rules/r1"
        ));
        assert!(matches!(
            delete_downtime_rule_request("t1", "r1"),
            Request::Rest { method, route, .. }
                if method == reqwest::Method::DELETE
                    && route == "/family-center/t1/restricted-schedule/rules/r1"
        ));
    }
}
