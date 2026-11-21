//! The bodies of the billing pages that hold real account data: Subscriptions, and the payment methods and
//! transaction history of Billing. Measured in `design/dom/settings/{subscriptions,billing}-darker-cozy.json`.
//!
//! Only the empty states are drawn: the captured account has no subscription, credit, payment source or payment.
//! A saved source, a running subscription or a past payment needs the field layout of its API record, which is not in
//! the recorded traffic; those lists draw nothing until it is (`BLOCKED.md`).

use super::{ButtonKind, Root, button, color};
use crate::ui::{icon, modal};
use dc_state::premium::{self, PremiumType, Subscription, status};
use gpui::{
    AnyElement, FontWeight, IntoElement, SharedString, div, linear_color_stop, linear_gradient,
    prelude::*, px,
};

/// `Dmq.PAUSE_PENDING` (module 652215).
const PAUSE_PENDING: u64 = 9;
/// `Dmq.BILLING_RETRY` and `Dmq.PAUSED`.
const BILLING_RETRY: u64 = 7;
const PAUSED: u64 = 8;
/// The answer of the cancellation request (module 673761 `handleCancellation`).
pub(super) const CANCEL_KEY: &str = "settings-nitro-cancel";
/// `purchase_token` of module 107351: a random id kept 60 days in local storage.
const PURCHASE_TOKEN: &str = "purchase_token";
const PURCHASE_TOKEN_DAYS_MS: i64 = 60 * 86_400_000;

/// `o` of module 107351: the stored purchase token while it is fresh, else a new one kept for 60 days.
fn purchase_token() -> String {
    let now = chrono::Utc::now().timestamp_millis();
    if let Some(stored) = crate::storage::get(PURCHASE_TOKEN)
        && stored["expires"]
            .as_i64()
            .is_some_and(|expires| expires >= now)
        && let Some(token) = stored["purchaseToken"].as_str()
    {
        return token.to_owned();
    }
    let token = uuid::Uuid::new_v4().to_string();
    crate::storage::set(
        PURCHASE_TOKEN,
        serde_json::json!({"purchaseToken": token, "expires": now + PURCHASE_TOKEN_DAYS_MS}),
    );
    token
}

