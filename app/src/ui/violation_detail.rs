//! Safety Hub Account Standing Violation Detail modal (module 163002):
//! Classification header (`$`), "What this means for you" (`B`), "What you can do" (`Q`),
//! Policy Explainer card (`q`), Appeal Footer (`H`), and COPPA minimum-age section (`z`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ViolationOrigin {
    User,
    OwnedGuild,
    JoinedGuild,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ViolationAppealState {
    Eligible,
    Appealed,
    Expired,
    CoppaAgeVerify,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ViolationDetailModal {
    pub classification: String,
    pub guild_name: Option<String>,
    pub origin: ViolationOrigin,
    pub actions: Vec<String>,
    pub expiration_date: Option<String>,
    pub explainer_url: String,
    pub appeal: ViolationAppealState,
}

impl Root {
    pub(crate) fn close_violation_detail(&mut self) -> bool {
        self.violation_detail_modal.take().is_some()
    }

    pub(in crate::ui) fn violation_detail_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.violation_detail_modal.as_ref()?;
        let explainer_url = modal.explainer_url.clone();

        // Header `$`: HpvELh / X1ngSd / rmpEPD with `classificationHook` bold.
        let header_runs = match modal.origin {
            ViolationOrigin::User => dc_intl::format_runs(
                "HpvELh",
                &[(
                    "classification_type",
                    Arg::Str(Cow::Borrowed(&modal.classification)),
                )],
            ),
            ViolationOrigin::OwnedGuild => dc_intl::format_runs(
                "X1ngSd",
                &[
                    (
                        "guildName",
                        Arg::Str(Cow::Borrowed(
                            modal.guild_name.as_deref().unwrap_or("Server"),
                        )),
                    ),
                    (
                        "classification_type",
                        Arg::Str(Cow::Borrowed(&modal.classification)),
                    ),
                ],
            ),
            ViolationOrigin::JoinedGuild => dc_intl::format_runs(
                "rmpEPD",
                &[
                    (
                        "guildName",
                        Arg::Str(Cow::Borrowed(
                            modal.guild_name.as_deref().unwrap_or("Server"),
                        )),
                    ),
                    (
                        "classification_type",
                        Arg::Str(Cow::Borrowed(&modal.classification)),
                    ),
                ],
            ),
        };

        let tos_runs = dc_intl::format_runs(
            "1Z/+aA",
            &[
                (
                    "tosLink",
                    Arg::Str(Cow::Borrowed("https://discord.com/terms")),
                ),
                (
                    "communityGuidelinesLink",
                    Arg::Str(Cow::Borrowed("https://discord.com/guidelines")),
                ),
            ],
        );

        let explainer_text = dc_intl::format(
            "zxUdpj",
            &[(
                "classificationDescription",
                Arg::Str(Cow::Borrowed(&modal.classification)),
            )],
        );

        let tos_paragraph = div()
            .flex()
            .flex_wrap()
            .text_size(px(13.))
            .line_height(px(18.))
            .text_color(color("--text-muted"))
            .children(tos_runs.into_iter().enumerate().map(|(i, (chunk, hook))| {
                let is_link = hook.as_deref() == Some("$link");
                if is_link {
                    let target = if chunk.contains("Terms") {
                        "https://discord.com/terms"
                    } else {
                        "https://discord.com/guidelines"
                    };
                    div()
                        .id(SharedString::from(format!("viol-link-{i}")))
                        .text_color(color("--text-link"))
                        .cursor_pointer()
                        .child(chunk)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.open_url(target);
                        }))
                        .into_any_element()
                } else {
                    div().child(chunk).into_any_element()
                }
            }));

        let appeal_footer: AnyElement = match modal.appeal {
            ViolationAppealState::Appealed => div()
                .text_size(px(13.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("I2H0/E"))
                .into_any_element(),
            ViolationAppealState::Expired => div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("PHriyz")),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("YsxY1D")),
                )
                .into_any_element(),
            ViolationAppealState::Eligible => {
                let runs = dc_intl::format_runs("IFxUaT", &[]);
                div()
                    .flex()
                    .flex_wrap()
                    .text_size(px(13.))
                    .text_color(color("--text-muted"))
                    .children(runs.into_iter().enumerate().map(|(i, (chunk, hook))| {
                        if hook.as_deref() == Some("letUsKnowHook") {
                            div()
                                .id(SharedString::from(format!("viol-appeal-{i}")))
                                .text_color(color("--text-link"))
                                .cursor_pointer()
                                .child(chunk)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.violation_detail_modal.clone() {
                                        let token = this
                                            .suspended_safety_hub
                                            .as_ref()
                                            .map(|h| h.suspended_token.clone());
                                        this.open_violation_appeal(
                                            format!("violation:{}", m.classification),
                                            m.classification,
                                            m.explainer_url,
                                            m.appeal == ViolationAppealState::CoppaAgeVerify,
                                            token,
                                            cx,
                                        );
                                    }
                                }))
                                .into_any_element()
                        } else {
                            div().child(chunk).into_any_element()
                        }
                    }))
                    .into_any_element()
            }
            ViolationAppealState::CoppaAgeVerify => div().into_any_element(),
        };

        // Official `.modalRoot_bmGE2U`: width 600px, `.body_QJMoIk`: gap 24px.
        let dialog = div()
            .id("violation-detail-modal")
            .occlude()
            .w(px(600.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.35),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_wrap()
                            .text_size(px(22.))
                            .line_height(px(28.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .children(header_runs.into_iter().map(|(chunk, hook)| {
                                div()
                                    .when(hook.as_deref() == Some("classificationHook"), |d| {
                                        d.font_weight(FontWeight(700.))
                                    })
                                    .child(chunk)
                            })),
                    )
                    .child(
                        div()
                            .id("violation-detail-close")
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
                                this.close_violation_detail();
                                cx.notify();
                            })),
                    ),
            )
            // "What this means for you" (`O2nYk+`) + bullet rows + expiration date (`TByIjT`)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("O2nYk+")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(modal.actions.iter().map(|act| {
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .mt(px(7.))
                                            .size(px(5.))
                                            .rounded_full()
                                            .bg(color("--text-strong")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(act.clone()),
                                    )
                            }))
                            .when_some(modal.expiration_date.as_deref(), |col, exp| {
                                let line = dc_intl::format(
                                    "TByIjT",
                                    &[("expirationDate", Arg::Str(Cow::Borrowed(exp)))],
                                );
                                col.child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .mt(px(7.))
                                                .size(px(5.))
                                                .rounded_full()
                                                .bg(color("--text-strong")),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-default"))
                                                .child(line),
                                        ),
                                )
                            }),
                    ),
            )
            // Either COPPA age verification (`z`) or Standard "What you can do" (`Q`)
            .child(if modal.appeal == ViolationAppealState::CoppaAgeVerify {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-subtle"))
                            .child(dc_intl::t("RVEiD0")),
                    )
                    .child(
                        div()
                            .id("viol-coppa-verify")
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(14.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("YQPbuc")),
                            )
                            .child(icon(
                                "bot_chevron_small_right",
                                18.,
                                "--interactive-text-default",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.violation_detail_modal.as_mut() {
                                    m.appeal = ViolationAppealState::Appealed;
                                }
                                cx.notify();
                            })),
                    )
                    .child(tos_paragraph)
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("977iei")),
                    )
                    .child(tos_paragraph)
                    .child(
                        div()
                            .id("viol-policy-explainer")
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(icon("role_shield", 20., "--border-focus"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(explainer_text),
                            )
                            .child(icon(
                                "bot_chevron_small_right",
                                18.,
                                "--interactive-text-default",
                            ))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.open_url(&explainer_url);
                            })),
                    )
                    .child(appeal_footer)
            });

        Some(
            deferred(
                div()
                    .id("violation-detail-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_violation_detail();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_violation_detail(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let (origin, appeal) = match arg {
            "coppa" => (ViolationOrigin::User, ViolationAppealState::CoppaAgeVerify),
            "expired" => (ViolationOrigin::OwnedGuild, ViolationAppealState::Expired),
            "appealed" => (ViolationOrigin::JoinedGuild, ViolationAppealState::Appealed),
            _ => (ViolationOrigin::User, ViolationAppealState::Eligible),
        };
        self.violation_detail_modal = Some(ViolationDetailModal {
            classification: "Spam & Platform Manipulation".to_owned(),
            guild_name: Some("gravecord fixture".to_owned()),
            origin,
            actions: vec![
                "Your account has received a temporary warning.".to_owned(),
                "Direct messages to non-friends are limited for 24 hours.".to_owned(),
            ],
            expiration_date: Some("Nov 20, 2027".to_owned()),
            explainer_url: "https://discord.com/safety/spam-policy-explainer".to_owned(),
            appeal,
        });
        cx.notify();
        format!("violation-detail: {arg}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_14_keys_of_violation_detail_163002_resolve() {
        for key in [
            "1Z/+aA", "977iei", "HpvELh", "I2H0/E", "IFxUaT", "O2nYk+", "PHriyz", "RVEiD0",
            "TByIjT", "X1ngSd", "YQPbuc", "YsxY1D", "rmpEPD", "zxUdpj",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
