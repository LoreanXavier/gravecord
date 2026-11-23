//! Safety Flows Interstitial / Account Unlock Modal (`SafetyFlowsModal`, module 129939).
//!
//! Full-viewport interstitial flow for account-locking safety tasks (`/safety-flows/task`):
//! - Left 240px stepper sidebar (`/OpRAP` "Unlock your account", step list, `"Not you? Log out"` `0DHxym`)
//! - Refresh Required (`REFRESH_APP = -1`: `v52itt`, `5am8D3`, `GDsHl+`)
//! - App Store Parental Revocation (`APP_STORE_PARENTAL_REVOCATION = 11`: `Z87TFb`, `1YhkP1`, `BaI6L4`, `6FXIU6`)
//! - Parental Consent Connection (`PARENTAL_CONSENT_CONNECTION = 10`: `dMMSA0`, pending parent
//!   connection request rows with 36px green accept + bordered decline circular buttons, `YQP5dE` /
//!   `2HvOvh`, `OaHZUf` "Continue to Account", `RD76/V` "Share New Invite" / `CLAQas` "Back to Requests")
//! - Error fallback (`c6kn6F`, `ZUEGFn`, `2jxGer` "Log Out", `7NqTJn` "Try Again")

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use serde_json::json;

/// App Store parental revocation guide URL (module 129939).
pub(crate) const APP_STORE_LOCKDOWN_GUIDE_URL: &str =
    "https://support.discord.com/hc/en-us/articles/42855178312087";

/// Official Safety Flows task types (`b` enum in module 129939).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SafetyFlowTaskType {
    RefreshApp = -1,
    EmailVerification = 3,
    AgeVerification = 9,
    ParentalConsentConnection = 10,
    AppStoreParentalRevocation = 11,
}

impl SafetyFlowTaskType {
    pub(crate) fn step_label(self) -> String {
        let (key, fb) = match self {
            Self::RefreshApp => ("SzfxQ3", "Refresh your app"),
            Self::EmailVerification => ("Qm6K/s", "Verify your email"),
            Self::AgeVerification => ("dSkE/A", "Verify age group"),
            Self::ParentalConsentConnection => {
                ("dMMSA0", "Connect to a guardian to access your account")
            }
            Self::AppStoreParentalRevocation => ("Z87TFb", "Account locked by guardian"),
        };
        sf_copy(key, fb)
    }
}

/// Link status of a pending parent/guardian row in Parental Consent Connection (`eh`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ParentRequestLinkStatus {
    Pending,
    Active,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingParentRequest {
    pub parent_id: Id,
    pub display_name: String,
    pub sent_ago: String,
    pub status: ParentRequestLinkStatus,
}

/// Active `SafetyFlowsModal` state (module 129939).
#[derive(Clone, Debug)]
pub(crate) struct SafetyFlowsModalState {
    pub task_id: String,
    pub flow_id: String,
    pub task_type: SafetyFlowTaskType,
    pub tasks: Vec<SafetyFlowTaskType>,
    pub loading: bool,
    pub error: bool,
    pub showing_invite: bool,
    pub pending_requests: Vec<PendingParentRequest>,
}

/// Module 129939 (`Rsh.SAFETY_FLOWS_TASK`): `GET /safety-flows/task`.
#[allow(dead_code)]
pub(crate) fn fetch_safety_flow_task_request() -> Request {
    Request::Fetch {
        key: "safety-flows:task".to_owned(),
        route: "/safety-flows/task".to_owned(),
    }
}

/// Module 129939 (`L`): `POST /safety-flows/task` (`{ task_id, flow_id, data }`).
pub(crate) fn submit_safety_flow_task_request(
    task_id: &str,
    flow_id: &str,
    data_type: &str,
) -> Request {
    Request::Rest {
        key: Some(format!("safety-flows:submit:{task_id}")),
        method: reqwest::Method::POST,
        route: "/safety-flows/task".to_owned(),
        body: Some(json!({
            "task_id": task_id,
            "flow_id": flow_id,
            "data": { "type": data_type },
        })),
    }
}

