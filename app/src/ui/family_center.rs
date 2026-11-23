//! The Family Center row of the Home sidebar (`ID` of module 596339), its Disable Family Center
//! dialog (module 371158), and the teen Guardian QR Share Code widget (module 509914 / 207049).
//!
//! The row shows while the account's `familyCenterEnabledV2` privacy setting is on and the account may see NSFW
//! content (`shouldShowFamilyCenterRow`: module 253932 `dm` and module 945276, `nsfwAllowed`). Its close button (`Ib`,
//! `XSmallIcon`, "Close") opens the dialog: "Disable Family Center", the sentence whose link opens User Settings >
//! Family Center, Cancel and the critical Disable Family Center, which turns the setting off and goes to Friends.

use super::modal::{self, Tone};
use super::{Root, color, icon};
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
        a || b || c
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
}
