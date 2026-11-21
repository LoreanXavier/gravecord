//! Uncancel Server Boost 2-step modal (module 342744).
//!
//! Sibling to `boost_cancel.rs` (`983511`), opened from User Settings -> Server Boost
//! (`settings/server_boost_page.rs`) for a pending-cancellation Server Boost slot.
//! Step 1 (`Confirm`): header `l52ih2` ("Uncancel Boost Subscription"), prompt `DY2CXs`
//! ("Would you like to uncancel this Server Boost?"), optional Nitro credits disabled note
//! `dq4vq7`, error banner `5mlOCW`, `oEAioF` ("Never Mind") + `etZP4B` ("Yes, Uncancel Now")
//! calling `POST /users/@me/guilds/premium/subscription-slots/{id}/uncancel`.
//! Step 2 (`Saved`): `H9QUAB` ("Boosts Saved!"), `G27uHe` ("This Server Boost is saved from
//! cancellation! Keep on Boosting!"), `BddRzS` ("Okay").

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::Value;

pub(crate) const UNCANCEL_SLOT_KEY: &str = "boost:uncancel-slot";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoostUncancelStep {
    Confirm,
    Saved,
}

#[derive(Clone, Debug)]
pub(crate) struct BoostUncancelModal {
    pub slot_id: String,
    pub credits_active: bool,
    pub step: BoostUncancelStep,
    pub busy: bool,
    pub error: Option<String>,
}

impl BoostUncancelModal {
    pub(crate) fn new(slot_id: String, credits_active: bool) -> Self {
        Self {
            slot_id,
            credits_active,
            step: BoostUncancelStep::Confirm,
            busy: false,
            error: None,
        }
    }
}

/// Official `USER_GUILD_BOOST_SLOT_UNCANCEL` (module 342744 / 559201).
pub(crate) fn uncancel_slot_request(slot_id: &str) -> Request {
    Request::Rest {
        key: Some(UNCANCEL_SLOT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/users/@me/guilds/premium/subscription-slots/{slot_id}/uncancel"),
        body: None,
    }
}

impl Root {
    /// Opens the Uncancel Server Boost modal (`342744`).
    pub(super) fn open_boost_uncancel_modal(
        &mut self,
        slot_id: String,
        credits_active: bool,
        cx: &mut Context<Self>,
    ) {
        self.boost_uncancel_modal = Some(BoostUncancelModal::new(slot_id, credits_active));
        cx.notify();
    }

    /// Handles the `POST .../uncancel` REST response.
    pub(super) fn handle_boost_uncancel_response(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(modal) = self.boost_uncancel_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if failed {
            modal.error = Some(dc_intl::t("5mlOCW").to_owned());
        } else {
            modal.error = None;
            modal.step = BoostUncancelStep::Saved;
        }
        cx.notify();
    }

    /// Modal layer for Uncancel Server Boost (`342744`).
    pub(super) fn boost_uncancel_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.boost_uncancel_modal.as_ref()?;
        let slot_id = modal.slot_id.clone();
        let busy = modal.busy;
        let disabled = busy || modal.credits_active;

        let card = match modal.step {
            BoostUncancelStep::Confirm => div()
                .id("boost-uncancel-modal")
                .w(px(440.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("l52ih2")),
                        )
                        .child(
                            div()
                                .id("boost-uncancel-close")
                                .flex_none()
                                .size(px(32.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 18., "--icon-subtle"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.boost_uncancel_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("DY2CXs")),
                        )
                        .when(modal.credits_active, |col| {
                            col.child(
                                div()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(color("--status-warning"))
                                    .bg(rgba(0xf0b2321a))
                                    .px(px(12.))
                                    .py(px(8.))
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("dq4vq7")),
                            )
                        })
                        .children(modal.error.as_ref().map(|err| {
                            div()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .bg(rgba(0xda373c1a))
                                .px(px(12.))
                                .py(px(8.))
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-critical"))
                                .child(err.clone())
                        })),
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
                        .justify_end()
                        .gap(px(12.))
                        .child(
                            div()
                                .id("boost-uncancel-nevermind")
                                .cursor_pointer()
                                .px(px(14.))
                                .h(px(38.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("oEAioF"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.boost_uncancel_modal = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("boost-uncancel-confirm")
                                .when(!disabled, |d| d.cursor_pointer())
                                .when(disabled, |d| d.opacity(0.5))
                                .px(px(18.))
                                .h(px(38.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("etZP4B"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if disabled {
                                        return;
                                    }
                                    if let Some(m) = this.boost_uncancel_modal.as_mut() {
                                        m.busy = true;
                                        m.error = None;
                                    }
                                    let _ = this.requests.send(uncancel_slot_request(&slot_id));
                                    cx.notify();
                                })),
                        ),
                ),
            BoostUncancelStep::Saved => div()
                .id("boost-uncancel-saved")
                .w(px(440.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(28.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(64.))
                                .rounded_full()
                                .bg(rgba(0xf47fff2a))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("boost_gem", 34., "--white")),
                        )
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("H9QUAB")),
                        )
                        .child(
                            div()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("G27uHe")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("boost-uncancel-okay")
                                .cursor_pointer()
                                .w_full()
                                .h(px(38.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("BddRzS"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.boost_uncancel_modal = None;
                                    cx.notify();
                                })),
                        ),
                ),
        };

        Some(
            deferred(
                div()
                    .id("boost-uncancel-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.boost_uncancel_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `boost-uncancel [confirm|credits|error|saved|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_boost_uncancel(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut modal = BoostUncancelModal::new("slot_demo_1".to_owned(), false);
        match arg.trim() {
            "" | "confirm" => {
                self.boost_uncancel_modal = Some(modal);
                cx.notify();
                "boost-uncancel: confirm".to_owned()
            }
            "credits" => {
                modal.credits_active = true;
                self.boost_uncancel_modal = Some(modal);
                cx.notify();
                "boost-uncancel: credits".to_owned()
            }
            "error" => {
                modal.error = Some(dc_intl::t("5mlOCW").to_owned());
                self.boost_uncancel_modal = Some(modal);
                cx.notify();
                "boost-uncancel: error".to_owned()
            }
            "saved" => {
                modal.step = BoostUncancelStep::Saved;
                self.boost_uncancel_modal = Some(modal);
                cx.notify();
                "boost-uncancel: saved".to_owned()
            }
            "close" => {
                self.boost_uncancel_modal = None;
                cx.notify();
                "boost-uncancel: closed".to_owned()
            }
            other => format!("boost-uncancel: unknown {other}"),
        }
    }
}
