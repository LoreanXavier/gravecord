//! Verify by Email modal (module 9124 + 15000 + 830215 + 631670).
//!
//! Opened for accounts with `REQUIRE_VERIFIED_EMAIL` (`stores.account_events.required_action()`):
//! - Title `1MPz27` ("Verify by Email").
//! - Mode A (`can_resend`): subtitle `tSXg8O` (switches to `JqLgQL` after resend), secondary
//!   `WnX4J2` ("Resend Email" -> `POST /auth/verify/resend`) and optional primary `Vm8akB`
//!   ("Change Email").
//! - Mode B (`change_email`): subtitle `KW5DEh`, `dI4d4S` Email + `CIGa+7` Password inputs,
//!   primary `v740sh` ("Verify Account" -> `PATCH /users/@me`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgba};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(super) const VERIFY_RESEND_KEY: &str = "verify-email:resend";
pub(super) const VERIFY_CHANGE_KEY: &str = "verify-email:change";

#[derive(Clone, Debug)]
pub(super) struct VerifyEmailModal {
    pub email: String,
    pub password: String,
    pub change_mode: bool,
    pub can_change: bool,
    pub resent: bool,
    pub busy: bool,
    pub email_error: Option<String>,
    pub password_error: Option<String>,
}

/// Builds `POST /auth/verify/resend` (`830215.verifyResend`).
pub fn resend_request() -> Request {
    Request::Rest {
        key: Some(VERIFY_RESEND_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/auth/verify/resend".to_owned(),
        body: Some(json!({})),
    }
}

/// Builds `PATCH /users/@me` (`631670._L`) to set/change email during verification.
pub fn change_email_request(email: &str, password: &str) -> Request {
    Request::Rest {
        key: Some(VERIFY_CHANGE_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: "/users/@me".to_owned(),
        body: Some(json!({
            "email": email.trim(),
            "password": password,
        })),
    }
}

impl Root {
    /// Opens the Verify by Email modal (module `9124`).
    #[allow(dead_code)]
    pub(super) fn open_verify_email_modal(&mut self, can_change: bool, cx: &mut Context<Self>) {
        let email = self.stores.account.email.clone().unwrap_or_default();
        let change_mode = email.is_empty();
        self.verify_email_modal = Some(VerifyEmailModal {
            email,
            password: String::new(),
            change_mode,
            can_change,
            resent: false,
            busy: false,
            email_error: None,
            password_error: None,
        });
        cx.notify();
    }

    /// Handles `POST /auth/verify/resend` and `PATCH /users/@me` responses.
    pub(super) fn handle_verify_email_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.verify_email_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        if key == VERIFY_RESEND_KEY {
            if body.get("code").is_none() {
                modal.resent = true;
            } else {
                self.put_app_toast(dc_intl::t("R0RpRX").to_owned(), false, cx);
            }
            cx.notify();
            return;
        }
        if key == VERIFY_CHANGE_KEY {
            if body.get("code").is_some() || body.is_null() {
                modal.email_error = body["errors"]["email"]["_errors"][0]["message"]
                    .as_str()
                    .or_else(|| body["email"][0].as_str())
                    .map(str::to_owned);
                modal.password_error = body["errors"]["password"]["_errors"][0]["message"]
                    .as_str()
                    .or_else(|| body["password"][0].as_str())
                    .map(str::to_owned);
                if modal.email_error.is_none() && modal.password_error.is_none() {
                    self.put_app_toast(dc_intl::t("R0RpRX").to_owned(), false, cx);
                }
            } else {
                modal.change_mode = false;
                modal.resent = true;
            }
            cx.notify();
        }
    }

    /// Renders module `9124` Verify by Email modal.
    pub(super) fn verify_email_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.verify_email_modal.as_ref()?;
        let can_resend = !modal.change_mode && !modal.email.is_empty();
        let can_submit_change =
            !modal.busy && !modal.email.trim().is_empty() && !modal.password.is_empty();

        let subtitle = if can_resend {
            if modal.resent {
                dc_intl::format(
                    "JqLgQL",
                    &[("email", Arg::Str(Cow::Borrowed(&modal.email)))],
                )
            } else {
                dc_intl::t("tSXg8O").to_owned()
            }
        } else {
            dc_intl::t("KW5DEh").to_owned()
        };

        let card = div()
            .id("verify-email-modal")
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    // Close button row
                    .child(
                        div().w_full().flex().justify_end().child(
                            div()
                                .id("verify-email-close")
                                .size(px(28.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.verify_email_modal = None;
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--icon-subtle")),
                        ),
                    )
                    // Top illustration badge
                    .child(
                        div()
                            .size(px(56.))
                            .rounded(px(28.))
                            .bg(rgba(0x5865f22a))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("message_requests_envelope", 28., "--icon-strong")),
                    )
                    // Title (`1MPz27`: "Verify by Email")
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("1MPz27")),
                    )
                    // Subtitle (`tSXg8O` / `JqLgQL` / `KW5DEh`)
                    .child(
                        div()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(SharedString::from(subtitle)),
                    )
                    // Change Email fields (`dI4d4S` Email + `CIGa+7` Password)
                    .when(!can_resend, |col| {
                        col.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-muted"))
                                                .child(SharedString::from(
                                                    dc_intl::t("dI4d4S").to_uppercase(),
                                                )),
                                        )
                                        .child(
                                            div()
                                                .h(px(40.))
                                                .px(px(12.))
                                                .rounded(px(8.))
                                                .bg(color("--background-base-lowest"))
                                                .border_1()
                                                .border_color(if modal.email_error.is_some() {
                                                    color("--status-danger")
                                                } else {
                                                    color("--border-subtle")
                                                })
                                                .flex()
                                                .items_center()
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .child(SharedString::from(
                                                    if modal.email.is_empty() {
                                                        "you@example.com".into()
                                                    } else {
                                                        modal.email.clone()
                                                    },
                                                )),
                                        )
                                        .when_some(modal.email_error.clone(), |f, err| {
                                            f.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-feedback-critical"))
                                                    .child(SharedString::from(err)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-muted"))
                                                .child(SharedString::from(
                                                    dc_intl::t("CIGa+7").to_uppercase(),
                                                )),
                                        )
                                        .child(
                                            div()
                                                .h(px(40.))
                                                .px(px(12.))
                                                .rounded(px(8.))
                                                .bg(color("--background-base-lowest"))
                                                .border_1()
                                                .border_color(if modal.password_error.is_some() {
                                                    color("--status-danger")
                                                } else {
                                                    color("--border-subtle")
                                                })
                                                .flex()
                                                .items_center()
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .child(SharedString::from(
                                                    if modal.password.is_empty() {
                                                        "••••••••".into()
                                                    } else {
                                                        "•".repeat(modal.password.len())
                                                    },
                                                )),
                                        )
                                        .when_some(modal.password_error.clone(), |f, err| {
                                            f.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-feedback-critical"))
                                                    .child(SharedString::from(err)),
                                            )
                                        }),
                                ),
                        )
                    }),
            )
            // Footer
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .when(can_resend, |bar| {
                        bar.child(
                            div()
                                .id("verify-email-resend")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.verify_email_modal.as_mut() {
                                        m.busy = true;
                                    }
                                    let _ = this.requests.send(resend_request());
                                    cx.notify();
                                }))
                                .child(dc_intl::t("WnX4J2")),
                        )
                        .when(modal.can_change, |b| {
                            b.child(
                                div()
                                    .id("verify-email-change")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.verify_email_modal.as_mut() {
                                            m.change_mode = true;
                                            cx.notify();
                                        }
                                    }))
                                    .child(dc_intl::t("Vm8akB")),
                            )
                        })
                    })
                    .when(!can_resend, |bar| {
                        bar.child(
                            div()
                                .id("verify-email-submit")
                                .w_full()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .when(can_submit_change, |s| s.cursor_pointer())
                                .when(!can_submit_change, |s| s.opacity(0.5))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !can_submit_change {
                                        return;
                                    }
                                    if let Some(m) = this.verify_email_modal.as_mut() {
                                        m.busy = true;
                                        let req = change_email_request(&m.email, &m.password);
                                        let _ = this.requests.send(req);
                                        cx.notify();
                                    }
                                }))
                                .child(dc_intl::t("v740sh")),
                        )
                    }),
            );

        Some(
            gpui::deferred(
                div()
                    .id("verify-email-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.verify_email_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `verify-email [resend|resent|change|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_verify_email(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "resent" => {
                self.verify_email_modal = Some(VerifyEmailModal {
                    email: "lorean@example.com".into(),
                    password: String::new(),
                    change_mode: false,
                    can_change: true,
                    resent: true,
                    busy: false,
                    email_error: None,
                    password_error: None,
                });
                cx.notify();
                "verify-email: resent".into()
            }
            "change" => {
                self.verify_email_modal = Some(VerifyEmailModal {
                    email: "lorean@example.com".into(),
                    password: "password123".into(),
                    change_mode: true,
                    can_change: true,
                    resent: false,
                    busy: false,
                    email_error: None,
                    password_error: None,
                });
                cx.notify();
                "verify-email: change".into()
            }
            "close" => {
                self.verify_email_modal = None;
                cx.notify();
                "verify-email: closed".into()
            }
            _ => {
                self.verify_email_modal = Some(VerifyEmailModal {
                    email: "lorean@example.com".into(),
                    password: String::new(),
                    change_mode: false,
                    can_change: true,
                    resent: false,
                    busy: false,
                    email_error: None,
                    password_error: None,
                });
                cx.notify();
                "verify-email: resend".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_verify_email_requests() {
        assert!(matches!(
            resend_request(),
            Request::Rest { route, .. } if route == "/auth/verify/resend"
        ));
        assert!(matches!(
            change_email_request("a@b.com", "pw"),
            Request::Rest { route, .. } if route == "/users/@me"
        ));
    }
}
