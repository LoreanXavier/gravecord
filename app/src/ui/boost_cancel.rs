//! Cancel Server Boost 3-step modal (module 983511, `559201` `HJ` cancel endpoint).
//!
//! Step 1 (`V`): confirmation prompt ("Cancel Server Boost From Inventory" or "Cancel Server Boost From A Server"),
//! handling external payment gateways (Apple/Google Subscription Management link), fractional Nitro credit pause dates,
//! and standard billing period end dates.
//! Step 2 (`k`): invoice/subscription updates preview ("Cancel Server Boost", "Subscription Updates",
//! "1 Server Boost Canceled", renewal delta, critical error alert, `Back` + `Yes, Cancel Boost`).
//! Step 3 (`w`): completion screen ("Boost Canceled!", end date, `Okay`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement, SharedString,
    StyledText, deferred, div, prelude::*, px, rgba,
};
use serde_json::Value;
use std::borrow::Cow;

pub(crate) type Run = (String, Option<Box<str>>);

pub(crate) const CANCEL_SLOT_KEY: &str = "boost:cancel-slot";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoostCancelStep {
    Confirm,
    #[allow(dead_code)]
    LoadingPreview,
    Preview,
    Canceled,
}

#[derive(Clone, Debug)]
pub(crate) struct BoostCancelModal {
    pub slot_id: String,
    pub guild_id: Option<Id>,
    pub period_end_long: String,
    pub period_end_medium: String,
    /// `(payment_gateway_name, subscription_management_url)` for Apple/Google external subscriptions.
    pub external_gateway: Option<(String, String)>,
    /// Medium-formatted date when subscription is paused for fractional Nitro credits.
    pub fractional_end_medium: Option<String>,
    pub step: BoostCancelStep,
    pub price_delta_label: String,
    pub renewal_total_label: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
}

impl BoostCancelModal {
    pub(crate) fn new(
        slot_id: String,
        guild_id: Option<Id>,
        period_end_long: String,
        period_end_medium: String,
    ) -> Self {
        Self {
            slot_id,
            guild_id,
            period_end_long,
            period_end_medium,
            external_gateway: None,
            fractional_end_medium: None,
            step: BoostCancelStep::Confirm,
            price_delta_label: "-$4.99/month".to_owned(),
            renewal_total_label: Some("$9.99/month".to_owned()),
            busy: false,
            error: None,
        }
    }

    pub(crate) fn is_inventory(&self) -> bool {
        self.guild_id.is_none()
    }
}

/// Official `USER_GUILD_BOOST_SLOT_CANCEL` (`559201` `HJ`, `652215`).
pub(crate) fn cancel_slot_request(slot_id: &str) -> Request {
    Request::Rest {
        key: Some(CANCEL_SLOT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/users/@me/guilds/premium/subscription-slots/{slot_id}/cancel"),
        body: None,
    }
}

/// Renders `dc_intl::format_runs` output with bold & link runs (and optional external URL for `$link` hooks).
fn render_rich_runs(
    id_prefix: &'static str,
    runs: Vec<Run>,
    link_url: Option<String>,
) -> Vec<AnyElement> {
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    for (chunk, hook) in runs {
        if !text.is_empty()
            && text.ends_with(['.', '!'])
            && chunk.starts_with(|c: char| c.is_ascii_uppercase())
        {
            text.push_str("\n\n");
        }
        // Also restore paragraph breaks inside flattened `$p` chunks (`pieces.It`, `2026.The`, `you.This`).
        let mut normalized = String::with_capacity(chunk.len() + 4);
        let chars: Vec<char> = chunk.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            normalized.push(ch);
            if matches!(ch, '.' | '!') && chars.get(i + 1).is_some_and(char::is_ascii_uppercase) {
                normalized.push_str("\n\n");
            }
        }
        let start = text.len();
        text.push_str(&normalized);
        let range = start..text.len();
        if range.is_empty() {
            continue;
        }
        match hook.as_deref() {
            Some("$b") => {
                highlights.push((
                    range,
                    HighlightStyle {
                        font_weight: Some(FontWeight(600.)),
                        color: Some(color("--text-strong").into()),
                        ..HighlightStyle::default()
                    },
                ));
            }
            Some("$i") => {
                highlights.push((
                    range,
                    HighlightStyle {
                        font_style: Some(gpui::FontStyle::Italic),
                        ..HighlightStyle::default()
                    },
                ));
            }
            Some("$link") => {
                highlights.push((
                    range.clone(),
                    HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..HighlightStyle::default()
                    },
                ));
                if let Some(url) = link_url.clone() {
                    links.push((range, url));
                }
            }
            _ => {}
        }
    }

    if links.is_empty() {
        let styled = StyledText::new(text).with_highlights(highlights);
        vec![
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(styled)
                .into_any_element(),
        ]
    } else {
        let ranges: Vec<_> = links.iter().map(|(r, _)| r.clone()).collect();
        let styled = StyledText::new(text).with_highlights(highlights);
        vec![
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(InteractiveText::new(id_prefix, styled).on_click(
                    ranges,
                    move |idx, _, cx| {
                        if let Some((_, url)) = links.get(idx) {
                            cx.open_url(url);
                        }
                    },
                ))
                .into_any_element(),
        ]
    }
}

