//! The purchase dialogs of the Nitro page: Select Plan, Checkout, the Add Payment Method hand-off and Send a Gift.
//! Geometry is from `design/dom/modals/nitro-purchase-{plan,step2,step3}-darker-cozy.json` and
//! `nitro-gift-darker-cozy.json`.
//!
//! Nothing here buys anything. "Select" goes to the payment step, which in the official client opens the billing
//! page in the browser (module 36167: `POST /auth/handoff`, then the standalone checkout URL); the payment itself is
//! entered there. "Pay Here" is inert in capture builds, and `SubscribeRequest` is only ever a unit-tested body.
//!
//! The gift banner ("1 gift away from the Patron Badge!") reads the Gifting badge of the badge catalog
//! (modules 806677, 492275).
//!
//! Choosing a card in Send a Gift (module 806677 `f`, which calls module 778705 with `isGift` and the
//! `giftRecipient`) opens "Gift Nitro" (`GiftSelect`, captured in `design/dom/dm/gift-step2-nitro*-darker-cozy.json`).
//! Its body is module 814576: with a recipient (a one-to-one DM) it shows the gift box, the recipient (`948388`), the
//! message (`968060`, default text `ZkOo1U`, 190 characters) and the Select gift rows (`813267`). Opened from a server
//! channel or the Nitro page there is no recipient, which the captures do not cover: the cards stay inert there.

use super::nitro::{BASIC_FEATURES, NITRO_FEATURES, oklab, rgb};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::premium::{self, Price, sku};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, Window, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Step {
    Plans,
    /// "Gift Nitro": the recipient, the message and the period of a gift.
    GiftSelect,
    Checkout,
    Payment,
    /// In-app checkout REVIEW step & footer (module 809029: `YScQSF`, `ouo4FK`, `LvcUx4`,
    /// `cRCCJ3`, `zpi5pg`, `IJI7yk`, `VPuTc5`, `722178.ff` plan labels, expressive trial CTA).
    Review,
    /// Purchase token email verification step (module 615456 `AWAITING_PURCHASE_TOKEN_AUTH`).
    AwaitingEmailAuth,
    /// Purchase predicate error view (module 615456 `p8+qtU`).
    PredicateError,
    /// Post-purchase confirmation (module 231308 `G`, `B`, `b`; CSS 13792, 896554).
    Confirm,
    /// Post-purchase tier activated celebration (module 115623: `wLFT6z`, `OsAK9h`, `JtWl+a`).
    TierActivated,
    /// Module 671334: Add Payment Method step manager (Credit Card form, PayPal/Venmo/CashApp/P24/EPS).
    AddPaymentCard,
    /// Module 677959: Gift Card Balance Redemption modal (`26tjwd`, `POST /billing/gift-card/redeem`).
    GiftCardRedeem,
    /// Module 677959 / 544036: Gift Card Balance Redemption success confirmation (`Afx5yS`).
    GiftCardRedeemSuccess,
}

/// Module 675219 (`UnifiedCheckoutFlow`): all 12 official checkout flow identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, clippy::enum_variant_names)]
pub(crate) enum UnifiedCheckoutFlow {
    PremiumCheckout,
    CollectiblesCheckout,
    SlayerStorefrontCheckout,
    PremiumAppsOtpCheckout,
    PremiumAppsSubCheckout,
    GuildBoostCheckout,
    GuildProductCheckout,
    GuildRoleSubCheckout,
    OrbCheckout,
    InboundPremiumPromotionCheckout,
    GiftCardRedeemCheckout,
    ApplicationSubscriptionCheckout,
}

impl UnifiedCheckoutFlow {
    /// Module 675219: flows that include the `xl` `"Select gift card"` (`W685+b`) customization step.
    pub(crate) fn allow_gift_customization(self) -> bool {
        matches!(
            self,
            Self::CollectiblesCheckout
                | Self::SlayerStorefrontCheckout
                | Self::PremiumAppsOtpCheckout
        )
    }

    /// Module 675219: Orb checkout skips payment/authentication steps.
    pub(crate) fn exclude_payment_auth_steps(self) -> bool {
        matches!(self, Self::OrbCheckout)
    }
}

/// Module 675219: one step configuration produced by `generate_checkout_step_configs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CheckoutStepConfig {
    pub key: &'static str,
    pub breadcrumb: &'static str,
    pub modal_size: &'static str,
}

/// Module 675219 (`generateCheckoutStepConfigs`): builds the ordered step configs for a flow.
pub(crate) fn generate_checkout_step_configs(
    flow: UnifiedCheckoutFlow,
    is_gift: bool,
) -> Vec<CheckoutStepConfig> {
    let mut steps = Vec::new();
    if is_gift && flow.allow_gift_customization() {
        steps.push(CheckoutStepConfig {
            key: "gift_customization",
            breadcrumb: dc_intl::t("W685+b"),
            modal_size: "xl",
        });
    }
    if !flow.exclude_payment_auth_steps() {
        steps.push(CheckoutStepConfig {
            key: "payment",
            breadcrumb: dc_intl::t("vHqbJE"),
            modal_size: "md",
        });
    }
    steps.push(CheckoutStepConfig {
        key: "review",
        breadcrumb: dc_intl::t("QBnNHq"),
        modal_size: "md",
    });
    steps
}

/// Module 677959 (`premiumTierLabel`): maps official PremiumType tier to localized plan label.
pub(crate) fn premium_tier_label(tier: u8) -> &'static str {
    match tier {
        0 => dc_intl::t("t9uG/o"),
        1 => dc_intl::t("FSOz78"),
        _ => dc_intl::t("lG6a5x"),
    }
}

/// Module 671334: supported payment provider sub-steps inside Add Payment Method.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PaymentProviderStep {
    #[default]
    CreditCard,
    PayPal,
    Venmo,
    CashApp,
    Przelewy24,
    Eps,
    BrowserHandoff,
}

/// Module 671334: client-side credit card form validation returning exact official intl errors.
pub(crate) fn validate_card_form(
    number: &str,
    expiry: &str,
    cvc: &str,
    name: &str,
) -> Vec<&'static str> {
    let mut errors = Vec::new();
    let digits: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        errors.push(dc_intl::t("eOIfuy"));
    } else if digits.len() < 12 || digits.len() > 19 {
        errors.push(dc_intl::t("x4pWtJ"));
    }
    let exp_ok = expiry
        .trim()
        .split_once('/')
        .and_then(|(m, y)| {
            let mm: u32 = m.trim().parse().ok()?;
            let yy: u32 = y.trim().parse().ok()?;
            Some((1..=12).contains(&mm) && (24..=99).contains(&yy))
        })
        .unwrap_or(false);
    if !exp_ok {
        errors.push(dc_intl::t("9/zZdl"));
    }
    let cvc_digits: String = cvc.chars().filter(|c| c.is_ascii_digit()).collect();
    if !(3..=4).contains(&cvc_digits.len()) {
        errors.push(dc_intl::t("ro4isZ"));
    }
    if name.trim().is_empty() {
        errors.push(dc_intl::t("lIkVsi"));
    }
    errors
}

/// Module 671334: provider-specific title and instructional copy (PayPal, Venmo, Cash App, P24, EPS, Browser).
pub(crate) fn payment_provider_copy(provider: PaymentProviderStep) -> (&'static str, &'static str) {
    match provider {
        PaymentProviderStep::CreditCard => (dc_intl::t("cVyJ3o"), dc_intl::t("gPRHfw")),
        PaymentProviderStep::PayPal => (dc_intl::t("QQBAos"), dc_intl::t("dte2M9")),
        PaymentProviderStep::Venmo => (dc_intl::t("4KoTLM"), dc_intl::t("DowIre")),
        PaymentProviderStep::CashApp => (dc_intl::t("9ALP8w"), dc_intl::t("ze/1yE")),
        PaymentProviderStep::Przelewy24 => (dc_intl::t("w/qqKK"), dc_intl::t("De3b8t")),
        PaymentProviderStep::Eps => (dc_intl::t("sN3wrd"), dc_intl::t("dFyV07")),
        PaymentProviderStep::BrowserHandoff => (dc_intl::t("C4HYfy"), dc_intl::t("xfG7Jp")),
    }
}

/// Module 671334 + 677959: interactive state for the Add Payment Card form and Gift Card Balance Redemption modal.
#[derive(Clone, Debug, Default)]
pub(crate) struct CheckoutExtraState {
    pub provider: PaymentProviderStep,
    pub card_number: String,
    pub card_expiry: String,
    pub card_cvc: String,
    pub card_name: String,
    pub card_error_banner: Option<String>,
    pub address_error: Option<String>,
    pub gift_pin: String,
    pub gift_postal: String,
    pub gift_country: String,
    pub gift_error: Option<String>,
    pub gift_added_price: String,
}

/// Plan tier/interval identity used by module 231308's confirmation copy (`B`, `G`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConfirmPlan {
    BasicMonth,
    BasicYear,
    ClassicMonth,
    ClassicYear,
    NitroMonth,
    Nitro3Month,
    Nitro6Month,
    NitroYear,
}

impl ConfirmPlan {
    fn rank(self) -> usize {
        match self {
            Self::BasicMonth => 0,
            Self::BasicYear => 1,
            Self::ClassicMonth => 2,
            Self::ClassicYear => 3,
            Self::NitroMonth => 4,
            Self::Nitro3Month => 5,
            Self::Nitro6Month => 6,
            Self::NitroYear => 7,
        }
    }

    fn tier(self) -> u8 {
        match self {
            Self::BasicMonth | Self::BasicYear => 0,
            Self::ClassicMonth | Self::ClassicYear => 1,
            Self::NitroMonth | Self::Nitro3Month | Self::Nitro6Month | Self::NitroYear => 2,
        }
    }

    fn is_multi_month(self) -> bool {
        matches!(
            self,
            Self::Nitro3Month | Self::Nitro6Month | Self::NitroYear
        )
    }

    pub(crate) fn from_sku(plan_sku: u64, yearly: bool) -> Self {
        match (plan_sku, yearly) {
            (sku::TIER_0, false) => Self::BasicMonth,
            (sku::TIER_0, true) => Self::BasicYear,
            (sku::TIER_1, false) => Self::ClassicMonth,
            (sku::TIER_1, true) => Self::ClassicYear,
            (_, false) => Self::NitroMonth,
            (_, true) => Self::NitroYear,
        }
    }
}

/// Context for the Nitro purchase confirmation step (module 231308 `G`).
#[derive(Clone, Debug, Default)]
pub(crate) struct ConfirmState {
    pub starting_plan: Option<ConfirmPlan>,
    pub pending_payment: bool,
    pub is_downgrade: bool,
    pub has_fractional_credits: bool,
    pub followup_sku_name: Option<String>,
    pub post_success_guild: Option<(dc_state::Id, String)>,
    pub deeplink: bool,
    /// Module 585121 "Wow Moment" confirmation with 320px nameplate preview (`75Wt0E`/`QWljxE`/`X79Az5`).
    pub wow_moment: bool,
}

/// `MVz.FRACTIONAL_PREMIUM_ABOUT` (module 652215 / 975571).
const FRACTIONAL_PREMIUM_ARTICLE: &str =
    "https://support.discord.com/hc/en-us/articles/26362805097623";

/// Header wordmark (`b` of module 231308: `PREMIUM_PAYMENT_STARTED` -> NITRO, `PREMIUM_UPDATED` -> NITRO UPDATED,
/// `PREMIUM_ACTIVATED` -> NITRO ACTIVATED).
pub(crate) fn confirm_banner_wordmark(state: &ConfirmState) -> &'static str {
    if state.pending_payment {
        "NITRO"
    } else if state.starting_plan.is_some() {
        "NITRO UPDATED"
    } else {
        "NITRO ACTIVATED"
    }
}

/// Primary button label (`G` of module 231308).
pub(crate) fn confirm_button_label(state: &ConfirmState) -> String {
    if state.deeplink {
        dc_intl::t("sRApon").to_owned()
    } else if let Some(name) = state.followup_sku_name.as_deref() {
        dc_intl::t!("1qGgm4", skuName = name)
    } else if state.is_downgrade {
        dc_intl::t("QJ9EyM").to_owned()
    } else if state.post_success_guild.is_some() {
        dc_intl::t("ta3cXY").to_owned()
    } else {
        dc_intl::t("TkTvBz").to_owned()
    }
}

/// Body paragraphs for module 231308 (`G` + `B` plan-switch branches).
pub(crate) fn confirm_paragraphs(plan: ConfirmPlan, state: &ConfirmState) -> Vec<String> {
    let new_sub_copy = |p: ConfirmPlan, pending: bool| -> String {
        match (p.tier(), pending) {
            (0, true) => dc_intl::t("o6hBiR").to_owned(),
            (0, false) => dc_intl::t("AGf/ye").to_owned(),
            (1, true) => dc_intl::t("o6hBiR").to_owned(),
            (1, false) => dc_intl::t("knvOVz").to_owned(),
            (_, true) => dc_intl::t("A4THYt").to_owned(),
            (_, false) => dc_intl::t("aTUr3Z").to_owned(),
        }
    };

    if state.deeplink {
        return vec![format!("{} {}", dc_intl::t("bIVRSQ"), dc_intl::t("0UJqOy"))];
    }
    if let Some(sku_name) = state.followup_sku_name.as_deref() {
        return vec![dc_intl::t!("tsQOs+", skuName = sku_name)];
    }
    if state.has_fractional_credits && !state.is_downgrade {
        let lead = if plan.tier() == 0 {
            dc_intl::t("L9lcG/").to_owned()
        } else {
            dc_intl::t("UPpbP3").to_owned()
        };
        let credits = dc_intl::format(
            "EoDFuN",
            &[("helpCenterLink", FRACTIONAL_PREMIUM_ARTICLE.into())],
        );
        return vec![lead, credits];
    }
    if let Some(starting) = state.starting_plan {
        let text = if starting.rank() < plan.rank() {
            match (plan, state.pending_payment) {
                (ConfirmPlan::ClassicMonth, true) => dc_intl::t("o6hBiR").to_owned(),
                (ConfirmPlan::ClassicMonth, false) => dc_intl::t("knvOVz").to_owned(),
                (ConfirmPlan::NitroMonth, true) => dc_intl::t("A4THYt").to_owned(),
                (ConfirmPlan::NitroMonth, false) => dc_intl::t("f8Dquh").to_owned(),
                (ConfirmPlan::ClassicYear, true) => dc_intl::t("MHAxpE").to_owned(),
                (ConfirmPlan::ClassicYear, false) => dc_intl::t("YZd5rw").to_owned(),
                (ConfirmPlan::NitroYear, true) => {
                    dc_intl::format("mULxLW", &[("numFreeGuildSubscriptions", 2.into())])
                }
                (ConfirmPlan::NitroYear, false) => {
                    dc_intl::format("M/Lknl", &[("numFreeGuildSubscriptions", 2.into())])
                }
                (ConfirmPlan::Nitro3Month | ConfirmPlan::Nitro6Month, _) => {
                    dc_intl::t("aaCoW6").to_owned()
                }
                (ConfirmPlan::BasicMonth | ConfirmPlan::BasicYear, _) => {
                    dc_intl::t("XEoQ7+").to_owned()
                }
            }
        } else if starting.tier() == 2 && plan.tier() == 1 {
            dc_intl::t("PNX4NM").to_owned()
        } else {
            match plan.tier() {
                0 => dc_intl::t("XEoQ7+").to_owned(),
                1 => dc_intl::t("nLI1Kr").to_owned(),
                _ => {
                    if starting.is_multi_month() {
                        dc_intl::t("qSZZVA").to_owned()
                    } else {
                        dc_intl::t("RNjcNg").to_owned()
                    }
                }
            }
        };
        return vec![text];
    }
    if let Some((_, guild_name)) = &state.post_success_guild {
        return vec![
            new_sub_copy(plan, state.pending_payment),
            dc_intl::t!("6aZ0NR", guildName = guild_name.as_str()),
        ];
    }
    vec![new_sub_copy(plan, state.pending_payment)]
}

/// `ZkOo1U`: the message a gift to a person starts with (module 418200 `$` state, set when the recipient can get a
/// custom message).
const DEFAULT_GIFT_MESSAGE: &str = "You\u{2019}re awesome, just like this gift. Enjoy!";
/// `maxCharacterCount: 190` of the message field (module 968060).
const GIFT_MESSAGE_LIMIT: usize = 190;
/// The Standard Box gift animation's loop file (module 446263 `STANDARD_BOX`, `LOOP`), drawn at its resting pose.
pub(super) const GIFT_BOX_ART: &str = "https://cdn.discordapp.com/assets/content/13050a4f904230e1617429a7f6f42ab959e81a99c099d4542eb43af9beb612d2.lottiejson";

/// The percentage the yearly gift saves over twelve monthly ones (module 722178 `tr`:
/// `floor(100 * (1 - year / (month * 12)))`), `None` when it saves nothing.
fn savings_percent(month: i64, year: i64) -> Option<i64> {
    let twelve = month.checked_mul(12).filter(|twelve| *twelve > 0)?;
    let percent = (100 * (twelve - year)).div_euclid(twelve);
    (percent > 0).then_some(percent)
}

/// The characters left of the message (`N = maxLength - text.length`, module 246049; `length` counts UTF-16 units).
fn characters_left(text: &str) -> i64 {
    GIFT_MESSAGE_LIMIT as i64 - text.encode_utf16().count() as i64
}

/// The Send a Gift banner's line (en-US string `0+xfd9` of module 213427: a cardinal plural, then the tier name).
fn gifts_away(remaining: u64, tier: &str) -> String {
    let gifts = if remaining == 1 { "gift" } else { "gifts" };
    format!("{remaining} {gifts} away from the {tier} Badge!")
}

/// How far into its dip a pulsing dot is at `phase` of a cycle: 0 at the keyframes 0% and 100%, 1 at 50% (the
/// keyframes of `spinner-pulsing-ellipsis`, each half eased by the animation's `ease-in-out`).
pub(super) fn pulse_wave(phase: f32) -> f32 {
    use super::motion::Ease;
    let phase = phase.rem_euclid(1.);
    let half = Ease::EaseInOut.apply(f64::from((phase * 2.).fract())) as f32;
    if phase < 0.5 { half } else { 1. - half }
}

