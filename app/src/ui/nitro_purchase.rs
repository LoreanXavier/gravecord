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
pub(crate) enum Step {
    Plans,
    /// "Gift Nitro": the recipient, the message and the period of a gift.
    GiftSelect,
    Checkout,
    Payment,
    /// Post-purchase confirmation (module 231308 `G`, `B`, `b`; CSS 13792, 896554).
    Confirm,
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
        });
        cx.notify();
    }

    fn close_purchase(&mut self) {
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
        let header =
            div()
                .relative()
                .flex_none()
                .h(px(48.))
                .px(px(24.))
                .child(div().absolute().top_0().left_0().right_0().h(px(48.)).when(
                    gradient,
                    |this| {
                        this.bg(linear_gradient(
                            180.,
                            linear_color_stop(oklab(0.45, 0.17, -0.09, 0.55), 0.),
                            linear_color_stop(oklab(0.263, 0.0025, -0.0089, 0.), 1.),
                        ))
                    },
                ))
                .child(
                    div()
                        .relative()
                        .pt(px(16.))
                        .flex()
                        .child(
                            div()
                                .flex_1()
                                .h(px(24.))
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
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
                );
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
            .child(body)
            .children(footer);
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
            (Step::Confirm, _) => self.confirm_modal(window, cx),
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
                "Gift Nitro"
            } else {
                "Gift Nitro Basic"
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
        let name = if nitro { "Nitro" } else { "Nitro Basic" };
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
            .child(
                div()
                    .mt(px(8.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("circle_info", 20., "--status-warning"))
                    .child(Self::nitro_text(
                        format!(
                            "{name} is a recurring subscription. Cancel anytime in your settings."
                        ),
                        14.,
                        18.,
                        400.,
                        "--text-default",
                    )),
            );
        let body = div()
            .px(px(24.))
            .pt(px(16.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(Self::nitro_heading(name, 24., 24., true))
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

    /// Post-purchase confirmation modal (`G` + `B` + `b` of module 231308; CSS `13792`, `896554`).
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

    /// Capture helper: `nitro-confirm [activated|updated|basic|credits|guild|pending]` opens module 231308.
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_confirm(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut confirm = ConfirmState::default();
        let (plan_sku, yearly) = if arg.contains("basic") {
            (sku::TIER_0, false)
        } else {
            (sku::TIER_2, true)
        };
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
        });
        cx.notify();
        format!("nitro-confirm: opened ({arg})")
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
}
