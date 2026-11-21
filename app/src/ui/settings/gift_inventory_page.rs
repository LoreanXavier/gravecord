//! The bodies of the Gift Inventory page: the redeem field, the Gifting Badge progress and the gifts the account
//! purchased. Measured in `design/dom/settings/gift-inventory-darker-cozy.json`; the category headings come from the
//! generated page.
//!
//! The redeem field, its lookup and accept dialog are in `gift_redeem.rs`. The list of purchased gifts draws only its empty state: the record layout of a gift code is not in the
//! captured traffic.

use super::Root;
use crate::net::Request;
use crate::ui::nitro::art_key;
use crate::ui::{color, icon};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, deferred, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
};

/// REST key prefix for claiming a recurring partner reward (module 116011).
pub(in crate::ui) const PARTNER_REWARD_CLAIM_PREFIX: &str = "partner-reward-claim:";

/// State for Recurring Partner Reward Code Card (module 116011: `65EEvD`, `vwASIl`, `8LKchl`,
/// `i2EuFO`, `DF68t7`, `POST /outbound-promotions/{id}/claim`).
#[derive(Clone, Debug)]
pub(in crate::ui) struct PartnerRewardState {
    pub promo_id: String,
    pub partner_name: String,
    pub title: String,
    pub body: String,
    pub days_until_next: u32,
    pub progress_pct: f32,
    pub claimed_code: Option<String>,
    pub claiming: bool,
    pub copied: bool,
    pub error: bool,
    pub redemption_url: String,
}

/// Builds `POST /outbound-promotions/{promo_id}/claim` (module 116011).
pub(in crate::ui) fn claim_partner_reward_request(promo_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("{PARTNER_REWARD_CLAIM_PREFIX}{promo_id}")),
        method: reqwest::Method::POST,
        route: format!("/outbound-promotions/{promo_id}/claim"),
        body: Some(serde_json::json!({})),
    }
}

const TIERS: [(&str, &str, &str); 6] = [
    ("Patron", "Gifted 1x", "gift-box"),
    ("Champion", "Gifted 2x", "gift-champion"),
    ("Luminary", "Gifted 3x", "gift-luminary"),
    ("Icon", "Gifted 6x", "gift-icon"),
    ("Hero", "Gifted 10x", "gift-hero"),
    ("Legend", "Gifted 20x", "gift-legend"),
];

fn text(
    value: impl Into<gpui::SharedString>,
    size: f32,
    line: f32,
    weight: f32,
    tint: &str,
) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
        .child(value.into())
}

fn at(x: f32, y: f32) -> gpui::Div {
    div().absolute().left(px(x)).top(px(y))
}

impl Root {
    pub(super) fn gift_inventory_custom(
        &self,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(match key {
            "redeem_code_input" => self.gift_redeem_row(cx),
            "gifting_badge_content" => self.gifting_badge_body(cx),
            "purchased_gifts_content" => self.purchased_gifts_body(cx),
            _ => return None,
        })
    }

    fn gifting_badge_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div().relative().w_full().h(px(304.));
        let mut card = at(0., 8.)
            .w(px(696.))
            .h(px(200.))
            .bg(gpui::rgba(0x97979f14))
            .rounded_t(px(8.));
        for (index, (name, count, art)) in TIERS.iter().enumerate() {
            let centre = 88. + index as f32 * 104.;
            card = card
                .child(
                    at(centre - 22., 40.).size(px(44.)).children(
                        self.images
                            .get(&art_key(art))
                            .cloned()
                            .map(|picture| img(picture).size(px(44.))),
                    ),
                )
                .child(
                    at(centre - 52., 92.)
                        .w(px(104.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(text(dc_intl::tr(name), 14., 18., 600., "--text-muted"))
                        .child(text(*count, 12., 16., 400., "--text-muted")),
                );
        }
        card = card.child(
            at(0., 152.).w(px(696.)).flex().justify_center().child(text(
                dc_intl::tr("Your badge progresses as you gift. Exceptions include when a gift is refunded or when fraud is detected."),
                12.,
                16.,
                400.,
                "--text-muted",
            )),
        );
        page = page.child(card).child(
            at(0., 208.)
                .w(px(696.))
                .h(px(88.))
                .rounded_b(px(8.))
                .bg(color("--background-surface-high"))
                .child(
                    at(540., 24.)
                        .id("gift-badge-send")
                        .w(px(132.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.08))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(text(dc_intl::tr("Send a Gift"), 16., 20., 500., "--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_settings(cx);
                            this.open_nitro(cx);
                            this.open_purchase(true, cx);
                        })),
                ),
        );
        page.into_any_element()
    }