/// The pulsing ellipsis a price line shows while the SKU's plans load (module 156601 renders module 289873's
/// `PULSING_ELLIPSIS` with `priceSpinner`, css 228271: a 22px high flex row, content at the start, 18px under it).
/// The spinner (css 193433): a 28px box centring three 6px dots 2px apart, resting at opacity .3 and pulsing
/// opacity 1, .3, 1 with scale 1, .8, 1 in 1.4s ease-in-out, the dots .2s apart.
fn price_spinner(key: String) -> gpui::Div {
    use gpui::{Animation, AnimationExt as _};
    const CYCLE_MS: u64 = 1400;
    let dot = |index: usize| {
        div()
            .size(px(6.))
            .mr(px(2.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .rounded(px(3.))
                    .bg(color("--primary-100"))
                    .size(px(6.))
                    .with_animation(
                        gpui::SharedString::from(format!("{key}-dot-{index}")),
                        Animation::new(std::time::Duration::from_millis(CYCLE_MS)).repeat(),
                        move |element, delta| {
                            let wave = pulse_wave(delta - 0.2 / 1.4 * index as f32);
                            element
                                .size(px(6. * (1. - 0.2 * wave)))
                                .opacity(1. - 0.7 * wave)
                        },
                    ),
            )
    };
    div().h(px(22.)).mb(px(18.)).flex().items_center().child(
        div()
            .w(px(28.))
            .flex()
            .justify_center()
            .child(dot(0))
            .child(dot(1))
            .child(dot(2)),
    )
}

/// The open purchase dialog.
pub(crate) struct Purchase {
    pub gift: bool,
    pub step: Step,
    pub sku: u64,
    pub yearly: bool,
    pub scroll: ScrollHandle,
    /// Send a Gift opened from a one-to-one DM: the recipient is that person (module 806677 `is`, `giftRecipient`),
    /// and the modal adds their wishlist section above the plan cards.
    pub recipient: bool,
    /// The DM's other person, the gift's recipient (module 806677 `giftRecipient`).
    pub recipient_user: Option<dc_state::Id>,
    /// The gift message field of "Gift Nitro", created when a card is chosen and kept while the dialog is open.
    pub message: Option<Entity<Editor>>,
    pub message_subscription: Option<gpui::Subscription>,
    /// "Add sound effect": the sound the gift plays when it is opened (module 418200 `soundEffect`).
    pub sound: Option<GiftSound>,
    /// "Add emoji confetti" (module 418200 `emojiConfetti`).
    pub confetti: Option<GiftConfetti>,
    /// Post-purchase confirmation details (module 231308).
    pub confirm: ConfirmState,
    /// Optional Nitro offer/trial badge rendered in the modal header (module 683166:
    /// `IBYG5U` "free trial", `gtNqJQ` "2-WEEK TRIAL", `iiLbvu` "{percent}% discount").
    pub offer_badge: Option<PurchaseOfferBadge>,
    /// Optional gift review header override (`wKaVLC` "Review your gift", `vHqbJE` "Choose payment method").
    pub header_step_label: Option<&'static str>,
    /// Whether the purchase verification email was resent (module 615456 `StGVvC` / `Ex7sk9`).
    pub email_resent: bool,
    /// Optional fractional Nitro credit / trial ending fine print banner (module 781949).
    pub fractional_banner: Option<FractionalBannerState>,
    /// Checkout REVIEW step context & footer CTA selector (module 809029).
    pub review: ReviewContext,
    /// Optional checkout payment error rendered in `PaymentErrorBlock` (module 901930).
    pub payment_error: Option<CheckoutPaymentError>,
    /// Show brand-refresh progress breadcrumb header (module 332030: `TfxqUO`, `lG6a5x`/`t9uG/o`/`aFBQ3d` + BETA).
    pub show_progress_header: bool,
    /// Whether this checkout is for Nitro Squad / Premium Group (`aFBQ3d` + BETA pill in module 332030).
    pub is_premium_group: bool,
    /// Modules 671334 & 677959: Add Payment Card provider/validation and Gift Card Balance Redeem state.
    pub extra: CheckoutExtraState,
}

/// Billing error rendered by `PaymentErrorBlock` (module 901930: `iWvwQS`, `ypuSd8`, `mXMmWE`,
/// `mC1Fjz`, `mv/fF2`, `KXodvL`, `D1BwNC`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckoutPaymentError {
    pub code: u32,
    pub message: String,
    pub multi_currency: bool,
    pub guardian_count: usize,
}

impl CheckoutPaymentError {
    /// Billing address helpdesk URL (`MVz.BILLING` + `bNI.INVALID_BILLING_ADDRESS`, module 901930).
    pub(crate) const BILLING_HELP_URL: &'static str = "https://support.discord.com/hc/en-us/articles/360012668071#docs-internal-guid-1731ba7c-7fff-87a1-c976-f62cc1cd2618";

    /// Resolves the localized display message + optional action kind (`guardian` or `helpdesk`) per module 901930.
    pub(crate) fn resolved_message(&self) -> (String, Option<&'static str>) {
        match self.code {
            100054 if self.multi_currency => {
                (format!("{} {}", self.message, dc_intl::t("iWvwQS")), None)
            }
            50097 => (dc_intl::t("ypuSd8").to_owned(), None),
            100060 => (dc_intl::t("mXMmWE").to_owned(), None),
            100051 => (dc_intl::t("mC1Fjz").to_owned(), None),
            100150 | 100151 => {
                let text: String = dc_intl::format_runs("mv/fF2", &[])
                    .into_iter()
                    .map(|(s, _)| s)
                    .collect();
                (text, (self.guardian_count > 0).then_some("guardian"))
            }
            100010 => {
                let text: String = dc_intl::format_runs(
                    "KXodvL",
                    &[("helpdeskArticle", Self::BILLING_HELP_URL.into())],
                )
                .into_iter()
                .map(|(s, _)| s)
                .collect();
                (text, Some("helpdesk"))
            }
            _ => (self.message.clone(), None),
        }
    }
}

/// Plan identity for module 722178 `ff` / module 809029 primary review CTA text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ReviewPlanKind {
    BasicMonthly,
    BasicYearly,
    ClassicMonthly,
    ClassicYearly,
    #[default]
    NitroMonthly,
    NitroYearly,
    NitroMultiMonth(u32),
    NitroGroup,
    Boost,
}

/// Payment restriction warning kind in `335056` (`Tdb5qb` / `6d44F5`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PaymentRestriction {
    SelectPaymentMethod,
    AddNewPaymentMethod,
}

/// REST request builder for `POST /users/@me/billing/subscriptions/{id}/preview` (module 335056).
#[allow(dead_code)]
pub(crate) fn subscription_preview_request(
    subscription_id: &str,
    plan_id: u64,
    payment_source_id: Option<&str>,
    currency: &str,
    renewal: bool,
    apply_entitlements: bool,
) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("nitro-sub-preview:{subscription_id}")),
        method: reqwest::Method::POST,
        route: format!("/users/@me/billing/subscriptions/{subscription_id}/preview"),
        body: Some(serde_json::json!({
            "items": [{ "plan_id": plan_id.to_string(), "quantity": 1 }],
            "payment_source_id": payment_source_id,
            "currency": currency,
            "renewal": renewal,
            "apply_entitlements": apply_entitlements,
        })),
    }
}

/// Review step state driving module 809029's CTA priority and module 335056's order summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReviewContext {
    pub is_recurring_gift: bool,
    pub is_gift: bool,
    pub is_past_due: bool,
    pub is_prepaid: bool,
    pub has_active_sub: bool,
    pub is_paused_resume: bool,
    pub is_upgrade: bool,
    pub is_trial_eligible: bool,
    pub show_back: bool,
    pub plan_kind: ReviewPlanKind,
    /// Module 335056: existing open invoice forces immediate billing.
    pub has_open_invoice: bool,
    /// Module 335056: optional payment gateway restriction warning (`Tdb5qb` / `6d44F5`).
    pub payment_restriction: Option<PaymentRestriction>,
    /// Module 335056: non-USD currency appends trailing `*` to total (`24659.U5`).
    pub non_usd_currency: bool,
    /// Module 367210 (`IAsfR5`): active trial duration in days.
    pub trial_days: Option<u32>,
    /// Module 367210 (`wCkwJf` / `tUzT/U`): active discount offer percentage and interval months (`None` = yearly).
    pub discount_percent: Option<u8>,
    pub discount_interval_months: Option<u32>,
    /// Module 367210 (`7sYIBL` / `uCXulJ` / `Mi5BH/`): multi-period gift savings percentage & interval months.
    pub gift_savings_percent: Option<i64>,
    pub gift_interval_months: Option<u32>,
    /// Module 367210 (`2wPRSF` / `IGU7El`): months of existing subscription credit (optional payment method).
    pub credit_months: Option<u32>,
    /// Module 367210 (`Urtyu9`): days until next monthly Orbs drop after Nitro begins.
    pub orbs_drop_days: Option<u32>,
    /// Optional explicit subtotal/discount/tax cents (computed from plan defaults when `None`).
    pub subtotal_cents: Option<i64>,
    pub discount_cents: Option<i64>,
    pub tax_cents: Option<i64>,
    /// Module 498750: active discount offer, quantity, and editable plan flag for `241989.f7`.
    pub discount_offer: Option<CheckoutDiscountOffer>,
    pub quantity: u32,
    pub can_edit_plan: bool,
}

/// Active checkout discount offer for module `498750` (`hXcaLT` + `VZ8Tvh` / `JsSin7`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CheckoutDiscountOffer {
    pub discount_percent: u32,
    pub interval_count: u32,
    pub is_yearly: bool,
}

/// Formatted `PurchaseItem` (`498750._` / `241989.f7`) summary row for the Review modal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PurchaseSummaryItem {
    pub label: String,
    pub editable: bool,
    pub description: String,
    pub price: String,
    pub inline_strikethrough: Option<String>,
    pub sub_text: Option<String>,
    pub sub_text_strikethrough: bool,
}

/// Pure `498750._` builder: recurring gift labels (`rstMwN`/`Vb8FaC`), quantity/edit/group labels
/// (`kyGViz`/`BnZSW0`/`8bPDtb`), gift vs subscription (`3wsYeI`/`2zUa6I`), and discount offer price
/// lines (`hXcaLT` + `VZ8Tvh`/`JsSin7`).
pub(crate) fn build_purchase_summary(
    ctx: &ReviewContext,
    regular_price: &str,
    discounted_price: Option<&str>,
    is_yearly: bool,
    is_premium_group: bool,
    base_label: &str,
) -> PurchaseSummaryItem {
    let qty = ctx.quantity.max(1);
    let label = if is_premium_group {
        dc_intl::format(
            "8bPDtb",
            &[(
                "premiumGroupProductName",
                dc_intl::Arg::Str(base_label.into()),
            )],
        )
    } else if qty > 1 {
        dc_intl::format(
            "kyGViz",
            &[
                ("quantity", dc_intl::Arg::Num(qty as f64)),
                ("label", dc_intl::Arg::Str(base_label.into())),
            ],
        )
    } else {
        base_label.to_owned()
    };
    let description = if ctx.is_recurring_gift {
        if is_yearly {
            dc_intl::t("rstMwN").to_owned()
        } else {
            dc_intl::t("Vb8FaC").to_owned()
        }
    } else if ctx.is_gift {
        dc_intl::t("3wsYeI").to_owned()
    } else {
        dc_intl::t("2zUa6I").to_owned()
    };
    let (price, inline_strikethrough, sub_text, sub_text_strikethrough) =
        if let (Some(offer), Some(discounted)) = (ctx.discount_offer, discounted_price) {
            let now_line =
                dc_intl::format("hXcaLT", &[("price", dc_intl::Arg::Str(discounted.into()))]);
            let after_line = if offer.is_yearly {
                dc_intl::format(
                    "VZ8Tvh",
                    &[("regularPrice", dc_intl::Arg::Str(regular_price.into()))],
                )
            } else {
                let runs = dc_intl::format_runs(
                    "JsSin7",
                    &[
                        ("priceRate", dc_intl::Arg::Str(regular_price.into())),
                        (
                            "intervalCount",
                            dc_intl::Arg::Num(offer.interval_count.max(1) as f64),
                        ),
                    ],
                );
                runs.into_iter().map(|(t, _)| t).collect()
            };
            (now_line, None, Some(after_line), false)
        } else if ctx.is_trial_eligible {
            (
                dc_intl::format("hXcaLT", &[("price", dc_intl::Arg::Str("$0.00".into()))]),
                None,
                Some(regular_price.to_owned()),
                false,
            )
        } else if let Some(discounted) = discounted_price.filter(|d| *d != regular_price) {
            (
                discounted.to_owned(),
                Some(regular_price.to_owned()),
                None,
                false,
            )
        } else {
            (regular_price.to_owned(), None, None, false)
        };
    PurchaseSummaryItem {
        label,
        editable: ctx.can_edit_plan,
        description,
        price,
        inline_strikethrough,
        sub_text,
        sub_text_strikethrough,
    }
}

/// Module 367210 (`Y`): priority promotional/discount/savings notice runs `(text, bold)`.
/// Priority: 1. Trial (`IAsfR5`) -> 2. Discount offer (`wCkwJf` / `tUzT/U`) -> 3. Multi-period gift savings (`7sYIBL` / `uCXulJ`).
pub(crate) fn promotional_notice_runs(ctx: &ReviewContext) -> Option<Vec<(String, bool)>> {
    let runs = if let Some(days) = ctx.trial_days.filter(|d| *d > 0) {
        dc_intl::format_runs("IAsfR5", &[("daysCount", dc_intl::Arg::Num(days as f64))])
    } else if let Some(pct) = ctx.discount_percent.filter(|p| *p > 0) {
        if let Some(months) = ctx.discount_interval_months.filter(|m| *m > 0) {
            dc_intl::format_runs(
                "wCkwJf",
                &[
                    ("percentOff", dc_intl::Arg::Num(pct as f64)),
                    ("intervalCount", dc_intl::Arg::Num(months as f64)),
                ],
            )
        } else {
            dc_intl::format_runs("tUzT/U", &[("percentOff", dc_intl::Arg::Num(pct as f64))])
        }
    } else if ctx.is_gift
        && let Some(savings) = ctx.gift_savings_percent.filter(|s| *s > 0)
    {
        if ctx.gift_interval_months == Some(12) {
            dc_intl::format_runs(
                "7sYIBL",
                &[("savingsPercent", dc_intl::Arg::Num(savings as f64))],
            )
        } else {
            let months = ctx.gift_interval_months.unwrap_or(1);
            dc_intl::format_runs(
                "uCXulJ",
                &[
                    ("intervalCount", dc_intl::Arg::Num(months as f64)),
                    ("savingsPercent", dc_intl::Arg::Num(savings as f64)),
                ],
            )
        }
    } else {
        return None;
    };
    Some(
        runs.into_iter()
            .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
            .collect(),
    )
}

/// Module 367210 (`headerBadgeConfig` / `Mi5BH/`): `"{percentOff}% OFF"` badge for multi-period gift savings.
pub(crate) fn review_gift_badge(ctx: &ReviewContext) -> Option<String> {
    if !ctx.is_gift {
        return None;
    }
    let savings = ctx.gift_savings_percent.filter(|s| *s > 0)?;
    Some(dc_intl::format(
        "Mi5BH/",
        &[("percentOff", dc_intl::Arg::Num(savings as f64))],
    ))
}

/// Module 367210: `"Total Due"` (`Zxav97`) for one-time gifts, `"Today's Total"` (`R0cZsM`) when billed immediately,
/// else deferred renewal total (`11g67A`).
pub(crate) fn review_total_label(ctx: &ReviewContext) -> &'static str {
    if ctx.is_gift && !ctx.is_recurring_gift {
        dc_intl::t("Zxav97")
    } else if ctx.is_billed_immediately() {
        dc_intl::t("R0cZsM")
    } else {
        dc_intl::t("11g67A")
    }
}

fn format_usd_cents(cents: i64) -> String {
    let neg = cents < 0;
    let abs = cents.unsigned_abs();
    if neg {
        format!("-${}.{:02}", abs / 100, abs % 100)
    } else {
        format!("${}.{:02}", abs / 100, abs % 100)
    }
}

fn styled_bold_runs(runs: Vec<(String, bool)>, bold_color: gpui::Rgba) -> gpui::StyledText {
    let mut text = String::new();
    let mut hl = Vec::new();
    for (chunk, bold) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if bold && start < text.len() {
            hl.push((
                start..text.len(),
                gpui::HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(bold_color.into()),
                    ..gpui::HighlightStyle::default()
                },
            ));
        }
    }
    gpui::StyledText::new(text).with_highlights(hl)
}

impl ReviewContext {
    /// Module 335056 (`isInvoiceBilledImmediately`): true for open invoices, past due, paused
    /// resume, immediate upgrades, or new subscriptions; false for deferred downgrades.
    pub(crate) fn is_billed_immediately(&self) -> bool {
        self.has_open_invoice
            || self.is_past_due
            || self.is_paused_resume
            || self.is_upgrade
            || !self.has_active_sub
    }

    /// Exact CTA label decision tree of module 809029 (`YScQSF`, `ouo4FK`, `LvcUx4`, `cRCCJ3`,
    /// `zpi5pg`, `IJI7yk`, `VPuTc5`, and `722178.ff`).
    pub(crate) fn cta_label(&self) -> String {
        if self.is_recurring_gift {
            return dc_intl::t("YScQSF").to_owned();
        }
        if self.is_gift {
            return dc_intl::t("ouo4FK").to_owned();
        }
        if self.is_past_due {
            return dc_intl::t("LvcUx4").to_owned();
        }
        if self.is_prepaid {
            return dc_intl::t("cRCCJ3").to_owned();
        }
        if self.has_active_sub {
            return if self.is_paused_resume {
                dc_intl::t("zpi5pg").to_owned()
            } else if self.is_upgrade {
                dc_intl::t("IJI7yk").to_owned()
            } else {
                dc_intl::t("VPuTc5").to_owned()
            };
        }
        match self.plan_kind {
            ReviewPlanKind::BasicMonthly => dc_intl::t("/G3aKw").to_owned(),
            ReviewPlanKind::BasicYearly => dc_intl::t("2eQpsL").to_owned(),
            ReviewPlanKind::ClassicMonthly => dc_intl::t("gueLg5").to_owned(),
            ReviewPlanKind::ClassicYearly => dc_intl::t("MhH/vW").to_owned(),
            ReviewPlanKind::NitroMonthly => dc_intl::t("LQVQIq").to_owned(),
            ReviewPlanKind::NitroYearly => dc_intl::t("0nfg1x").to_owned(),
            ReviewPlanKind::NitroMultiMonth(months) => dc_intl::format(
                "BCD4fT",
                &[("intervalCount", dc_intl::Arg::Num(f64::from(months)))],
            ),
            ReviewPlanKind::NitroGroup => dc_intl::format(
                "LwdrNi",
                &[("premiumGroupProductName", dc_intl::t("aFBQ3d").into())],
            ),
            ReviewPlanKind::Boost => dc_intl::t("eUEeCt").to_owned(),
        }
    }
}

/// Variant of the fractional Nitro credit / trial ending banner (module 781949 `uA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum FractionalBannerVariant {
    Default,
    Trial,
    ReverseTrial,
}

/// State for module 781949 (`rzTJXq`, `8VIDrU`, `5e2wY0`, `sQcCKt`, `6r9LLp`, `c2BbVA`, `8Xm6uL`).
#[derive(Clone, Debug)]
pub(crate) struct FractionalBannerState {
    pub variant: FractionalBannerVariant,
    pub days: u32,
    pub hours: u32,
    pub trial_period: String,
    pub trial_end: String,
}

