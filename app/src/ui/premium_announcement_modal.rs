//! Server-driven Nitro announcement modal (`PremiumAnnouncementModalVariant1`, module 962263).
//!
//! Renders the 716px `nitro-pink` announcement dialog with a 16:9 hero media surface, optional
//! top pill, header/subheader/body, even (`wideStyle`) or odd vertical feature cards with
//! floating tag badges, expressive CTA (fallback `YScQSF` "Subscribe"), and disclaimer footer.

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;

/// Official `308588.d` CTA actions for `PremiumAnnouncementModalVariant1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AnnouncementButtonAction {
    #[default]
    OpenMarketingPage,
    OpenTier2PaymentModal,
    OpenTier1PaymentModal,
    OpenTier2PaymentCustomFooter,
    OpenPlanSelectionModal,
    OpenPremiumGroupPaymentModal,
    OpenSocialLayerStorefront,
    OpenGuildBoostCheckout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AnnouncementFeatureCard {
    pub header: String,
    pub body: String,
    pub tag_text: Option<String>,
    pub image_url: Option<String>,
}

/// Active state for module 962263 (`PremiumAnnouncementModalVariant1`).
#[derive(Clone, Debug)]
pub(crate) struct PremiumAnnouncementModalState {
    pub promotion_id: String,
    pub modal_top_pill: Option<String>,
    pub hero_image_url: Option<String>,
    pub header: String,
    pub subheader: String,
    pub body: String,
    pub feature_cards: Vec<AnnouncementFeatureCard>,
    pub cta_copy: Option<String>,
    pub cta_action: AnnouncementButtonAction,
    pub disclaimer: Option<String>,
    pub terms_url: Option<String>,
    pub is_preview: bool,
}

impl PremiumAnnouncementModalState {
    /// Module 962263: `wideStyle = cards.length % 2 === 0`.
    pub(crate) fn is_wide_cards(&self) -> bool {
        self.feature_cards.len().is_multiple_of(2)
    }

    /// Module 962263 (`YScQSF` fallback `"Subscribe"`).
    pub(crate) fn resolved_cta_label(&self) -> String {
        self.cta_copy
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| dc_intl::t("YScQSF").to_owned())
    }
}