/// Step 1 copy runs (`983511` `V`).
pub(crate) fn step1_body_runs(modal: &BoostCancelModal) -> Vec<Run> {
    if let Some((gateway, url)) = &modal.external_gateway {
        dc_intl::format_runs(
            "HbpFLg",
            &[
                ("paymentGatewayName", Arg::Str(Cow::Borrowed(gateway))),
                ("subscriptionManagementLink", Arg::Str(Cow::Borrowed(url))),
            ],
        )
    } else if let Some(exp) = &modal.fractional_end_medium {
        dc_intl::format_runs(
            "Hzqe6y",
            &[(
                "expirationDate",
                Arg::When {
                    date: Cow::Borrowed(exp),
                    time: Cow::Borrowed(""),
                },
            )],
        )
    } else if modal.is_inventory() {
        dc_intl::format_runs(
            "gXSnul",
            &[(
                "endDate",
                Arg::When {
                    date: Cow::Borrowed(&modal.period_end_long),
                    time: Cow::Borrowed(""),
                },
            )],
        )
    } else {
        dc_intl::format_runs(
            "otHdfR",
            &[(
                "endDate",
                Arg::When {
                    date: Cow::Borrowed(&modal.period_end_long),
                    time: Cow::Borrowed(""),
                },
            )],
        )
    }
}

/// Step 2 intro copy runs (`983511` `k`).
pub(crate) fn step2_intro_runs(modal: &BoostCancelModal) -> Vec<Run> {
    if let Some(exp) = &modal.fractional_end_medium {
        dc_intl::format_runs(
            "eb0xgS",
            &[(
                "expirationDate",
                Arg::When {
                    date: Cow::Borrowed(exp),
                    time: Cow::Borrowed(""),
                },
            )],
        )
    } else {
        dc_intl::format_runs(
            "0W23cu",
            &[(
                "endDate",
                Arg::When {
                    date: Cow::Borrowed(&modal.period_end_medium),
                    time: Cow::Borrowed(""),
                },
            )],
        )
    }
}

/// Step 3 confirmation copy runs (`983511` `w`).
pub(crate) fn step3_body_runs(modal: &BoostCancelModal) -> Vec<Run> {
    let date = modal
        .fractional_end_medium
        .as_deref()
        .unwrap_or(&modal.period_end_medium);
    dc_intl::format_runs(
        "H+z0ns",
        &[(
            "endDate",
            Arg::When {
                date: Cow::Borrowed(date),
                time: Cow::Borrowed(""),
            },
        )],
    )
}

impl Root {
    pub(crate) fn open_boost_cancel_modal(
        &mut self,
        slot_id: String,
        guild_id: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        self.boost_cancel_modal = Some(BoostCancelModal::new(
            slot_id,
            guild_id,
            "December 20, 2026".to_owned(),
            "Dec 20, 2026".to_owned(),
        ));
        cx.notify();
    }

