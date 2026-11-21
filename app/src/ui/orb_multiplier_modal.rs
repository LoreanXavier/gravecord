//! Quest Orbs Multiplier Info / Nitro Upsell Modal (module 50292 on graphic_modal 772707).
//!
//! Displays the Orbs reward multiplier upsell or active-perk confirmation:
//! - Nitro eligible (`NpUfej`), Xbox Game Pass (`c5usUr` "Earn More Orbs", `UkrcSH`), or
//!   Upsell/Ineligible (`Csf5Ol` "Win more Orbs with Nitro", `G5k+lZ`).
//! - Actions: `cpT0Cq` ("Close") + `hvVgAZ` ("Learn More") when holding the perk, or
//!   `PcTCB7` ("View all Nitro perks") + expressive `pj0XBN` ("Get Nitro" with `nitro_wheel`)
//!   otherwise.

use super::{Root, color, icon, modal};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};

const ORB_MULTIPLIER_HELP_URL: &str =
    "https://support.discord.com/hc/en-us/articles/30593690165783#h_01KKYTZQ8C3VZN6SPPHS7ZPGHQ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum OrbMultiplierEligibility {
    Upsell,
    Nitro,
    XboxGamePass,
    Ineligible,
}

#[derive(Clone, Debug)]
pub(crate) struct OrbMultiplierModal {
    pub multiplier: f64,
    pub eligibility: OrbMultiplierEligibility,
}

impl OrbMultiplierModal {
    pub(crate) fn has_perk(&self) -> bool {
        matches!(
            self.eligibility,
            OrbMultiplierEligibility::Nitro | OrbMultiplierEligibility::XboxGamePass
        )
    }

    pub(crate) fn is_xbox(&self) -> bool {
        self.eligibility == OrbMultiplierEligibility::XboxGamePass
    }

    pub(crate) fn is_nitro_pink(&self) -> bool {
        matches!(
            self.eligibility,
            OrbMultiplierEligibility::Nitro | OrbMultiplierEligibility::Upsell
        )
    }

    pub(crate) fn title(&self) -> &'static str {
        if self.is_xbox() {
            dc_intl::t("c5usUr")
        } else {
            dc_intl::t("Csf5Ol")
        }
    }

    pub(crate) fn subtitle(&self) -> String {
        let key = match self.eligibility {
            OrbMultiplierEligibility::XboxGamePass => "UkrcSH",
            OrbMultiplierEligibility::Nitro => "NpUfej",
            OrbMultiplierEligibility::Upsell | OrbMultiplierEligibility::Ineligible => "G5k+lZ",
        };
        dc_intl::format(key, &[("bonusOrbMultiplier", Arg::Num(self.multiplier))])
    }
}

impl Root {
    /// Opens the Quest Orbs Multiplier Info / Nitro Upsell modal (`50292`).
    pub(super) fn open_orb_multiplier_modal(&mut self, cx: &mut Context<Self>) {
        let eligibility = if self.premium().is_subscriber() {
            OrbMultiplierEligibility::Nitro
        } else {
            OrbMultiplierEligibility::Upsell
        };
        self.orb_multiplier_modal = Some(OrbMultiplierModal {
            multiplier: 1.5,
            eligibility,
        });
        cx.notify();
    }

    /// Modal layer for Quest Orbs Multiplier Info / Upsell (`50292`).
    pub(super) fn orb_multiplier_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.orb_multiplier_modal.as_ref()?;
        let has_perk = m.has_perk();
        let is_pink = m.is_nitro_pink();
        let title = m.title();
        let subtitle = m.subtitle();
        let mult_badge = format!("{}x", m.multiplier);

        let header_bg = if is_pink {
            linear_gradient(
                135.,
                linear_color_stop(rgba(0x8547c6ee), 0.),
                linear_color_stop(rgba(0xb845c1dd), 1.),
            )
        } else {
            linear_gradient(
                135.,
                linear_color_stop(rgba(0x5865f2ee), 0.),
                linear_color_stop(rgba(0x3ba55cdd), 1.),
            )
        };