/// Module 962263 (`543862.qr`): dismisses the marketing moment via user settings proto.
pub(crate) fn dismiss_premium_announcement_request(promotion_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("premium-announcement:dismiss:{promotion_id}")),
        method: reqwest::Method::PATCH,
        route: "/users/@me/settings-proto/1".to_owned(),
        body: Some(json!({ "dismissed_promotion_id": promotion_id })),
    }
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_premium_announcement_modal(
        &mut self,
        state: PremiumAnnouncementModalState,
        cx: &mut Context<Self>,
    ) {
        self.premium_announcement_modal = Some(state);
        cx.notify();
    }

    pub(crate) fn close_premium_announcement_modal(&mut self) -> bool {
        let Some(prev) = self.premium_announcement_modal.take() else {
            return false;
        };
        if !prev.is_preview && !prev.promotion_id.is_empty() {
            let _ = self
                .requests
                .send(dismiss_premium_announcement_request(&prev.promotion_id));
        }
        true
    }

    pub(in crate::ui) fn premium_announcement_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.premium_announcement_modal.as_ref()?;
        let wide = state.is_wide_cards();
        let cta_label = state.resolved_cta_label();
        let cta_action = state.cta_action;
        let hero_img = state
            .hero_image_url
            .as_deref()
            .and_then(|u| self.images.get(u).cloned());
        let terms_url = state.terms_url.clone();

        let cards: Vec<AnyElement> = state
            .feature_cards
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let card_img = card
                    .image_url
                    .as_deref()
                    .and_then(|u| self.images.get(u).cloned());
                let thumb = if wide {
                    div()
                        .size(px(70.))
                        .flex_none()
                        .rounded(px(10.))
                        .overflow_hidden()
                        .bg(color("--background-surface-highest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match card_img {
                            Some(im) => img(im)
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover)
                                .into_any_element(),
                            None => icon("nitro_wheel", 28., "--text-brand").into_any_element(),
                        })
                } else {
                    div()
                        .w(px(120.))
                        .h(px(80.))
                        .flex_none()
                        .rounded(px(10.))
                        .overflow_hidden()
                        .bg(color("--background-surface-highest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match card_img {
                            Some(im) => img(im)
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover)
                                .into_any_element(),
                            None => icon("nitro_wheel", 30., "--text-brand").into_any_element(),
                        })
                };

                let mut box_el = div()
                    .id(SharedString::from(format!("premium-ann-card-{i}")))
                    .relative()
                    .rounded(px(16.))
                    .bg(color("--background-mod-muted"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .gap(px(12.));
                box_el = if wide {
                    box_el
                        .w(px(304.))
                        .flex_row()
                        .items_center()
                        .pt(px(20.))
                        .pb(px(20.))
                        .pr(px(16.))
                        .pl(px(12.))
                } else {
                    box_el.w(px(198.)).flex_col().items_center().p(px(20.))
                };

                box_el
                    .children(card.tag_text.clone().map(|tag| {
                        div()
                            .absolute()
                            .top(px(-8.))
                            .left(px(16.))
                            .px(px(8.))
                            .h(px(18.))
                            .rounded_full()
                            .bg(linear_gradient(
                                90.,
                                linear_color_stop(rgba(0xb845c1ff), 0.),
                                linear_color_stop(rgba(0xe060a8ff), 1.),
                            ))
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(tag)
                    }))
                    .child(thumb)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .when(!wide, |c| c.items_center().text_center())
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(card.header.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(card.body.clone()),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        Some(
            deferred(
                div()
                    .id("premium-announcement-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_premium_announcement_modal();
                            cx.notify();
                        }),
                    )
                    .child(
                        // `.root_FNz3hO`: 716px wide, `--background-surface-high`, `--radius-lg` (16px)
                        div()
                            .id("premium-announcement-modal")
                            .relative()
                            .w(px(716.))
                            .max_h(px(660.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            // Top nitro-pink gradient wash
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .h(px(220.))
                                    .bg(linear_gradient(
                                        180.,
                                        linear_color_stop(rgba(0xb845c155), 0.),
                                        linear_color_stop(rgba(0xb845c100), 1.),
                                    )),
                            )
                            // Top-right close button (`815021.J`)
                            .child(
                                div()
                                    .id("premium-announcement-close")
                                    .absolute()
                                    .top(px(14.))
                                    .right(px(14.))
                                    .size(px(32.))
                                    .rounded_full()
                                    .bg(rgba(0x00000055))
                                    .hover(|s| s.bg(rgba(0x00000088)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .aria_label(SharedString::from(dc_intl::t("cpT0Cq")))
                                    .child(icon("modal_close", 18., "--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_premium_announcement_modal();
                                        cx.notify();
                                    })),
                            )
                            // Scrollable `.gradientWrapper_FNz3hO` + `.content_FNz3hO`
                            .child(
                                div()
                                    .id("premium-announcement-scroll")
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_y_scroll()
                                    .px(px(48.))
                                    .pt(px(36.))
                                    .pb(px(24.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(20.))
                                    // Hero media (16:9, rounded 16px)
                                    .child(
                                        div()
                                            .w(px(620.))
                                            .h(px(240.))
                                            .rounded(px(16.))
                                            .overflow_hidden()
                                            .bg(linear_gradient(
                                                135.,
                                                linear_color_stop(rgba(0x5865f2ff), 0.),
                                                linear_color_stop(rgba(0xe060a8ff), 1.),
                                            ))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(match hero_img {
                                                Some(im) => img(im)
                                                    .size_full()
                                                    .object_fit(gpui::ObjectFit::Cover)
                                                    .into_any_element(),
                                                None => div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(icon("nitro_wheel", 44., "--white"))
                                                    .child(icon("shop_nitro_wordmark", 36., "--white"))
                                                    .into_any_element(),
                                            }),
                                    )
                                    // Optional top pill
                                    .children(state.modal_top_pill.clone().map(|pill| {
                                        div()
                                            .px(px(10.))
                                            .py(px(4.))
                                            .rounded_full()
                                            .bg(rgba(0xb845c133))
                                            .border_1()
                                            .border_color(rgba(0xe060a888))
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(pill)
                                    }))
                                    // Header + Subheader + Body
                                    .child(
                                        div()
                                            .w(px(620.))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_center()
                                                    .text_size(px(28.))
                                                    .line_height(px(34.))
                                                    .font_weight(FontWeight(800.))
                                                    .text_color(color("--text-strong"))
                                                    .child(state.header.clone()),
                                            )
                                            .when(!state.subheader.is_empty(), |col| {
                                                col.child(
                                                    div()
                                                        .max_w(px(548.))
                                                        .text_center()
                                                        .text_size(px(16.))
                                                        .line_height(px(22.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-default"))
                                                        .child(state.subheader.clone()),
                                                )
                                            })
                                            .when(!state.body.is_empty(), |col| {
                                                col.child(
                                                    div()
                                                        .max_w(px(500.))
                                                        .pt(px(4.))
                                                        .text_center()
                                                        .text_size(px(15.))
                                                        .line_height(px(20.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(state.body.clone()),
                                                )
                                            }),
                                    )
                                    // Feature cards (`.featureCardGroup_FNz3hO`)
                                    .when(!cards.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .w(px(620.))
                                                .pt(px(6.))
                                                .flex()
                                                .flex_wrap()
                                                .justify_center()
                                                .gap(px(12.))
                                                .children(cards),
                                        )
                                    })
                                    // Footer CTA + disclaimer (`.footerContainer_FNz3hO`)
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .id("premium-announcement-cta")
                                                    .h(px(44.))
                                                    .px(px(28.))
                                                    .rounded(px(8.))
                                                    .bg(linear_gradient(
                                                        90.,
                                                        linear_color_stop(rgba(0xb845c1ff), 0.),
                                                        linear_color_stop(rgba(0xe060a8ff), 1.),
                                                    ))
                                                    .hover(|s| s.opacity(0.92))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(icon("nitro_wheel", 18., "--white"))
                                                    .child(
                                                        div()
                                                            .text_size(px(15.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--white"))
                                                            .child(cta_label),
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.close_premium_announcement_modal();
                                                        match cta_action {
                                                            AnnouncementButtonAction::OpenSocialLayerStorefront => {
                                                                this.open_shop(cx);
                                                            }
                                                            _ => this.open_nitro(cx),
                                                        }
                                                    })),
                                            )
                                            .children(state.disclaimer.clone().map(|disc| {
                                                div()
                                                    .max_w(px(480.))
                                                    .flex()
                                                    .flex_wrap()
                                                    .justify_center()
                                                    .gap(px(4.))
                                                    .text_center()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-muted"))
                                                    .child(disc)
                                                    .children(terms_url.clone().map(|url| {
                                                        div()
                                                            .id("premium-announcement-terms")
                                                            .cursor_pointer()
                                                            .text_color(color("--text-link"))
                                                            .underline()
                                                            .child(dc_intl::t("aA6q/z"))
                                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                                cx.open_url(&url);
                                                            }))
                                                    }))
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_premium_announcement(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.close_premium_announcement_modal();
            cx.notify();
            return "premium announcement closed".to_owned();
        }
        self.open_premium_announcement_modal(
            PremiumAnnouncementModalState {
                promotion_id: "promo_2026_spring".to_owned(),
                modal_top_pill: Some("NEW WITH NITRO".to_owned()),
                hero_image_url: None,
                header: "Make Your Profile Unmistakably Yours".to_owned(),
                subheader: "Unlock animated profile frames, custom app icons, and higher-bitrate HD streaming across every server.".to_owned(),
                body: "Included in Nitro".to_owned(),
                feature_cards: vec![
                    AnnouncementFeatureCard {
                        header: "Profile Frames & Effects".to_owned(),
                        body: "Layer animated borders and entry effects on your profile card.".to_owned(),
                        tag_text: Some("NEW".to_owned()),
                        image_url: None,
                    },
                    AnnouncementFeatureCard {
                        header: "1440p 60fps Streaming".to_owned(),
                        body: "Share crisp gameplay with friends in any voice channel.".to_owned(),
                        tag_text: None,
                        image_url: None,
                    },
                ],
                cta_copy: None,
                cta_action: AnnouncementButtonAction::OpenTier2PaymentModal,
                disclaimer: Some("Cancel anytime. Recurring subscription terms apply.".to_owned()),
                terms_url: Some("https://discord.com/terms".to_owned()),
                is_preview: true,
            },
            cx,
        );
        "premium announcement opened".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_announcement_962263_cards_cta_and_dismiss() {
        assert!(!dc_intl::t("YScQSF").is_empty());
        let mut state = PremiumAnnouncementModalState {
            promotion_id: "p42".into(),
            modal_top_pill: None,
            hero_image_url: None,
            header: "Header".into(),
            subheader: "Sub".into(),
            body: "Body".into(),
            feature_cards: vec![
                AnnouncementFeatureCard {
                    header: "A".into(),
                    body: "a".into(),
                    tag_text: Some("NEW".into()),
                    image_url: None,
                },
                AnnouncementFeatureCard {
                    header: "B".into(),
                    body: "b".into(),
                    tag_text: None,
                    image_url: None,
                },
            ],
            cta_copy: None,
            cta_action: AnnouncementButtonAction::OpenTier2PaymentModal,
            disclaimer: None,
            terms_url: None,
            is_preview: false,
        };
        assert!(state.is_wide_cards());
        assert_eq!(state.resolved_cta_label(), dc_intl::t("YScQSF"));
        state.cta_copy = Some("Try Nitro".into());
        assert_eq!(state.resolved_cta_label(), "Try Nitro");
        state.feature_cards.pop();
        assert!(!state.is_wide_cards());
        let req = dismiss_premium_announcement_request("p42");
        assert!(matches!(
            req,
            Request::Rest { route, .. } if route == "/users/@me/settings-proto/1"
        ));
    }
}
