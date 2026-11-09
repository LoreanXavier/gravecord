//! The purchase dialogs of the Nitro page: Select Plan, Checkout, the Add Payment Method hand-off and Send a Gift.
//! Geometry is from `design/dom/modals/nitro-purchase-{plan,step2,step3}-darker-cozy.json` and
//! `nitro-gift-darker-cozy.json`.
//!
//! Nothing here buys anything. "Select" goes to the payment step, which in the official client opens the billing
//! page in the browser (module 36167: `POST /auth/handoff`, then the standalone checkout URL); the payment itself is
//! entered there. "Pay Here" is inert in capture builds, and `SubscribeRequest` is only ever a unit-tested body.
//!
//! Not built: the gift banner ("1 gift away from the Patron Badge!"): its text is not in the strings dump and its
//! source is not in the recorded traffic. The gift flow after choosing a plan is not captured either.

use super::nitro::{BASIC_FEATURES, NITRO_FEATURES, oklab, rgb};
use super::{Root, color, icon};
use crate::parity::Parity;
use dc_state::premium::{self, Price, sku};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, Window, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Plans,
    Checkout,
    Payment,
}

/// The open purchase dialog.
pub(crate) struct Purchase {
    pub gift: bool,
    pub step: Step,
    pub sku: u64,
    pub yearly: bool,
    pub scroll: ScrollHandle,
}

#[cfg(not(feature = "capture"))]
pub(super) const HANDOFF_KEY: &str = "nitro:handoff";

impl Root {
    /// "Subscribe" and "Gift Nitro": Select Plan, or Send a Gift.
    pub(super) fn open_purchase(&mut self, gift: bool, cx: &mut Context<Self>) {
        self.request_nitro();
        self.nitro.purchase = Some(Purchase {
            gift,
            step: Step::Plans,
            sku: sku::TIER_2,
            yearly: true,
            scroll: ScrollHandle::new(),
        });
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
        });
        cx.notify();
    }

    fn close_purchase(&mut self) {
        self.nitro.purchase = None;
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
        let limit = window.viewport_size().height.to_f64() as f32 - 114.;
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
        deferred(
            div()
                .id(format!("{id}-layer"))
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b8))
                .flex()
                .items_center()
                .justify_center()
                .on_click(scrim_close)
                .child(modal),
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
            (Step::Checkout, _) => self.checkout_modal(window, cx),
            (Step::Payment, _) => self.payment_modal(window, cx),
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
        let amount =
            |price: Option<Price>, size: f32, line: f32, unit: &'static str, tint: &'static str| {
                div()
                    .flex()
                    .items_end()
                    .gap(px(2.))
                    .children(
                        price
                            .map(|price| Self::nitro_text(price.display(), size, line, 600., tint)),
                    )
                    .child(
                        Self::nitro_text(unit, 12., 16., 500., "--text-muted")
                            .pb(px(if size > 20. { 4. } else { 2. })),
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
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .when(!gift, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_checkout(plan_sku, true, cx);
                    }))
            });
        card.children(nitro.then(|| {
            div()
                .absolute()
                .inset_0()
                .rounded(px(12.))
                .bg(linear_gradient(
                    270.,
                    linear_color_stop(oklab(0.395045, -0.0154365, -0.253271, 0.4), 0.),
                    linear_color_stop(oklab(0.449387, 0.167872, -0.086863, 0.4), 1.),
                ))
        }))
        .children(nitro.then(|| {
            // Positions and sizes from the capture, inside the card: (left, top, width, height).
            let (big, small, wumpus) = if gift {
                (
                    (237., 19., 67., 20.),
                    (161., 35., 37., 14.),
                    (190., 39., 106., 121.),
                )
            } else {
                (
                    (334., 12., 86., 31.),
                    (209., 40., 47., 18.),
                    (269., 30., 135., 155.),
                )
            };
            let place = |name: &str, (left, top, width, height): (f32, f32, f32, f32)| {
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
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
                .top(px(-13.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w(px(71.))
                        .h(px(29.))
                        .rounded(px(15.))
                        .bg(rgb(89, 31, 114))
                        .border_1()
                        .border_color(rgb(122, 69, 168))
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
                    27.,
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
                                .items_start()
                                .min_h(px(20.))
                                .child(div().pt(px(1.)).child(icon(glyph, 18., "--text-default")))
                                .child(div().flex_1().child(Self::nitro_text(
                                    *line,
                                    16.,
                                    20.,
                                    400.,
                                    "--text-default",
                                )))
                        })),
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
        let body = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .mt(px(16.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .id("purchase-plans-scroll")
                    .size_full()
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
        let body = div()
            .pl(px(18.))
            .pr(px(26.))
            .pt(px(16.))
            .pb(px(25.))
            .flex()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(self.modal_plan_card(true, true, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(self.modal_plan_card(false, true, cx)),
            );
        self.purchase_shell(
            "nitro-gift",
            680.,
            "Send a Gift",
            false,
            body.into_any_element(),
            None,
            window,
            cx,
        )
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
            .child(self.back_link(cx, Step::Checkout))
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
        }
    }
}