        let buttons = if has_perk {
            vec![
                modal::button(
                    "orb-mult-close",
                    dc_intl::t("cpT0Cq"),
                    modal::Tone::Secondary,
                    |this, _, _| this.orb_multiplier_modal = None,
                ),
                modal::button(
                    "orb-mult-learn-more",
                    dc_intl::t("hvVgAZ"),
                    modal::Tone::Primary,
                    |this, _, cx| {
                        this.orb_multiplier_modal = None;
                        cx.open_url(ORB_MULTIPLIER_HELP_URL);
                    },
                ),
            ]
        } else {
            vec![
                modal::button(
                    "orb-mult-view-perks",
                    dc_intl::t("PcTCB7"),
                    modal::Tone::Secondary,
                    |this, _, cx| {
                        this.orb_multiplier_modal = None;
                        this.open_nitro(cx);
                    },
                ),
                modal::button(
                    "orb-mult-get-nitro",
                    dc_intl::t("pj0XBN"),
                    modal::Tone::Expressive,
                    |this, _, cx| {
                        this.orb_multiplier_modal = None;
                        this.open_purchase(false, cx);
                    },
                )
                .with_icon("nitro_wheel", 16., "--white"),
            ]
        };

        let rendered_buttons: Vec<AnyElement> = buttons
            .into_iter()
            .map(|b| modal::footer_button(b, cx))
            .collect();

        Some(
            deferred(
                div()
                    .id("orb-multiplier-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.orb_multiplier_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("orb-multiplier-modal")
                            .w(px(460.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Graphic hero banner
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(148.))
                                    .bg(header_bg)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .size(px(64.))
                                            .rounded_full()
                                            .bg(rgba(0x00000044))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("shop_orb_18", 34., "--white")),
                                    )
                                    .child(
                                        div()
                                            .px(px(12.))
                                            .py(px(6.))
                                            .rounded(px(10.))
                                            .bg(rgba(0x00000055))
                                            .text_size(px(24.))
                                            .font_weight(FontWeight(800.))
                                            .text_color(color("--white"))
                                            .child(mult_badge),
                                    )
                                    .child(
                                        div()
                                            .id("orb-multiplier-close")
                                            .absolute()
                                            .top(px(10.))
                                            .right(px(10.))
                                            .size(px(30.))
                                            .rounded_full()
                                            .bg(rgba(0x00000044))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 16., "--white"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.orb_multiplier_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Copy
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(16.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(20.))
                                            .line_height(px(26.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_center()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(subtitle),
                                    ),
                            )
                            // Footer
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .children(rendered_buttons),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `orb-multiplier [upsell|nitro|xbox|close]` (module 50292).
    #[cfg(feature = "capture")]
    pub(crate) fn script_orb_multiplier(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.orb_multiplier_modal = None;
                cx.notify();
                "orb-multiplier: closed".to_owned()
            }
            "nitro" => {
                self.orb_multiplier_modal = Some(OrbMultiplierModal {
                    multiplier: 1.5,
                    eligibility: OrbMultiplierEligibility::Nitro,
                });
                cx.notify();
                "orb-multiplier: nitro".to_owned()
            }
            "xbox" => {
                self.orb_multiplier_modal = Some(OrbMultiplierModal {
                    multiplier: 1.5,
                    eligibility: OrbMultiplierEligibility::XboxGamePass,
                });
                cx.notify();
                "orb-multiplier: xbox".to_owned()
            }
            _ => {
                self.orb_multiplier_modal = Some(OrbMultiplierModal {
                    multiplier: 1.5,
                    eligibility: OrbMultiplierEligibility::Upsell,
                });
                cx.notify();
                "orb-multiplier: upsell".to_owned()
            }
        }
    }
}
