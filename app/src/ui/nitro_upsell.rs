//! Nitro offer countdown banner and feature upsell modal (module 811611).
//!
//! Implements `format_offer_countdown` (`G` / `ux`), promo pill banner (`x`), free-trial card (`B`),
//! subscribe CTA (`k`), footer (`F`), and every `PremiumUpsellType` variant (`EMPTY_STICKER_PICKER_UPSELL`,
//! `STICKER_PICKER_UPSELL`, `AGGREGATE_PREMIUM_UPSELL_MODAL_FEATURE_EMOJI`, `UPLOAD_ERROR_UPSELL`,
//! `BURST_REACTION_UPSELL`, `BURST_REACTION_QUICK_ACTION_UPSELL`, `MESSAGE_LENGTH_UPSELL`,
//! `MESSAGE_LENGTH_IN_EDITOR_UPSELL`, `PREMIUM_CLIENT_THEME_TRY_IT_OUT`, `CUSTOM_PROFILE_TRY_OUT_UPSELL`).

use super::discover::text;
use super::{Root, color, icon};
use chrono::{DateTime, Utc};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, IntoElement, deferred, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};
use std::borrow::Cow;

/// `PremiumUpsellType` (`v.e` of module 811611).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub enum UpsellKind {
    EmptyStickerPicker,
    StickerPicker,
    EmojiPicker,
    UploadError,
    BurstReaction,
    BurstReactionQuickAction,
    MessageLength,
    MessageLengthInEditor,
    ClientThemeTryItOut,
    CustomProfileTryOut,
    /// Module 34255: "Get Another Week of Nitro" (`M/ytwa`, `xMkRkc`, `j7LkaR`, `mPWeSh`).
    TrialEndingWeek,
}

/// Active offer attached to a Nitro upsell modal (`trialOffer` / `discountOffer` of module 811611).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NitroOffer {
    pub is_tier0: bool,
    pub discount_percent: Option<u64>,
    pub expires_at: Option<String>,
}

