//! Premium Application Subscription Cancellation & Survey flow (module 548750).

#[cfg(feature = "capture")]
use super::editor_style;
use super::{Root, color, icon};
use crate::editor::Editor;
#[cfg(feature = "capture")]
use crate::editor::EditorEvent;
use dc_intl::Arg;
#[cfg(feature = "capture")]
use gpui::Window;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, deferred,
    div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppSubCancelStep {
    Confirm,
    Survey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppSubCancelScope {
    User,
    Guild,
}

/// Official `L` survey options of module 548750.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppSubSurveyChoice {
    NotUsingEnough,
    TooExpensive,
    NotWorthIt,
    NotSureHowToUse,
    Other,
}

impl AppSubSurveyChoice {
    pub(crate) const ALL: &[Self] = &[
        Self::NotUsingEnough,
        Self::TooExpensive,
        Self::NotWorthIt,
        Self::NotSureHowToUse,
        Self::Other,
    ];

    pub(crate) fn label(self) -> &'static str {
        let key = match self {
            Self::NotUsingEnough => "+RZgNV",
            Self::TooExpensive => "kgPXgv",
            Self::NotWorthIt => "vujsUK",
            Self::NotSureHowToUse => "AYAOLF",
            Self::Other => "wZLu3Y",
        };
        dc_intl::t(key)
    }
}

#[allow(dead_code)]
pub(crate) struct AppSubCancelModal {
    pub app_name: String,
    pub guild_name: Option<String>,
    pub scope: AppSubCancelScope,
    pub period_end_label: String,
    pub benefits: Vec<String>,
    pub error: Option<String>,
    pub step: AppSubCancelStep,
    pub selected: Option<AppSubSurveyChoice>,
    pub dont_show_again: bool,
    pub other_input: Entity<Editor>,
    _sub: Subscription,
}

impl Root {
    pub(crate) fn close_app_sub_cancel(&mut self) -> bool {
        self.app_sub_cancel_modal.take().is_some()
    }

    pub(in crate::ui) fn app_sub_cancel_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.app_sub_cancel_modal.as_ref()?;

