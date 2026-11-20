//! Social Layer Game Item Gift Code Redeem flow (module 173971):
//! Link/Claim step (`K`), Claimed Success step (`z`), and Already Owned / Exhausted Error step (`B`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GameGiftStep {
    Link,
    Success,
    ErrorAlreadyOwned,
    ErrorExhausted,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct GameGiftRedeemModal {
    pub sku_name: String,
    pub app_name: String,
    pub sender_name: String,
    pub custom_message: Option<String>,
    pub has_already_linked: bool,
    pub can_start_auth: bool,
    pub is_launchable: bool,
    pub step: GameGiftStep,
}

impl Root {
    pub(crate) fn close_game_gift_redeem(&mut self) -> bool {
        self.game_gift_redeem_modal.take().is_some()
    }

    pub(in crate::ui) fn game_gift_redeem_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.game_gift_redeem_modal.as_ref()?;
        let has_linked = modal.has_already_linked;
        let can_auth = modal.can_start_auth;
        let is_launchable = modal.is_launchable;

        let close_btn = div()
            .id("game-gift-close")
            .flex_shrink_0()
            .size(px(30.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("modal_close", 18., "--interactive-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_game_gift_redeem();
                cx.notify();
            }));

        let content: AnyElement = match modal.step {
            GameGiftStep::Link => {
                let body = if has_linked {
                    dc_intl::format(
                        "TNRN/e",
                        &[("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name)))],
                    )
                } else {
                    dc_intl::format(
                        "VkR30M",
                        &[("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name)))],
                    )
                };
                let (cta_icon, cta_label) = if has_linked {
                    (Some("gift"), dc_intl::t("go1e4x"))
                } else if can_auth {
                    (Some("bot_game_controller"), dc_intl::t("VDAhr+"))
                } else if is_launchable {
                    (Some("open_external"), dc_intl::t("s+J8Dl"))
                } else {
                    (None, dc_intl::t("cpT0Cq"))
                };

                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("Bn1J+a")),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .p(px(16.))
                            .rounded(px(10.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size(px(48.))
                                    .rounded(px(10.))
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("shop_gift", 24., "--white")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(modal.sku_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(modal.app_name.clone()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .child(body),
                    )
                    .child(
                        div()
                            .id("game-gift-link-cta")
                            .w_full()
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .when_some(cta_icon, |btn, ic| btn.child(icon(ic, 16., "--white")))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(cta_label),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.game_gift_redeem_modal.as_mut() {
                                    if m.has_already_linked {
                                        m.step = GameGiftStep::Success;
                                    } else if m.can_start_auth {
                                        m.has_already_linked = true;
                                    } else {
                                        this.close_game_gift_redeem();
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            GameGiftStep::Success => {
                let sender_line = dc_intl::format(
                    "6yrIzU",
                    &[("sender", Arg::Str(Cow::Borrowed(&modal.sender_name)))],
                );
                let custom_msg = modal
                    .custom_message
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| dc_intl::t("ZkOo1U"))
                    .to_owned();
                let sent_notice = dc_intl::format(
                    "W2znvX",
                    &[
                        ("skuName", Arg::Str(Cow::Borrowed(&modal.sku_name))),
                        ("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name))),
                    ],
                );
                let (cta_icon, cta_label) = if is_launchable {
                    (Some("open_external"), dc_intl::t("s+J8Dl"))
                } else {
                    (None, dc_intl::t("cpT0Cq"))
                };

                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(16.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("shop_gift", 32., "--white")),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(modal.sku_name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-subtle"))
                            .text_center()
                            .child(sender_line),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-subtle"))
                            .text_center()
                            .child(custom_msg),
                    )
                    .child(
                        div()
                            .w_full()
                            .mt(px(6.))
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(rgba(0x23a55a1f))
                            .border_1()
                            .border_color(color("--status-positive"))
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .child(div().flex_shrink_0().mt(px(1.)).child(icon(
                                "circle_check",
                                18.,
                                "--status-positive",
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-strong"))
                                    .child(sent_notice),
                            ),
                    )
                    .child(
                        div()
                            .id("game-gift-success-cta")
                            .w_full()
                            .mt(px(4.))
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .when_some(cta_icon, |btn, ic| btn.child(icon(ic, 16., "--white")))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(cta_label),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_game_gift_redeem();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            GameGiftStep::ErrorAlreadyOwned | GameGiftStep::ErrorExhausted => {
                let (title, body) = if modal.step == GameGiftStep::ErrorAlreadyOwned {
                    let b = if has_linked {
                        dc_intl::format(
                            "TNRN/e",
                            &[("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name)))],
                        )
                    } else {
                        dc_intl::format(
                            "VkR30M",
                            &[("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name)))],
                        )
                    };
                    (dc_intl::t("uXnSGo"), b)
                } else {
                    (dc_intl::t("dMYg1c"), dc_intl::t("Iw2TUW").to_owned())
                };

                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(icon("circle_info", 36., "--text-feedback-critical"))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .text_center()
                            .child(body),
                    )
                    .child(
                        div()
                            .id("game-gift-dismiss")
                            .w_full()
                            .mt(px(4.))
                            .h(px(40.))
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
                            .child(dc_intl::t("WAI6xu"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_game_gift_redeem();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("game-gift-redeem-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.35),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(content);

        Some(
            deferred(
                div()
                    .id("game-gift-redeem-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_game_gift_redeem();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_game_gift_redeem(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let (step, linked) = match arg {
            "link" => (GameGiftStep::Link, false),
            "claim" => (GameGiftStep::Link, true),
            "owned" => (GameGiftStep::ErrorAlreadyOwned, true),
            "exhausted" => (GameGiftStep::ErrorExhausted, false),
            _ => (GameGiftStep::Success, true),
        };
        self.game_gift_redeem_modal = Some(GameGiftRedeemModal {
            sku_name: "Starlight Vanguard Bundle".to_owned(),
            app_name: "Marvel Rivals".to_owned(),
            sender_name: "lorean xavier".to_owned(),
            custom_message: None,
            has_already_linked: linked,
            can_start_auth: true,
            is_launchable: true,
            step,
        });
        cx.notify();
        format!("game-gift-redeem: {arg}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_14_keys_of_game_gift_redeem_173971_resolve() {
        for key in [
            "6yrIzU", "Bn1J+a", "Iw2TUW", "TNRN/e", "VDAhr+", "VkR30M", "W2znvX", "WAI6xu",
            "ZkOo1U", "cpT0Cq", "dMYg1c", "go1e4x", "s+J8Dl", "uXnSGo",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