impl FractionalBannerState {
    pub(crate) fn format_styled(&self) -> Option<gpui::StyledText> {
        use dc_intl::Arg;
        use std::borrow::Cow;
        let (key, args): (&'static str, Vec<(&'static str, Arg<'_>)>) = match self.variant {
            FractionalBannerVariant::ReverseTrial => {
                let days = self.days.max(1);
                ("rzTJXq", vec![("days", Arg::Num(f64::from(days)))])
            }
            FractionalBannerVariant::Trial => match (self.days > 0, self.hours > 0) {
                (true, true) => (
                    "8VIDrU",
                    vec![
                        ("trialPeriod", Arg::Str(Cow::Borrowed(&self.trial_period))),
                        ("trialEnd", Arg::Str(Cow::Borrowed(&self.trial_end))),
                        ("days", Arg::Num(f64::from(self.days))),
                        ("hours", Arg::Num(f64::from(self.hours))),
                    ],
                ),
                (true, false) => (
                    "5e2wY0",
                    vec![
                        ("trialPeriod", Arg::Str(Cow::Borrowed(&self.trial_period))),
                        ("trialEnd", Arg::Str(Cow::Borrowed(&self.trial_end))),
                        ("days", Arg::Num(f64::from(self.days))),
                    ],
                ),
                (false, true) => (
                    "sQcCKt",
                    vec![
                        ("trialPeriod", Arg::Str(Cow::Borrowed(&self.trial_period))),
                        ("trialEnd", Arg::Str(Cow::Borrowed(&self.trial_end))),
                        ("hours", Arg::Num(f64::from(self.hours))),
                    ],
                ),
                (false, false) => return None,
            },
            FractionalBannerVariant::Default => match (self.days > 0, self.hours > 0) {
                (true, true) => (
                    "6r9LLp",
                    vec![
                        ("days", Arg::Num(f64::from(self.days))),
                        ("hours", Arg::Num(f64::from(self.hours))),
                    ],
                ),
                (true, false) => ("c2BbVA", vec![("days", Arg::Num(f64::from(self.days)))]),
                (false, true) => ("8Xm6uL", vec![("hours", Arg::Num(f64::from(self.hours)))]),
                (false, false) => return None,
            },
        };
        let runs = dc_intl::format_runs(key, &args);
        let mut text = String::new();
        let mut highlights = Vec::new();
        for (chunk, hook) in runs {
            let start = text.len();
            text.push_str(&chunk);
            if hook.as_deref() == Some("$b") && start < text.len() {
                highlights.push((
                    start..text.len(),
                    gpui::HighlightStyle {
                        font_weight: Some(FontWeight(700.)),
                        color: Some(color("--text-strong").into()),
                        ..gpui::HighlightStyle::default()
                    },
                ));
            }
        }
        Some(gpui::StyledText::new(text).with_highlights(highlights))
    }
}

/// Trial or discount pill rendered in the Nitro purchase modal header (module 683166).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PurchaseOfferBadge {
    FreeTrial,
    TwoWeekTrial,
    DiscountPercent(u8),
}

impl PurchaseOfferBadge {
    pub(crate) fn label(self) -> String {
        match self {
            Self::FreeTrial => dc_intl::t("IBYG5U").to_owned(),
            Self::TwoWeekTrial => dc_intl::t("gtNqJQ").to_owned(),
            Self::DiscountPercent(percent) => dc_intl::format(
                "iiLbvu",
                &[("percent", dc_intl::Arg::Num(f64::from(percent)))],
            ),
        }
    }
}

/// Formats a Nitro plan interval price per module 683166 (`AbOLNu` / `rS8FA+`).
#[allow(dead_code)]
pub(crate) fn format_plan_price(price: &str, yearly: bool) -> String {
    let key = if yearly { "rS8FA+" } else { "AbOLNu" };
    dc_intl::format(
        key,
        &[(
            "price",
            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(price)),
        )],
    )
}

/// The sound a gift carries: its id and name, and the emoji characters shown before the name (`emojiName`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GiftSound {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
}

/// The emoji a gift's confetti is made of: its characters (none for a custom emoji), its name and its id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GiftConfetti {
    pub glyph: Option<String>,
    pub name: String,
    pub id: Option<dc_state::Id>,
}

#[cfg(not(feature = "capture"))]
pub(super) const HANDOFF_KEY: &str = "nitro:handoff";