/// The request of `handleCancellation` (module 673761) for a subscription: a past due, paused or retrying one is
/// deleted at once (`M2`, `DELETE`), any other is updated to renew without its premium plans (`nV`: `PATCH` with the
/// items `ez` keeps, nothing due now and the renewal price of what is kept).
/// GAP: kept items beyond the premium plan (boosts) go through the PREVIEW step, which is not built, so only a
/// subscription whose items are all premium plans reaches this request; its renewal price is zero.
pub(super) fn cancel_request(sub: &Subscription, token: &str) -> crate::net::Request {
    let route = format!("/users/@me/billing/subscriptions/{}", sub.id);
    if [status::PAST_DUE, PAUSED, BILLING_RETRY].contains(&sub.status) {
        return crate::net::Request::Rest {
            key: Some(CANCEL_KEY.to_owned()),
            method: reqwest::Method::DELETE,
            route,
            body: None,
        };
    }
    let currency = sub.currency.clone().unwrap_or_else(|| "usd".to_owned());
    let kept: Vec<serde_json::Value> = sub
        .renewal_mutations
        .as_ref()
        .map_or(&sub.items, |mutations| &mutations.items)
        .iter()
        .filter(|item| premium::plan_premium_type(item.plan_id).is_none())
        .map(|item| serde_json::json!({"plan_id": item.plan_id.to_string(), "quantity": item.quantity}))
        .collect();
    crate::net::Request::Rest {
        key: Some(CANCEL_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route,
        body: Some(serde_json::json!({
            "payment_source_token": null,
            "gateway_checkout_context": null,
            "purchase_token": token,
            "expected_invoice_price": {"amount": 0, "currency": currency},
            "expected_renewal_price": {"amount": 0, "currency": currency},
            "items": kept,
        })),
    }
}

/// The critical inline notice (module 683071 `w`, `type: "critical"`) as the account dialogs draw it.
fn critical_notice(message: &str) -> AnyElement {
    div()
        .p(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--inlinenotice-border-critical"))
        .bg(color("--background-feedback-mod-critical"))
        .text_size(px(16.))
        .line_height(px(20.))
        .text_color(color("--text-strong"))
        .child(message.to_owned())
        .into_any_element()
}

/// `b` of module 95337: the subscription keeps items that are not premium plans, so Next leads to the preview of
/// what renews instead of Confirm.
pub(super) fn keeps_other_items(sub: &Subscription) -> bool {
    let other = |items: &[premium::SubscriptionItem]| {
        items
            .iter()
            .any(|item| premium::plan_premium_type(item.plan_id).is_none())
    };
    match &sub.renewal_mutations {
        None => other(&sub.items),
        Some(mutations) => other(&mutations.items),
    }
}

/// The subtitle of module 95337, its paragraphs as runs (bold where the message says): the immediate cancellation
/// of a past due, paused or retrying subscription, else the end date and what the tier loses.
/// GAP: the payments-blocked variants (Russia, `m.Hp`) and the group plan's (`MI8Rml`) are not chosen: no store
/// says the account is in either case.
pub(super) fn cancel_confirm_text(
    status: u64,
    tier: PremiumType,
    end: Option<&str>,
) -> Vec<Vec<(String, bool)>> {
    if [status::PAST_DUE, PAUSED, BILLING_RETRY].contains(&status) {
        return vec![vec![(dc_intl::t("FClXh9").to_owned(), false)]];
    }
    let key = match tier {
        PremiumType::Basic => "Y6Wfaw",
        PremiumType::Classic => "X7i9D8",
        _ => "fCdmNs",
    };
    let date = end.and_then(medium_date).unwrap_or_default();
    let when = dc_intl::Arg::When {
        date: date.into(),
        time: String::new().into(),
    };
    dc_intl::paragraph_runs(key, &[("date", when)])
        .into_iter()
        .map(|runs| {
            runs.into_iter()
                .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
                .collect()
        })
        .collect()
}

/// `MVz.PREMIUM_DETAILS` and `MVz.STREAM_QUALITY_SETTINGS` (module 652215), through `getArticleURL`.
const PREMIUM_DETAILS: &str = "https://support.discord.com/hc/en-us/articles/115000435108";
const STREAM_QUALITY: &str =
    "https://support.discord.com/hc/en-us/articles/360040816151#h_01F02D2RBXRNRC20WS3ZHMMCTE";

/// The non-personalised pictures of the What You Lose items, dark theme (`eN` of module 736560).
const LOSE_EMOJI_ART: &str = "https://discord.com/assets/27244382f9e8cc5f.svg";
const LOSE_BOOST_ART: &str = "https://discord.com/assets/f30ca678d7e3549e.svg";
const LOSE_SCREEN_ART: &str = "https://discord.com/assets/893dc4a04464a54a.svg";
const LOSE_UPLOADS_ART: &str = "https://discord.com/assets/b8183e94e7ff0020.svg";

/// The items `eD` (module 736560) lists for a cancellation (`isDowngrade: false`), in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lose {
    Profile,
    EmojiStickers,
    Boost,
    ScreenShare,
    Uploads,
}

/// `eD` by premium type. Nitro's first item, the tenure badge (`ex`), needs the badge progress and is not built.
fn lose_items(tier: PremiumType) -> &'static [Lose] {
    match tier {
        PremiumType::Basic => &[Lose::EmojiStickers, Lose::Uploads],
        PremiumType::Classic => &[
            Lose::Profile,
            Lose::EmojiStickers,
            Lose::ScreenShare,
            Lose::Uploads,
            Lose::Boost,
        ],
        _ => &[
            Lose::Profile,
            Lose::EmojiStickers,
            Lose::Boost,
            Lose::ScreenShare,
            Lose::Uploads,
        ],
    }
}

/// The item's text as the rebrand draws it (`strong` at weight 400, so plain).
fn lose_text(item: Lose, tier: PremiumType) -> &'static str {
    let classic = tier == PremiumType::Classic;
    match item {
        // `xCaYwE` / `gpqr+n`
        Lose::Profile if classic => "An animated avatar and exclusive Nitro badge",
        Lose::Profile => {
            "Customizing your profile with an animated avatar, profile banner, or profile theme"
        }
        // `couiKJ` / `0hUHi6`
        Lose::EmojiStickers if classic => "Access to custom emoji across all of your servers",
        Lose::EmojiStickers => "Access to custom emoji and stickers across all of your servers",
        // `K4Hv69` / `wRxEDW`
        Lose::Boost if classic => "Your 30% Boost discount",
        Lose::Boost => "Your 2 free Boosts and 30% Boost discount",
        // `wK04T1`
        Lose::ScreenShare => "Higher screen share quality",
        // `GsOFRJ` / `wFWO6D`
        Lose::Uploads if tier == PremiumType::Nitro => "Bigger uploads and longer messages",
        Lose::Uploads => "Bigger uploads",
    }
}

fn empty(loaded: Option<&serde_json::Value>) -> bool {
    loaded.is_none_or(|body| body.as_array().is_some_and(|rows| rows.is_empty()))
}

/// `You have no active subscriptions`: the dark 62px row with the empty-state glyph.
fn empty_row(id: &'static str, text: &'static str) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .h(px(62.))
        .rounded(px(8.))
        .bg(color("--background-surface-high"))
        .px(px(20.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(icon("billing_empty", 30., "--text-strong"))
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text),
        )
        .into_any_element()
}