    fn purchased_gifts_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let none = self
            .nitro
            .gift_codes
            .as_ref()
            .is_none_or(|body| body.as_array().is_some_and(|rows| rows.is_empty()));
        if !none {
            return div().h(px(152.1)).into_any_element();
        }
        div()
            .h(px(152.1))
            .pt(px(47.))
            .flex()
            .flex_col()
            .items_center()
            .child(text(
                dc_intl::tr("There are no gifts."),
                24.,
                28.,
                400.,
                "--text-default",
            ))
            .child(
                div()
                    .mt(px(9.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child("Feeling generous? Give someone the gift of ")
                    .child(
                        div()
                            .id("gifts-nitro-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child(dc_intl::tr("Nitro"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_settings(cx);
                                this.open_nitro(cx);
                            })),
                    )
                    .child(" and make their day!"),
            )
            .into_any_element()
    }

    /// Capture helper for module 116011 (`partner-reward locked|claimed|error|close`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_partner_reward(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.partner_reward_modal = None;
            cx.notify();
            return "partner-reward: closed".to_owned();
        }
        self.partner_reward_modal = Some(PartnerRewardState {
            promo_id: "promo-monthly-1".to_owned(),
            partner_name: "Partner Perk".to_owned(),
            title: "Monthly Nitro Partner Reward".to_owned(),
            body: "Claim your recurring monthly partner code as a Nitro subscriber.".to_owned(),
            days_until_next: 12,
            progress_pct: 0.6,
            claimed_code: (mode == "claimed").then(|| "NITRO-8X4K-92QF".to_owned()),
            claiming: false,
            copied: false,
            error: mode == "error",
            redemption_url: "https://discord.com/nitro".to_owned(),
        });
        cx.notify();
        format!("partner-reward: {mode}")
    }

    /// Applies the REST response from `POST /outbound-promotions/{id}/claim` (module 116011).
    pub(in crate::ui) fn handle_partner_reward_claim_response(
        &mut self,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.partner_reward_modal.as_mut() else {
            return;
        };
        m.claiming = false;
        if let Some(code) = body
            .get("code")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            m.claimed_code = Some(code.to_owned());
            m.error = false;
        } else {
            m.error = true;
        }
        cx.notify();
    }

    /// Renders Recurring Partner Reward Code Card modal (module 116011: `65EEvD`, `vwASIl`,
    /// `8LKchl`, `i2EuFO`, `DF68t7`, `POST /outbound-promotions/{id}/claim`).
    pub(in crate::ui) fn partner_reward_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reward = self.partner_reward_modal.as_ref()?;
        let countdown = dc_intl::format(
            "65EEvD",
            &[("days", dc_intl::Arg::Num(f64::from(reward.days_until_next)))],
        );
        let pct = reward.progress_pct.clamp(0.05, 1.0);
        let promo_id = reward.promo_id.clone();
        let redemption_url = reward.redemption_url.clone();
        let body_text = if reward.error {
            dc_intl::t("i2EuFO").to_owned()
        } else {
            reward.body.clone()
        };

        let card = div()
            .id("partner-reward-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(520.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-surface-highest"))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                // `vw()`: monthly countdown progress bar (`65EEvD`).
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(text(countdown, 12., 16., 600., "--text-muted"))
                            .child(
                                div()
                                    .id("partner-reward-close")
                                    .size(px(24.))
                                    .rounded(px(6.))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.partner_reward_modal = None;
                                        cx.notify();
                                    }))
                                    .child(icon("close", 14., "--interactive-text-default")),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(6.))
                            .rounded_full()
                            .bg(color("--background-mod-subtle"))
                            .overflow_hidden()
                            .child(div().w(px(480. * pct)).h_full().rounded_full().bg(
                                linear_gradient(
                                    90.,
                                    linear_color_stop(rgb(0x3b82f6), 0.),
                                    linear_color_stop(rgb(0x93c5fd), 1.),
                                ),
                            )),
                    ),
            )
            .child(
                // `wx(props)`: partner logo/art + title + body (`i2EuFO` on error).
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(text(
                                reward.partner_name.clone(),
                                12.,
                                16.,
                                700.,
                                "--text-brand",
                            ))
                            .child(text(reward.title.clone(), 18., 22., 700., "--text-strong"))
                            .child(text(body_text, 14., 20., 400., "--text-muted")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .size(px(88.))
                            .rounded(px(12.))
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(rgba(0x5865f2ff), 0.),
                                linear_color_stop(rgba(0xeb459eff), 1.),
                            ))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("gift", 36., "--white")),
                    ),
            )
            .child(if let Some(code) = reward.claimed_code.clone() {
                let copied = reward.copied;
                let code_copy = code.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(40.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(text(code, 14., 18., 600., "--text-strong"))
                            .child(
                                div()
                                    .id("partner-reward-copy")
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(px(4.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(if copied { "Copied" } else { "Copy" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            code_copy.clone(),
                                        ));
                                        if let Some(m) = this.partner_reward_modal.as_mut() {
                                            m.copied = true;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("partner-reward-visit")
                            .h(px(40.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(text(dc_intl::t("DF68t7"), 14., 18., 600., "--white"))
                            .on_click(move |_, _, cx| cx.open_url(&redemption_url)),
                    )
                    .into_any_element()
            } else if reward.error {
                div()
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .opacity(0.65)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(icon("circle_info", 16., "--text-muted"))
                    .child(text(dc_intl::t("8LKchl"), 14., 18., 600., "--text-muted"))
                    .into_any_element()
            } else {
                div()
                    .id("partner-reward-unlock")
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(icon("access_lock", 16., "--white"))
                    .child(text(dc_intl::t("vwASIl"), 14., 18., 600., "--white"))
                    .when(reward.claiming, |d| d.opacity(0.65))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(m) = this.partner_reward_modal.as_mut() else {
                            return;
                        };
                        if m.claiming {
                            return;
                        }
                        m.claiming = true;
                        let _ = this.requests.send(claim_partner_reward_request(&promo_id));
                        cx.notify();
                    }))
                    .into_any_element()
            });

        Some(
            deferred(
                div()
                    .id("partner-reward-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.partner_reward_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partner_reward_116011_keys_and_claim_route() {
        let Request::Rest {
            key, method, route, ..
        } = claim_partner_reward_request("promo-42")
        else {
            panic!("expected Rest request");
        };
        assert_eq!(key.as_deref(), Some("partner-reward-claim:promo-42"));
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/outbound-promotions/promo-42/claim");

        for k in ["65EEvD", "vwASIl", "8LKchl", "i2EuFO", "DF68t7"] {
            assert!(!dc_intl::t(k).is_empty(), "intl key {k} should resolve");
        }
    }
}
