//! Application Test Mode modal (module 868511 + 271866 + 795816 + 158935).
//!
//! Opened from Settings > Advanced > Application Test Mode (when Developer Mode is on):
//! - Title `f8fzky` ("Application Test Mode"), subtitle `a6Vill`.
//! - Application ID input (`P6TzgI`, max 19 digits, error `gPNgKO` "Invalid application ID").
//! - URL Origin Type selector (`/GTqXG`: `+Y9Y6r` "Localhost" or `uaksyW` "Discord Proxy").
//! - Localhost Port Number input (`fF4zxq`, default `"8080"`, max 5 digits, shown only for Localhost).
//! - Primary CTA `qwuK5I` ("Activate") or destructive `d6TR3I` ("Clear") when already authorized.
//! - Calls `GET /activities/{applicationId}/test-mode` (`795816.Ir`) and displays critical alert
//!   banner when authorization fails.

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgba};
use serde_json::Value;

pub(super) const TEST_MODE_KEY: &str = "app-test-mode:authorize";
const ACTIVITY_APPLICATION_HOST: &str = "discordsays.com";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginType {
    Localhost,
    DiscordProxy,
}

/// Persisted active test mode session (`271866`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TestModeActive {
    pub application_id: Option<String>,
    pub origin_url: Option<String>,
}

/// Modal state for module `868511`.
#[derive(Clone, Debug)]
pub(super) struct AppTestModeModal {
    pub app_id: String,
    pub origin_type: OriginType,
    pub port: String,
    pub origin_dropdown_open: bool,
    pub busy: bool,
    pub auth_error: Option<String>,
}

/// Returns true iff `id` is empty or 1..=19 ASCII digits (`/^\d+$|^$/`, `maxLength: 19`).
pub fn is_valid_application_id(id: &str) -> bool {
    id.len() <= 19 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Builds the embedded application origin URL (`868511` / `956518.Ay`).
pub fn build_origin_url(
    app_id: &str,
    origin: OriginType,
    port: &str,
) -> Result<String, &'static str> {
    if app_id.is_empty() || !is_valid_application_id(app_id) {
        return Err("Invalid application ID");
    }
    match origin {
        OriginType::Localhost => {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Invalid Origin URL for embedded application");
            }
            Ok(format!("https://localhost:{port}"))
        }
        OriginType::DiscordProxy => Ok(format!("https://{app_id}.{ACTIVITY_APPLICATION_HOST}")),
    }
}

/// Builds `GET /activities/{applicationId}/test-mode` (`795816.Ir`).
pub fn authorize_request(app_id: &str) -> Request {
    Request::Fetch {
        key: TEST_MODE_KEY.to_owned(),
        route: format!("/activities/{app_id}/test-mode"),
    }
}

impl Root {
    /// Opens the Application Test Mode modal (module `868511`).
    pub(super) fn open_app_test_mode_modal(&mut self, cx: &mut Context<Self>) {
        let app_id = self
            .app_test_mode_active
            .application_id
            .clone()
            .unwrap_or_default();
        self.app_test_mode_modal = Some(AppTestModeModal {
            app_id,
            origin_type: OriginType::Localhost,
            port: "8080".into(),
            origin_dropdown_open: false,
            busy: false,
            auth_error: None,
        });
        cx.notify();
    }