impl Root {
    pub(super) fn billing_custom(
        &self,
        key: &'static str,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        Some(match key {
            "premium_guild_subscriptions_setting" => self.server_boost_body(cx),
            "nitro_setting" => self.nitro_settings_body(cx),
            "subscriptions_settings" => self.subscriptions_body(cx),
            "billing_payment_methods" => self.payment_methods_body(cx),
            "billing_transaction_history" => self.transactions_body(),
            _ => return self.gift_inventory_custom(key, cx),
        })
    }

    fn subscriptions_body(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let none = empty(self.nitro.subscriptions.as_ref());
        let row = self.premium_subscription();
        div()
            .w_full()
            .pt(px(9.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Your Subscriptions")),
            )
            .child(
                div()
                    .mt(px(16.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(
                        dc_intl::tr("These are your current subscriptions. They will be billed on the same billing cycle. You can update any subscription at any time."),
                    ),
            )
            .child(
                div()
                    .mt(px(16.))
                    .children(none.then(|| {
                        empty_row("subscriptions-none", dc_intl::tr("You have no active subscriptions"))
                    }))
                    .children(row.map(|sub| self.subscription_row(&sub, cx))),
            )
            .child(
                div()
                    .mt(px(32.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Subscription Credit")),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(
                        dc_intl::tr("When you accept a gift while you have a running subscription, redeem a promotion, or accept a gift that differs from your current subscription, it will appear here as credit."),
                    ),
            )
            .child(div().mt(px(32.)).child(empty_row(
                "subscriptions-no-credit",
                dc_intl::tr("You have no unused credits"),
            )))
            .child(div().mt(px(40.)).h(px(1.)).bg(color("--border-subtle")))
            .into_any_element()
    }

    /// The subscription's banner (module 358787 `$`, `banner_TJpZiO`: 16px padding, 4px radius, the tier's gradient;
    /// canceled is orange, past due red), its plan name, the renewal sentence of `getBillingInformationString` and
    /// Cancel and Switch Plans. Built from the module with no capture; the price comes from the plan list, the
    /// module's fallback when no invoice preview is loaded. Cancel opens the first step of the cancellation flow,
    /// Switch Plans the plan select of the purchase flow.
    fn subscription_row(&self, sub: &Subscription, cx: &mut gpui::Context<Self>) -> AnyElement {
        let tier = sub.premium_type().unwrap_or(PremiumType::Nitro);
        let price = premium::plan_sku(sub.plan_id).and_then(|sku| {
            let plan = self
                .nitro
                .plans
                .iter()
                .find(|plan| plan.id == sub.plan_id)?;
            let price = self.plan_price(sku, plan.is_yearly())?.display();
            // `CE` of module 722178: the price with its interval ("TRY 104.99/month").
            Some(
                premium::price_with_interval(&price, plan.interval, plan.interval_count)
                    .unwrap_or(price),
            )
        });
        let date = sub.current_period_end.as_deref().and_then(medium_date);
        let segments = info_segments(sub.status, date.as_deref(), price.as_deref());
        let canceled = sub.status == status::CANCELED;
        let ink = if canceled { "--black" } else { "--white" };
        let gradient = |from: &str, to: &str| {
            linear_gradient(
                90.,
                linear_color_stop(color(from), 0.),
                linear_color_stop(color(to), 1.),
            )
        };
        let banner = div()
            .id("subscription-row")
            .relative()
            .w_full()
            .rounded(px(4.))
            .p(px(16.))
            .flex()
            .flex_col()
            .text_color(color(ink));
        let banner = if canceled {
            banner.bg(linear_gradient(
                90.,
                linear_color_stop(gpui::rgb(0xf18623), 0.),
                linear_color_stop(gpui::rgb(0xfab930), 1.),
            ))
        } else if sub.status == status::PAST_DUE {
            banner.bg(linear_gradient(
                270.,
                linear_color_stop(gpui::rgb(0xe82427), 0.),
                linear_color_stop(gpui::rgb(0xc81031), 1.),
            ))
        } else {
            match tier {
                PremiumType::Basic => banner.bg(gradient(
                    "--premium-tier-0-purple-for-gradients",
                    "--premium-tier-0-blue-for-gradients",
                )),
                PremiumType::Classic => banner.bg(gradient(
                    "--premium-tier-1-blue-for-gradients",
                    "--premium-tier-1-dark-blue-for-gradients",
                )),
                _ => banner.bg(gradient(
                    "--premium-tier-2-purple-for-gradients",
                    "--premium-tier-2-pink-for-gradients",
                )),
            }
        };
        let info = div()
            .mt(px(8.))
            .text_size(px(16.))
            .line_height(px(20.))
            .flex()
            .flex_wrap()
            .children(segments.into_iter().map(|(text, bold)| {
                div()
                    .font_weight(FontWeight(if bold { 700. } else { 400. }))
                    .child(text)
            }));
        let tools = div()
            .absolute()
            .top(px(16.))
            .right(px(20.))
            .flex()
            .items_center()
            .gap(px(16.))
            .children((!canceled).then(|| {
                // `el`: Cancel opens the cancellation flow for an active, past due or pause-pending subscription.
                let cancellable =
                    [status::ACTIVE, status::PAST_DUE, PAUSE_PENDING].contains(&sub.status);
                div()
                    .id("subscription-cancel")
                    .when(cancellable, |link| {
                        link.cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_cancel_nitro();
                                cx.notify();
                            }))
                    })
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .child(dc_intl::tr("Cancel"))
            }))
            .child(
                div()
                    .id("subscription-switch")
                    // Switch Plans (`dylp/7`) opens module 778705; Re-subscribe (`iIvF2z`) opens module 674809.
                    .cursor_pointer()
                    .on_click({
                        let sub_id = sub.id.clone();
                        let renewal = sub
                            .current_period_end
                            .as_deref()
                            .and_then(medium_date)
                            .unwrap_or_else(|| "next billing date".to_owned());
                        cx.listener(move |this, _, _, cx| {
                            if canceled {
                                this.open_nitro_resubscribe_modal(
                                    sub_id.clone(),
                                    "$9.99".to_owned(),
                                    renewal.clone(),
                                    "month".to_owned(),
                                    cx,
                                );
                            } else {
                                this.open_purchase(false, cx);
                            }
                        })
                    })
                    .h(px(32.))
                    .px(px(16.))
                    .rounded(px(3.))
                    .bg(gpui::rgb(0xffffff))
                    .text_color(gpui::rgb(0x000000))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .child(if canceled {
                        dc_intl::t("iIvF2z")
                    } else {
                        dc_intl::tr("Switch Plans")
                    }),
            );
        // Module 774774 (`Lj` / `rm` / `HU`): "free trial activated" / "discount applied" / "{percent}% discount" / "2-WEEK TRIAL" pill.
        let offer_badge: Option<String> = {
            let discount_pct = self
                .fetched
                .get("user-offer")
                .and_then(|v| v["user_discount_offer"]["discount"]["amount"].as_u64());
            let has_trial = sub.trial_id.is_some() || sub.trial_ends_at.is_some();
            if tier == PremiumType::Basic {
                has_trial.then(|| dc_intl::t("qYKftX").to_owned())
            } else if let Some(pct) = discount_pct {
                Some(dc_intl::format(
                    "iiLbvu",
                    &[("percent", dc_intl::Arg::Num(pct as f64))],
                ))
            } else if has_trial {
                Some(dc_intl::t("qYKftX").to_owned())
            } else if self
                .fetched
                .get("user-offer")
                .is_some_and(|v| v["user_discount_offer"].is_object())
            {
                Some(dc_intl::t("EyjDRE").to_owned())
            } else {
                None
            }
        };
        let _trial_copy = dc_intl::t("IBYG5U");
        let _two_week_copy = dc_intl::t("gtNqJQ");
        banner
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(28.))
                            .font_weight(FontWeight(800.))
                            .font_family(crate::ui::nitro::NITRO_FONT)
                            .child(
                                sub.display_name()
                                    .unwrap_or(dc_intl::tr("Nitro"))
                                    .to_uppercase(),
                            ),
                    )
                    .when_some(offer_badge, |row, badge| {
                        // Module 774774 `HU` (`PremiumPillWithSparkles`, `PREMIUM_TIER_2_WHITE_FILL`)
                        row.child(
                            div()
                                .id("subscription-offer-pill")
                                .h(px(22.))
                                .px(px(10.))
                                .rounded_full()
                                .bg(gpui::rgb(0xffffff))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(11.))
                                .font_weight(FontWeight(700.))
                                .text_color(if tier == PremiumType::Basic {
                                    color("--premium-tier-0-purple-for-gradients")
                                } else {
                                    color("--premium-tier-2-purple-for-gradients")
                                })
                                .child("✦")
                                .child(badge.to_uppercase())
                                .child("✦"),
                        )
                    }),
            )
            .child(info)
            .child(tools)
            .into_any_element()
    }

    /// Cancel: the cancellation flow opens on What You Lose (`F = WHAT_YOU_LOSE` for every Nitro tier).
    fn open_cancel_nitro(&mut self) {
        for url in [
            LOSE_EMOJI_ART,
            LOSE_BOOST_ART,
            LOSE_SCREEN_ART,
            LOSE_UPLOADS_ART,
        ] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        self.settings_extra.dialog = Some(super::actions::Dialog::CancelNitro);
    }

    /// `Z` of module 606423 with no churn discount offer: "Are you sure you want to cancel?" (`PWq8TL`), the perks
    /// (`SOC4ML`, then `eD` of module 736560), "Never mind, keep Nitro" (`rzVN6j`) and Continue (`3PatSz`). Continue
    /// leads to the step that cancels (`CONFIRM`), which is not built: it does nothing here.
    /// The md dialog (480 wide, `size-md_dvN-MP`) as tall as its outer container allows (56px clear above and below,
    /// as the voters dialog measures it); the perks scroll.
    pub(super) fn cancel_nitro_dialog(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let height = (self.viewport.1 - 112.).min(800.);
        let tier = self.cancel_tier();
        // Header 104, perks' padding and margins 40, footer 64; each perk at least 140 and 8 apart.
        let natural = 208. + lose_items(tier).len() as f32 * 148.;
        let spec = self.cancel_nitro_spec(natural > height, cx);
        if natural > height {
            modal::render_pinned(spec, 480., 56., height, cx)
        } else {
            modal::render(spec, cx)
        }
    }

    pub(super) fn cancel_tier(&self) -> PremiumType {
        self.premium_subscription()
            .and_then(|sub| sub.premium_type())
            .unwrap_or(PremiumType::Nitro)
    }

    fn cancel_nitro_spec(&self, scrolls: bool, cx: &mut gpui::Context<Self>) -> modal::Spec {
        let tier = self.cancel_tier();
        let items: Vec<AnyElement> = lose_items(tier)
            .iter()
            .map(|item| self.lose_item(*item, tier, cx))
            .collect();
        let close = |this: &mut Root, _: &mut gpui::Window, _: &mut gpui::Context<Root>| {
            this.settings_extra.dialog = None;
        };
        modal::Spec {
            id: "settings-cancel-nitro",
            title: dc_intl::tr("Are you sure you want to cancel?").into(),
            subtitle: Some(
                div()
                    .child(
                        dc_intl::tr("If you cancel, you will lose these perks at the end of your current billing period:"),
                    )
                    .into_any_element(),
            ),
            // `whatYouLoseItemContainer_o6yNAQ premiumRebrand_o6yNAQ`: a column 8px apart, 12px above and below.
            body: Some(
                div()
                    .id("settings-cancel-nitro-scroll")
                    .when(scrolls, |body| body.size_full().overflow_y_scroll().px(px(24.)).pt(px(16.)))
                    .child(
                        div()
                            .my(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .when_some(
                                self.fetched
                                    .get("user-offer")
                                    .and_then(|v| v["body"]["user_discount_offer"]["discount"]["amount"].as_u64()),
                                |col, pct| {
                                    col.child(super::nitro_mgmt::render_discount_offer_card(
                                        (pct as u32).max(1),
                                        1,
                                        "$6.99",
                                        "$9.99",
                                        "month",
                                        false,
                                        super::nitro_mgmt::DiscountOfferState::None,
                                    ))
                                },
                            )
                            .children(items),
                    )
                    .into_any_element(),
            ),
            buttons: vec![
                modal::button(
                    "settings-cancel-nitro-keep",
                    dc_intl::tr("Never mind, keep Nitro"),
                    modal::Tone::Secondary,
                    close,
                ),
                // `l(g.g.CONFIRM)`: the confirmation step (module 95337).
                modal::button(
                    "settings-cancel-nitro-continue",
                    dc_intl::tr("Continue"),
                    modal::Tone::Primary,
                    |this, _, _| {
                        this.settings_extra.dialog =
                            Some(super::actions::Dialog::CancelNitroConfirm {
                                busy: false,
                                failed: false,
                            });
                    },
                ),
            ],
            close: Box::new(close),
        }
    }

    /// Module 95337 (`size: "md"`): "Confirm cancellation", the subtitle of the status and tier, "Never mind, keep
    /// Nitro" and the critical Confirm (Next to the renewal preview for a subscription that keeps boosts), and the
    /// critical notice "Something went wrong..." under it after a failed request.
    pub(super) fn cancel_confirm_dialog(
        &self,
        busy: bool,
        failed: bool,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let sub = self.premium_subscription();
        let tier = self.cancel_tier();
        let paragraphs = sub.as_ref().map_or_else(Vec::new, |sub| {
            cancel_confirm_text(sub.status, tier, sub.current_period_end.as_deref())
        });
        let next = sub.as_ref().is_some_and(keeps_other_items);
        let close = |this: &mut Root, _: &mut gpui::Window, _: &mut gpui::Context<Root>| {
            this.settings_extra.dialog = None;
        };
        let subtitle = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(paragraphs.into_iter().map(|runs| {
                // One wrapping text per paragraph, its `$b` runs bold.
                let mut text = String::new();
                let mut bold = Vec::new();
                for (run, strong) in runs {
                    let start = text.len();
                    text.push_str(&run);
                    if strong {
                        bold.push((
                            start..text.len(),
                            gpui::HighlightStyle {
                                font_weight: Some(FontWeight(700.)),
                                ..Default::default()
                            },
                        ));
                    }
                }
                div().child(gpui::StyledText::new(text).with_highlights(bold))
            }))
            .into_any_element();
        let confirm = if next {
            // Module 347378: Next opens the PREVIEW step for subscriptions keeping Server Boosts.
            modal::button(
                "settings-cancel-confirm",
                dc_intl::t("PDTjLN"),
                modal::Tone::Critical,
                |this, _, cx| {
                    this.settings_extra.dialog = None;
                    if let Some(sub) = this.premium_subscription() {
                        this.open_cancel_nitro_preview(&sub, cx);
                    }
                },
            )
        } else {
            modal::button(
                "settings-cancel-confirm",
                dc_intl::t("cY+Oob"),
                if busy {
                    modal::Tone::PrimaryLoading
                } else {
                    modal::Tone::Critical
                },
                |this, _, _| this.send_nitro_cancellation(),
            )
        };
        let spec = modal::Spec {
            id: "settings-cancel-confirm-modal",
            title: dc_intl::t("LZunzZ").into(),
            subtitle: Some(subtitle),
            body: failed.then(|| critical_notice(dc_intl::t("5mlOCW"))),
            buttons: vec![
                modal::button(
                    "settings-cancel-keep",
                    dc_intl::t("rzVN6j"),
                    modal::Tone::Secondary,
                    close,
                ),
                confirm,
            ],
            close: Box::new(close),
        };
        modal::render(spec, cx)
    }

    /// Capture only: `cancel-nitro lose|confirm|failed`, a step of the cancellation flow over the open page.
    #[cfg(feature = "capture")]
    pub(crate) fn script_cancel_nitro(&mut self, step: &str) -> String {
        match step {
            "lose" => self.open_cancel_nitro(),
            "confirm" | "failed" => {
                self.settings_extra.dialog = Some(super::actions::Dialog::CancelNitroConfirm {
                    busy: false,
                    failed: step == "failed",
                });
            }
            _ => return "usage: cancel-nitro lose|confirm|failed".to_owned(),
        }
        format!("cancel nitro: {step}")
    }

    /// Confirm of module 95337: the request of `handleCancellation`, once.
    fn send_nitro_cancellation(&mut self) {
        if matches!(
            self.settings_extra.dialog,
            Some(super::actions::Dialog::CancelNitroConfirm { busy: true, .. })
        ) {
            return;
        }
        let Some(sub) = self.premium_subscription() else {
            return;
        };
        let _ = self.requests.send(cancel_request(&sub, &purchase_token()));
        self.settings_extra.dialog = Some(super::actions::Dialog::CancelNitroConfirm {
            busy: true,
            failed: false,
        });
    }

    /// The cancellation came back: the flow closes and the subscription is read again
    /// (`BILLING_SUBSCRIPTION_UPDATE_SUCCESS` puts the answer in the store), or the error notice shows.
    pub(super) fn cancel_nitro_answered(&mut self, body: &serde_json::Value) {
        if !matches!(
            self.settings_extra.dialog,
            Some(super::actions::Dialog::CancelNitroConfirm { .. })
        ) {
            return;
        }
        if body["ok"].as_bool() == Some(true) {
            self.settings_extra.dialog = None;
            self.nitro.subscriptions_injected = false;
            self.nitro.billing_requested = false;
            self.request_billing();
        } else {
            self.settings_extra.dialog = Some(super::actions::Dialog::CancelNitroConfirm {
                busy: false,
                failed: true,
            });
        }
    }

    /// `eT`: the text and Learn More on the left (`whatYouLoseItemLeftColumn`, 40px clear of the picture), the
    /// 180px picture on the right; `premiumRebrand` box: subtle fill, muted 1px border, 11px padding, 140px tall at
    /// least.
    fn lose_item(&self, item: Lose, tier: PremiumType, cx: &mut gpui::Context<Self>) -> AnyElement {
        let id = match item {
            Lose::Profile => "cancel-lose-profile",
            Lose::EmojiStickers => "cancel-lose-emoji",
            Lose::Boost => "cancel-lose-boost",
            Lose::ScreenShare => "cancel-lose-screen-share",
            Lose::Uploads => "cancel-lose-uploads",
        };
        let learn = div()
            .id(SharedString::from(format!("{id}-learn-more")))
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-brand"))
            .hover(|style| style.underline())
            .on_click(cx.listener(move |this, _, _, cx| {
                match item {
                    // `ef`: close, then the profile settings.
                    Lose::Profile => {
                        this.settings_extra.dialog = None;
                        this.open_profile_editor(cx);
                    }
                    // `ey`: close, then Server Boost.
                    Lose::Boost => {
                        this.settings_extra.dialog = None;
                        this.open_settings_section(dc_intl::tr("Server Boost"), cx);
                    }
                    Lose::ScreenShare => this.settings_open_url(STREAM_QUALITY, cx),
                    Lose::EmojiStickers | Lose::Uploads => {
                        this.settings_open_url(PREMIUM_DETAILS, cx);
                    }
                }
                cx.notify();
            }))
            .child(dc_intl::tr("Learn More"));
        let visual: AnyElement = match item {
            Lose::Profile => self.lose_profile_card(),
            other => {
                let url = match other {
                    Lose::EmojiStickers => LOSE_EMOJI_ART,
                    Lose::Boost => LOSE_BOOST_ART,
                    Lose::ScreenShare => LOSE_SCREEN_ART,
                    _ => LOSE_UPLOADS_ART,
                };
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(
                        self.images
                            .get(url)
                            .cloned()
                            .map(|art| gpui::img(art).max_w(px(168.)).w_full()),
                    )
                    .into_any_element()
            }
        };
        div()
            .id(id)
            .min_h(px(140.))
            .p(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-mod-subtle"))
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pr(px(40.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(lose_text(item, tier)),
                    )
                    .child(learn),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(180.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .flex()
                    .child(visual),
            )
            .into_any_element()
    }

    /// `profileCardTier1_o6yNAQ`: the 56px avatar, the name and the Nitro wheel on the low background.
    fn lose_profile_card(&self) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&crate::ui::avatar_source(&user).0).cloned();
        div()
            .w_full()
            .min_h(px(96.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .left(px(60.))
                    .top(px(16.))
                    .size(px(56.))
                    .rounded_full()
                    .overflow_hidden()
                    .children(avatar.map(|image| gpui::img(image).size(px(56.)).rounded_full())),
            )
            .child(
                div()
                    .pt(px(16.))
                    .flex()
                    .justify_center()
                    .items_center()
                    .child(
                        div()
                            .max_w(px(118.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--interactive-text-active"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(div().ml(px(6.)).child(icon(
                        "nitro_wheel",
                        20.,
                        "--interactive-text-active",
                    ))),
            )
            .into_any_element()
    }

    /// No saved payment source: the notice and "Add Payment Method". The button is drawn but not wired: the
    /// browser page that adds a source on its own is not in the snapshot.
    fn payment_methods_body(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        if !empty(self.nitro.sources.as_ref()) {
            // Module 158935 `o9` for each saved source: a divider above all but the first, then the row
            // (`paymentSourceRow_d3LvfJ`) with the source (module 219887) and Edit.
            let subscribed = self
                .premium_subscription()
                .and_then(|sub| sub.payment_source_id);
            let rows = super::payment_sources::sources_of(self.nitro.sources.as_ref())
                .into_iter()
                .enumerate()
                .map(|(index, source)| {
                    let for_subscription = subscribed.as_deref() == Some(source.id.as_str());
                    div()
                        .flex()
                        .flex_col()
                        .when(index > 0, |rows| {
                            rows.child(div().h(px(1.)).bg(color("--border-subtle")))
                        })
                        .child(
                            div()
                                .my(px(16.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(self.payment_source_view(&source, for_subscription))
                                .child(
                                    crate::ui::dm::small_button(
                                        "billing-source-edit",
                                        dc_intl::t("bt75uw"),
                                        crate::ui::dm::Tone::Secondary,
                                        true,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.billing_address_modal = Some(
                                            crate::ui::billing_address::BillingAddressModal::demo(
                                                "US", false,
                                            ),
                                        );
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                })
                .collect::<Vec<_>>();
            return div().flex().flex_col().children(rows).into_any_element();
        }
        div()
            .h(px(76.))
            .pt(px(29.))
            .flex()
            .items_start()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .child(
                        div()
                            .text_color(color("--text-default"))
                            .child(dc_intl::tr("No payment method saved")),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::tr("Checkout faster by saving a payment method")),
                    ),
            )
            .child(
                div()
                    .id("billing-add-payment")
                    .cursor_pointer()
                    // A money flow: only its first dialog opens here (`actions::Dialog::Account("payment")`).
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_extra.dialog =
                            Some(super::actions::Dialog::Account("payment"));
                        cx.notify();
                    }))
                    .child(button(
                        "billing-add-payment-button",
                        dc_intl::tr("Add Payment Method"),
                        ButtonKind::Primary,
                    )),
            )
            .into_any_element()
    }

    fn transactions_body(&self) -> AnyElement {
        if !empty(self.nitro.payments.as_ref()) {
            return div().h(px(48.)).into_any_element();
        }
        div()
            .h(px(48.))
            .pt(px(9.))
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-muted"))
            .child(dc_intl::tr("You haven't made any transactions yet."))
            .into_any_element()
    }
}

/// `medium` date of the ICU message format (en-US): "Nov 12, 2026", read from an ISO 8601 timestamp.
fn medium_date(iso: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = iso.get(..10)?.split('-');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    Some(format!(
        "{} {day}, {year}",
        MONTHS.get(month.checked_sub(1)?)?
    ))
}

/// The sentence of `getBillingInformationString` (module 722178) by status, as (text, bold) runs: active renews
/// ("Q18lRK"), canceled ends ("Whp/qk"), past due asks for payment info ("qEIzyi").
fn info_segments(state: u64, date: Option<&str>, price: Option<&str>) -> Vec<(String, bool)> {
    let date = date.unwrap_or("").to_owned();
    let price = price.unwrap_or("").to_owned();
    let run = |text: &str| (text.to_owned(), false);
    if state == status::CANCELED {
        vec![
            run("Your subscription will end on "),
            (date, true),
            run("."),
        ]
    } else if state == status::PAST_DUE {
        vec![
            run("Your "),
            (price, true),
            run(
                " subscription is past due. Please update your payment info or your subscription will end on ",
            ),
            (date, true),
            run("."),
        ]
    } else {
        vec![
            run("Your subscriptions will automatically renew on "),
            (date, true),
            run(" and you'll be charged "),
            (price, true),
            run("."),
        ]
    }
}

#[cfg(test)]
mod subscription_row_tests {
    use super::{info_segments, medium_date};
    use dc_state::premium::status;

    #[test]
    fn dates_read_like_icu_medium() {
        assert_eq!(
            medium_date("2026-11-12T08:00:00.000000+00:00").as_deref(),
            Some("Nov 12, 2026")
        );
        assert_eq!(medium_date("2027-01-03").as_deref(), Some("Jan 3, 2027"));
        assert_eq!(medium_date("soon"), None);
    }

    #[test]
    fn the_sentence_follows_the_status() {
        let line = |state| {
            info_segments(state, Some("Dec 1, 2026"), Some("TRY 104.99/month"))
                .into_iter()
                .map(|(text, _)| text)
                .collect::<String>()
        };
        assert_eq!(
            line(status::ACTIVE),
            "Your subscriptions will automatically renew on Dec 1, 2026 and you'll be charged TRY 104.99/month."
        );
        assert_eq!(
            line(status::CANCELED),
            "Your subscription will end on Dec 1, 2026."
        );
        assert!(line(status::PAST_DUE).contains("is past due"));
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::{Lose, lose_items, lose_text};
    use dc_state::premium::PremiumType;

    #[test]
    fn what_you_lose_lists_the_tier_items_in_the_module_order() {
        assert_eq!(
            lose_items(PremiumType::Basic),
            &[Lose::EmojiStickers, Lose::Uploads]
        );
        assert_eq!(
            lose_items(PremiumType::Nitro),
            &[
                Lose::Profile,
                Lose::EmojiStickers,
                Lose::Boost,
                Lose::ScreenShare,
                Lose::Uploads
            ]
        );
        assert_eq!(lose_items(PremiumType::Classic).last(), Some(&Lose::Boost));
        assert_eq!(
            lose_text(Lose::Uploads, PremiumType::Basic),
            "Bigger uploads"
        );
        assert_eq!(
            lose_text(Lose::Boost, PremiumType::Nitro),
            "Your 2 free Boosts and 30% Boost discount"
        );
    }
}

#[cfg(test)]
mod confirm_tests {
    use super::{cancel_confirm_text, cancel_request, keeps_other_items};
    use dc_state::premium::{PremiumType, Subscription, plan_id, status};
    use serde_json::json;

    fn sub(status: u64, plans: &[u64]) -> Subscription {
        let items: Vec<_> = plans
            .iter()
            .map(|plan| json!({"id": "1", "plan_id": plan.to_string(), "quantity": 1}))
            .collect();
        Subscription::from_json(&json!({
            "id": "77", "type": 1, "status": status, "items": items, "currency": "eur",
            "current_period_end": "2026-12-02T09:30:00.000000+00:00",
        }))
        .unwrap()
    }

    fn text(paragraphs: &[Vec<(String, bool)>]) -> Vec<String> {
        paragraphs
            .iter()
            .map(|runs| runs.iter().map(|(text, _)| text.as_str()).collect())
            .collect()
    }

    #[test]
    fn the_confirm_step_says_when_access_ends() {
        let shown = cancel_confirm_text(
            status::ACTIVE,
            PremiumType::Nitro,
            Some("2026-12-02T09:30:00.000000+00:00"),
        );
        assert_eq!(
            text(&shown),
            [
                "Cancellation will take effect at the end of your current billing period. On Dec 2, 2026, you will lose access to Nitro, and your included Server Boosts will be removed.",
                "You can restart your subscription anytime.",
            ]
        );
        assert!(
            shown[0]
                .iter()
                .any(|(text, bold)| *bold && text == "Dec 2, 2026")
        );
        let now = cancel_confirm_text(status::PAST_DUE, PremiumType::Nitro, None);
        assert_eq!(
            text(&now),
            [
                "Cancellation will take effect immediately for any Nitro and Server Boost subscriptions."
            ]
        );
        let classic = cancel_confirm_text(status::ACTIVE, PremiumType::Classic, Some("2027-01-03"));
        assert!(text(&classic)[0].ends_with("on Jan 3, 2027."));
    }

    #[test]
    fn cancelling_renews_without_the_premium_plan_or_deletes_a_late_one() {
        let crate::net::Request::Rest {
            method,
            route,
            body,
            ..
        } = cancel_request(&sub(status::ACTIVE, &[plan_id::MONTH_TIER_2]), "tok")
        else {
            panic!("not a REST request");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me/billing/subscriptions/77");
        let body = body.unwrap();
        assert_eq!(body["items"], json!([]));
        assert_eq!(body["purchase_token"], "tok");
        assert_eq!(
            body["expected_invoice_price"],
            json!({"amount": 0, "currency": "eur"})
        );
        let crate::net::Request::Rest { method, body, .. } =
            cancel_request(&sub(status::PAST_DUE, &[plan_id::MONTH_TIER_2]), "tok")
        else {
            panic!("not a REST request");
        };
        assert_eq!(method, reqwest::Method::DELETE);
        assert!(body.is_none());
    }

    #[test]
    fn boosts_kept_beside_nitro_lead_to_the_preview() {
        assert!(!keeps_other_items(&sub(
            status::ACTIVE,
            &[plan_id::MONTH_TIER_2]
        )));
        assert!(keeps_other_items(&sub(
            status::ACTIVE,
            &[plan_id::MONTH_TIER_2, plan_id::MONTH_GUILD]
        )));
    }
}