    pub(crate) fn close_boost_cancel_modal(&mut self) -> bool {
        self.boost_cancel_modal.take().is_some()
    }

    pub(crate) fn boost_cancel_fetched(&mut self, body: &Value) {
        let Some(modal) = self.boost_cancel_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        if body["ok"] == true {
            modal.error = None;
            modal.step = BoostCancelStep::Canceled;
        } else {
            modal.error = Some(dc_intl::t("5mlOCW").to_owned());
        }
    }

    fn boost_cancel_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.boost_cancel_modal.as_mut() else {
            return;
        };
        if modal.busy {
            return;
        }
        modal.busy = true;
        modal.error = None;
        let slot_id = modal.slot_id.clone();
        let _ = self.requests.send(cancel_slot_request(&slot_id));
        cx.notify();
    }

    pub(in crate::ui) fn boost_cancel_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.boost_cancel_modal.as_ref()?;
        let (title, body, footer): (String, AnyElement, Option<AnyElement>) = match modal.step {
            BoostCancelStep::Confirm => {
                let title = if modal.is_inventory() {
                    dc_intl::t("iIoSd6").to_owned()
                } else {
                    dc_intl::t("Skzv0u").to_owned()
                };
                let runs = step1_body_runs(modal);
                let link_url = modal.external_gateway.as_ref().map(|(_, u)| u.clone());
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .children(render_rich_runs("boost-cancel-step1", runs, link_url))
                    .into_any_element();
                let footer = if modal.external_gateway.is_some() {
                    None
                } else {
                    Some(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("boost-cancel-close")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("ETE/oC"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_boost_cancel_modal();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("boost-cancel-next")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("PDTjLN"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.boost_cancel_modal.as_mut() {
                                            m.step = BoostCancelStep::Preview;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .into_any_element(),
                    )
                };
                (title, body, footer)
            }
            BoostCancelStep::LoadingPreview => {
                let title = dc_intl::t("CRwE7n").to_owned();
                let body = div()
                    .h(px(120.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(28.))
                            .rounded_full()
                            .border_2()
                            .border_color(color("--control-primary-background-default")),
                    )
                    .into_any_element();
                (title, body, None)
            }
            BoostCancelStep::Preview => {
                let title = dc_intl::t("CRwE7n").to_owned();
                let intro_runs = step2_intro_runs(modal);
                let canceled_label =
                    dc_intl::format("Vg+LRr", &[("subscriptionCount", Arg::Num(1.))]);
                let price_delta = modal.price_delta_label.clone();
                let renewal_total = modal.renewal_total_label.clone();
                let busy = modal.busy;
                let error = modal.error.clone();

                let summary_card = div()
                    .mt(px(12.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-higher"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("iqhIp4").to_uppercase()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(canceled_label),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(price_delta),
                            ),
                    )
                    .when_some(renewal_total, |card, renewal| {
                        card.child(div().h(px(1.)).w_full().bg(color("--border-subtle")))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::tr("Total")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(renewal),
                                    ),
                            )
                    });

                let body = div()
                    .flex()
                    .flex_col()
                    .when_some(error, |col, msg| {
                        col.child(
                            div()
                                .mb(px(12.))
                                .p(px(12.))
                                .rounded(px(8.))
                                .bg(rgba(0xf23f421f))
                                .border_1()
                                .border_color(color("--text-feedback-critical"))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-feedback-critical"))
                                .child(msg),
                        )
                    })
                    .children(render_rich_runs("boost-cancel-step2", intro_runs, None))
                    .child(summary_card)
                    .into_any_element();

                let footer = Some(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("boost-cancel-back")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("13/7kX"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.boost_cancel_modal.as_mut() {
                                        m.step = BoostCancelStep::Confirm;
                                        m.error = None;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("boost-cancel-confirm")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-critical-primary-background-default"))
                                .hover(|s| {
                                    s.bg(color("--control-critical-primary-background-hover"))
                                })
                                .when(busy, |d| d.opacity(0.5))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("/AS/gM"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.boost_cancel_confirm(cx);
                                })),
                        )
                        .into_any_element(),
                );
                (title, body, footer)
            }
            BoostCancelStep::Canceled => {
                let title = dc_intl::t("fZzV/B").to_owned();
                let runs = step3_body_runs(modal);
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .children(render_rich_runs("boost-cancel-step3", runs, None))
                    .into_any_element();
                let footer = Some(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(
                            div()
                                .id("boost-cancel-done")
                                .h(px(38.))
                                .px(px(20.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("BddRzS"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_boost_cancel_modal();
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                );
                (title, body, footer)
            }
        };

        let dialog = div()
            .id("boost-cancel-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.28),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
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
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .id("boost-cancel-x")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_boost_cancel_modal();
                                cx.notify();
                            })),
                    ),
            )
            .child(div().px(px(24.)).pb(px(20.)).child(body))
            .when_some(footer, |card, foot| {
                card.child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .rounded_b(px(12.))
                        .child(foot),
                )
            });

        Some(
            deferred(
                div()
                    .id("boost-cancel-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_boost_cancel_modal();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_boost_cancel(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut m = BoostCancelModal::new(
            "slot_capture_1".to_owned(),
            if arg == "inventory" {
                None
            } else {
                Some(1568692564637524109)
            },
            "December 20, 2026".to_owned(),
            "Dec 20, 2026".to_owned(),
        );
        match arg {
            "preview" => m.step = BoostCancelStep::Preview,
            "loading" => m.step = BoostCancelStep::LoadingPreview,
            "canceled" => m.step = BoostCancelStep::Canceled,
            "external" => {
                m.external_gateway = Some((
                    "Apple App Store".to_owned(),
                    "https://apps.apple.com/account/subscriptions".to_owned(),
                ));
            }
            "fractional" => {
                m.fractional_end_medium = Some("Jan 12, 2027".to_owned());
            }
            "error" => {
                m.step = BoostCancelStep::Preview;
                m.error = Some(dc_intl::t("5mlOCW").to_owned());
            }
            _ => {}
        }
        self.boost_cancel_modal = Some(m);
        cx.notify();
        format!("boost-cancel: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_slot_request_targets_official_route() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = cancel_slot_request("slot_99")
        else {
            panic!("expected rest request");
        };
        assert_eq!(key.as_deref(), Some(CANCEL_SLOT_KEY));
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(
            route,
            "/users/@me/guilds/premium/subscription-slots/slot_99/cancel"
        );
        assert!(body.is_none());
    }

    #[test]
    fn step_copy_resolves_all_983511_variants() {
        let mut m = BoostCancelModal::new(
            "s1".to_owned(),
            None,
            "December 20, 2026".to_owned(),
            "Dec 20, 2026".to_owned(),
        );
        let inv: String = step1_body_runs(&m).into_iter().map(|(t, _)| t).collect();
        assert!(inv.contains("Inventory"));
        assert!(inv.contains("December 20, 2026"));

        m.guild_id = Some(1);
        let srv: String = step1_body_runs(&m).into_iter().map(|(t, _)| t).collect();
        assert!(srv.contains("cancel this Server Boost"));

        m.fractional_end_medium = Some("Jan 5, 2027".to_owned());
        let frac: String = step1_body_runs(&m).into_iter().map(|(t, _)| t).collect();
        assert!(frac.contains("Jan 5, 2027"));
        let frac2: String = step2_intro_runs(&m).into_iter().map(|(t, _)| t).collect();
        assert!(frac2.contains("Jan 5, 2027"));

        m.external_gateway = Some((
            "Apple App Store".to_owned(),
            "https://apps.apple.com/account/subscriptions".to_owned(),
        ));
        let ext = step1_body_runs(&m);
        assert!(ext.iter().any(|(_, h)| h.as_deref() == Some("$link")));

        let s3: String = step3_body_runs(&m).into_iter().map(|(t, _)| t).collect();
        assert!(s3.contains("Jan 5, 2027"));
    }
}