        let close_btn = div()
            .id("appsub-cancel-close")
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
                this.close_app_sub_cancel();
                cx.notify();
            }));

        let content: AnyElement = match modal.step {
            AppSubCancelStep::Confirm => {
                let ts = Arg::When {
                    date: Cow::Borrowed(&modal.period_end_label),
                    time: Cow::Borrowed(""),
                };
                let subtitle = match modal.scope {
                    AppSubCancelScope::User => dc_intl::format(
                        "fZP9QD",
                        &[
                            ("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name))),
                            ("timestamp", ts),
                        ],
                    ),
                    AppSubCancelScope::Guild => match modal.guild_name.as_deref() {
                        Some(guild) => dc_intl::format(
                            "3LeWBF",
                            &[
                                ("guild", Arg::Str(Cow::Borrowed(guild))),
                                ("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name))),
                                ("timestamp", ts),
                            ],
                        ),
                        None => dc_intl::format(
                            "8vum6M",
                            &[
                                ("applicationName", Arg::Str(Cow::Borrowed(&modal.app_name))),
                                ("timestamp", ts),
                            ],
                        ),
                    },
                };

                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(72.))
                            .px(px(20.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .size(px(40.))
                                            .rounded(px(10.))
                                            .bg(rgba(0x00000033))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("bot_game_controller", 22., "--white")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child(modal.app_name.clone()),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("CeCHk1")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
                                    ),
                            )
                            .when_some(modal.error.clone(), |col, msg| {
                                col.child(
                                    div()
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .bg(rgba(0xf23f421f))
                                        .border_1()
                                        .border_color(color("--text-feedback-critical"))
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(msg),
                                )
                            })
                            .child(
                                div()
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .children(modal.benefits.iter().map(|b| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(
                                                "circle_check",
                                                16.,
                                                "--control-primary-background-default",
                                            ))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-strong"))
                                                    .child(b.clone()),
                                            )
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(
                                div()
                                    .id("appsub-cancel-confirm-btn")
                                    .h(px(38.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-critical-primary-background-default"))
                                    .hover(|s| {
                                        s.bg(color("--control-critical-primary-background-hover"))
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("KSqyfW"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.app_sub_cancel_modal.as_mut() {
                                            m.step = AppSubCancelStep::Survey;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            AppSubCancelStep::Survey => {
                let selected = modal.selected;
                let dont_show = modal.dont_show_again;
                let can_submit = selected.is_some();

                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(20.))
                            .pt(px(18.))
                            .pb(px(12.))
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("zPxMdQ")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("QJGdB+")),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .pb(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                // Official `.answerChoicesContainer_11vtwc`: bg --background-mod-subtle, radius 8px, padding 12px.
                                div()
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .children(AppSubSurveyChoice::ALL.iter().enumerate().map(
                                        |(i, &choice)| {
                                            let active = selected == Some(choice);
                                            div()
                                                .id(SharedString::from(format!(
                                                    "appsub-choice-{i}"
                                                )))
                                                .px(px(10.))
                                                .py(px(8.))
                                                .rounded(px(6.))
                                                .bg(if active {
                                                    color("--background-surface-higher")
                                                } else {
                                                    gpui::hsla(0., 0., 0., 0.).into()
                                                })
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .gap(px(10.))
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .size(px(18.))
                                                        .rounded_full()
                                                        .border_2()
                                                        .border_color(if active {
                                                            color(
                                                                "--control-primary-background-default",
                                                            )
                                                        } else {
                                                            color("--border-strong")
                                                        })
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .when(active, |d| {
                                                            d.child(
                                                                div()
                                                                    .size(px(8.))
                                                                    .rounded_full()
                                                                    .bg(color(
                                                                        "--control-primary-background-default",
                                                                    )),
                                                            )
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .text_size(px(14.))
                                                        .text_color(color("--text-strong"))
                                                        .child(choice.label()),
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.app_sub_cancel_modal.as_mut()
                                                    {
                                                        m.selected = Some(choice);
                                                    }
                                                    cx.notify();
                                                }))
                                        },
                                    )),
                            )
                            .when(selected == Some(AppSubSurveyChoice::Other), |col| {
                                col.child(
                                    div()
                                        .min_h(px(68.))
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--input-border-default"))
                                        .bg(color("--input-background-default"))
                                        .child(modal.other_input.clone()),
                                )
                            })
                            .child(
                                div()
                                    .id("appsub-dont-show")
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .size(px(18.))
                                            .rounded(px(4.))
                                            .border_1()
                                            .border_color(if dont_show {
                                                color("--control-primary-background-default")
                                            } else {
                                                color("--border-strong")
                                            })
                                            .bg(if dont_show {
                                                color("--control-primary-background-default")
                                            } else {
                                                gpui::hsla(0., 0., 0., 0.).into()
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(dont_show, |b| {
                                                b.child(icon("checkbox_check", 12., "--white"))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("3vPFQi")),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.app_sub_cancel_modal.as_mut() {
                                            m.dont_show_again = !m.dont_show_again;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(
                                div()
                                    .id("appsub-survey-submit")
                                    .h(px(38.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(can_submit, |b| {
                                        b.hover(|s| {
                                            s.bg(color("--control-primary-background-hover"))
                                        })
                                        .cursor_pointer()
                                    })
                                    .when(!can_submit, |b| b.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("geKm7t"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if can_submit {
                                            this.close_app_sub_cancel();
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("appsub-cancel-modal")
            .occlude()
            .w(px(460.))
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
                    .id("appsub-cancel-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_app_sub_cancel();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_app_sub_cancel(
        &mut self,
        arg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let other_input = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder(dc_intl::t("s6tM8c").to_owned());
            ed
        });
        let sub = cx.subscribe_in(&other_input, window, |_, _, _: &EditorEvent, _, cx| {
            cx.notify();
        });
        let (step, selected) = match arg {
            "confirm" => (AppSubCancelStep::Confirm, None),
            _ => (AppSubCancelStep::Survey, Some(AppSubSurveyChoice::Other)),
        };
        self.app_sub_cancel_modal = Some(AppSubCancelModal {
            app_name: "Midjourney Pro".to_owned(),
            guild_name: Some("gravecord fixture".to_owned()),
            scope: AppSubCancelScope::Guild,
            period_end_label: "Dec 31, 2026".to_owned(),
            benefits: vec![
                "Priority image generation queue".to_owned(),
                "Unlimited upscales across server channels".to_owned(),
            ],
            error: None,
            step,
            selected,
            dont_show_again: false,
            other_input,
            _sub: sub,
        });
        cx.notify();
        format!("app-sub-cancel: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_15_keys_of_app_sub_cancel_548750_resolve() {
        assert_eq!(AppSubSurveyChoice::ALL.len(), 5);
        for &c in AppSubSurveyChoice::ALL {
            assert!(!c.label().is_empty());
        }
        for key in [
            "3LeWBF", "3vPFQi", "8vum6M", "CeCHk1", "KSqyfW", "QJGdB+", "fZP9QD", "geKm7t",
            "s6tM8c", "zPxMdQ",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
