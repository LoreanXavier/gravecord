//! Finish Signing Up / Claim Account Modal (module 110744 `v` / `N`, CSS `27977` `_pwDXlN`).
//!
//! Opened for unclaimed accounts (`current_user.email.is_none()`, module 158935 `_Y` / module 315982 `R`
//! `claimRequired` desktop gate).
//! - Step 1 (`Form`): illustration, `"Finish signing up"` (`uQShv/`), subtitle (`sW28gZ` when
//!   `claim_required`, else `gP/vPe`), Email (`dI4d4S`) + Password (`CIGa+7`, revealable) editors,
//!   `"Claim Account"` (`fiNVin`) sending `PATCH /users/@me` (`{ email, password }`), optional
//!   `"Log Out"` (`2jxGer`) when `claim_required`. Parses `body.email` and `body.password` form errors.
//! - Step 2 (`Success`): illustration, `"Thanks! We've sent a confirmation link to {email}..."`
//!   (`D7trIB` when `claim_required`, else `JNWX7G`), subtext (`eXqM2d`), and `"Okay"` (`BddRzS`) or
//!   `"Get the Desktop App"` (`ygArIV`).

use super::{Root, color, editor_style, icon};
use crate::editor::Editor;
use crate::net::Request;
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, IntoElement, StyledText, div,
    prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::borrow::Cow;

/// REST response key for `PATCH /users/@me` claim request (`631670.KD`).
pub(super) const CLAIM_KEY: &str = "claim-account:submit";
/// Download URL opened by `"Get the Desktop App"` (`ygArIV`).
const DOWNLOAD_URL: &str = "https://discord.com/download";

/// Active state of the `110744` Claim Account modal.
#[derive(Clone)]
pub(super) struct ClaimAccountModal {
    pub claim_required: bool,
    pub claimed: bool,
    pub submitting: bool,
    pub show_password: bool,
    pub submitted_email: String,
    pub email_error: Option<String>,
    pub password_error: Option<String>,
    pub email: Entity<Editor>,
    pub password: Entity<Editor>,
}

