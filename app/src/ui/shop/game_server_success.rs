//! Game Server Created / Updated Success Modal (`module 978084`, CSS `759204`).
//!
//! Celebratory `size="sm"` (440px) modal displayed after creating or updating a Game Server
//! subscription plan, with blurred cover backdrop, 120x160 glossy cover card, top-right
//! overlay close button (`cpT0Cq`), plan-change/downgrade title & description (`CrCSF3` /
//! `VPDgHa` / `H9ti8H` / `+y0Tjy`), and primary `"View Game Server"` CTA (`FEWMXW`).

use super::super::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GameServerSuccessModal {
    pub game_id: String,
    pub game_name: String,
    pub cover_url: Option<String>,
    pub is_plan_change: bool,
    pub is_downgrade: bool,
    pub renewal_date: Option<String>,
}

fn gs_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() && s != key {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

impl GameServerSuccessModal {
    pub(crate) fn title(&self) -> String {
        if self.is_plan_change {
            gs_copy("VPDgHa", "Game server updated!")
        } else {
            gs_copy("CrCSF3", "Game server created!")
        }
    }

    pub(crate) fn description(&self) -> String {
        if self.is_plan_change
            && self.is_downgrade
            && let Some(date) = self
                .renewal_date
                .as_deref()
                .filter(|s| !s.trim().is_empty())
        {
            let formatted = dc_intl::format("+y0Tjy", &[("renewalDate", date.into())]);
            if !formatted.trim().is_empty() {
                return formatted.replace("**", "");
            }
            return format!("Your subscription will change starting on {date}.");
        }
        gs_copy(
            "H9ti8H",
            "Your game server is ready! Manage it anytime from \u{2018}Game Servers\u{2019} tab in Shop.",
        )
    }

    pub(crate) fn cta_label(&self) -> String {
        gs_copy("FEWMXW", "View Game Server")
    }
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_game_server_success_modal(
        &mut self,
        modal: GameServerSuccessModal,
        cx: &mut Context<Self>,
    ) {
        self.game_server_success_modal = Some(modal);
        cx.notify();
    }

    pub(crate) fn close_game_server_success_modal(&mut self) -> bool {
        self.game_server_success_modal.take().is_some()
    }

    pub(in crate::ui) fn game_server_success_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.game_server_success_modal.as_ref()?;
        let cover_img = modal
            .cover_url
            .as_deref()
            .and_then(|u| self.images.get(u).cloned());
        let title = modal.title();
        let description = modal.description();
        let cta = modal.cta_label();
        let game_name = modal.game_name.clone();

        Some(
            deferred(
                div()
                    .id("game-server-success-backdrop")
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
                            this.close_game_server_success_modal();
                            cx.notify();
                        }),
                    )
                    .child(
                        // `.card_idf9mq`: 440px, `--background-surface-high`, `--radius-lg` (16px)
                        div()
                            .id("game-server-success-modal")
                            .w(px(440.))
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
                            // `.art_idf9mq`: 228px art stage with blurred backdrop + glossy 120x160 cover card
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(228.))
                                    .bg(color("--background-base-low"))
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    // Soft celebration backdrop gradient
                                    .child(div().absolute().inset_0().bg(linear_gradient(
                                        135.,
                                        linear_color_stop(rgba(0x5865f244), 0.),
                                        linear_color_stop(rgba(0x35ed7e22), 1.),
                                    )))
                                    // Confetti accent dots (`.confettiCanvas_idf9mq`)
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(28.))
                                            .left(px(54.))
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(rgba(0xffe047cc)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(52.))
                                            .right(px(68.))
                                            .size(px(7.))
                                            .rounded_full()
                                            .bg(rgba(0x35ed7ecc)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom(px(36.))
                                            .left(px(82.))
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(rgba(0xe060a8cc)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom(px(44.))
                                            .right(px(90.))
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(rgba(0x34e2f1cc)),
                                    )
                                    // `.coverCard_idf9mq` (120x160 glossy cover card)
                                    .child(
                                        div()
                                            .relative()
                                            .w(px(120.))
                                            .h(px(160.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(rgba(0xffffff20))
                                            .bg(linear_gradient(
                                                149.,
                                                linear_color_stop(rgba(0x4752c4ff), 0.),
                                                linear_color_stop(rgba(0x1f2238ff), 1.),
                                            ))
                                            .overflow_hidden()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .p(px(10.))
                                            .child(match cover_img {
                                                Some(im) => img(im)
                                                    .size_full()
                                                    .object_fit(gpui::ObjectFit::Cover)
                                                    .into_any_element(),
                                                None => div()
                                                    .text_center()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .child(game_name)
                                                    .into_any_element(),
                                            })
                                            // `.coverGloss_idf9mq`
                                            .child(div().absolute().inset_0().bg(linear_gradient(
                                                149.,
                                                linear_color_stop(rgba(0xffffff33), 0.),
                                                linear_color_stop(rgba(0xffffff00), 0.36),
                                            ))),
                                    )
                                    // `.closeButton_idf9mq` (`cpT0Cq`)
                                    .child(
                                        div()
                                            .id("game-server-success-close")
                                            .absolute()
                                            .top(px(8.))
                                            .right(px(8.))
                                            .size(px(30.))
                                            .rounded_full()
                                            .bg(rgba(0x00000066))
                                            .hover(|s| s.bg(rgba(0x00000099)))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .aria_label(SharedString::from(dc_intl::t("cpT0Cq")))
                                            .child(icon("modal_close", 16., "--white"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.close_game_server_success_modal();
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // `.content_idf9mq` (padding 24px, gap 16px, centered)
                            .child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(16.))
                                    .child(
                                        div()
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
                                                    .max_w(px(341.))
                                                    .text_center()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-muted"))
                                                    .child(description),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("game-server-success-view")
                                            .h(px(38.))
                                            .px(px(18.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(cta)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.close_game_server_success_modal();
                                                this.open_shop(cx);
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
    pub(crate) fn script_game_server_success(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        match arg.trim() {
            "close" => {
                self.close_game_server_success_modal();
                cx.notify();
                "game server success closed".to_owned()
            }
            "downgrade" => {
                self.open_game_server_success_modal(
                    GameServerSuccessModal {
                        game_id: "minecraft".into(),
                        game_name: "Minecraft".into(),
                        cover_url: None,
                        is_plan_change: true,
                        is_downgrade: true,
                        renewal_date: Some("Dec 15, 2026".into()),
                    },
                    cx,
                );
                "game server success opened (downgrade)".to_owned()
            }
            _ => {
                self.open_game_server_success_modal(
                    GameServerSuccessModal {
                        game_id: "valheim".into(),
                        game_name: "Valheim".into(),
                        cover_url: None,
                        is_plan_change: false,
                        is_downgrade: false,
                        renewal_date: None,
                    },
                    cx,
                );
                "game server success opened (created)".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_server_success_978084_copy_variants() {
        assert!(dc_intl::has_key("+y0Tjy"));
        assert!(dc_intl::has_key("cpT0Cq"));
        let created = GameServerSuccessModal {
            game_id: "g1".into(),
            game_name: "Valheim".into(),
            cover_url: None,
            is_plan_change: false,
            is_downgrade: false,
            renewal_date: None,
        };
        assert!(!created.title().is_empty());
        assert!(!created.description().is_empty());
        assert!(!created.cta_label().is_empty());

        let updated = GameServerSuccessModal {
            is_plan_change: true,
            is_downgrade: true,
            renewal_date: Some("Dec 15, 2026".into()),
            ..created
        };
        assert_ne!(updated.title(), "Game server created!");
        assert!(updated.description().contains("Dec 15, 2026"));
    }
}