/// Open state for the Nitro offer / feature upsell modal (module 811611).
#[derive(Clone, Debug)]
pub struct NitroUpsellModal {
    pub kind: UpsellKind,
    pub heading: String,
    pub context: Option<String>,
    pub offer: NitroOffer,
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// `G` / `ux` of module 811611: `"Offer ends in {numDays}"` / `{numHours}` / `{numMinutes}`.
pub fn format_offer_countdown(expires_at: Option<&str>, now_ms: i64) -> Option<String> {
    let iso = expires_at?;
    let end = DateTime::parse_from_rfc3339(iso).ok()?.timestamp_millis();
    let diff_ms = (end - now_ms).max(0);
    let hours = diff_ms / (3_600 * 1_000);
    if hours > 24 {
        let days = diff_ms / (86_400 * 1_000);
        return Some(dc_intl::format(
            "xs9VPm",
            &[("numDays", Arg::Num(days as f64))],
        ));
    }
    if hours > 1 {
        return Some(dc_intl::format(
            "p7KX59",
            &[("numHours", Arg::Num(hours as f64))],
        ));
    }
    let minutes = (diff_ms / (60 * 1_000)).max(1);
    Some(dc_intl::format(
        "XtQ+Ap",
        &[("numMinutes", Arg::Num(minutes as f64))],
    ))
}

/// Badge text inside the Nitro offer promo card (`iiLbvu` or `IBYG5U` of module 811611).
pub fn offer_pill_text(offer: &NitroOffer) -> String {
    match offer.discount_percent {
        Some(pct) => dc_intl::format("iiLbvu", &[("percent", Arg::Num(pct as f64))]),
        None => dc_intl::t("IBYG5U").to_owned(),
    }
}

/// Primary button label (`bkQ4bH` for discount offers, else `"Free Nitro trial"` `8CVUrV`, module 811611).
pub fn offer_cta_text(offer: &NitroOffer) -> String {
    match offer.discount_percent {
        Some(pct) => dc_intl::format("bkQ4bH", &[("percent", Arg::Num(pct as f64))]),
        None => dc_intl::t("8CVUrV").to_owned(),
    }
}

/// Body copy for each upsell kind (`Ay` of module 811611).
pub fn upsell_body_copy(kind: UpsellKind, is_tier0: bool) -> (Option<String>, String) {
    let plan_name = if is_tier0 { "Nitro Basic" } else { "Nitro" };
    match kind {
        UpsellKind::EmptyStickerPicker => (
            Some(dc_intl::t("HEm04J").to_owned()),
            format_plain(
                "2HoFKF",
                &[("planName", Arg::Str(Cow::Borrowed(plan_name)))],
            ),
        ),
        UpsellKind::StickerPicker => (
            Some(dc_intl::t("jJG1pl").to_owned()),
            format_plain(
                "2HoFKF",
                &[("planName", Arg::Str(Cow::Borrowed(plan_name)))],
            ),
        ),
        UpsellKind::EmojiPicker => (
            Some(dc_intl::t("1SsvhY").to_owned()),
            format_plain(
                "md4nP5",
                &[("planName", Arg::Str(Cow::Borrowed(plan_name)))],
            ),
        ),
        UpsellKind::UploadError => {
            let max_size = if is_tier0 {
                dc_intl::t("C/Rhb9")
            } else {
                "500MB"
            };
            (
                None,
                format_plain(
                    "F4qoD7",
                    &[
                        ("planName", Arg::Str(Cow::Borrowed(plan_name))),
                        ("premiumMaxSize", Arg::Str(Cow::Borrowed(max_size))),
                    ],
                ),
            )
        }
        UpsellKind::BurstReaction | UpsellKind::BurstReactionQuickAction => (
            None,
            format_plain(
                "poib9C",
                &[("planName", Arg::Str(Cow::Borrowed(plan_name)))],
            ),
        ),
        UpsellKind::MessageLength => (
            None,
            format_plain("GUHtE3", &[("premiumMaxMessageLength", Arg::Num(4000.))]),
        ),
        UpsellKind::MessageLengthInEditor => (
            Some(dc_intl::t("ZvHg3N").to_owned()),
            format_plain("GUHtE3", &[("premiumMaxMessageLength", Arg::Num(4000.))]),
        ),
        UpsellKind::ClientThemeTryItOut => (None, format_plain("5KMAnK", &[])),
        UpsellKind::CustomProfileTryOut => (
            Some(dc_intl::t("8CVUrV").to_owned()),
            format_plain("fF+cgd", &[]),
        ),
        UpsellKind::TrialEndingWeek => (
            Some(dc_intl::t("M/ytwa").to_owned()),
            format_plain("xMkRkc", &[("price", Arg::Str(Cow::Borrowed("$2.99")))]),
        ),
    }
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_upsell(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let now = Utc::now();
        let exp_days = (now + chrono::Duration::days(3)).to_rfc3339();
        let (kind, discount, heading, context) = match arg.trim() {
            "trial-ending" => (UpsellKind::TrialEndingWeek, None, String::new(), None),
            "trial" => (UpsellKind::CustomProfileTryOut, None, String::new(), None),
            "upload" => (
                UpsellKind::UploadError,
                Some(30),
                "Your files are too powerful".to_owned(),
                Some("Max file size is 10MB without Nitro.".to_owned()),
            ),
            "length" => (
                UpsellKind::MessageLengthInEditor,
                Some(25),
                String::new(),
                Some("2,048 / 2,000".to_owned()),
            ),
            "emoji" => (UpsellKind::EmojiPicker, Some(30), String::new(), None),
            "burst" => (
                UpsellKind::BurstReactionQuickAction,
                Some(30),
                "Super Reactions".to_owned(),
                Some("Add extra flair to your reactions.".to_owned()),
            ),
            "sticker-inline" => (UpsellKind::StickerPicker, Some(30), String::new(), None),
            "burst-modal" => (
                UpsellKind::BurstReaction,
                Some(30),
                "Super Reactions".to_owned(),
                None,
            ),
            "length-modal" => (
                UpsellKind::MessageLength,
                Some(25),
                "Your message is too long".to_owned(),
                None,
            ),
            "theme" => (UpsellKind::ClientThemeTryItOut, None, String::new(), None),
            _ => (
                UpsellKind::EmptyStickerPicker,
                Some(30),
                String::new(),
                None,
            ),
        };
        self.nitro_upsell_modal = Some(NitroUpsellModal {
            kind,
            heading,
            context,
            offer: NitroOffer {
                is_tier0: false,
                discount_percent: discount,
                expires_at: Some(exp_days),
            },
        });
        cx.notify();
        format!("nitro-upsell: kind={kind:?}")
    }

    /// Renders the Nitro Offer Countdown / Feature Upsell modal overlay (modules 811611 & 34255).
    pub(super) fn nitro_upsell_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.nitro_upsell_modal.as_ref()?;
        // Module 34255: Nitro Trial Ending Modal ("Get Another Week of Nitro").
        if modal.kind == UpsellKind::TrialEndingWeek {
            let price = self
                .plan_price(dc_state::premium::sku::TIER_2, false)
                .map(|p| p.display())
                .unwrap_or_else(|| "$2.99".to_owned());
            let body_text =
                dc_intl::format("xMkRkc", &[("price", Arg::Str(Cow::Borrowed(&price)))]);
            let cta_text = dc_intl::format("mPWeSh", &[("price", Arg::Str(Cow::Borrowed(&price)))]);
            let card = div()
                .id("nitro-trial-ending-modal")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .relative()
                .w(px(480.))
                .rounded(px(12.))
                .overflow_hidden()
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-low"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .h(px(184.))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(rgba(0xb845c1ff), 0.),
                            linear_color_stop(rgba(0x5865f2ff), 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(88.))
                                .rounded_full()
                                .bg(rgba(0xffffff22))
                                .border_1()
                                .border_color(rgba(0xffffff44))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("nitro_wheel", 44., "--white")),
                        ),
                )
                .child(
                    div()
                        .id("nitro-trial-ending-close")
                        .absolute()
                        .top(px(10.))
                        .right(px(10.))
                        .size(px(28.))
                        .rounded(px(6.))
                        .bg(rgba(0x00000044))
                        .hover(|s| s.bg(rgba(0x00000066)))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.nitro_upsell_modal = None;
                            cx.notify();
                        }))
                        .child(icon("close", 16., "--white")),
                )
                .child(
                    div()
                        .px(px(32.))
                        .pt(px(24.))
                        .pb(px(28.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .text_center()
                        .child(text(22., 28., 700., "--text-strong").child(dc_intl::t("M/ytwa")))
                        .child(text(14., 20., 400., "--text-muted").child(body_text))
                        .child(
                            div()
                                .mt(px(12.))
                                .w_full()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .id("nitro-trial-ending-learn")
                                        .flex_1()
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            text(14., 18., 600., "--text-strong")
                                                .child(dc_intl::t("j7LkaR")),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.nitro_upsell_modal = None;
                                            this.open_nitro(cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .id("nitro-trial-ending-cta")
                                        .flex_1()
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .bg(linear_gradient(
                                            90.,
                                            linear_color_stop(rgba(0x8547c6ff), 0.),
                                            linear_color_stop(rgba(0xeb459eff), 1.),
                                        ))
                                        .hover(|s| s.opacity(0.92))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(6.))
                                        .child(icon("nitro_wheel", 16., "--white"))
                                        .child(text(14., 18., 600., "--white").child(cta_text))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.nitro_upsell_modal = None;
                                            this.open_purchase(false, cx);
                                        })),
                                ),
                        ),
                );
            return Some(
                deferred(
                    div()
                        .id("nitro-trial-ending-backdrop")
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
                                this.nitro_upsell_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(card),
                )
                .with_priority(3)
                .into_any_element(),
            );
        }
        let now_ms = Utc::now().timestamp_millis();
        let (default_heading, body_copy) = upsell_body_copy(modal.kind, modal.offer.is_tier0);
        let heading = if modal.heading.is_empty() {
            default_heading.unwrap_or_else(|| dc_intl::t("HEm04J").to_owned())
        } else {
            modal.heading.clone()
        };
        let pill = offer_pill_text(&modal.offer);
        let cta = offer_cta_text(&modal.offer);
        let countdown = format_offer_countdown(modal.offer.expires_at.as_deref(), now_ms);
        let sticker_alt = dc_intl::t("do7AoM");
        let is_trial_card = modal.kind == UpsellKind::CustomProfileTryOut;

        let promo_card = div()
            .w_full()
            .p(px(16.))
            .rounded(px(12.))
            .bg(if modal.offer.is_tier0 {
                linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x00a8fcff), 0.),
                    linear_color_stop(rgba(0x5865f2ff), 1.),
                )
            } else {
                linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x8547c6ff), 0.),
                    linear_color_stop(rgba(0xb845c1ff), 1.),
                )
            })
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .text_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("home_nitro", 20., "--text-overlay-light"))
                    .child(text(15., 20., 700., "--text-overlay-light").child(
                        if modal.offer.is_tier0 {
                            "NITRO BASIC"
                        } else {
                            "NITRO"
                        },
                    )),
            )
            .child(text(14., 20., 500., "--text-overlay-light").child(body_copy))
            .child(
                div()
                    .px(px(10.))
                    .h(px(22.))
                    .rounded_full()
                    .bg(rgba(0xffffffff))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .font_weight(gpui::FontWeight(700.))
                            .text_color(rgba(0x5865f2ff))
                            .child(pill),
                    ),
            )
            .when_some(countdown.clone(), |col, cd| {
                col.child(text(12., 16., 500., "--text-overlay-light").child(cd))
            });

        let card = div()
            .id("nitro-upsell-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .when(false, |el| el.child(sticker_alt))
            // Close icon
            .child(
                div()
                    .id("nitro-upsell-x")
                    .absolute()
                    .top(px(14.))
                    .right(px(14.))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.nitro_upsell_modal = None;
                        cx.notify();
                    }))
                    .child(icon("close", 16., "--interactive-text-default")),
            )
            // Content
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .text_center()
                    .when(is_trial_card, |col| {
                        col.child(
                            div()
                                .px(px(12.))
                                .h(px(24.))
                                .rounded_full()
                                .bg(linear_gradient(
                                    90.,
                                    linear_color_stop(rgba(0x8547c6ff), 0.),
                                    linear_color_stop(rgba(0xeb459eff), 1.),
                                ))
                                .flex()
                                .items_center()
                                .child(
                                    text(12., 16., 700., "--text-overlay-light")
                                        .child(dc_intl::t("8CVUrV")),
                                ),
                        )
                    })
                    .when(!is_trial_card, |col| {
                        col.child(
                            div()
                                .size(px(56.))
                                .rounded_full()
                                .bg(rgba(0x5865f226))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("home_nitro", 28., "--text-brand")),
                        )
                    })
                    .when_some(modal.context.clone(), |col, ctx_line| {
                        col.child(
                            text(
                                14.,
                                18.,
                                600.,
                                if modal.kind == UpsellKind::MessageLengthInEditor {
                                    "--text-feedback-critical"
                                } else {
                                    "--text-muted"
                                },
                            )
                            .child(ctx_line),
                        )
                    })
                    .child(text(20., 26., 700., "--text-strong").child(heading))
                    .child(promo_card),
            )
            // Footer (`F`: Close + green Subscribe/Save CTA)
            .child(
                div()
                    .px(px(24.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .rounded_b(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("nitro-upsell-close")
                            .h(px(36.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.nitro_upsell_modal = None;
                                cx.notify();
                            }))
                            .child(
                                text(14., 18., 500., "--text-default").child(dc_intl::t("cpT0Cq")),
                            ),
                    )
                    .child(
                        div()
                            .id("nitro-upsell-subscribe")
                            .h(px(36.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--status-positive"))
                            .hover(|s| s.opacity(0.9))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.nitro_upsell_modal = None;
                                this.open_nitro(cx);
                            }))
                            .child(text(14., 18., 600., "--text-overlay-light").child(cta)),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("nitro-upsell-backdrop")
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
                            this.nitro_upsell_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_formats_days_hours_and_minutes() {
        let base = DateTime::parse_from_rfc3339("2026-11-20T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            format_offer_countdown(Some("2026-11-23T02:00:00Z"), base).as_deref(),
            Some("Offer ends in 3 days")
        );
        assert_eq!(
            format_offer_countdown(Some("2026-11-20T05:00:00Z"), base).as_deref(),
            Some("Offer ends in 5 hours")
        );
        assert_eq!(
            format_offer_countdown(Some("2026-11-20T00:25:00Z"), base).as_deref(),
            Some("Offer ends in 25 minutes")
        );
    }

    #[test]
    fn upsell_copy_resolves_for_every_variant() {
        let (h, b) = upsell_body_copy(UpsellKind::EmptyStickerPicker, false);
        assert_eq!(h.as_deref(), Some("Choose from 300 stickers with Nitro"));
        assert!(b.contains("Nitro"));

        let (_, b_up) = upsell_body_copy(UpsellKind::UploadError, true);
        assert!(b_up.contains("50MB"));

        let (h_len, b_len) = upsell_body_copy(UpsellKind::MessageLengthInEditor, false);
        assert_eq!(h_len.as_deref(), Some("You've hit the character limit."));
        assert!(b_len.contains("4000") || b_len.contains("4,000"));

        let (h_end, b_end) = upsell_body_copy(UpsellKind::TrialEndingWeek, false);
        assert_eq!(h_end.as_deref(), Some("Get Another Week of Nitro"));
        assert!(b_end.contains("$2.99"));
        for k in ["M/ytwa", "xMkRkc", "j7LkaR", "mPWeSh"] {
            assert!(dc_intl::has_key(k), "missing 34255 key {k}");
        }
    }
}