    /// Handles `GET /activities/{id}/test-mode` response (`795816.Ir`).
    pub(super) fn handle_app_test_mode_response(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(modal) = self.app_test_mode_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        if body.is_null() || body.get("code").is_some() || body.get("message").is_some() {
            modal.auth_error = Some(
                body["message"]
                    .as_str()
                    .unwrap_or("Do not have access!")
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        let origin = build_origin_url(&modal.app_id, modal.origin_type, &modal.port).ok();
        self.app_test_mode_active = TestModeActive {
            application_id: Some(modal.app_id.clone()),
            origin_url: origin,
        };
        self.app_test_mode_modal = None;
        self.put_app_toast(dc_intl::t("f8fzky").to_owned(), true, cx);
        cx.notify();
    }

    /// Renders module `868511` Application Test Mode modal.
    pub(super) fn app_test_mode_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.app_test_mode_modal.as_ref()?;
        let valid_id = is_valid_application_id(&modal.app_id);
        let id_non_empty = !modal.app_id.is_empty();
        let is_authorized_match = self
            .app_test_mode_active
            .application_id
            .as_deref()
            .is_some_and(|auth| auth == modal.app_id && !auth.is_empty());
        let port_ok = modal.origin_type != OriginType::Localhost
            || (!modal.port.is_empty() && modal.port.bytes().all(|b| b.is_ascii_digit()));
        let can_submit = !modal.busy && valid_id && id_non_empty && port_ok;
        let origin_disabled = !valid_id || !id_non_empty;
        let origin_label = match modal.origin_type {
            OriginType::Localhost => dc_intl::t("+Y9Y6r"),
            OriginType::DiscordProxy => dc_intl::t("uaksyW"),
        };

        let card = div()
            .id("app-test-mode-modal")
            .w(px(460.))
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
                    .gap(px(14.))
                    // Header (`f8fzky` + close)
                    .child(
                        div()
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
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("f8fzky")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("a6Vill")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("app-test-mode-close")
                                    .size(px(28.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.app_test_mode_modal = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    // Critical auth error banner (`683071.w`)
                    .when_some(modal.auth_error.clone(), |col, err| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(6.))
                                .bg(rgba(0xda373c22))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(div().flex_none().child(icon(
                                    "shop_circle_error",
                                    18.,
                                    "--status-danger",
                                )))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(SharedString::from(err)),
                                ),
                        )
                    })
                    // Application ID field (`P6TzgI`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(dc_intl::t("P6TzgI").to_uppercase()))
                                    .child(
                                        div()
                                            .text_color(color("--text-feedback-critical"))
                                            .child("*"),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lowest"))
                                    .border_1()
                                    .border_color(if !valid_id {
                                        color("--status-danger")
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(if modal.app_id.is_empty() {
                                        color("--text-muted")
                                    } else {
                                        color("--text-default")
                                    })
                                    .child(if modal.app_id.is_empty() {
                                        SharedString::from("123456789012345678")
                                    } else {
                                        SharedString::from(modal.app_id.clone())
                                    }),
                            )
                            .when(!valid_id, |f| {
                                f.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("gPNgKO")),
                                )
                            }),
                    )
                    // URL Origin Type dropdown (`/GTqXG`)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .when(origin_disabled, |el| el.opacity(0.5))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(dc_intl::t("/GTqXG").to_uppercase())),
                            )
                            .child(
                                div()
                                    .relative()
                                    .child(
                                        div()
                                            .id("app-test-mode-origin-select")
                                            .relative()
                                            .h(px(40.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .bg(color("--background-base-lowest"))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .when(!origin_disabled, |s| s.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !origin_disabled
                                                    && let Some(m) =
                                                        this.app_test_mode_modal.as_mut()
                                                {
                                                    m.origin_dropdown_open =
                                                        !m.origin_dropdown_open;
                                                    cx.notify();
                                                }
                                            }))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .text_color(color("--text-default"))
                                                    .child(origin_label),
                                            )
                                            .child(icon("chevron_down", 14., "--icon-subtle"))
                                            .child(
                                                self.anchor_probe("app-test-mode-origin-select"),
                                            ),
                                    )
                                    .when(modal.origin_dropdown_open && !origin_disabled, |el| {
                                        el.child(gpui::deferred(
                                    div()
                                        .id("app-test-mode-origin-list")
                                        .occlude()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(44.))
                                        .shadow(vec![super::shadow_high()])
                                        .on_click(|_, _, cx| cx.stop_propagation())
                                        .on_mouse_down_out(cx.listener(
                                            move |this, event: &gpui::MouseDownEvent, _, cx| {
                                                let on_trigger = this
                                                    .tip_anchors
                                                    .borrow()
                                                    .get("app-test-mode-origin-select")
                                                    .is_some_and(|b| b.contains(&event.position));
                                                if !origin_disabled
                                                    && !on_trigger
                                                    && let Some(m) =
                                                        this.app_test_mode_modal.as_mut()
                                                {
                                                    m.origin_dropdown_open = false;
                                                    cx.notify();
                                                }
                                            },
                                        ))
                                        .rounded(px(8.))
                                        .bg(color("--background-base-lower"))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .p(px(4.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .id("app-test-mode-origin-localhost")
                                                .flex_none()
                                                .px(px(10.))
                                                .py(px(8.))
                                                .rounded(px(6.))
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(color("--interactive-background-hover"))
                                                })
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.app_test_mode_modal.as_mut()
                                                    {
                                                        m.origin_type = OriginType::Localhost;
                                                        m.origin_dropdown_open = false;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(dc_intl::t("+Y9Y6r")),
                                        )
                                        .child(
                                            div()
                                                .id("app-test-mode-origin-proxy")
                                                .flex_none()
                                                .px(px(10.))
                                                .py(px(8.))
                                                .rounded(px(6.))
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(color("--interactive-background-hover"))
                                                })
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.app_test_mode_modal.as_mut()
                                                    {
                                                        m.origin_type = OriginType::DiscordProxy;
                                                        m.origin_dropdown_open = false;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(dc_intl::t("uaksyW")),
                                        ),
                                ).with_priority(99))
                                    }),
                            ),
                    )
                    // Localhost Port Number (`fF4zxq`, shown only for Localhost)
                    .when(modal.origin_type == OriginType::Localhost, |col| {
                        col.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-muted"))
                                        .child(SharedString::from(
                                            dc_intl::t("fF4zxq").to_uppercase(),
                                        ))
                                        .child(
                                            div()
                                                .text_color(color("--text-feedback-critical"))
                                                .child("*"),
                                        ),
                                )
                                .child(
                                    div()
                                        .h(px(40.))
                                        .px(px(12.))
                                        .rounded(px(8.))
                                        .bg(color("--background-base-lowest"))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .text_color(color("--text-default"))
                                        .child(SharedString::from(modal.port.clone())),
                                ),
                        )
                    }),
            )
            // Footer (`qwuK5I` Activate or `d6TR3I` Clear)
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("app-test-mode-submit")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(if is_authorized_match {
                                color("--control-critical-primary-background-default")
                            } else {
                                color("--control-primary-background-default")
                            })
                            .when(can_submit || is_authorized_match, |s| s.cursor_pointer())
                            .when(!can_submit && !is_authorized_match, |s| s.opacity(0.5))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if is_authorized_match {
                                    this.app_test_mode_active = TestModeActive::default();
                                    this.app_test_mode_modal = None;
                                    cx.notify();
                                    return;
                                }
                                if !can_submit {
                                    return;
                                }
                                if let Some(m) = this.app_test_mode_modal.as_mut() {
                                    match build_origin_url(&m.app_id, m.origin_type, &m.port) {
                                        Ok(_) => {
                                            m.busy = true;
                                            m.auth_error = None;
                                            let req = authorize_request(&m.app_id);
                                            let _ = this.requests.send(req);
                                        }
                                        Err(err) => {
                                            m.auth_error = Some(err.to_owned());
                                        }
                                    }
                                    cx.notify();
                                }
                            }))
                            .child(dc_intl::t(if is_authorized_match {
                                "d6TR3I"
                            } else {
                                "qwuK5I"
                            })),
                    ),
            );

        Some(
            gpui::deferred(
                div()
                    .id("app-test-mode-backdrop")
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
                        this.app_test_mode_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `app-test-mode [open|proxy|active|error|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_app_test_mode(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "proxy" => {
                self.app_test_mode_modal = Some(AppTestModeModal {
                    app_id: "123456789012345678".into(),
                    origin_type: OriginType::DiscordProxy,
                    port: "8080".into(),
                    origin_dropdown_open: false,
                    busy: false,
                    auth_error: None,
                });
                cx.notify();
                "app-test-mode: proxy".into()
            }
            "active" => {
                self.app_test_mode_active = TestModeActive {
                    application_id: Some("123456789012345678".into()),
                    origin_url: Some("https://localhost:8080".into()),
                };
                self.open_app_test_mode_modal(cx);
                "app-test-mode: active".into()
            }
            "error" => {
                self.app_test_mode_modal = Some(AppTestModeModal {
                    app_id: "123456789012345678".into(),
                    origin_type: OriginType::Localhost,
                    port: "8080".into(),
                    origin_dropdown_open: false,
                    busy: false,
                    auth_error: Some("Do not have access!".into()),
                });
                cx.notify();
                "app-test-mode: error".into()
            }
            "close" => {
                self.app_test_mode_modal = None;
                cx.notify();
                "app-test-mode: closed".into()
            }
            _ => {
                self.app_test_mode_modal = Some(AppTestModeModal {
                    app_id: "123456789012345678".into(),
                    origin_type: OriginType::Localhost,
                    port: "8080".into(),
                    origin_dropdown_open: false,
                    busy: false,
                    auth_error: None,
                });
                cx.notify();
                "app-test-mode: open".into()
            }
        }
    }
}

impl Root {
    pub(super) fn close_app_test_mode_origin_select(&mut self) -> bool {
        if let Some(m) = self.app_test_mode_modal.as_mut()
            && m.origin_dropdown_open
        {
            m.origin_dropdown_open = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_app_id_and_origin_urls() {
        assert!(is_valid_application_id(""));
        assert!(is_valid_application_id("123456789012345678"));
        assert!(!is_valid_application_id("abc123"));
        assert!(!is_valid_application_id("12345678901234567890"));
        assert_eq!(
            build_origin_url("12345", OriginType::Localhost, "8080").unwrap(),
            "https://localhost:8080"
        );
        assert_eq!(
            build_origin_url("12345", OriginType::DiscordProxy, "").unwrap(),
            "https://12345.discordsays.com"
        );
    }
}