/// Builds the `PATCH /users/@me` claim request (`631670.KD`, module 110744).
pub fn claim_account_request(email: &str, password: &str) -> Request {
    Request::Rest {
        key: Some(CLAIM_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: "/users/@me".to_owned(),
        body: Some(json!({
            "email": email,
            "password": password,
        })),
    }
}

/// Extracts `email` and `password` error strings from a `PATCH /users/@me` error response (`oldFormErrors` or `errors`).
pub fn parse_claim_errors(body: &Value) -> (Option<String>, Option<String>) {
    let field = |name: &str| -> Option<String> {
        if let Some(arr) = body[name].as_array()
            && let Some(first) = arr.first().and_then(|v| v.as_str())
        {
            return Some(first.to_owned());
        }
        if let Some(s) = body[name].as_str()
            && !s.is_empty()
        {
            return Some(s.to_owned());
        }
        body["errors"][name]["_errors"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|e| e["message"].as_str())
            .map(str::to_owned)
    };
    let mut email_err = field("email");
    let password_err = field("password");
    if email_err.is_none()
        && password_err.is_none()
        && let Some(msg) = body["message"].as_str()
        && !msg.is_empty()
    {
        email_err = Some(msg.to_owned());
    }
    (email_err, password_err)
}

/// Builds bold-highlighted `StyledText` from a `dc_intl` key with `$b` runs.
fn bold_styled_text(key: &str, args: &[(&str, Arg<'_>)]) -> StyledText {
    let runs = dc_intl::format_runs(key, args);
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (run, tag) in runs {
        let start = text.len();
        text.push_str(&run);
        if tag.as_deref() == Some("$b") {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Opens the `110744` Claim Account modal.
    #[allow(dead_code)]
    pub(super) fn open_claim_account_modal(
        &mut self,
        claim_required: bool,
        cx: &mut Context<Self>,
    ) {
        let email = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder("you@example.com".to_owned());
            ed
        });
        let password = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder("••••••••".to_owned());
            ed
        });
        self.claim_account_modal = Some(ClaimAccountModal {
            claim_required,
            claimed: false,
            submitting: false,
            show_password: false,
            submitted_email: String::new(),
            email_error: None,
            password_error: None,
            email,
            password,
        });
        cx.notify();
    }

    /// Closes the Claim Account modal unless `claim_required` blocks dismissal (`315982.R`).
    pub(super) fn close_claim_account_if_allowed(&mut self) {
        if self
            .claim_account_modal
            .as_ref()
            .is_some_and(|m| !m.claim_required || m.claimed)
        {
            self.claim_account_modal = None;
        }
    }

    /// Handles the `PATCH /users/@me` claim response (`110744.js`).
    pub(super) fn handle_claim_account_response(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(modal) = self.claim_account_modal.as_mut() else {
            return;
        };
        modal.submitting = false;
        let has_err = body.is_null()
            || body.get("code").is_some()
            || body.get("errors").is_some()
            || body.get("email").is_some_and(|v| v.is_array())
            || body.get("password").is_some_and(|v| v.is_array());
        if !has_err && body.get("id").is_some() {
            modal.claimed = true;
            modal.email_error = None;
            modal.password_error = None;
            if let Some(em) = body["email"].as_str() {
                modal.submitted_email = em.to_owned();
                self.stores.account.email = Some(em.to_owned());
            }
        } else {
            let (e_err, p_err) = parse_claim_errors(body);
            modal.email_error = e_err;
            modal.password_error = p_err;
        }
        cx.notify();
    }

    /// Renders the `110744` Claim Account modal (Step 1 Form or Step 2 Success).
    pub(super) fn claim_account_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.claim_account_modal.as_ref()?;
        let can_dismiss = !modal.claim_required || modal.claimed;
        let card = if modal.claimed {
            self.render_claim_account_success(modal, cx)
        } else {
            self.render_claim_account_form(modal, cx)
        };
        Some(
            gpui::deferred(
                div()
                    .id("claim-account-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if can_dismiss {
                            this.claim_account_modal = None;
                            cx.notify();
                        }
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    fn render_claim_account_form(
        &self,
        modal: &ClaimAccountModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let email_val = modal.email.read(cx).text().trim().to_owned();
        let pass_val = modal.password.read(cx).text().to_owned();
        let disabled = modal.submitting || email_val.is_empty() || pass_val.is_empty();
        let subtitle = dc_intl::t(if modal.claim_required {
            "sW28gZ"
        } else {
            "gP/vPe"
        });
        let can_close = !modal.claim_required;
        let masked_pass = if modal.show_password || pass_val.is_empty() {
            None
        } else {
            Some("•".repeat(pass_val.chars().count()))
        };

        div()
            .id("claim-account-form-modal")
            .relative()
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .p(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .on_click(|_, _, cx| cx.stop_propagation())
            .when(can_close, |this| {
                this.child(
                    div()
                        .id("claim-account-close")
                        .absolute()
                        .top(px(12.))
                        .right(px(12.))
                        .size(px(28.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.claim_account_modal = None;
                            cx.notify();
                        }))
                        .child(icon("modal_close", 16., "--interactive-text-default")),
                )
            })
            // Top emblem (`formImage_pwDXlN`: 88x80 art slot)
            .child(
                div()
                    .w(px(72.))
                    .h(px(72.))
                    .mb(px(12.))
                    .rounded_full()
                    .bg(color("--brand-500"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("user_check", 34., "--white")),
            )
            // `uQShv/`: "Finish signing up"
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(22.))
                    .line_height(px(28.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .text_center()
                    .child(dc_intl::t("uQShv/")),
            )
            .child(
                div()
                    .mb(px(20.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .text_center()
                    .child(subtitle),
            )
            // Form stack (`gap=16`)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    // Email field (`dI4d4S`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(if modal.email_error.is_some() {
                                        color("--text-feedback-critical")
                                    } else {
                                        color("--text-subtle")
                                    })
                                    .child(dc_intl::t("dI4d4S").to_uppercase()),
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
                                    .child(div().flex_1().min_w_0().child(modal.email.clone())),
                            )
                            .children(modal.email_error.as_ref().map(|err| {
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-feedback-critical"))
                                    .child(err.clone())
                            })),
                    )
                    // Password field (`CIGa+7`, revealable)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(if modal.password_error.is_some() {
                                        color("--text-feedback-critical")
                                    } else {
                                        color("--text-subtle")
                                    })
                                    .child(dc_intl::t("CIGa+7").to_uppercase()),
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
                                    .gap(px(8.))
                                    .child(if let Some(masked) = masked_pass {
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.))
                                            .text_color(color("--text-default"))
                                            .child(masked)
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(modal.password.clone())
                                            .into_any_element()
                                    })
                                    .child(
                                        div()
                                            .id("claim-account-toggle-pw")
                                            .flex_none()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-link"))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) = this.claim_account_modal.as_mut() {
                                                    m.show_password = !m.show_password;
                                                    cx.notify();
                                                }
                                            }))
                                            .child(if modal.show_password {
                                                "Hide"
                                            } else {
                                                "Show"
                                            }),
                                    ),
                            )
                            .children(modal.password_error.as_ref().map(|err| {
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-feedback-critical"))
                                    .child(err.clone())
                            })),
                    )
                    // Primary submit (`fiNVin`: "Claim Account")
                    .child(
                        div()
                            .id("claim-account-submit")
                            .w_full()
                            .h(px(42.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(!disabled, |s| {
                                s.cursor_pointer()
                                    .hover(|h| h.bg(color("--control-primary-background-hover")))
                            })
                            .when(disabled, |s| s.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--control-primary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if disabled {
                                    return;
                                }
                                if let Some(m) = this.claim_account_modal.as_mut() {
                                    m.submitting = true;
                                    m.submitted_email = email_val.clone();
                                    m.email_error = None;
                                    m.password_error = None;
                                    let req = claim_account_request(&email_val, &pass_val);
                                    let _ = this.requests.send(req);
                                    cx.notify();
                                }
                            }))
                            .child(dc_intl::t("fiNVin")),
                    )
                    // `2jxGer`: "Log Out" when `claimRequired`
                    .when(modal.claim_required, |stack| {
                        stack.child(
                            div().mt(px(2.)).w_full().flex().justify_center().child(
                                div()
                                    .id("claim-account-logout")
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-link"))
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.claim_account_modal = None;
                                        cx.emit(crate::ui::logout::RootEvent::LogOut);
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("2jxGer")),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_claim_account_success(
        &self,
        modal: &ClaimAccountModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let email = modal.submitted_email.as_str();
        let heading = bold_styled_text(
            if modal.claim_required {
                "D7trIB"
            } else {
                "JNWX7G"
            },
            &[("email", Arg::Str(Cow::Borrowed(email)))],
        );
        let claim_required = modal.claim_required;
        div()
            .id("claim-account-success-modal")
            .relative()
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .p(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .on_click(|_, _, cx| cx.stop_propagation())
            .when(!claim_required, |this| {
                this.child(
                    div()
                        .id("claim-account-success-close")
                        .absolute()
                        .top(px(12.))
                        .right(px(12.))
                        .size(px(28.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.claim_account_modal = None;
                            cx.notify();
                        }))
                        .child(icon("modal_close", 16., "--interactive-text-default")),
                )
            })
            // Success illustration (`successImage_pwDXlN`)
            .child(
                div()
                    .my(px(12.))
                    .w(px(76.))
                    .h(px(76.))
                    .rounded_full()
                    .bg(color("--green-360"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("checkmark_large", 36., "--white")),
            )
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(heading),
            )
            // `eXqM2d`
            .child(
                div()
                    .mb(px(20.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("eXqM2d")),
            )
            .child(
                div()
                    .id("claim-account-success-cta")
                    .w_full()
                    .h(px(42.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(15.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-primary-text-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !claim_required {
                            cx.open_url(DOWNLOAD_URL);
                        }
                        this.claim_account_modal = None;
                        cx.notify();
                    }))
                    // `BddRzS`: "Okay" | `ygArIV`: "Get the Desktop App"
                    .child(dc_intl::t(if claim_required { "BddRzS" } else { "ygArIV" })),
            )
            .into_any_element()
    }

    /// Capture-only helper: `claim-account <form|required|success|error|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_claim_account(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "form" => {
                self.open_claim_account_modal(false, cx);
                "claim-account: form".into()
            }
            "required" => {
                self.open_claim_account_modal(true, cx);
                "claim-account: required".into()
            }
            "error" => {
                self.open_claim_account_modal(false, cx);
                if let Some(m) = self.claim_account_modal.as_mut() {
                    m.email_error = Some("Not a well formed email address.".into());
                    m.password_error = Some("Must be 8 or more in length.".into());
                }
                cx.notify();
                "claim-account: error".into()
            }
            "success" => {
                self.open_claim_account_modal(true, cx);
                if let Some(m) = self.claim_account_modal.as_mut() {
                    m.claimed = true;
                    m.submitted_email = "lorean@example.com".into();
                }
                cx.notify();
                "claim-account: success".into()
            }
            "close" => {
                self.claim_account_modal = None;
                cx.notify();
                "claim-account: closed".into()
            }
            other => format!("claim-account: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_account_request_targets_users_me_patch() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = claim_account_request("a@b.co", "secret123")
        else {
            panic!("expected Rest");
        };
        assert_eq!(key.as_deref(), Some(CLAIM_KEY));
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me");
        assert_eq!(body.unwrap()["email"], "a@b.co");
    }

    #[test]
    fn parse_claim_errors_reads_both_old_and_nested_shapes() {
        let old = json!({"email": ["Email is already registered."], "password": ["Too short."]});
        assert_eq!(
            parse_claim_errors(&old),
            (
                Some("Email is already registered.".into()),
                Some("Too short.".into())
            )
        );
        let nested = json!({
            "errors": {
                "email": {"_errors": [{"message": "Invalid email"}]}
            }
        });
        assert_eq!(
            parse_claim_errors(&nested),
            (Some("Invalid email".into()), None)
        );
    }
}