/// Module 129939 (`899847.nt`): `PATCH /users/@me/linked-users` (`link_status`: 2=ACTIVE, 3=DECLINED).
pub(crate) fn respond_parent_consent_request(parent_id: Id, accept: bool) -> Request {
    Request::Rest {
        key: Some(format!("safety-flows:parent:{parent_id}")),
        method: reqwest::Method::PATCH,
        route: "/users/@me/linked-users".to_owned(),
        body: Some(json!({
            "linked_user_id": parent_id.to_string(),
            "link_status": if accept { 2 } else { 3 },
        })),
    }
}

fn sf_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

impl Root {
    /// Renders the `129939` Safety Flows Account Unlock modal when open.
    pub(super) fn safety_flows_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.safety_flows_modal.as_ref()?;
        let task_id = state.task_id.clone();
        let flow_id = state.flow_id.clone();

        let modal_card: AnyElement = if state.loading {
            div()
                .w(px(360.))
                .h(px(220.))
                .rounded(px(16.))
                .bg(color("--background-surface-higher"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .justify_center()
                .child(super::voice_view::spinner())
                .into_any_element()
        } else if state.error {
            div()
                .w(px(400.))
                .p(px(24.))
                .rounded(px(16.))
                .bg(color("--background-surface-higher"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(icon("shop_circle_error", 36., "--status-danger"))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("c6kn6F")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("ZUEGFn")),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .id("safety-flow-err-logout")
                                .h(px(36.))
                                .px(px(14.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("2jxGer"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.safety_flows_modal = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("safety-flow-err-retry")
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("7NqTJn"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(s) = this.safety_flows_modal.as_mut() {
                                        s.error = false;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else if state.task_type == SafetyFlowTaskType::ParentalConsentConnection {
            let has_active_guardian = state
                .pending_requests
                .iter()
                .any(|r| r.status == ParentRequestLinkStatus::Active);
            div()
                .id("safety-flow-parental-consent")
                .w(px(500.))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(
                            div()
                                .text_center()
                                .text_size(px(20.))
                                .line_height(px(26.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(sf_copy(
                                    "dMMSA0",
                                    "Connect to a guardian to access your account",
                                )),
                        )
                        .child(if state.showing_invite {
                            div()
                                .p(px(16.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-highest"))
                                .text_center()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child(sf_copy(
                                    "6GaRTu",
                                    "Share your Family Center link code with a parent or guardian.",
                                ))
                                .into_any_element()
                        } else {
                            div()
                                .rounded(px(16.))
                                .bg(color("--background-surface-highest"))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .children(state.pending_requests.iter().map(|req| {
                                    let pid = req.parent_id;
                                    let initial = req
                                        .display_name
                                        .chars()
                                        .next()
                                        .unwrap_or('G')
                                        .to_ascii_uppercase()
                                        .to_string();
                                    let trailing: AnyElement = match req.status {
                                        ParentRequestLinkStatus::Active => div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--status-positive"))
                                            .child(sf_copy("YQP5dE", "Connected"))
                                            .into_any_element(),
                                        ParentRequestLinkStatus::Declined => div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(sf_copy("2HvOvh", "Declined"))
                                            .into_any_element(),
                                        ParentRequestLinkStatus::Pending => div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sf-parent-accept-{pid}"
                                                    )))
                                                    .size(px(36.))
                                                    .rounded_full()
                                                    .bg(color("--status-positive"))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon("checkmark_large", 18., "--white"))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(s) =
                                                                this.safety_flows_modal.as_mut()
                                                                && let Some(r) = s
                                                                    .pending_requests
                                                                    .iter_mut()
                                                                    .find(|r| r.parent_id == pid)
                                                            {
                                                                r.status =
                                                                    ParentRequestLinkStatus::Active;
                                                            }
                                                            let _ = this.requests.send(
                                                                respond_parent_consent_request(
                                                                    pid, true,
                                                                ),
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sf-parent-decline-{pid}"
                                                    )))
                                                    .size(px(36.))
                                                    .rounded_full()
                                                    .border_1()
                                                    .border_color(color("--border-strong"))
                                                    .bg(color("--background-surface-highest"))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon(
                                                        "modal_close",
                                                        18.,
                                                        "--interactive-text-default",
                                                    ))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(s) =
                                                                this.safety_flows_modal.as_mut()
                                                                && let Some(r) = s
                                                                    .pending_requests
                                                                    .iter_mut()
                                                                    .find(|r| r.parent_id == pid)
                                                            {
                                                                r.status =
                                                                ParentRequestLinkStatus::Declined;
                                                            }
                                                            let _ = this.requests.send(
                                                                respond_parent_consent_request(
                                                                    pid, false,
                                                                ),
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .into_any_element(),
                                    };
                                    div()
                                        .px(px(16.))
                                        .py(px(12.))
                                        .border_b_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(12.))
                                                .child(
                                                    div()
                                                        .size(px(40.))
                                                        .rounded_full()
                                                        .bg(color("--background-brand"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(15.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--white"))
                                                        .child(initial),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color("--text-strong"))
                                                                .child(req.display_name.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(px(12.))
                                                                .text_color(color("--text-muted"))
                                                                .child(req.sent_ago.clone()),
                                                        ),
                                                ),
                                        )
                                        .child(trailing)
                                }))
                                .into_any_element()
                        }),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("sf-parent-toggle-invite")
                                .h(px(36.))
                                .px(px(14.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(if state.showing_invite {
                                    sf_copy("CLAQas", "Back to Requests")
                                } else {
                                    sf_copy("RD76/V", "Share New Invite")
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(s) = this.safety_flows_modal.as_mut() {
                                        s.showing_invite = !s.showing_invite;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("sf-parent-continue")
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .when(!has_active_guardian, |d| d.opacity(0.5))
                                .when(has_active_guardian, |d| d.cursor_pointer())
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(sf_copy("OaHZUf", "Continue to Account"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !has_active_guardian {
                                        return;
                                    }
                                    let _ = this.requests.send(submit_safety_flow_task_request(
                                        &task_id, &flow_id, "empty",
                                    ));
                                    this.safety_flows_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            // Standard 750x520 two-column Safety Flows container (`container_orXH-o`)
            let hide_steps = state.task_type == SafetyFlowTaskType::AppStoreParentalRevocation;
            let main_pane: AnyElement = match state.task_type {
                SafetyFlowTaskType::AppStoreParentalRevocation => div()
                    .flex_1()
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(sf_copy("Z87TFb", "Account locked by guardian")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(sf_copy(
                                        "1YhkP1",
                                        "The account is suspended because a guardian turned off access to Discord through the app store's parental controls, not because of a policy violation.",
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(sf_copy(
                                        "BaI6L4",
                                        "Ask the guardian to restore access in the app store's parental controls (Apple or Google Play).",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .pt(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .child(
                                div()
                                    .id("safety-flow-view-guide")
                                    .w_full()
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(sf_copy("6FXIU6", "View Guide"))
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.open_url(APP_STORE_LOCKDOWN_GUIDE_URL);
                                    })),
                            ),
                    )
                    .into_any_element(),
                _ => div()
                    .flex_1()
                    .p(px(32.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(sf_copy("v52itt", "Refresh Required")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(color("--text-muted"))
                                    .child(sf_copy("5am8D3", "Please refresh the page to continue.")),
                            ),
                    )
                    .child(
                        div()
                            .id("safety-flow-refresh-btn")
                            .h(px(38.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(sf_copy("GDsHl+", "Refresh"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.requests.send(submit_safety_flow_task_request(
                                    &task_id, &flow_id, "empty",
                                ));
                                this.safety_flows_modal = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            };

            div()
                .id("safety-flow-container")
                .w(px(750.))
                .h(px(520.))
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .w(px(240.))
                        .h_full()
                        .p(px(20.))
                        .bg(color("--background-surface-highest"))
                        .border_r_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(sf_copy("/OpRAP", "Unlock your account")),
                                )
                                .when(!hide_steps, |col| {
                                    col.children(state.tasks.iter().enumerate().map(|(i, t)| {
                                        let active = *t == state.task_type;
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(
                                                div()
                                                    .size(px(24.))
                                                    .rounded_full()
                                                    .bg(color(if active {
                                                        "--background-brand"
                                                    } else {
                                                        "--background-mod-strong"
                                                    }))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .child(format!("{}", i + 1)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(if active {
                                                        600.
                                                    } else {
                                                        400.
                                                    }))
                                                    .text_color(color(if active {
                                                        "--text-strong"
                                                    } else {
                                                        "--text-muted"
                                                    }))
                                                    .child(t.step_label()),
                                            )
                                    }))
                                }),
                        )
                        .child(
                            div()
                                .id("safety-flow-sidebar-logout")
                                .cursor_pointer()
                                .text_size(px(12.))
                                .text_color(color("--text-link"))
                                .hover(|s| s.underline())
                                .child(dc_intl::t("2jxGer"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.safety_flows_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(main_pane)
                .into_any_element()
        };

        Some(
            deferred(
                div()
                    .id("safety-flows-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000cc))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .child(modal_card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_safety_flows(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.safety_flows_modal = None;
            cx.notify();
            return "safety-flows: closed".to_owned();
        }
        let (task_type, error) = match arg.trim() {
            "lockdown" => (SafetyFlowTaskType::AppStoreParentalRevocation, false),
            "refresh" => (SafetyFlowTaskType::RefreshApp, false),
            "error" => (SafetyFlowTaskType::RefreshApp, true),
            _ => (SafetyFlowTaskType::ParentalConsentConnection, false),
        };
        self.safety_flows_modal = Some(SafetyFlowsModalState {
            task_id: "task-129939".to_owned(),
            flow_id: "flow-1".to_owned(),
            task_type,
            tasks: vec![
                SafetyFlowTaskType::EmailVerification,
                SafetyFlowTaskType::AgeVerification,
                SafetyFlowTaskType::RefreshApp,
            ],
            loading: false,
            error,
            showing_invite: false,
            pending_requests: vec![PendingParentRequest {
                parent_id: 5511,
                display_name: "ParentGuardian".to_owned(),
                sent_ago: "Sent 5 minutes ago".to_owned(),
                status: ParentRequestLinkStatus::Pending,
            }],
        });
        cx.notify();
        format!("safety-flows: opened ({})", arg.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_flows_129939_requests_and_intl() {
        for key in [
            "c6kn6F", "ZUEGFn", "2jxGer", "7NqTJn", "/OpRAP", "v52itt", "5am8D3", "GDsHl+",
            "Z87TFb", "6FXIU6", "dMMSA0", "OaHZUf", "RD76/V", "CLAQas", "YQP5dE", "2HvOvh",
        ] {
            assert!(dc_intl::has_key(key), "missing 129939 key {key}");
        }
        assert!(matches!(
            fetch_safety_flow_task_request(),
            Request::Fetch { route, .. } if route == "/safety-flows/task"
        ));
        assert!(matches!(
            submit_safety_flow_task_request("t1", "f1", "empty"),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/safety-flows/task" && b["task_id"] == "t1" && b["data"]["type"] == "empty"
        ));
        assert!(matches!(
            respond_parent_consent_request(99, true),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/users/@me/linked-users" && b["link_status"] == 2
        ));
    }
}