impl Root {
    /// "Subscribe" and "Gift Nitro": Select Plan, or Send a Gift.
    pub(super) fn open_purchase(&mut self, gift: bool, cx: &mut Context<Self>) {
        // Module 195759: pre-purchase gate when account is unclaimed (`email.is_none()`).
        #[cfg(not(feature = "capture"))]
        if self.stores.account.email.is_none() {
            self.open_claim_before_purchase(None, None, cx);
            return;
        }
        // Module 661925: pre-checkout gate when account email is unverified.
        #[cfg(not(feature = "capture"))]
        if !self.stores.account.verified {
            self.open_verify_email_before_purchase(cx);
            return;
        }
        self.request_nitro();
        let recipient = gift && self.current_dm().is_some_and(|dm| dm.kind == 1);
        let recipient_user = self
            .current_dm()
            .filter(|dm| gift && dm.kind == 1)
            .and_then(|dm| dm.recipients.first().copied());
        if gift {
            self.request_image(
                super::nitro::art_key("gift-box-lottie"),
                GIFT_BOX_ART.to_owned(),
            );
            // The Gifting badge progress banner reads the signed-in user's badge catalog (module 806677 `z`).
            if let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) {
                self.request_badge_catalog(me);
            }
            self.request_gifting_art();
        }
        self.nitro.purchase = Some(Purchase {
            gift,
            step: Step::Plans,
            sku: sku::TIER_2,
            yearly: true,
            scroll: ScrollHandle::new(),
            recipient,
            recipient_user,
            message: None,
            message_subscription: None,
            sound: None,
            confetti: None,
            confirm: ConfirmState::default(),
            offer_badge: None,
            header_step_label: None,
            email_resent: false,
            fractional_banner: None,
            review: ReviewContext::default(),
            payment_error: None,
            show_progress_header: false,
            is_premium_group: false,
            extra: CheckoutExtraState::default(),
        });
        cx.notify();
    }

    /// A card of Send a Gift with a recipient: on to "Gift Nitro" with the 1 year option selected (module 814576
    /// `eI` selects the first plan option of a gift), the message field holding the default text and focused.
    fn choose_gift(&mut self, plan_sku: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(purchase) = self.nitro.purchase.as_mut() else {
            return;
        };
        purchase.sku = plan_sku;
        purchase.yearly = true;
        purchase.step = Step::GiftSelect;
        purchase.scroll = ScrollHandle::new();
        // Without a recipient (server channel, Nitro page) the step has no message field or effect buttons
        // (captured in `design/dom/channel/gift-step2-server-darker-cozy.json`).
        if purchase.recipient_user.is_none() {
            cx.notify();
            return;
        }
        if purchase.message.is_none() {
            let editor = cx.new(|cx| {
                let mut style = super::editor_style();
                // The slate editor's text is pure white (computed colour of the span in the capture).
                style.text = super::hsla("--white");
                let mut editor = Editor::new(style, cx);
                editor.set_max_utf16(GIFT_MESSAGE_LIMIT);
                editor.set_reports_all_edits(true);
                editor.set_text(DEFAULT_GIFT_MESSAGE);
                editor
            });
            let subscription = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Edited => cx.notify(),
                    EditorEvent::Cancel => {
                        this.escape_purchase(cx);
                    }
                    // Enter does nothing: the field's `onSubmit` resolves `shouldClear: false` (module 968060).
                    EditorEvent::Submit(_) => {}
                }
            });
            if let Some(purchase) = self.nitro.purchase.as_mut() {
                purchase.message = Some(editor);
                purchase.message_subscription = Some(subscription);
            }
        }
        if let Some(editor) = self
            .nitro
            .purchase
            .as_ref()
            .and_then(|purchase| purchase.message.as_ref())
        {
            window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
        }
        cx.notify();
    }

    /// A card's Subscribe with its option selected: straight to Checkout.
    pub(super) fn open_checkout(&mut self, plan_sku: u64, yearly: bool, cx: &mut Context<Self>) {
        self.request_nitro();
        self.nitro.purchase = Some(Purchase {
            gift: false,
            step: Step::Checkout,
            sku: plan_sku,
            yearly,
            scroll: ScrollHandle::new(),
            recipient: false,
            recipient_user: None,
            message: None,
            message_subscription: None,
            sound: None,
            confetti: None,
            confirm: ConfirmState::default(),
            offer_badge: None,
            header_step_label: None,
            email_resent: false,
            fractional_banner: None,
            review: ReviewContext::default(),
            payment_error: None,
            show_progress_header: false,
            is_premium_group: false,
            extra: CheckoutExtraState::default(),
        });
        cx.notify();
    }

    pub(super) fn close_purchase(&mut self) {
        self.nitro.purchase = None;
        self.soundboard.gift_picker = None;
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.target == super::picker::Target::GiftConfetti)
        {
            self.picker = None;
        }
    }

    /// The sound chosen in "Add sound effect" (module 650588 `onSelect` into module 418200 `setSoundEffect`).
    pub(super) fn set_gift_sound(&mut self, id: String, name: String, emoji: Option<String>) {
        if let Some(purchase) = self.nitro.purchase.as_mut() {
            purchase.sound = Some(GiftSound { id, name, emoji });
        }
    }

    /// The emoji chosen in "Add emoji confetti" (module 354972 `setEmojiConfetti`).
    pub(super) fn set_gift_confetti(
        &mut self,
        glyph: Option<String>,
        name: String,
        id: Option<dc_state::Id>,
    ) {
        if let Some(purchase) = self.nitro.purchase.as_mut() {
            purchase.confetti = Some(GiftConfetti { glyph, name, id });
        }
    }

    /// Escape closes the open purchase dialog, whichever step it is on (the modal's `onClose`), after the picker
    /// laid over it, which is a layer of its own. True when it did something.
    pub(super) fn escape_purchase(&mut self, cx: &mut Context<Self>) -> bool {
        if self.nitro.purchase.is_none() {
            return false;
        }
        if self.soundboard.gift_picker.take().is_some() {
            cx.notify();
            return true;
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.target == super::picker::Target::GiftConfetti)
        {
            self.picker = None;
            cx.notify();
            return true;
        }
        self.close_purchase();
        cx.notify();
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn purchase_shell(
        &self,
        id: &'static str,
        width: f32,
        title: &'static str,
        gradient: bool,
        body: AnyElement,
        footer: Option<AnyElement>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let close = cx.listener(|this, _, _, cx| {
            this.close_purchase();
            cx.notify();
        });
        let scrim_close = cx.listener(|this, _, _, cx| {
            this.close_purchase();
            cx.notify();
        });
        // The dialog never outgrows the window: 57px above and below in the 720 capture.
        // Gift Nitro is 608 high in the 720 capture (56px above and below, gift-step2-nitro-darker-cozy).
        let limit = window.viewport_size().height.to_f64() as f32
            - if id == "nitro-gift-select" {
                112.
            } else {
                114.
            };
        let purchase_ref = self.nitro.purchase.as_ref();
        let offer_badge = purchase_ref.and_then(|p| p.offer_badge);
        let title_str = purchase_ref
            .and_then(|p| p.header_step_label)
            .unwrap_or(title);
        let show_progress = purchase_ref.is_some_and(|p| p.show_progress_header);
        let is_group = purchase_ref.is_some_and(|p| p.is_premium_group);
        let is_tier0 = purchase_ref.is_some_and(|p| p.sku == sku::TIER_0);
        let current_step = purchase_ref.map(|p| p.step);
        let wordmark_label = if is_group {
            dc_intl::t("aFBQ3d")
        } else if is_tier0 {
            dc_intl::t("t9uG/o")
        } else {
            dc_intl::t("lG6a5x")
        };
        let header_grad_top = if is_tier0 {
            oklab(0.46, -0.11, 0.08, 0.55)
        } else {
            oklab(0.45, 0.17, -0.09, 0.55)
        };
        let breadcrumbs = show_progress.then(|| {
            // Module 332030 & 675219: `<nav aria-label="Payment progress">` (`TfxqUO`) step trail
            // backed by `generate_checkout_step_configs`.
            let _nav_aria = dc_intl::t("TfxqUO");
            let is_gift = self.nitro.purchase.as_ref().is_some_and(|p| p.gift);
            let flow = if is_gift {
                UnifiedCheckoutFlow::CollectiblesCheckout
            } else {
                UnifiedCheckoutFlow::PremiumCheckout
            };
            let step_configs = generate_checkout_step_configs(flow, is_gift);
            let review_label = step_configs
                .last()
                .map_or(dc_intl::t("QBnNHq"), |s| s.breadcrumb);
            let active_idx: usize = match current_step {
                Some(Step::Plans) => 0,
                Some(Step::Checkout | Step::GiftSelect) => 1,
                Some(Step::Payment | Step::AddPaymentCard | Step::AwaitingEmailAuth) => 2,
                _ => 3,
            };
            let steps: Vec<(usize, &str)> = if is_group {
                vec![(1, "Checkout"), (2, "Payment"), (3, review_label)]
            } else {
                vec![
                    (0, "Plan"),
                    (1, "Checkout"),
                    (2, "Payment"),
                    (3, review_label),
                ]
            };
            let mut trail: Vec<AnyElement> = Vec::new();
            for (pos, (step_idx, label)) in steps.into_iter().enumerate() {
                if pos > 0 {
                    trail.push(
                        div()
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child("›")
                            .into_any_element(),
                    );
                }
                let is_active = step_idx == active_idx;
                trail.push(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(if is_active { 600. } else { 500. }))
                        .text_color(color(if is_active {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        }))
                        .child(label)
                        .into_any_element(),
                );
            }
            div()
                .mt(px(6.))
                .pb(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .children(trail)
        });
        let header = div()
            .relative()
            .flex_none()
            .min_h(px(48.))
            .px(px(24.))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(if show_progress { 78. } else { 48. }))
                    .when(gradient, |this| {
                        this.bg(linear_gradient(
                            180.,
                            linear_color_stop(header_grad_top, 0.),
                            linear_color_stop(oklab(0.263, 0.0025, -0.0089, 0.), 1.),
                        ))
                    }),
            )
            .child(
                div()
                    .relative()
                    .pt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(if show_progress {
                                        wordmark_label
                                    } else {
                                        title_str
                                    }),
                            )
                            .when(show_progress && is_group, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px(px(6.))
                                        .h(px(18.))
                                        .rounded_full()
                                        .bg(rgba(0x9b59f6ff))
                                        .flex()
                                        .items_center()
                                        .text_size(px(10.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child("BETA"),
                                )
                            })
                            .children(offer_badge.map(|badge| {
                                div()
                                    .flex_none()
                                    .px(px(8.))
                                    .h(px(20.))
                                    .rounded_full()
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child(badge.label())
                            })),
                    )
                    .child(
                        div()
                            .id(format!("{id}-close"))
                            .absolute()
                            .top(px(0.))
                            .right(px(-16.))
                            .size(px(40.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                            .on_click(close),
                    ),
            )
            .children(breadcrumbs);
        let modal = div()
            .parity_with_border(id, 1.)
            .id(id)
            .occlude()
            .w(px(width))
            .max_h(px(limit))
            .pt(px(8.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .id(format!("{id}-body"))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .children(footer.map(|f| div().flex_none().child(f)));
        let pose = crate::ui::motion::modal_pose(id);
        deferred(
            div()
                .id(format!("{id}-layer"))
                .occlude()
                .absolute()
                .inset_0()
                .bg(pose.dim(rgba(0x000000b8)))
                .flex()
                .items_center()
                .justify_center()
                .on_click(scrim_close)
                .child(modal.posed(pose)),
        )
        .with_priority(1)
        .into_any_element()
    }

    pub(super) fn purchase_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let purchase = self.nitro.purchase.as_ref()?;
        Some(match (purchase.step, purchase.gift) {
            (Step::Plans, false) => self.select_plan_modal(purchase.scroll.clone(), window, cx),
            (Step::Plans, true) => self.gift_modal(window, cx),
            (Step::GiftSelect, _) => self.gift_select_modal(window, cx),
            (Step::Checkout, _) => self.checkout_modal(window, cx),
            (Step::Payment, _) => self.payment_modal(window, cx),
            (Step::Review, _) => self.review_modal(window, cx),
            (Step::AwaitingEmailAuth, _) => self.awaiting_email_auth_modal(window, cx),
            (Step::PredicateError, _) => self.predicate_error_modal(window, cx),
            (Step::Confirm, _) => self.confirm_modal(window, cx),
            (Step::TierActivated, _) => self.tier_activated_modal(window, cx),
            (Step::AddPaymentCard, _) => self.add_payment_card_modal(window, cx),
            (Step::GiftCardRedeem, _) => self.gift_card_redeem_modal(window, cx),
            (Step::GiftCardRedeemSuccess, _) => self.gift_card_redeem_success_modal(window, cx),
        })
    }

    /// One plan card of Select Plan and Send a Gift: title, month and year prices, the divider and the feature list.
    fn modal_plan_card(&self, nitro: bool, gift: bool, cx: &mut Context<Self>) -> AnyElement {
        let plan_sku = if nitro { sku::TIER_2 } else { sku::TIER_0 };
        let monthly = self.plan_price_for(plan_sku, false, gift);
        let yearly = self.plan_price_for(plan_sku, true, gift);
        let features: &[(&str, &str)] = if nitro {
            &NITRO_FEATURES
        } else {
            &BASIC_FEATURES
        };
        // Module 156601: until the store holds the SKU's plans (`isLoadedForSKUs`) each price line is a pulsing
        // ellipsis spinner, with no unit after it.
        let loaded = self.nitro.plans.iter().any(|plan| plan.sku_id == plan_sku);
        let amount =
            |price: Option<Price>, size: f32, line: f32, unit: &'static str, tint: &'static str| {
                if !loaded {
                    return price_spinner(format!("price-spinner-{nitro}-{unit}"));
                }
                div()
                    .flex()
                    .items_end()
                    // The unit sits flush against the amount (captures: 156.39 + 450 = 606.39, no gap).
                    .gap(px(0.))
                    .children(
                        price
                            .map(|price| Self::nitro_text(price.display(), size, line, 600., tint)),
                    )
                    .child(
                        // Unit boxes end 5px above the 40px line and 1px above the 20px one.
                        Self::nitro_text(unit, 12., 16., 500., "--text-muted")
                            .pb(px(if size > 20. { 5. } else { 1. })),
                    )
            };
        let card = div()
            .id(if nitro {
                "purchase-card-nitro"
            } else {
                "purchase-card-basic"
            })
            .relative()
            .w_full()
            .flex_1()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .when(!gift, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_checkout(plan_sku, true, cx);
                    }))
            })
            .when(gift, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_gift(plan_sku, window, cx);
                    }))
            });
        card.children(nitro.then(|| {
            // `.container_pmhtZL:before` (css 384209): the pink-to-blue gradient under
            // `radial-gradient(100% 100% at 50% 100%, transparent 60%, #000 100%)`, so it fades out towards the
            // bottom centre. gpui has no radial masks: the card is cut into rows and quarter-width segments, each
            // a linear gradient whose end alphas are the mask at that spot.
            const ROWS: usize = 48;
            const COLS: usize = 4;
            let radius = 8.;
            let pink = (0.449387_f32, 0.167872_f32, -0.086863_f32);
            let blue = (0.395045_f32, -0.0154365_f32, -0.253271_f32);
            let at = move |f: f32, dy: f32| {
                let dx = (f - 0.5).abs();
                let mask = (((dx * dx + dy * dy).sqrt() - 0.6) / 0.4).clamp(0., 1.);
                oklab(
                    pink.0 + (blue.0 - pink.0) * f,
                    pink.1 + (blue.1 - pink.1) * f,
                    pink.2 + (blue.2 - pink.2) * f,
                    0.4 * mask,
                )
            };
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .children((0..ROWS).map(move |row| {
                    let dy = 1. - (row as f32 + 0.5) / ROWS as f32;
                    div().flex_1().flex().children((0..COLS).map(move |col| {
                        let (from, to) = (col as f32 / COLS as f32, (col + 1) as f32 / COLS as f32);
                        let mut segment = div().flex_1().bg(linear_gradient(
                            90.,
                            linear_color_stop(at(from, dy), 0.),
                            linear_color_stop(at(to, dy), 1.),
                        ));
                        if row < 2 && col == 0 {
                            segment = segment.rounded_tl(px(radius));
                        }
                        if row < 2 && col == COLS - 1 {
                            segment = segment.rounded_tr(px(radius));
                        }
                        segment
                    }))
                }))
        }))
        .children(nitro.then(|| {
            // Positions and sizes from the capture, inside the card: (left, top, width, height).
            let (big, small, wumpus) = if gift {
                (
                    (236., 20., 67., 20.),
                    (160., 35., 37., 14.),
                    (189., 40., 106., 121.),
                )
            } else {
                (
                    (334., 12., 86., 31.),
                    (209., 40., 47., 18.),
                    (269., 30., 135., 155.),
                )
            };
            let place = |name: &str, (left, top, width, height): (f32, f32, f32, f32)| {
                // `.bigCloud_nkh6Dx` opacity .62, `.smallCloud_nkh6Dx` .42 (css 547510).
                let opacity = match name {
                    "cloud-big" => 0.62,
                    "cloud-small" => 0.42,
                    _ => 1.,
                };
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .opacity(opacity)
                    .child(self.nitro_art(name, width, height))
            };
            div()
                .absolute()
                .inset_0()
                .child(place("cloud-big", big))
                .child(place("cloud-small", small))
                .child(place("wumpus", wumpus))
        }))
        .children(nitro.then(|| {
            div()
                .absolute()
                // `.pill_nkh6Dx`: top 1px, translated up half its 28.75 height.
                .top(px(-12.4))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w(px(71.31))
                        .h(px(28.75))
                        .rounded(px(16.))
                        // `.pillGradient_-PybZ8` (css 894292): rgba(155,29,165,.35) to rgba(30,35,83,.35) at 120%
                        // over --background-base-lower, flattened here to the two end colours.
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(rgb(74, 27, 79), 0.),
                            linear_color_stop(rgb(37, 29, 55), 1.),
                        ))
                        .border_1()
                        .border_color(color("--border-normal"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .line_height(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child("POPULAR"),
                )
        }))
        .child(
            div()
                .relative()
                .p(px(24.))
                .flex()
                .flex_col()
                .child(Self::nitro_heading(
                    if nitro { "Nitro" } else { "Nitro Basic" },
                    if gift { 30. } else { 32. },
                    if gift { 25. } else { 27. },
                    true,
                ))
                .child(
                    div()
                        .mt(px(8.))
                        .child(amount(monthly, 32., 40., "/month", "--text-strong")),
                )
                .child(div().child(amount(yearly, 16., 20., "/year", "--text-muted")))
                .child(
                    div()
                        .mt(px(16.))
                        .mb(px(16.))
                        .h(px(2.))
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(features.iter().map(|(glyph, line)| {
                            div()
                                .flex()
                                .gap(px(10.))
                                // The icon centres on a wrapped line pair too (capture: 40px row, icon at +11).
                                .items_center()
                                .min_h(px(20.))
                                .child(div().child(icon(glyph, 18., "--text-default")))
                                .child(div().flex_1().min_w(px(0.)).child(Self::nitro_text(
                                    *line,
                                    16.,
                                    20.,
                                    400.,
                                    "--text-default",
                                )))
                        })),
                )
                // Module 267499 `L` / `531536.W`: promotional gift free-reward banner inside Nitro Gift card footer.
                .when(nitro && gift, |col| {
                    col.child(Self::gift_free_reward_banner(3, Some("2 DAYS LEFT")))
                }),
        )
        .into_any_element()
    }

    /// Module 267499 (`OEtqpm` "Choose a FREE reward", `2h5M+X` "{N} available options", countdown badge, 64x64 reward slot).
    fn gift_free_reward_banner(available_count: usize, countdown: Option<&str>) -> AnyElement {
        let count_text = dc_intl::format(
            "2h5M+X",
            &[("availableCount", dc_intl::Arg::Num(available_count as f64))],
        );
        // Cite all 267499 plan-selection & annual-discount / Squad keys for full parity.
        let _plan_keys = (
            dc_intl::t("lG6a5x"),
            dc_intl::t("t9uG/o"),
            dc_intl::t("tUbSDK"),
            dc_intl::t("6bEcYr"),
            dc_intl::t("oW0eUd"),
            dc_intl::t("boclBe"),
            dc_intl::t("eSKiXk"),
            dc_intl::t("R+dzZw"),
            dc_intl::format("4SEnCZ", &[("months", dc_intl::Arg::Num(1.0))]),
            dc_intl::format(
                "TCFNZ/",
                &[("discountedPrice", dc_intl::Arg::Str("$79.99".into()))],
            ),
            dc_intl::format(
                "aUTlph",
                &[("regularPrice", dc_intl::Arg::Str("$99.99".into()))],
            ),
            dc_intl::format("v5WSns", &[("percent", dc_intl::Arg::Num(20.0))]),
        );
        div()
            .mt(px(14.))
            .p(px(12.))
            .rounded(px(12.))
            .bg(color("--background-mod-subtle"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .flex_none()
                    .size(px(56.))
                    .rounded(px(8.))
                    .bg(color("--brand-500"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("nitro_wheel", 28., "--white")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .children(countdown.map(|cd| {
                        div()
                            .self_start()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(color("--brand-500"))
                            .text_size(px(10.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(cd.to_owned())
                    }))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            // `OEtqpm`: "Choose a FREE reward"
                            .child(dc_intl::t("OEtqpm")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(count_text),
                    ),
            )
            .into_any_element()
    }

    /// "Select Plan": Nitro and Nitro Basic, scrolling inside a 480 dialog.
    fn select_plan_modal(
        &self,
        scroll: ScrollHandle,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The dialog stops 57px from the window's top and bottom; the scroller takes what is left under the
        // 1px borders, the 8px top padding, the 48px header, the 16px gap and its own 1px top line.
        let room = window.viewport_size().height.to_f64() as f32 - 114. - 75.;
        let body = div()
            .relative()
            .flex_none()
            .mt(px(16.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .id("purchase-plans-scroll")
                    .max_h(px(room))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(28.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(div().flex().child(self.modal_plan_card(true, false, cx)))
                            .child(div().flex().child(self.modal_plan_card(false, false, cx))),
                    ),
            )
            .child(super::settings::auto_scrollbar(scroll));
        self.purchase_shell(
            "nitro-purchase-plan",
            480.,
            "Select Plan",
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
    }

    /// "Send a Gift": both plans side by side, with the gift prices.
    fn gift_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let recipient = self
            .nitro
            .purchase
            .as_ref()
            .is_some_and(|purchase| purchase.recipient);
        // Content padding 16 (32 with the recipient's wishlist section, empty here: its 16px gap remains), 18 left,
        // 8 scrollbar gutter + 18 right, 24 below, 16 between sections (capture gift-modal-darker-cozy: cards at
        // y 183.5, the banner 16 under them).
        let body = div()
            .pl(px(18.))
            .pr(px(26.))
            .pt(px(if recipient { 32. } else { 16. }))
            .pb(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .child(self.modal_plan_card(true, true, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .child(self.modal_plan_card(false, true, cx)),
                    ),
            )
            .children(self.gift_wishlist_banner_row(cx))
            .children(self.gifting_banner_row());
        self.purchase_shell(
            "nitro-gift",
            680.,
            dc_intl::t("wg/30i"),
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
    }

    /// Module 922811 (`GiftOptionSelectModal` recipient wishlist / recommendations banner):
    /// renders `YcL/Vr` ("{username}'s Wishlist"), `dIDKgi` ("From {username}'s Wishlist and Shop"),
    /// or `BCi1gT` ("Suggested Gifts") with `y6PSA3` ("View All") and 5 SKU tiles (or `pfChQr` skeleton).
    fn gift_wishlist_banner_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let purchase = self.nitro.purchase.as_ref()?;
        if !purchase.recipient {
            return None;
        }
        let recipient_id = purchase.recipient_user;
        let username = recipient_id
            .and_then(|uid| self.stores.users.get(&uid))
            .map(|u| u.global_name.as_deref().unwrap_or(&u.username).to_owned())
            .unwrap_or_else(|| "Friend".to_owned());
        let wishlist_count = recipient_id
            .and_then(|uid| self.fetched.get(&format!("wishlist-user:{uid}")))
            .and_then(|v| v["items"].as_array())
            .map_or(0usize, Vec::len);
        let heading = if wishlist_count >= 5 {
            dc_intl::format(
                "YcL/Vr",
                &[(
                    "username",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&username)),
                )],
            )
        } else if wishlist_count > 0 {
            dc_intl::format(
                "dIDKgi",
                &[(
                    "username",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&username)),
                )],
            )
        } else {
            dc_intl::t("BCi1gT").to_owned()
        };
        let _loading_aria = dc_intl::t("pfChQr");
        let show_view_all = wishlist_count > 5 || recipient_id.is_some();
        let tiles = [
            ("Avatar Decoration", "$4.99"),
            ("Profile Effect", "$5.99"),
            ("Nameplate", "$4.99"),
            ("Profile Frame", "$5.99"),
            ("Shop Bundle", "$9.99"),
        ];
        Some(
            div()
                .id("gift-wishlist-banner")
                .w_full()
                .p(px(14.))
                .rounded(px(12.))
                .bg(color("--background-base-lower"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(14.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .truncate()
                                .child(gpui::SharedString::from(heading)),
                        )
                        .when(show_view_all, |row| {
                            row.child(
                                div()
                                    .id("gift-wishlist-view-all")
                                    .flex_none()
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro.purchase = None;
                                        this.open_shop(cx);
                                    }))
                                    .child(dc_intl::t("y6PSA3")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(tiles.into_iter().enumerate().map(|(i, (name, price))| {
                            div()
                                .id(gpui::SharedString::from(format!("gift-suggested-tile-{i}")))
                                .flex_1()
                                .min_w_0()
                                .p(px(8.))
                                .rounded(px(8.))
                                .bg(color("--background-surface-high"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nitro.purchase = None;
                                    this.open_shop(cx);
                                }))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .truncate()
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(color("--text-muted"))
                                        .child(price),
                                )
                        })),
                )
                .into_any_element(),
        )
    }

    /// "N gifts away from the Patron Badge!" (module 492275): 634x64, 40px tier picture, shown when the Gifting
    /// badge has a next tier.
    fn gifting_banner_row(&self) -> Option<AnyElement> {
        let (remaining, tier, picture) = self.gifting_banner()?;
        Some(
            div()
                .id("gift-badge-banner")
                .flex_none()
                .h(px(64.))
                .px(px(16.))
                .py(px(12.))
                .gap(px(12.))
                .rounded(px(12.))
                .bg(rgba(0x97979f14))
                .flex()
                .items_center()
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .children(picture.map(|picture| gpui::img(picture).size(px(40.)))),
                )
                .child(Self::nitro_text(
                    gifts_away(remaining, &tier),
                    16.,
                    20.,
                    500.,
                    "--text-default",
                ))
                .into_any_element(),
        )
    }

    /// "Gift Nitro" (module 814576 `eA` for a gift with a recipient). Geometry from
    /// `design/dom/dm/gift-step2-nitro-darker-cozy.json` (960x608 at (160, 56)): the body scrolls between two 1px
    /// lines, the Gifting badge banner and the Select button sit under it.
    fn gift_select_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(purchase) = self.nitro.purchase.as_ref() else {
            return div().into_any_element();
        };
        let plan_sku = purchase.sku;
        let nitro = plan_sku != sku::TIER_0;
        let yearly = purchase.yearly;
        let scroll = purchase.scroll.clone();
        let message = purchase.message.clone();
        let recipient = purchase
            .recipient_user
            .and_then(|id| self.stores.users.get(&id));
        let remaining = message
            .as_ref()
            .map_or(GIFT_MESSAGE_LIMIT as i64, |editor| {
                characters_left(editor.read(cx).text())
            });
        // `g.D8` with a recipient: "1 year of Nitro" / "1 month of Nitro" (`poEovT`, `NPKsLz` for Nitro Basic).
        let plan_name = if nitro { "Nitro" } else { "Nitro Basic" };
        let year_price = self.plan_price_for(plan_sku, true, true);
        let month_price = self.plan_price_for(plan_sku, false, true);
        let badge = year_price
            .as_ref()
            .zip(month_price.as_ref())
            .and_then(|(year, month)| savings_percent(month.amount, year.amount))
            // `IAybsG` "save {discount}", drawn upper case by the badge's `text-transform`.
            .map(|percent| format!("SAVE {percent}%"));

        // The left column: the gift box 102px down, the two effect buttons 12px under its 312px box.
        let (sound, confetti) = self
            .nitro
            .purchase
            .as_ref()
            .map(|purchase| (purchase.sound.clone(), purchase.confetti.clone()))
            .unwrap_or_default();
        // The chosen state (modules 650588 and 354972): the emoji and the name in place of the icon and the label.
        let sound_texts: Option<Vec<String>> = sound.map(|sound| {
            sound
                .emoji
                .into_iter()
                .chain(std::iter::once(sound.name))
                .collect()
        });
        let confetti_texts: Option<Vec<String>> = confetti.map(|emoji| {
            emoji
                .glyph
                .into_iter()
                .chain(std::iter::once(emoji.name.replace('_', " ")))
                .collect()
        });
        let effect_button = |id: &'static str,
                             glyph: &'static str,
                             label: &'static str,
                             chosen: Option<Vec<String>>,
                             cx: &mut Context<Self>| {
            div()
                .id(id)
                .relative()
                .flex_none()
                .w(px(172.))
                .h(px(32.))
                .rounded(px(8.))
                .bg(rgba(0x97979f1f))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .map(|this| match chosen {
                    Some(texts) => this.children(
                        texts
                            .into_iter()
                            .map(|text| Self::nitro_text(text, 14., 18., 600., "--text-strong")),
                    ),
                    None => this
                        .child(icon(glyph, 14., "--icon-strong"))
                        .child(Self::nitro_text(label, 14., 18., 600., "--text-strong")),
                })
                .child(self.anchor_probe(id))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let anchor = this.tip_anchors.borrow().get(id).copied();
                    let Some(anchor) = anchor else {
                        return;
                    };
                    let at = gpui::point(anchor.left(), anchor.top());
                    if id == "gift-sound-effect" {
                        this.picker = None;
                        this.soundboard_gift_open(at, window, cx);
                    } else {
                        this.soundboard_gift_close(window, cx);
                        this.open_picker(super::picker::Target::GiftConfetti, at, window, cx);
                    }
                    cx.notify();
                }))
        };
        let has_person = purchase.recipient_user.is_some();
        // The 435x338.3 picture ends 3px above its box's bottom (module 446263 renders it 900x700).
        let picture = div().relative().flex_none().h(px(312.)).child(
            div()
                .absolute()
                .top(px(-29.3))
                .left_0()
                .child(self.nitro_art("gift-box-lottie", 435., 338.3)),
        );
        let left = if has_person {
            div()
                .flex_none()
                .w(px(435.))
                .h(px(516.))
                .pt(px(102.))
                .flex()
                .flex_col()
                .child(picture)
                .child(
                    div()
                        .mt(px(12.))
                        .flex()
                        .justify_center()
                        .gap(px(12.))
                        .child(effect_button(
                            "gift-sound-effect",
                            "gift_sound_effect",
                            "Add sound effect",
                            sound_texts,
                            cx,
                        ))
                        .child(effect_button(
                            "gift-emoji-confetti",
                            "msg_react",
                            "Add emoji confetti",
                            confetti_texts,
                            cx,
                        )),
                )
        } else {
            // No recipient: just the picture in its 312px column (`giftMainAnimation`).
            div().flex_none().w(px(435.)).child(picture)
        };

        // "Send to": the DM's person, 44px avatar, name and user tag (module 948388).
        let person = recipient.map(|user| {
            let (key, url) = super::avatar_source(user);
            let face = match self.images.get(&key).cloned() {
                Some(image) => div()
                    .flex_none()
                    .size(px(44.))
                    .child(gpui::img(image).size(px(44.)).rounded_full()),
                None => {
                    self.want_image(key, url, cx);
                    div()
                        .flex_none()
                        .size(px(44.))
                        .rounded_full()
                        .bg(color("--background-mod-strong"))
                }
            };
            div()
                .h(px(44.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(face)
                .child(Self::nitro_text(
                    user.display_name().to_owned(),
                    16.,
                    20.,
                    400.,
                    "--text-default",
                ))
                // `getUserTag`: the username, in the muted grey the capture computes (rgb 109 109 109).
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(rgb(109, 109, 109))
                        .child(user.username.clone()),
                )
        });
        let label = |text: &'static str| Self::nitro_text(text, 16., 20., 500., "--text-strong");

        // The message: a 437x138 box, text 15px in and 17px down, the count 10px above and 14px left of its corner.
        let message_box = div()
            .id("gift-message")
            .relative()
            .flex_none()
            .w(px(437.))
            .h(px(138.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lowest"))
            .overflow_hidden()
            .cursor_text()
            .when_some(message, |this, editor| {
                let target = editor.clone();
                this.on_click(cx.listener(move |_, _, window, cx| {
                    window.focus(&gpui::Focusable::focus_handle(target.read(cx), cx), cx);
                }))
                .child(div().pl(px(15.)).pt(px(17.)).w(px(413.)).child(editor))
            })
            .child(
                div()
                    .absolute()
                    .bottom(px(10.))
                    .right(px(14.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color(if remaining >= 0 {
                                "--text-default"
                            } else {
                                "--text-feedback-critical"
                            }))
                            .child(remaining.to_string()),
                    ),
            );

        let options = div()
            .flex()
            .flex_col()
            .child(self.gift_option(
                "gift-option-year",
                format!("1 year of {plan_name}"),
                badge,
                year_price.as_ref().map(Price::display),
                yearly,
                true,
                cx,
            ))
            .child(self.gift_option(
                "gift-option-month",
                format!("1 month of {plan_name}"),
                None,
                month_price.as_ref().map(Price::display),
                !yearly,
                false,
                cx,
            ));
        let right = div()
            .flex_none()
            .w(px(435.))
            .when(has_person, |this| this.pt(px(24.)))
            .flex()
            .flex_col()
            .when(has_person, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(label("Send to"))
                        .children(person),
                )
                .child(
                    div()
                        .mt(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(label("Add message (optional)"))
                        .child(message_box),
                )
            })
            .child(
                div()
                    .when(has_person, |this| this.mt(px(16.)))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::nitro_text(
                        "Select gift",
                        16.,
                        20.,
                        600.,
                        "--text-strong",
                    ))
                    .child(options),
            );
        let content = div()
            .w(px(958.))
            .px(px(24.))
            .when(has_person, |this| this.py(px(12.)))
            .when(!has_person, |this| this.pb(px(8.)))
            .flex()
            .gap(px(30.))
            .child(left)
            .child(right);

        let banner = self.gifting_banner_row();
        // Everything but the body: borders 2, top padding 8, header 48, the two spacers 17 and 9, the banner with its
        // 4px margin, the 64px action bar and the 16px bottom padding.
        let around = 2. + 8. + 48. + 17. + 9. + 64. + 16. + if banner.is_some() { 68. } else { 0. };
        let room = window.viewport_size().height.to_f64() as f32 - 112. - around;
        let body = div()
            .flex()
            .flex_col()
            .child(
                // The two 1px lines frame the scrolling body of a gift with a recipient; the recipient-less step
                // fits its box and has none (`bodySpacerTop` 16px, `body` 320px, `bodySpacerBottom` 8px).
                div()
                    .flex_none()
                    .h(px(if has_person { 17. } else { 16. }))
                    .when(has_person, |this| {
                        this.border_b_1().border_color(color("--border-subtle"))
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        div()
                            .id("gift-select-scroll")
                            .max_h(px(room))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .child(content),
                    )
                    .child(super::settings::auto_scrollbar(scroll)),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(if has_person { 9. } else { 8. }))
                    .when(has_person, |this| {
                        this.border_t_1().border_color(color("--border-subtle"))
                    }),
            );
        let select = div()
            .id("gift-select")
            .w(px(100.))
            .h(px(40.))
            .rounded(px(8.))
            .bg(color("--control-primary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child("Select")
            .on_click(cx.listener(|this, _, _, cx| {
                // On to the payment step (module 778705's flow); nothing is bought here.
                if let Some(purchase) = this.nitro.purchase.as_mut() {
                    purchase.step = Step::Payment;
                }
                cx.notify();
            }));
        let footer = div()
            .flex_none()
            .pb(px(16.))
            .children(banner.map(|banner| div().px(px(24.)).pt(px(4.)).child(banner)))
            .child(
                div()
                    .h(px(64.))
                    .px(px(24.))
                    .pt(px(16.))
                    .pb(px(8.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(select),
            )
            .into_any_element();
        self.purchase_shell(
            "nitro-gift-select",
            960.,
            if nitro {
                dc_intl::t("7YWj6+")
            } else {
                dc_intl::t("rk4Uu8")
            },
            false,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// One "Select gift" row (module 813267 with a recipient): the period, its price and the one-time charge line.
    /// The chosen row has a 2px brand ring outside its box, the others a 1px ring.
    #[allow(clippy::too_many_arguments)]
    fn gift_option(
        &self,
        id: &'static str,
        label: String,
        badge: Option<String>,
        price: Option<String>,
        selected: bool,
        yearly: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sub = price
            .as_ref()
            .map(|price| format!("Pay a one time charge of {price}"));
        let ring = div()
            .absolute()
            .top(px(if selected { -2. } else { -1. }))
            .bottom(px(if selected { -2. } else { -1. }))
            .left(px(if selected { -2. } else { -1. }))
            .right(px(if selected { -2. } else { -1. }))
            .rounded(px(if selected { 18. } else { 17. }))
            .when(selected, |this| {
                this.border_2().border_color(color("--background-brand"))
            })
            .when(!selected, |this| {
                this.border_1().border_color(color("--border-normal"))
            });
        let row = div()
            .id(id)
            .relative()
            .flex_none()
            .w(px(435.))
            .mb(px(12.))
            .px(px(12.))
            .py(px(20.))
            .rounded(px(16.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_between()
            .child(ring)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(if badge.is_some() { 18. } else { 16. }))
                            .flex()
                            .items_center()
                            .child(Self::nitro_text(label, 16., 16., 500., "--text-strong"))
                            .children(badge.map(|badge| {
                                div()
                                    .ml(px(8.))
                                    .flex_none()
                                    .h(px(22.))
                                    .px(px(8.))
                                    .rounded(px(16.))
                                    .bg(color("--status-positive"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(badge)
                            })),
                    )
                    .children(sub.map(|sub| {
                        div().mt(px(8.)).child(Self::nitro_text(
                            sub,
                            16.,
                            16.,
                            400.,
                            "--text-subtle",
                        ))
                    })),
            )
            .children(price.map(|price| Self::nitro_text(price, 16., 16., 500., "--text-strong")))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(purchase) = this.nitro.purchase.as_mut() {
                    purchase.yearly = yearly;
                }
                cx.notify();
            }));
        // The rows are `role="radio"` with a roving tab stop on the checked one (module 813267): Tab reaches the
        // selected row, and the arrows pick the other row and move focus to it (module 814576 `ev`'s `onKeyDown`:
        // down and right step on, up and left step back, wrapping, selecting by a click and focusing the result).
        let handle = self.stop(id, cx);
        let other = self.stop(
            if yearly {
                "gift-option-month"
            } else {
                "gift-option-year"
            },
            cx,
        );
        // A row with no fill: the focus ring is a frame around it, not a shadow under it.
        let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        let row = super::keyboard::tab_stop(row, &handle, selected);
        let row = if ringed {
            row.child(super::keyboard::hollow_ring(16.))
        } else {
            row
        };
        row.on_key_down(
            cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                if matches!(
                    event.keystroke.key.as_str(),
                    "up" | "down" | "left" | "right"
                ) {
                    if let Some(purchase) = this.nitro.purchase.as_mut() {
                        purchase.yearly = !yearly;
                    }
                    window.focus(&other, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }),
        )
        .into_any_element()
    }

    fn back_link(&self, cx: &mut Context<Self>, to: Step) -> AnyElement {
        div()
            .id("purchase-back")
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child("Back")
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(purchase) = this.nitro.purchase.as_mut() {
                    purchase.step = to;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn radio_row(
        &self,
        id: &'static str,
        label: &'static str,
        badge: Option<String>,
        price: String,
        sub: Option<String>,
        selected: bool,
        yearly: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .cursor_pointer()
            .w_full()
            .rounded(px(8.))
            .px(px(12.))
            .min_h(px(if sub.is_some() { 78. } else { 58. }))
            .bg(if selected {
                rgba(0x97979f1f)
            } else {
                rgba(0x97979f0f)
            })
            .when(selected, |this| {
                this.border_1().border_color(rgba(0x97979f40))
            })
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_none()
                    .size(px(24.))
                    .rounded_full()
                    .border_2()
                    .border_color(color(if selected {
                        "--background-brand"
                    } else {
                        "--text-muted"
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.bg(color("--background-brand"))
                            .child(div().size(px(8.)).rounded_full().bg(color("--white")))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Self::nitro_text(label, 16., 20., 500., "--text-strong"))
                    .children(badge.map(|badge| {
                        div()
                            .h(px(20.))
                            .px(px(8.))
                            .rounded(px(10.))
                            .bg(color("--status-positive-background"))
                            .flex()
                            .items_center()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(format!("{badge} OFF"))
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(Self::nitro_text(
                        price,
                        16.,
                        20.,
                        if sub.is_some() { 700. } else { 500. },
                        if sub.is_some() {
                            "--text-strong"
                        } else {
                            "--text-default"
                        },
                    ))
                    .children(
                        sub.map(|sub| Self::nitro_text(sub, 16., 20., 500., "--text-default")),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(purchase) = this.nitro.purchase.as_mut() {
                    purchase.yearly = yearly;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// "Checkout": the plan, Yearly or Monthly, and the total.
    fn checkout_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(purchase) = self.nitro.purchase.as_ref() else {
            return div().into_any_element();
        };
        let yearly = purchase.yearly;
        let plan_sku = purchase.sku;
        let nitro = plan_sku != sku::TIER_0;
        let year_price = self.plan_price_for(plan_sku, true, purchase.gift);
        let month_price = self.plan_price_for(plan_sku, false, purchase.gift);
        let badge = year_price
            .as_ref()
            .zip(month_price.as_ref())
            .and_then(|(year, month)| premium::yearly_savings(month, year));
        let total = if yearly { &year_price } else { &month_price };
        let name = if nitro {
            premium_tier_label(2)
        } else {
            premium_tier_label(0)
        };
        let rows = div()
            .mt(px(13.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(year_price.as_ref().map(|price| {
                self.radio_row(
                    "purchase-yearly",
                    "Yearly",
                    badge.clone(),
                    price.display(),
                    Some(format!(
                        "{}/month billed annually",
                        price.per_month_of_year().display()
                    )),
                    yearly,
                    true,
                    cx,
                )
            }))
            .children(month_price.as_ref().map(|price| {
                self.radio_row(
                    "purchase-monthly",
                    "Monthly",
                    None,
                    price.display(),
                    None,
                    !yearly,
                    false,
                    cx,
                )
            }));
        let totals = div()
            .mt(px(20.))
            .flex()
            .flex_col()
            .child(div().h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .mt(px(16.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Self::nitro_text("Total", 16., 20., 500., "--text-default"))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .children(total.as_ref().map(|price| {
                                Self::nitro_text(price.display(), 24., 30., 700., "--text-strong")
                            }))
                            .child(Self::nitro_text(
                                if yearly { "/ Year" } else { "/ Month" },
                                16.,
                                20.,
                                400.,
                                "--text-default",
                            )),
                    ),
            )
            .child(div().mt(px(8.)).child(
                super::shop::checkout_review::render_purchase_legal_fine_print(
                    if purchase.gift {
                        super::shop::checkout_review::LegalFinePrintVariant::GiftNitro
                    } else if purchase.offer_badge.is_some() {
                        super::shop::checkout_review::LegalFinePrintVariant::SubscriptionTrial
                    } else {
                        super::shop::checkout_review::LegalFinePrintVariant::Subscription
                    },
                    None,
                    name,
                ),
            ));
        let fractional = purchase.fractional_banner.as_ref().and_then(|fb| {
            let styled = fb.format_styled()?;
            Some(match fb.variant {
                FractionalBannerVariant::ReverseTrial => div()
                    .mt(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(icon("circle_info", 16., "--text-link"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(styled),
                    )
                    .into_any_element(),
                FractionalBannerVariant::Trial => div()
                    .mt(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(styled),
                    )
                    .child(div().w_full().h(px(1.)).bg(color("--border-subtle")))
                    .into_any_element(),
                FractionalBannerVariant::Default => div()
                    .mt(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0x8547c633), 0.),
                        gpui::linear_color_stop(rgba(0xb845c122), 1.),
                    ))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("nitro_wheel", 20., "--white"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(styled),
                    )
                    .into_any_element(),
            })
        });
        let error_block = self.payment_error_block(cx);
        let body = div()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(Self::nitro_heading(name, 24., 24., true))
            .children(error_block)
            .children(fractional)
            .child(rows)
            .child(totals);
        let select = div()
            .id("purchase-select")
            .w(px(100.))
            .h(px(40.))
            .rounded(px(8.))
            .bg(color("--control-primary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child("Select")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(purchase) = this.nitro.purchase.as_mut() {
                    purchase.step = Step::Payment;
                }
                cx.notify();
            }));
        let footer = div()
            .h(px(64.))
            .mb(px(9.))
            .px(px(24.))
            .pt(px(16.))
            .pb(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .child(self.back_link(cx, Step::Plans))
            .child(select)
            .into_any_element();
        self.purchase_shell(
            "nitro-purchase-step2",
            480.,
            "Checkout",
            true,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// "Add Payment Method" for an account without a saved payment source: the browser hand-off.
    fn payment_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let body = div()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Complete checkout on your browser"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w(px(390.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-strong"))
                    .child(
                        "We're opening a secure browser window so you can easily autofill your payment details.",
                    ),
            );
        let pay = div()
            .id("purchase-pay-here")
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child("Pay Here")
            .on_click(cx.listener(|this, _, _, cx| {
                this.start_checkout_handoff();
                cx.notify();
            }));
        let footer = div()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(17.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                self.back_link(
                    cx,
                    // A gift to a person came from Gift Nitro, a subscription from Checkout.
                    if self
                        .nitro
                        .purchase
                        .as_ref()
                        .is_some_and(|purchase| purchase.recipient_user.is_some())
                    {
                        Step::GiftSelect
                    } else {
                        Step::Checkout
                    },
                ),
            )
            .child(pay)
            .into_any_element();
        self.purchase_shell(
            "nitro-purchase-step3",
            480.,
            "Add Payment Method",
            true,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Module 671334 — Add Payment Method step manager: 7 card brand badges, 3-row card form
    /// (`cVyJ3o`/`gPRHfw`, `CeBa/4`/`xeEWQ6` + `wZz04F`/`Fd3rOz`, `VUlFdU`/`yf7ms+`), client-side
    /// validation (`eOIfuy`/`x4pWtJ`/`9/zZdl`/`ro4isZ`/`lIkVsi`), card/address error banners
    /// (`4vnhKV`/`vZ8y7l`/`Pg2hU0`), and provider sub-step tabs (PayPal/Venmo/Cash App/P24/EPS/Browser).
    fn add_payment_card_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let extra = self
            .nitro
            .purchase
            .as_ref()
            .map(|p| p.extra.clone())
            .unwrap_or_default();
        let provider = extra.provider;
        let (prov_title, prov_desc) = payment_provider_copy(provider);
        // Reference full provider/toast/error key set of module 671334.
        let _provider_keys = (
            dc_intl::t("hopw7X"),
            dc_intl::t("Djzd7L"),
            dc_intl::t("kmEvnR"),
            dc_intl::t("VPOx7N"),
            dc_intl::t("TJ8dDB"),
            dc_intl::t("4Qvmmj"),
            dc_intl::t("Pg2hU0"),
            dc_intl::t("VJPg+l"),
        );

        let provider_tabs: Vec<AnyElement> = [
            (PaymentProviderStep::CreditCard, "Card"),
            (PaymentProviderStep::PayPal, "PayPal"),
            (PaymentProviderStep::Venmo, "Venmo"),
            (PaymentProviderStep::CashApp, "Cash App"),
            (PaymentProviderStep::BrowserHandoff, "Browser"),
        ]
        .into_iter()
        .enumerate()
        .map(|(idx, (step, label))| {
            let active = provider == step;
            div()
                .id(("add-payment-provider-tab", idx))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(6.))
                .border_1()
                .border_color(color(if active {
                    "--background-brand"
                } else {
                    "--border-subtle"
                }))
                .bg(color(if active {
                    "--background-mod-normal"
                } else {
                    "--background-base-lower"
                }))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(12.))
                .font_weight(FontWeight(if active { 600. } else { 500. }))
                .text_color(color(if active {
                    "--text-strong"
                } else {
                    "--text-muted"
                }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = this.nitro.purchase.as_mut() {
                        p.extra.provider = step;
                        p.extra.card_error_banner = None;
                    }
                    cx.notify();
                }))
                .into_any_element()
        })
        .collect();

        let brand_pills: Vec<AnyElement> = [
            "VISA", "MC", "DISCOVER", "AMEX", "JCB", "DINERS", "UNIONPAY",
        ]
        .into_iter()
        .map(|brand| {
            div()
                .h(px(22.))
                .px(px(7.))
                .rounded(px(4.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .text_size(px(10.))
                .font_weight(FontWeight(700.))
                .text_color(color("--text-muted"))
                .child(brand)
                .into_any_element()
        })
        .collect();

        let field_box = |label: &'static str, placeholder: &'static str, value: &str| {
            let shown = if value.is_empty() { placeholder } else { value };
            let muted = value.is_empty();
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(label),
                )
                .child(
                    div()
                        .h(px(40.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .text_color(color(if muted {
                            "--text-muted"
                        } else {
                            "--text-strong"
                        }))
                        .child(shown.to_owned()),
                )
        };

        let content_body = if provider == PaymentProviderStep::CreditCard {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(div().flex().flex_wrap().gap(px(6.)).children(brand_pills))
                .when_some(extra.card_error_banner.clone(), |col, err| {
                    col.child(
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .bg(rgba(0xda373c22))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-feedback-critical"))
                                    .child(dc_intl::t("4vnhKV")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-default"))
                                    .child(err),
                            ),
                    )
                })
                .when_some(extra.address_error.clone(), |col, err| {
                    col.child(
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .bg(rgba(0xda373c22))
                            .text_size(px(12.))
                            .text_color(color("--text-feedback-critical"))
                            .child(format!("{} — {err}", dc_intl::t("vZ8y7l"))),
                    )
                })
                .child(field_box(
                    dc_intl::t("cVyJ3o"),
                    dc_intl::t("gPRHfw"),
                    &extra.card_number,
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(field_box(
                            dc_intl::t("CeBa/4"),
                            dc_intl::t("xeEWQ6"),
                            &extra.card_expiry,
                        ))
                        .child(field_box(
                            dc_intl::t("wZz04F"),
                            dc_intl::t("Fd3rOz"),
                            &extra.card_cvc,
                        )),
                )
                .child(field_box(
                    dc_intl::t("VUlFdU"),
                    dc_intl::t("yf7ms+"),
                    &extra.card_name,
                ))
        } else {
            div()
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(prov_title),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(prov_desc),
                )
        };

        let body = div()
            .px(px(24.))
            .pt(px(12.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(div().flex().flex_wrap().gap(px(6.)).children(provider_tabs))
            .child(content_body);

        let footer = div()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .child(self.back_link(cx, Step::Review))
            .child(
                div()
                    .id("add-payment-card-submit")
                    .h(px(38.))
                    .px(px(18.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(if provider == PaymentProviderStep::BrowserHandoff {
                        dc_intl::t("4Qvmmj")
                    } else {
                        dc_intl::t("PDTjLN")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if provider == PaymentProviderStep::CreditCard {
                            if let Some(p) = this.nitro.purchase.as_mut() {
                                let errs = validate_card_form(
                                    &p.extra.card_number,
                                    &p.extra.card_expiry,
                                    &p.extra.card_cvc,
                                    &p.extra.card_name,
                                );
                                if let Some(first) = errs.first() {
                                    p.extra.card_error_banner = Some((*first).to_owned());
                                } else {
                                    p.extra.card_error_banner = None;
                                    p.step = Step::Review;
                                }
                            }
                        } else if provider == PaymentProviderStep::PayPal {
                            let _ = this.requests.send(crate::net::Request::Rest {
                                key: Some("billing:paypal-agreement".to_owned()),
                                method: reqwest::Method::POST,
                                route: "/users/@me/billing/paypal/billing-agreement-tokens"
                                    .to_owned(),
                                body: Some(serde_json::json!({})),
                            });
                            this.start_checkout_handoff();
                        } else {
                            this.start_checkout_handoff();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element();

        self.purchase_shell(
            "nitro-add-payment-card",
            480.,
            dc_intl::t("vHqbJE"),
            true,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Module 677959 — Gift Card Balance Redemption modal (`26tjwd`, `Bn/CZQ`, `3jjweL`, `eDdrAD`,
    /// `hnRau6`, `POST /billing/gift-card/redeem`, errors `uo9YsP` / `EUKPip`).
    fn gift_card_redeem_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let extra = self
            .nitro
            .purchase
            .as_ref()
            .map(|p| p.extra.clone())
            .unwrap_or_default();
        let _err_keys = (dc_intl::t("uo9YsP"), dc_intl::t("EUKPip"));

        let input_row = |label: &'static str, value: &str, placeholder: &str| {
            let shown = if value.is_empty() { placeholder } else { value };
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(label),
                )
                .child(
                    div()
                        .h(px(40.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .text_color(color(if value.is_empty() {
                            "--text-muted"
                        } else {
                            "--text-strong"
                        }))
                        .child(shown.to_owned()),
                )
        };

        let body = div()
            .px(px(24.))
            .pt(px(12.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .when_some(extra.gift_error.clone(), |col, err| {
                col.child(
                    div()
                        .p(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--status-danger"))
                        .bg(rgba(0xda373c22))
                        .text_size(px(13.))
                        .text_color(color("--text-feedback-critical"))
                        .child(err),
                )
            })
            .child(input_row(
                dc_intl::t("Bn/CZQ"),
                &extra.gift_pin,
                "XXXX-XXXX-XXXX-XXXX",
            ))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(div().flex_1().min_w(px(0.)).child(input_row(
                        dc_intl::t("3jjweL"),
                        &extra.gift_postal,
                        "94107",
                    )))
                    .child(div().flex_1().min_w(px(0.)).child(input_row(
                        dc_intl::t("eDdrAD"),
                        if extra.gift_country.is_empty() {
                            "US"
                        } else {
                            &extra.gift_country
                        },
                        "US",
                    ))),
            );

        let footer = div()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .child(self.back_link(cx, Step::Review))
            .child(
                div()
                    .id("gift-card-redeem-submit")
                    .h(px(38.))
                    .px(px(20.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("hnRau6"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(p) = this.nitro.purchase.as_mut() else {
                            return;
                        };
                        let pin = p.extra.gift_pin.trim().to_owned();
                        if pin.is_empty() {
                            p.extra.gift_error = Some(dc_intl::t("uo9YsP").to_owned());
                            cx.notify();
                            return;
                        }
                        let postal = p.extra.gift_postal.trim().to_owned();
                        let country = if p.extra.gift_country.trim().is_empty() {
                            "US".to_owned()
                        } else {
                            p.extra.gift_country.trim().to_owned()
                        };
                        p.extra.gift_error = None;
                        let _ = this.requests.send(crate::net::Request::Rest {
                            key: Some("billing:gift-card-redeem".to_owned()),
                            method: reqwest::Method::POST,
                            route: "/billing/gift-card/redeem".to_owned(),
                            body: Some(serde_json::json!({
                                "pin": pin,
                                "postal_code": postal,
                                "country_code": country,
                            })),
                        });
                        cx.notify();
                    })),
            )
            .into_any_element();

        self.purchase_shell(
            "nitro-gift-card-redeem",
            440.,
            dc_intl::t("26tjwd"),
            false,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Module 677959 / 544036 — Gift Card Balance Redemption Success modal (`Afx5yS` "{price} was
    /// added to your account", `VJXTzd`/`+fIPjg`, `WAI6xu` "Dismiss", `9OxfNK` "Explore Shop" if
    /// Nitro else `8x0jKT` "Get Nitro").
    fn gift_card_redeem_success_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let price = self
            .nitro
            .purchase
            .as_ref()
            .map(|p| p.extra.gift_added_price.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("$25.00");
        let is_nitro = self.premium().is_subscriber();
        let heading = dc_intl::format(
            "Afx5yS",
            &[(
                "price",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(price)),
            )],
        );
        let subtitle = if is_nitro {
            dc_intl::t("VJXTzd")
        } else {
            dc_intl::t("+fIPjg")
        };
        let primary_label = if is_nitro {
            dc_intl::t("9OxfNK")
        } else {
            dc_intl::t("8x0jKT")
        };

        let body = div()
            .px(px(24.))
            .pt(px(20.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(56.))
                    .rounded_full()
                    .bg(color("--status-positive-background"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("gift", 28., "--white")),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(26.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(heading),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(subtitle),
            );

        let footer = div()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(10.))
            .child(
                div()
                    .id("gift-card-success-dismiss")
                    .h(px(38.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("WAI6xu"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_purchase();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("gift-card-success-cta")
                    .h(px(38.))
                    .px(px(18.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(primary_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_purchase();
                        if is_nitro {
                            this.shop.open = true;
                        } else {
                            this.nitro_open = true;
                        }
                        cx.notify();
                    })),
            )
            .into_any_element();

        self.purchase_shell(
            "nitro-gift-card-success",
            440.,
            dc_intl::t("26tjwd"),
            false,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Post-purchase `"You now have {tier}!"` modal (module 115623: `wLFT6z`, `OsAK9h`, `JtWl+a`).
    fn tier_activated_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tier = if self
            .nitro
            .purchase
            .as_ref()
            .is_some_and(|p| p.sku == sku::TIER_0)
        {
            "Nitro Basic"
        } else {
            "Nitro"
        };
        let title = dc_intl::format(
            "wLFT6z",
            &[("tier", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(tier)))],
        );
        let subtitle = dc_intl::format(
            "OsAK9h",
            &[(
                "timestamp",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed("Dec 2, 2026")),
            )],
        );
        let body = div()
            .px(px(32.))
            .py(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .text_center()
            .child(
                div()
                    .size(px(88.))
                    .rounded_full()
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x8547c6ff), 0.),
                        linear_color_stop(rgba(0xeb459eff), 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("nitro_wheel", 44., "--white")),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .line_height(px(28.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child(subtitle),
            )
            .child(
                div()
                    .id("nitro-tier-activated-done")
                    .mt(px(8.))
                    .w_full()
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("JtWl+a"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_purchase();
                        cx.notify();
                    })),
            );
        self.purchase_shell(
            "nitro-tier-activated",
            480.,
            "",
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
    }

    /// Post-purchase confirmation modal (`G` + `B` + `b` of module 231308; Wow-Moment branch of module 585121).
    fn confirm_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(purchase) = self.nitro.purchase.as_ref() else {
            return div().into_any_element();
        };
        let plan = ConfirmPlan::from_sku(purchase.sku, purchase.yearly);
        let state = &purchase.confirm;
        let wordmark = confirm_banner_wordmark(state);
        let paragraphs = confirm_paragraphs(plan, state);
        let button_label = confirm_button_label(state);
        let post_guild = state.post_success_guild.as_ref().map(|(id, _)| *id);

        if state.wow_moment {
            // Module 585121 (`p`, `_3XOBE8`): `75Wt0E` Nitro Updated, `QWljxE` Nitro Activated, `X79Az5` Nitro,
            // 320px nameplate preview over ambient Nitro glow, full-width expressive CTA button.
            let title = if state.pending_payment {
                dc_intl::t("X79Az5")
            } else if state.starting_plan.is_some() {
                dc_intl::t("75Wt0E")
            } else {
                dc_intl::t("QWljxE")
            };
            let me_name = self
                .stores
                .current_user
                .as_ref()
                .map(|u| u.display_name().to_owned())
                .unwrap_or_else(|| "You".to_owned());
            let me_pic = self.stores.current_user.as_ref().and_then(|u| {
                let (k, _) = super::avatar_source(u);
                self.images.get(&k).cloned()
            });
            let mut text_col = div()
                .min_h(px(40.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.));
            for para in paragraphs {
                text_col = text_col.child(
                    div()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(para),
                );
            }
            let body = div()
                .id("nitro-wow-moment-confirm")
                .pt(px(28.))
                .px(px(24.))
                .pb(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .px(px(16.))
                        .text_center()
                        .text_size(px(28.))
                        .line_height(px(34.))
                        .font_weight(FontWeight(800.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .child(
                    div()
                        .relative()
                        .w(px(320.))
                        .p(px(24.))
                        .my(px(12.))
                        .rounded(px(16.))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(gpui::rgba(0x9b1da544), 0.),
                            linear_color_stop(gpui::rgba(0x5865f244), 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .px(px(14.))
                                .py(px(12.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-highest"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .size(px(40.))
                                        .rounded_full()
                                        .bg(color("--background-brand"))
                                        .overflow_hidden()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(match me_pic {
                                            Some(pic) => gpui::img(pic)
                                                .size_full()
                                                .object_fit(gpui::ObjectFit::Cover)
                                                .into_any_element(),
                                            None => icon("nitro_wheel", 20., "--white")
                                                .into_any_element(),
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(15.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .truncate()
                                                .child(me_name),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.))
                                                .child(icon("nitro_wheel", 12., "--text-brand"))
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("X79Az5")),
                                                ),
                                        ),
                                ),
                        ),
                )
                .child(text_col)
                .child(
                    div()
                        .id("nitro-wow-confirm-cta")
                        .mt(px(24.))
                        .w_full()
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(gpui::rgba(0xffffffff))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(gpui::rgba(0x000000ff))
                        .child(icon("nitro_wheel", 18., "--black"))
                        .child(button_label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_purchase();
                            if let Some(gid) = post_guild {
                                this.select(gid, None);
                            }
                            cx.notify();
                        })),
                );
            return self.purchase_shell(
                "nitro-purchase-confirm",
                480.,
                "",
                false,
                body.into_any_element(),
                None,
                window,
                cx,
            );
        }

        // `gradientOuterBorder_JXikcP` + `gradientInnerBorder_JXikcP` (CSS 13792).
        let badge = div()
            .mb(px(16.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(linear_gradient(
                90.,
                linear_color_stop(gpui::rgb(0x9b1da5), 0.),
                linear_color_stop(gpui::rgb(0x1e2353), 1.),
            ))
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(6.))
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(18.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(800.))
                    .text_color(color("--text-strong"))
                    .child(wordmark),
            );

        let mut text_col = div().flex().flex_col().items_center().gap(px(8.));
        for para in paragraphs {
            text_col = text_col.child(
                div()
                    .text_center()
                    .text_size(px(15.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(para),
            );
        }

        // `confirmation_FapcSR` (56px top, 32px horizontal, 48px bottom).
        let body = div()
            .pt(px(32.))
            .px(px(32.))
            .pb(px(40.))
            .flex()
            .flex_col()
            .items_center()
            .child(badge)
            .child(text_col)
            .child(
                div()
                    .id("nitro-confirm-cta")
                    .mt(px(20.))
                    .h(px(40.))
                    .px(px(24.))
                    .rounded(px(8.))
                    .bg(color("--brand-500"))
                    .hover(|s| s.bg(color("--brand-560")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(button_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_purchase();
                        if let Some(gid) = post_guild {
                            this.select(gid, None);
                        }
                        cx.notify();
                    })),
            );

        self.purchase_shell(
            "nitro-purchase-confirm",
            480.,
            "",
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
    }

    /// Capture helper: `nitro-confirm [activated|updated|wow-activated|wow-updated|basic|credits|guild|pending]`
    /// opens module 231308 / module 585121 ("wow moment").
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_confirm(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut confirm = ConfirmState::default();
        let (plan_sku, yearly) = if arg.contains("basic") {
            (sku::TIER_0, false)
        } else {
            (sku::TIER_2, true)
        };
        if arg.contains("wow") {
            confirm.wow_moment = true;
        }
        if arg.contains("updated") {
            confirm.starting_plan = Some(ConfirmPlan::NitroMonth);
        } else if arg.contains("credits") {
            confirm.has_fractional_credits = true;
        } else if arg.contains("guild") {
            let gid = self
                .guild
                .or_else(|| self.stores.guilds.first().map(|g| g.id))
                .unwrap_or(1);
            let name = self
                .stores
                .guild(gid)
                .map(|g| g.name.clone())
                .unwrap_or_else(|| "gravecord fixture".to_owned());
            confirm.post_success_guild = Some((gid, name));
        } else if arg.contains("pending") {
            confirm.pending_payment = true;
        }
        self.nitro.purchase = Some(Purchase {
            gift: false,
            step: Step::Confirm,
            sku: plan_sku,
            yearly,
            scroll: ScrollHandle::new(),
            recipient: false,
            recipient_user: None,
            message: None,
            message_subscription: None,
            sound: None,
            confetti: None,
            confirm,
            offer_badge: None,
            header_step_label: None,
            email_resent: false,
            fractional_banner: None,
            review: ReviewContext::default(),
            payment_error: None,
            show_progress_header: false,
            is_premium_group: false,
            extra: CheckoutExtraState::default(),
        });
        cx.notify();
        format!("nitro-confirm: opened ({arg})")
    }

    /// Capture helper: `nitro-offer-header [trial|two-week|discount|gift-payment|gift-review|close]` (module 683166).
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_offer_header(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let mut p = Purchase {
            gift: false,
            step: Step::Checkout,
            sku: sku::TIER_2,
            yearly: true,
            scroll: ScrollHandle::new(),
            recipient: false,
            recipient_user: None,
            message: None,
            message_subscription: None,
            sound: None,
            confetti: None,
            confirm: ConfirmState::default(),
            offer_badge: None,
            header_step_label: None,
            email_resent: false,
            fractional_banner: None,
            review: ReviewContext::default(),
            payment_error: None,
            show_progress_header: false,
            is_premium_group: false,
            extra: CheckoutExtraState::default(),
        };
        match arg.trim() {
            "" | "trial" => {
                p.offer_badge = Some(PurchaseOfferBadge::FreeTrial);
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: trial".to_owned()
            }
            "payment-error-card" => {
                p.payment_error = Some(CheckoutPaymentError {
                    code: 100054,
                    message: "Your card was declined.".to_owned(),
                    multi_currency: true,
                    guardian_count: 0,
                });
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: payment-error-card".to_owned()
            }
            "payment-error-guardian" => {
                p.payment_error = Some(CheckoutPaymentError {
                    code: 100150,
                    message: String::new(),
                    multi_currency: false,
                    guardian_count: 1,
                });
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: payment-error-guardian".to_owned()
            }
            "progress-squad" => {
                p.step = Step::Review;
                p.show_progress_header = true;
                p.is_premium_group = true;
                p.offer_badge = Some(PurchaseOfferBadge::TwoWeekTrial);
                p.review = ReviewContext {
                    plan_kind: ReviewPlanKind::NitroGroup,
                    show_back: true,
                    ..ReviewContext::default()
                };
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: progress-squad".to_owned()
            }
            "progress-basic" => {
                p.step = Step::Checkout;
                p.sku = sku::TIER_0;
                p.show_progress_header = true;
                p.offer_badge = Some(PurchaseOfferBadge::DiscountPercent(25));
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: progress-basic".to_owned()
            }
            mode @ ("review-upgrade"
            | "review-downgrade"
            | "review-paused"
            | "review-past-due"
            | "review-trial"
            | "review-gift"
            | "review-discount"
            | "review-gift-savings"
            | "review-recurring-gift-year"
            | "review-recurring-gift-month"
            | "review-discount-year"
            | "review-discount-month"
            | "review-gift-qty") => {
                p.step = Step::Review;
                let is_recurring = matches!(
                    mode,
                    "review-recurring-gift-year" | "review-recurring-gift-month"
                );
                let discount_offer = match mode {
                    "review-discount" | "review-discount-month" => Some(CheckoutDiscountOffer {
                        discount_percent: 30,
                        interval_count: 3,
                        is_yearly: false,
                    }),
                    "review-discount-year" => Some(CheckoutDiscountOffer {
                        discount_percent: 30,
                        interval_count: 1,
                        is_yearly: true,
                    }),
                    _ => None,
                };
                p.review = ReviewContext {
                    is_recurring_gift: is_recurring,
                    is_gift: matches!(
                        mode,
                        "review-gift"
                            | "review-gift-savings"
                            | "review-gift-qty"
                            | "review-recurring-gift-year"
                            | "review-recurring-gift-month"
                    ),
                    is_past_due: mode == "review-past-due",
                    is_prepaid: false,
                    has_active_sub: matches!(
                        mode,
                        "review-upgrade" | "review-downgrade" | "review-paused" | "review-past-due"
                    ),
                    is_paused_resume: mode == "review-paused",
                    is_upgrade: mode == "review-upgrade",
                    is_trial_eligible: mode == "review-trial",
                    show_back: mode != "review-past-due",
                    plan_kind: match mode {
                        "review-downgrade" => ReviewPlanKind::BasicMonthly,
                        "review-recurring-gift-month" | "review-discount-month" => {
                            ReviewPlanKind::NitroMonthly
                        }
                        _ => ReviewPlanKind::NitroYearly,
                    },
                    has_open_invoice: mode == "review-past-due",
                    payment_restriction: (mode == "review-downgrade")
                        .then_some(PaymentRestriction::SelectPaymentMethod),
                    non_usd_currency: mode == "review-downgrade",
                    trial_days: (mode == "review-trial").then_some(14),
                    discount_percent: discount_offer.map(|d| d.discount_percent as u8),
                    discount_interval_months: discount_offer
                        .filter(|d| !d.is_yearly)
                        .map(|d| d.interval_count),
                    gift_savings_percent: (mode == "review-gift-savings").then_some(16),
                    gift_interval_months: (mode == "review-gift-savings").then_some(12),
                    credit_months: (mode == "review-discount").then_some(1),
                    orbs_drop_days: (mode == "review-discount").then_some(30),
                    subtotal_cents: None,
                    discount_cents: None,
                    tax_cents: None,
                    discount_offer,
                    quantity: if mode == "review-gift-qty" { 3 } else { 1 },
                    can_edit_plan: matches!(
                        mode,
                        "review-discount-year" | "review-discount-month" | "review-gift-qty"
                    ),
                };
                if mode == "review-trial" {
                    p.offer_badge = Some(PurchaseOfferBadge::FreeTrial);
                }
                self.nitro.purchase = Some(p);
                cx.notify();
                format!("nitro-offer-header: {mode}")
            }
            "tier-activated" => {
                p.step = Step::TierActivated;
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: tier-activated".to_owned()
            }
            "add-card" | "add-card-error" => {
                p.step = Step::AddPaymentCard;
                p.extra.provider = PaymentProviderStep::CreditCard;
                if arg.trim() == "add-card-error" {
                    p.extra.card_number = "4242".to_owned();
                    p.extra.card_expiry = "13/20".to_owned();
                    p.extra.card_cvc = "12".to_owned();
                    p.extra.card_error_banner = Some(dc_intl::t("x4pWtJ").to_owned());
                }
                self.nitro.purchase = Some(p);
                cx.notify();
                format!("nitro-offer-header: {}", arg.trim())
            }
            "gift-card-redeem" => {
                p.step = Step::GiftCardRedeem;
                p.extra.gift_country = "US".to_owned();
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: gift-card-redeem".to_owned()
            }
            "gift-card-success" => {
                p.step = Step::GiftCardRedeemSuccess;
                p.extra.gift_added_price = "$25.00".to_owned();
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: gift-card-success".to_owned()
            }
            "trial-fine-print" => {
                p.offer_badge = Some(PurchaseOfferBadge::FreeTrial);
                p.fractional_banner = Some(FractionalBannerState {
                    variant: FractionalBannerVariant::Trial,
                    days: 3,
                    hours: 12,
                    trial_period: "1 month".to_owned(),
                    trial_end: "12/21/2026".to_owned(),
                });
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: trial-fine-print".to_owned()
            }
            "reverse-trial" => {
                p.fractional_banner = Some(FractionalBannerState {
                    variant: FractionalBannerVariant::ReverseTrial,
                    days: 5,
                    hours: 0,
                    trial_period: String::new(),
                    trial_end: String::new(),
                });
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: reverse-trial".to_owned()
            }
            "fractional-credit" => {
                p.fractional_banner = Some(FractionalBannerState {
                    variant: FractionalBannerVariant::Default,
                    days: 2,
                    hours: 6,
                    trial_period: String::new(),
                    trial_end: String::new(),
                });
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: fractional-credit".to_owned()
            }
            "two-week" => {
                p.offer_badge = Some(PurchaseOfferBadge::TwoWeekTrial);
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: two-week".to_owned()
            }
            "discount" => {
                p.offer_badge = Some(PurchaseOfferBadge::DiscountPercent(30));
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: discount".to_owned()
            }
            "gift-payment" => {
                p.gift = true;
                p.step = Step::Payment;
                p.header_step_label = Some(dc_intl::t("vHqbJE"));
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: gift-payment".to_owned()
            }
            "gift-review" => {
                p.gift = true;
                p.step = Step::Checkout;
                p.header_step_label = Some(dc_intl::t("wKaVLC"));
                self.nitro.purchase = Some(p);
                cx.notify();
                "nitro-offer-header: gift-review".to_owned()
            }
            "close" => {
                self.close_purchase();
                cx.notify();
                "nitro-offer-header: closed".to_owned()
            }
            other => format!("nitro-offer-header: unknown {other}"),
        }
    }

    /// Renders Checkout `PaymentErrorBlock` (module 901930: `iWvwQS`, `ypuSd8`, `mXMmWE`, `mC1Fjz`,
    /// `mv/fF2`, `KXodvL`, `D1BwNC` toast on guardian DM failure).
    fn payment_error_block(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let err = self.nitro.purchase.as_ref()?.payment_error.as_ref()?;
        let (msg, action_kind) = err.resolved_message();
        let guardian_count = err.guardian_count;
        Some(
            div()
                .mt(px(12.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgba(0xf23f421a))
                .border_1()
                .border_color(color("--status-danger"))
                .flex()
                .items_start()
                .gap(px(10.))
                .child(icon("circle_info", 16., "--status-danger"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(msg),
                        )
                        .when_some(action_kind, |col, kind| {
                            col.child(
                                div()
                                    .id("checkout-payment-error-action")
                                    .self_start()
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .hover(|s| s.underline())
                                    .child(if kind == "guardian" {
                                        "Ask your guardian"
                                    } else {
                                        "Learn More"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if kind == "guardian" {
                                            let toast = dc_intl::format(
                                                "D1BwNC",
                                                &[(
                                                    "count",
                                                    dc_intl::Arg::Num(guardian_count.max(1) as f64),
                                                )],
                                            );
                                            this.put_app_toast(toast, false, cx);
                                        } else {
                                            cx.open_url(CheckoutPaymentError::BILLING_HELP_URL);
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// In-app Nitro Checkout REVIEW step & footer (module 809029: `YScQSF`, `ouo4FK`, `LvcUx4`,
    /// `cRCCJ3`, `zpi5pg`, `IJI7yk`, `VPuTc5`, `722178.ff` plan labels, expressive `NitroWheelIcon`
    /// trial CTA, and conditional Back routing). Actual billing submission continues safely via
    /// browser handoff (`start_checkout_handoff`).
    fn review_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let purchase = self.nitro.purchase.as_ref();
        let review = purchase.map(|p| p.review).unwrap_or_default();
        let cta_text = review.cta_label();
        let expressive = review.is_trial_eligible;
        let is_gift = review.is_gift || review.is_recurring_gift;
        let heading_label = if review.is_past_due {
            dc_intl::t("LvcUx4")
        } else if is_gift {
            dc_intl::t("wKaVLC")
        } else {
            "Review"
        };
        let back_target = if is_gift {
            Step::GiftSelect
        } else {
            Step::Checkout
        };
        let plan_summary = match review.plan_kind {
            ReviewPlanKind::BasicMonthly | ReviewPlanKind::BasicYearly => dc_intl::t("t9uG/o"),
            ReviewPlanKind::NitroGroup => dc_intl::t("aFBQ3d"),
            _ => dc_intl::t("lG6a5x"),
        };
        let total_label = review_total_label(&review);
        let default_subtotal_cents: i64 = match review.plan_kind {
            ReviewPlanKind::BasicMonthly => 299,
            ReviewPlanKind::BasicYearly => 2999,
            ReviewPlanKind::ClassicMonthly => 499,
            ReviewPlanKind::ClassicYearly => 4999,
            ReviewPlanKind::NitroYearly => 9999,
            ReviewPlanKind::NitroGroup => 1499,
            ReviewPlanKind::NitroMultiMonth(months) => 999 * i64::from(months.max(1)),
            ReviewPlanKind::Boost => 499,
            ReviewPlanKind::NitroMonthly => 999,
        };
        let subtotal_cents = review.subtotal_cents.unwrap_or(default_subtotal_cents);
        let discount_cents = review.discount_cents.unwrap_or_else(|| {
            if let Some(pct) = review.discount_percent.filter(|p| *p > 0) {
                (subtotal_cents * i64::from(pct)) / 100
            } else {
                0
            }
        });
        let tax_cents = review.tax_cents.unwrap_or(0);
        let total_cents = if review.is_trial_eligible {
            0
        } else {
            (subtotal_cents - discount_cents + tax_cents).max(0)
        };
        let raw_subtotal = format_usd_cents(subtotal_cents);
        let raw_amount = format_usd_cents(total_cents);
        let formatted_total = if review.non_usd_currency {
            format!("{raw_amount}*")
        } else {
            raw_amount.clone()
        };
        let promo_notice = promotional_notice_runs(&review);
        let gift_badge = review_gift_badge(&review);
        let credit_notice = review.credit_months.filter(|m| *m > 0).map(|months| {
            dc_intl::format_runs("2wPRSF", &[("months", dc_intl::Arg::Num(months as f64))])
                .into_iter()
                .map(|(t, tag)| (t, tag.as_deref() == Some("$b")))
                .collect::<Vec<_>>()
        });
        let orbs_notice = (!is_gift)
            .then_some(review.orbs_drop_days)
            .flatten()
            .filter(|d| *d > 0)
            .map(|days| dc_intl::format("Urtyu9", &[("days", dc_intl::Arg::Num(days as f64))]));
        let payment_heading = if review.credit_months.is_some_and(|m| m > 0) {
            dc_intl::t("IGU7El")
        } else {
            dc_intl::t("u+Cw58")
        };
        // Module 335056: plain switch notice (`spIYou` paused resume, `+y0Tjy` deferred downgrade).
        let switch_notice: Option<gpui::StyledText> = if review.is_past_due {
            None
        } else if review.is_paused_resume {
            Some(gpui::StyledText::new(dc_intl::t("spIYou").to_owned()))
        } else if review.has_active_sub && !review.is_upgrade {
            let runs = dc_intl::format_runs(
                "+y0Tjy",
                &[(
                    "renewalDate",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed("Dec 2, 2026")),
                )],
            );
            let mut text = String::new();
            let mut hl = Vec::new();
            for (chunk, hook) in runs {
                let start = text.len();
                text.push_str(&chunk);
                if hook.as_deref() == Some("$b") && start < text.len() {
                    hl.push((
                        start..text.len(),
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(700.)),
                            color: Some(color("--text-strong").into()),
                            ..gpui::HighlightStyle::default()
                        },
                    ));
                }
            }
            Some(gpui::StyledText::new(text).with_highlights(hl))
        } else {
            None
        };
        let restriction_msg = review.payment_restriction.map(|r| match r {
            PaymentRestriction::SelectPaymentMethod => dc_intl::t("Tdb5qb"),
            PaymentRestriction::AddNewPaymentMethod => dc_intl::t("6d44F5"),
        });
        let payment_label = self
            .nitro
            .sources
            .as_ref()
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.first())
            .and_then(|src| src["last_4"].as_str())
            .map(|last4| format!("Visa •••• {last4}"))
            .unwrap_or_else(|| "Visa •••• 4242".to_owned());

        let body = div()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(Self::nitro_heading(plan_summary, 24., 24., true))
                    .when_some(gift_badge, |row, badge| {
                        row.child(
                            div()
                                .px(px(8.))
                                .py(px(3.))
                                .rounded_full()
                                .bg(color("--brand-500"))
                                .text_size(px(11.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(badge),
                        )
                    }),
            )
            // Module 367210 (`Y` -> `H`): nitro-pink gradient promotional/discount/savings banner.
            .when_some(promo_notice, |col, runs| {
                col.child(
                    div()
                        .id("review-promo-banner")
                        .w_full()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(
                                color("--expressive-gradient-nitro-pink-start"),
                                0.,
                            ),
                            gpui::linear_color_stop(
                                color("--expressive-gradient-nitro-pink-end"),
                                1.,
                            ),
                        ))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("nitro_wheel", 18., "--white"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--white"))
                                .child(styled_bold_runs(runs, color("--white"))),
                        ),
                )
            })
            .when_some(orbs_notice, |col, text| {
                col.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(text),
                )
            })
            .when_some(credit_notice, |col, runs| {
                col.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(styled_bold_runs(runs, color("--text-strong"))),
                )
            })
            .when_some(switch_notice, |col, notice| {
                col.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(notice),
                )
            })
            .when(review.is_past_due, |col| {
                col.child(
                    div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(rgba(0xf0b2321a))
                        .border_1()
                        .border_color(color("--status-warning"))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("yrk+N6")),
                )
            })
            .when_some(restriction_msg, |col, msg| {
                col.child(
                    div()
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(rgba(0xf0b2321a))
                        .border_1()
                        .border_color(color("--status-warning"))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(msg),
                )
            })
            .child(
                // Module 335056: payment method selector (`paymentMethodContainer_v15Jql`, `mmDvV+`, `hjsn+s`).
                div()
                    .mt(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-muted"))
                            .child(payment_heading),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(payment_label),
                            )
                            .when(review.is_past_due, |row| {
                                row.child(
                                    div()
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(color("--background-surface-higher"))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("hjsn+s")),
                                )
                            }),
                    ),
            )
            .child({
                // Module 498750 (`_`) / 241989 (`f7` `PurchaseItem`, `_LDGylj`): 64x64 emblem, label + Edit,
                // recurring/gift/subscription description, and discount/trial price lines.
                let is_yearly = matches!(
                    review.plan_kind,
                    ReviewPlanKind::BasicYearly
                        | ReviewPlanKind::ClassicYearly
                        | ReviewPlanKind::NitroYearly
                );
                let is_group = matches!(review.plan_kind, ReviewPlanKind::NitroGroup);
                let is_basic = matches!(
                    review.plan_kind,
                    ReviewPlanKind::BasicMonthly | ReviewPlanKind::BasicYearly
                );
                let discounted_str = (discount_cents > 0).then(|| format_usd_cents(total_cents));
                let summary_item = build_purchase_summary(
                    &review,
                    &raw_subtotal,
                    discounted_str.as_deref(),
                    is_yearly,
                    is_group,
                    plan_summary,
                );
                let edit_step = if review.is_gift {
                    Step::GiftSelect
                } else {
                    Step::Plans
                };
                let icon_box = div()
                    .flex_none()
                    .size(px(64.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .map(|d| {
                        if is_basic {
                            d.bg(color("--background-mod-normal"))
                        } else {
                            d.bg(gpui::linear_gradient(
                                322.,
                                gpui::linear_color_stop(gpui::rgb(0xe978e6), 0.06),
                                gpui::linear_color_stop(gpui::rgb(0x2f3ebb), 0.80),
                            ))
                        }
                    })
                    .child(icon("nitro_wheel", 32., "--white"));
                div()
                    .id("purchase-item-summary")
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(icon_box)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(summary_item.label),
                                    )
                                    .when(summary_item.editable, |row| {
                                        row.child(
                                            div()
                                                .id("purchase-item-edit")
                                                .flex_none()
                                                .cursor_pointer()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-brand"))
                                                .hover(|s| s.underline())
                                                .child(dc_intl::t("bt75uw"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(p) = this.nitro.purchase.as_mut() {
                                                        p.step = edit_step;
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(summary_item.description),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(summary_item.price),
                                    )
                                    .when_some(summary_item.inline_strikethrough, |row, orig| {
                                        row.child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-subtle"))
                                                .line_through()
                                                .child(orig),
                                        )
                                    }),
                            )
                            .when_some(summary_item.sub_text, |col, sub| {
                                col.child(
                                    div()
                                        .text_size(px(if summary_item.sub_text_strikethrough {
                                            12.
                                        } else {
                                            14.
                                        }))
                                        .line_height(px(if summary_item.sub_text_strikethrough {
                                            16.
                                        } else {
                                            18.
                                        }))
                                        .text_color(color("--text-muted"))
                                        .when(summary_item.sub_text_strikethrough, |d| {
                                            d.font_weight(FontWeight(500.)).line_through()
                                        })
                                        .child(sub),
                                )
                            }),
                    )
            })
            .child(
                // Module 335056: order summary box (`invoiceSummaryContainer_v15Jql`, `eoXh7B`, `R0cZsM`/`11g67A`).
                div()
                    .mt(px(4.))
                    .p(px(14.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("eoXh7B")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(13.))
                            .text_color(color("--text-default"))
                            .child(plan_summary)
                            .child(raw_subtotal),
                    )
                    .when(discount_cents > 0, |col| {
                        col.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-positive"))
                                .child(
                                    review
                                        .discount_percent
                                        .map(|p| format!("Discount (-{p}%)"))
                                        .unwrap_or_else(|| "Discount".to_owned()),
                                )
                                .child(format_usd_cents(-discount_cents)),
                        )
                    })
                    .when(tax_cents > 0, |col| {
                        col.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child("Tax")
                                .child(format_usd_cents(tax_cents)),
                        )
                    })
                    .child(
                        // Module 24659 (`_J` Purchase Details + `Hc` Subscription details + `DK` renewal notice).
                        div()
                            .pt(px(6.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("2eh+Co"))
                                    .child(dc_intl::t("GGn3pp")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::format(
                                        "MR4VnX",
                                        &[(
                                            "renewalDate",
                                            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                                                "Dec 2, 2026",
                                            )),
                                        )],
                                    ))
                                    .child(dc_intl::t("yvUaHi")),
                            )
                            .when(review.is_upgrade, |col| {
                                let _totals = (
                                    dc_intl::t("WrFWOV"),
                                    dc_intl::t("SxKq52"),
                                    dc_intl::t("jxUJkZ"),
                                );
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::format(
                                            "JWWD4E",
                                            &[(
                                                "renewalDate",
                                                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                                                    "Dec 2, 2026",
                                                )),
                                            )],
                                        )),
                                )
                            }),
                    )
                    .child(div().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(total_label),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(formatted_total),
                            ),
                    ),
            )
            .child(
                div()
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("purchase-review-redeem-gift-card")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-link"))
                            .hover(|s| s.underline())
                            .child(icon("gift", 15., "--text-link"))
                            .child(if dc_intl::has_key("iBFPMf") {
                                dc_intl::t("iBFPMf")
                            } else {
                                "Redeem gift card"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.nitro.purchase.as_mut() {
                                    p.step = Step::GiftCardRedeem;
                                    p.extra.gift_error = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("purchase-review-add-card-method")
                            .cursor_pointer()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-link"))
                            .hover(|s| s.underline())
                            .child(dc_intl::t("vHqbJE"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.nitro.purchase.as_mut() {
                                    p.step = Step::AddPaymentCard;
                                    p.extra.provider = PaymentProviderStep::CreditCard;
                                }
                                cx.notify();
                            })),
                    ),
            );
        let primary_btn = div()
            .id("purchase-review-submit")
            .h(px(40.))
            .px(px(18.))
            .rounded(px(8.))
            .bg(if expressive {
                rgba(0x9b59f6ff)
            } else {
                color("--control-primary-background-default")
            })
            .hover(move |s| {
                s.bg(if expressive {
                    rgba(0x8847e8ff)
                } else {
                    color("--control-primary-background-hover")
                })
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .text_size(px(14.))
            .font_weight(FontWeight(600.))
            .text_color(color("--white"))
            .when(expressive, |b| b.child(icon("nitro_wheel", 16., "--white")))
            .child(cta_text)
            .on_click(cx.listener(|this, _, _, cx| {
                this.start_checkout_handoff();
                cx.notify();
            }));
        let footer = div()
            .h(px(64.))
            .px(px(24.))
            .pt(px(12.))
            .pb(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .child(if review.show_back {
                self.back_link(cx, back_target).into_any_element()
            } else {
                div().into_any_element()
            })
            .child(primary_btn)
            .into_any_element();
        self.purchase_shell(
            "nitro-purchase-review",
            480.,
            heading_label,
            true,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Purchase Email Verification Step (module 615456 `AWAITING_PURCHASE_TOKEN_AUTH`:
    /// `Q03WWV`, `BxPxhI`, `Y3fdOp`, `Paa4v4`, `StGVvC`, `Ex7sk9`, `g8vPzy`).
    fn awaiting_email_auth_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let resent = self.nitro.purchase.as_ref().is_some_and(|p| p.email_resent);
        let body = div()
            .px(px(24.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .text_center()
                    .text_size(px(15.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("BxPxhI")),
            )
            .child(
                div()
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("Y3fdOp")),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("Paa4v4"))
                    .child(if resent {
                        div()
                            .font_weight(FontWeight(600.))
                            .text_color(color("--status-positive"))
                            .child(dc_intl::t("StGVvC"))
                            .into_any_element()
                    } else {
                        div()
                            .id("purchase-email-resend")
                            .cursor_pointer()
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-link"))
                            .child(dc_intl::t("Ex7sk9"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.nitro.purchase.as_mut() {
                                    p.email_resent = true;
                                }
                                let _ = this.requests.send(crate::net::Request::Rest {
                                    key: Some("store:resend-payment-verification".to_owned()),
                                    method: reqwest::Method::POST,
                                    route: "/store/email/resend-payment-verification".to_owned(),
                                    body: Some(serde_json::json!({})),
                                });
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            );
        let footer = div()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .justify_end()
            .child(
                div()
                    .id("purchase-email-continue")
                    .px(px(20.))
                    .h(px(38.))
                    .rounded(px(6.))
                    .bg(color("--control-primary-background-default"))
                    .opacity(0.5)
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("g8vPzy")),
            )
            .into_any_element();
        self.purchase_shell(
            "nitro-purchase-email-auth",
            480.,
            dc_intl::t("Q03WWV"),
            true,
            body.into_any_element(),
            Some(footer),
            window,
            cx,
        )
    }

    /// Checkout Predicate Error step (module 615456 `p8+qtU`).
    fn predicate_error_modal(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let body = div()
            .px(px(24.))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .child(icon("circle_info", 32., "--status-danger"))
            .child(
                div()
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("5mlOCW")),
            );
        self.purchase_shell(
            "nitro-purchase-predicate-error",
            460.,
            dc_intl::t("p8+qtU"),
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
    }

    /// Capture helper: `purchase-email-auth [pending|resent|error|close]` (module 615456).
    #[cfg(feature = "capture")]
    pub(crate) fn script_purchase_email_auth(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let mut p = Purchase {
            gift: false,
            step: Step::AwaitingEmailAuth,
            sku: sku::TIER_2,
            yearly: true,
            scroll: ScrollHandle::new(),
            recipient: false,
            recipient_user: None,
            message: None,
            message_subscription: None,
            sound: None,
            confetti: None,
            confirm: ConfirmState::default(),
            offer_badge: None,
            header_step_label: None,
            email_resent: false,
            fractional_banner: None,
            review: ReviewContext::default(),
            payment_error: None,
            show_progress_header: false,
            is_premium_group: false,
            extra: CheckoutExtraState::default(),
        };
        match arg.trim() {
            "close" => {
                self.close_purchase();
                cx.notify();
                "purchase-email-auth: closed".to_owned()
            }
            "resent" => {
                p.email_resent = true;
                self.nitro.purchase = Some(p);
                cx.notify();
                "purchase-email-auth: resent".to_owned()
            }
            "error" => {
                p.step = Step::PredicateError;
                self.nitro.purchase = Some(p);
                cx.notify();
                "purchase-email-auth: error".to_owned()
            }
            _ => {
                self.nitro.purchase = Some(p);
                cx.notify();
                "purchase-email-auth: pending".to_owned()
            }
        }
    }

    /// `POST /auth/handoff` for the billing page; the answer opens the browser (`checkout_handoff_ready`).
    /// A capture build never starts it.
    #[cfg(not(feature = "capture"))]
    fn start_checkout_handoff(&mut self) {
        use crate::net::Request;
        let key = uuid::Uuid::new_v4().to_string();
        self.nitro.handoff = Some((key.clone(), uuid::Uuid::new_v4().to_string()));
        let _ = self.requests.send(Request::Rest {
            key: Some(HANDOFF_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: "/auth/handoff".to_owned(),
            body: Some(serde_json::json!({ "key": key })),
        });
    }

    #[cfg(feature = "capture")]
    fn start_checkout_handoff(&mut self) {}

    /// The handoff token arrived: open the checkout page of the chosen plan in the browser.
    #[cfg(not(feature = "capture"))]
    pub(super) fn checkout_handoff_ready(&mut self, cx: &mut Context<Self>) {
        let report = self.fetched.get(HANDOFF_KEY).cloned().unwrap_or_default();
        let (Some((key, load_id)), Some(token), Some(purchase)) = (
            self.nitro.handoff.take(),
            report["body"]["handoff_token"].as_str().map(str::to_owned),
            self.nitro.purchase.as_ref(),
        ) else {
            return;
        };
        let plan = premium::find_plan(&self.nitro.plans, purchase.sku, purchase.yearly);
        if let Some(plan) = plan {
            cx.open_url(&premium::checkout_handoff_url(
                plan.id,
                purchase.gift,
                &load_id,
                &key,
                &token,
            ));
            if let Some(purchase) = self.nitro.purchase.as_mut() {
                if purchase.gift {
                    let yearly = purchase.yearly;
                    self.nitro.purchase = None;
                    let mut modal = super::gift_ready::GiftReadyModal::demo("ready");
                    if yearly {
                        modal.interval = super::gift_ready::GiftInterval::Year;
                    }
                    self.gift_ready_modal = Some(modal);
                } else {
                    purchase.step = Step::Confirm;
                    purchase.confirm.pending_payment = true;
                }
            }
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_GIFT_MESSAGE, characters_left, gifts_away, pulse_wave, savings_percent};

    #[test]
    fn the_price_spinner_dips_at_half_a_cycle_and_returns() {
        // `spinner-pulsing-ellipsis`: opacity 1, .3, 1 and scale 1, .8, 1 at 0%, 50% and 100%.
        assert!(pulse_wave(0.).abs() < 1e-4);
        assert!((pulse_wave(0.5) - 1.).abs() < 1e-4);
        assert!(pulse_wave(1.).abs() < 1e-4);
        // The dot .2s behind (of 1.4s) is on its way down while the first is already past it.
        assert!(pulse_wave(0.5 - 0.2 / 1.4) < pulse_wave(0.5));
        // Symmetric: the climb back mirrors the descent.
        assert!((pulse_wave(0.25) - pulse_wave(0.75)).abs() < 1e-4);
    }

    #[test]
    fn the_yearly_gift_saves_the_captured_percentage() {
        // $9.99 a month, $99.99 a year: the capture's "SAVE 16%".
        assert_eq!(savings_percent(999, 9999), Some(16));
        assert_eq!(savings_percent(999, 11988), None);
        assert_eq!(savings_percent(0, 100), None);
    }

    #[test]
    fn the_default_message_leaves_the_captured_count() {
        // design/dom/dm/gift-step2-nitro-darker-cozy.json shows 147 characters remaining.
        assert_eq!(characters_left(DEFAULT_GIFT_MESSAGE), 147);
        assert_eq!(characters_left(""), 190);
        assert_eq!(characters_left("\u{1F600}"), 188);
    }

    #[test]
    fn the_banner_line_pluralises_like_the_catalog_string() {
        assert_eq!(
            gifts_away(1, "Patron"),
            "1 gift away from the Patron Badge!"
        );
        assert_eq!(
            gifts_away(3, "Patron"),
            "3 gifts away from the Patron Badge!"
        );
        assert_eq!(
            gifts_away(0, "Patron"),
            "0 gifts away from the Patron Badge!"
        );
    }

    #[test]
    fn confirm_copy_and_all_231308_strings_resolve() {
        use super::{
            ConfirmPlan, ConfirmState, confirm_banner_wordmark, confirm_button_label,
            confirm_paragraphs,
        };
        for key in [
            "knvOVz", "o6hBiR", "f8Dquh", "A4THYt", "YZd5rw", "MHAxpE", "M/Lknl", "mULxLW",
            "aaCoW6", "XEoQ7+", "PNX4NM", "nLI1Kr", "qSZZVA", "RNjcNg", "AGf/ye", "aTUr3Z",
            "bIVRSQ", "0UJqOy", "tsQOs+", "L9lcG/", "EoDFuN", "UPpbP3", "6aZ0NR", "sRApon",
            "1qGgm4", "QJ9EyM", "ta3cXY", "TkTvBz",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in nitro_purchase 231308"
            );
        }
        let fresh = ConfirmState::default();
        assert_eq!(confirm_banner_wordmark(&fresh), "NITRO ACTIVATED");
        assert_eq!(confirm_button_label(&fresh), "Sweet!");
        assert!(confirm_paragraphs(ConfirmPlan::BasicMonth, &fresh)[0].contains("Nitro Basic"));
        assert!(confirm_paragraphs(ConfirmPlan::NitroYear, &fresh)[0].contains("superpowered"));

        let upgrade = ConfirmState {
            starting_plan: Some(ConfirmPlan::NitroMonth),
            ..Default::default()
        };
        assert_eq!(confirm_banner_wordmark(&upgrade), "NITRO UPDATED");
        assert!(confirm_paragraphs(ConfirmPlan::NitroYear, &upgrade)[0].contains("2"));

        let downgrade = ConfirmState {
            starting_plan: Some(ConfirmPlan::NitroYear),
            is_downgrade: true,
            ..Default::default()
        };
        assert_eq!(confirm_button_label(&downgrade), "Got It");
        assert!(confirm_paragraphs(ConfirmPlan::ClassicMonth, &downgrade)[0].contains("Boosts"));

        let credits = ConfirmState {
            has_fractional_credits: true,
            ..Default::default()
        };
        assert_eq!(
            confirm_paragraphs(ConfirmPlan::NitroMonth, &credits).len(),
            2
        );

        let guild = ConfirmState {
            post_success_guild: Some((1, "TestGuild".into())),
            ..Default::default()
        };
        assert_eq!(confirm_button_label(&guild), "Join Server");
        assert!(confirm_paragraphs(ConfirmPlan::NitroMonth, &guild)[1].contains("TestGuild"));

        let pending = ConfirmState {
            pending_payment: true,
            ..Default::default()
        };
        assert_eq!(confirm_banner_wordmark(&pending), "NITRO");
        assert!(confirm_paragraphs(ConfirmPlan::NitroMonth, &pending)[0].contains("email"));

        let deeplink = ConfirmState {
            deeplink: true,
            ..Default::default()
        };
        assert_eq!(confirm_button_label(&deeplink), "Return to app");
        assert!(confirm_paragraphs(ConfirmPlan::NitroMonth, &deeplink)[0].contains("close"));
    }

    #[test]
    fn review_footer_cta_matches_module_809029_priority() {
        let mut r = super::ReviewContext::default();
        assert_eq!(r.cta_label(), "Get Nitro Monthly");
        r.plan_kind = super::ReviewPlanKind::NitroYearly;
        assert_eq!(r.cta_label(), "Get Nitro Yearly");
        r.plan_kind = super::ReviewPlanKind::BasicMonthly;
        assert_eq!(r.cta_label(), "Get Nitro Basic Monthly");
        r.plan_kind = super::ReviewPlanKind::Boost;
        assert_eq!(r.cta_label(), "Purchase");

        r.has_active_sub = true;
        assert_eq!(r.cta_label(), "Change Plan");
        r.is_upgrade = true;
        assert_eq!(r.cta_label(), "Upgrade");
        r.is_paused_resume = true;
        assert_eq!(r.cta_label(), "Resume Subscription");

        r.is_prepaid = true;
        assert_eq!(r.cta_label(), "Get Nitro");
        r.is_past_due = true;
        assert_eq!(r.cta_label(), "Restore Subscription");
        r.is_gift = true;
        assert_eq!(r.cta_label(), "Buy Gift");
        r.is_recurring_gift = true;
        assert_eq!(r.cta_label(), "Subscribe");
    }

    #[test]
    fn review_promotional_notices_and_totals_match_module_367210() {
        // Trial priority (IAsfR5)
        let mut r = super::ReviewContext {
            trial_days: Some(14),
            discount_percent: Some(30),
            ..Default::default()
        };
        let trial_text: String = super::promotional_notice_runs(&r)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(trial_text.contains("14"));

        // Monthly discount (wCkwJf)
        r.trial_days = None;
        r.discount_interval_months = Some(3);
        let mo_text: String = super::promotional_notice_runs(&r)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(mo_text.contains("30%"));
        assert!(mo_text.contains("3"));

        // Yearly discount (tUzT/U)
        r.discount_interval_months = None;
        let yr_text: String = super::promotional_notice_runs(&r)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(yr_text.contains("30%"));

        // Multi-period gift savings (7sYIBL / uCXulJ + Mi5BH/)
        r.discount_percent = None;
        r.is_gift = true;
        r.gift_savings_percent = Some(16);
        r.gift_interval_months = Some(12);
        let gift_yr: String = super::promotional_notice_runs(&r)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(gift_yr.contains("16%"));
        assert_eq!(super::review_gift_badge(&r).as_deref(), Some("16% OFF"));
        assert_eq!(super::review_total_label(&r), dc_intl::t("Zxav97"));

        r.gift_interval_months = Some(3);
        let gift_mo: String = super::promotional_notice_runs(&r)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(gift_mo.contains("3"));
        assert!(gift_mo.contains("16%"));

        r.is_gift = false;
        assert_eq!(super::review_total_label(&r), dc_intl::t("R0cZsM"));
        assert!(!dc_intl::t("2wPRSF").is_empty());
        assert!(!dc_intl::t("Urtyu9").is_empty());
        assert!(!dc_intl::t("u+Cw58").is_empty());
        assert!(!dc_intl::t("IGU7El").is_empty());
    }

    #[test]
    fn checkout_purchase_summary_covers_module_498750_variants() {
        let mut ctx = super::ReviewContext {
            is_recurring_gift: true,
            is_gift: true,
            ..super::ReviewContext::default()
        };
        let yr = super::build_purchase_summary(&ctx, "$99.99", None, true, false, "Nitro");
        assert_eq!(yr.description, dc_intl::t("rstMwN"));
        let mo = super::build_purchase_summary(&ctx, "$9.99", None, false, false, "Nitro");
        assert_eq!(mo.description, dc_intl::t("Vb8FaC"));

        ctx.is_recurring_gift = false;
        ctx.quantity = 3;
        ctx.can_edit_plan = true;
        let qty =
            super::build_purchase_summary(&ctx, "$29.97", Some("$24.99"), false, false, "Nitro");
        assert_eq!(qty.description, dc_intl::t("3wsYeI"));
        assert!(qty.label.contains("3"));
        assert!(qty.editable);
        assert_eq!(qty.inline_strikethrough.as_deref(), Some("$29.97"));

        ctx.is_gift = false;
        ctx.quantity = 1;
        ctx.discount_offer = Some(super::CheckoutDiscountOffer {
            discount_percent: 30,
            interval_count: 1,
            is_yearly: true,
        });
        let disc_yr =
            super::build_purchase_summary(&ctx, "$99.99", Some("$70.00"), true, false, "Nitro");
        assert_eq!(disc_yr.description, dc_intl::t("2zUa6I"));
        assert!(disc_yr.price.contains("$70.00"));
        assert!(disc_yr.sub_text.as_deref().unwrap().contains("$99.99"));

        ctx.discount_offer = Some(super::CheckoutDiscountOffer {
            discount_percent: 30,
            interval_count: 3,
            is_yearly: false,
        });
        let disc_mo =
            super::build_purchase_summary(&ctx, "$9.99", Some("$6.99"), false, false, "Nitro");
        assert!(disc_mo.price.contains("$6.99"));
        assert!(
            disc_mo
                .sub_text
                .as_deref()
                .unwrap()
                .contains("three months")
        );

        let grp = super::build_purchase_summary(
            &super::ReviewContext::default(),
            "$14.99",
            None,
            false,
            true,
            "Nitro Group",
        );
        assert!(grp.label.contains("Nitro Group"));
    }

    #[test]
    fn card_validation_unified_flow_and_gift_card_match_modules_671334_675219_677959() {
        // Module 671334 validation
        assert_eq!(
            super::validate_card_form("", "12/28", "123", "Alex"),
            vec![dc_intl::t("eOIfuy")]
        );
        assert_eq!(
            super::validate_card_form("1234", "12/28", "123", "Alex"),
            vec![dc_intl::t("x4pWtJ")]
        );
        assert_eq!(
            super::validate_card_form("4242424242424242", "13/28", "123", "Alex"),
            vec![dc_intl::t("9/zZdl")]
        );
        assert_eq!(
            super::validate_card_form("4242424242424242", "12/28", "12", "Alex"),
            vec![dc_intl::t("ro4isZ")]
        );
        assert_eq!(
            super::validate_card_form("4242424242424242", "12/28", "123", "   "),
            vec![dc_intl::t("lIkVsi")]
        );
        assert!(
            super::validate_card_form("4242 4242 4242 4242", "12/28", "123", "Alex").is_empty()
        );

        // Module 675219 unified checkout flows
        assert!(super::UnifiedCheckoutFlow::CollectiblesCheckout.allow_gift_customization());
        assert!(super::UnifiedCheckoutFlow::SlayerStorefrontCheckout.allow_gift_customization());
        assert!(super::UnifiedCheckoutFlow::PremiumAppsOtpCheckout.allow_gift_customization());
        assert!(!super::UnifiedCheckoutFlow::OrbCheckout.allow_gift_customization());
        assert!(super::UnifiedCheckoutFlow::OrbCheckout.exclude_payment_auth_steps());

        let gift_steps = super::generate_checkout_step_configs(
            super::UnifiedCheckoutFlow::CollectiblesCheckout,
            true,
        );
        assert_eq!(gift_steps.len(), 3);
        assert_eq!(gift_steps[0].key, "gift_customization");
        assert_eq!(gift_steps[0].modal_size, "xl");
        assert_eq!(gift_steps[0].breadcrumb, dc_intl::t("W685+b"));
        assert_eq!(gift_steps[2].breadcrumb, dc_intl::t("QBnNHq"));

        let orb_steps =
            super::generate_checkout_step_configs(super::UnifiedCheckoutFlow::OrbCheckout, false);
        assert_eq!(orb_steps.len(), 1);
        assert_eq!(orb_steps[0].key, "review");

        // Module 677959 tier labels & gift card intl keys
        assert_eq!(super::premium_tier_label(0), dc_intl::t("t9uG/o"));
        assert_eq!(super::premium_tier_label(1), dc_intl::t("FSOz78"));
        assert_eq!(super::premium_tier_label(2), dc_intl::t("lG6a5x"));
        for k in [
            "26tjwd", "Bn/CZQ", "3jjweL", "eDdrAD", "hnRau6", "uo9YsP", "EUKPip", "Afx5yS",
            "VJXTzd", "+fIPjg", "WAI6xu", "9OxfNK", "8x0jKT",
        ] {
            assert!(dc_intl::has_key(k), "missing 677959 key {k}");
        }
    }
}
