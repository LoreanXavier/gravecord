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
                    .child("Your Subscriptions"),
            )
            .child(
                div()
                    .mt(px(16.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(
                        "These are your current subscriptions. They will be billed on the same billing cycle. You can update any subscription at any time.",
                    ),
            )
            .child(
                div()
                    .mt(px(16.))
                    .children(none.then(|| {
                        empty_row("subscriptions-none", "You have no active subscriptions")
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
                    .child("Subscription Credit"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(
                        "When you accept a gift while you have a running subscription, redeem a promotion, or accept a gift that differs from your current subscription, it will appear here as credit.",
                    ),
            )
            .child(div().mt(px(32.)).child(empty_row(
                "subscriptions-no-credit",
                "You have no unused credits",
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
                    .child("Cancel")
            }))
            .child(
                div()
                    .id("subscription-switch")
                    // Switch Plans (`dylp/7`) opens module 778705 with `subscription: t`, whose first step is
                    // the plan select. Re-subscribe (`ea`) may open the resubscribe confirmation: not wired.
                    .when(!canceled, |button| {
                        button
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(false, cx)))
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
                        "Re-subscribe"
                    } else {
                        "Switch Plans"
                    }),
            );
        banner
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(28.))
                    .font_weight(FontWeight(800.))
                    .font_family(crate::ui::nitro::NITRO_FONT)
                    .child(sub.display_name().unwrap_or("Nitro").to_uppercase()),
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

    fn cancel_tier(&self) -> PremiumType {
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
            title: "Are you sure you want to cancel?".into(),
            subtitle: Some(
                div()
                    .child(
                        "If you cancel, you will lose these perks at the end of your current billing period:",
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
                            .children(items),
                    )
                    .into_any_element(),
            ),
            buttons: vec![
                modal::button(
                    "settings-cancel-nitro-keep",
                    "Never mind, keep Nitro",
                    modal::Tone::Secondary,
                    close,
                ),
                modal::button(
                    "settings-cancel-nitro-continue",
                    "Continue",
                    modal::Tone::Primary,
                    |_, _, _| {},
                ),
            ],
            close: Box::new(close),
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
                        this.open_settings_section("Server Boost", cx);
                    }
                    Lose::ScreenShare => this.settings_open_url(STREAM_QUALITY, cx),
                    Lose::EmojiStickers | Lose::Uploads => {
                        this.settings_open_url(PREMIUM_DETAILS, cx);
                    }
                }
                cx.notify();
            }))
            .child("Learn More");
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
            return div().h(px(76.)).into_any_element();
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
                            .child("No payment method saved"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_color(color("--text-muted"))
                            .child("Checkout faster by saving a payment method"),
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
                        "Add Payment Method",
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
            .child("You haven't made any transactions yet.")
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
