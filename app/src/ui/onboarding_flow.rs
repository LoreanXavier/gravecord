//! Member-facing Server Onboarding 4-step flow (module 215764):
//! Cover (`W`), Connect Your Account (`F`), Onboarding Prompts (`ea`/`er`/`el`), and Server Rules (`ec`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum OnboardingStep {
    Cover,
    Connections,
    Prompt(usize),
    Rules,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct OnboardingOption {
    pub id: String,
    pub title: String,
    pub description: String,
    pub emoji: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct OnboardingPrompt {
    pub title: String,
    pub required: bool,
    pub options: Vec<OnboardingOption>,
    pub selected: Vec<String>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct OnboardingConnection {
    pub provider: String,
    pub description: String,
    pub connected: bool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct OnboardingFlowModal {
    pub guild_name: String,
    pub step: OnboardingStep,
    pub loading: bool,
    pub not_applicable: bool,
    pub connections: Vec<OnboardingConnection>,
    pub prompts: Vec<OnboardingPrompt>,
    pub rules: Vec<String>,
    pub rules_agreed: bool,
}

impl Root {
    pub(crate) fn close_onboarding_flow(&mut self) -> bool {
        self.onboarding_flow_modal.take().is_some()
    }

    pub(in crate::ui) fn onboarding_flow_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.onboarding_flow_modal.as_ref()?;
        let has_conns = !modal.connections.is_empty();
        let has_rules = !modal.rules.is_empty();
        let prompt_count = modal.prompts.len();

        let close_btn = div()
            .id("onboard-flow-close")
            .size(px(30.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("modal_close", 18., "--interactive-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_onboarding_flow();
                cx.notify();
            }));

        let body: AnyElement = match modal.step {
            OnboardingStep::Cover => {
                let welcome = dc_intl::format(
                    "j59F/c",
                    &[("guildName", Arg::Str(Cow::Borrowed(&modal.guild_name)))],
                );
                let sub = if modal.not_applicable {
                    dc_intl::t("Q5ibtV")
                } else {
                    dc_intl::t("Alcl/e")
                };
                let privacy = dc_intl::format("kI6UoD", &[]);
                div()
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(20.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(
                                modal
                                    .guild_name
                                    .chars()
                                    .take(2)
                                    .collect::<String>()
                                    .to_uppercase(),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .text_color(color("--text-muted"))
                            .child(welcome),
                    )
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(sub),
                    )
                    .child(
                        div()
                            .id("onboard-cover-start")
                            .mt(px(8.))
                            .h(px(40.))
                            .px(px(24.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("PDTjLN"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.onboarding_flow_modal.as_mut() {
                                    if !m.connections.is_empty() {
                                        m.step = OnboardingStep::Connections;
                                    } else if !m.prompts.is_empty() {
                                        m.step = OnboardingStep::Prompt(0);
                                    } else if !m.rules.is_empty() {
                                        m.step = OnboardingStep::Rules;
                                    } else {
                                        this.onboarding_flow_modal = None;
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(11.))
                            .text_color(color("--text-muted"))
                            .text_center()
                            .child(privacy),
                    )
                    .into_any_element()
            }
            OnboardingStep::Connections => {
                if modal.loading {
                    div()
                        .py(px(64.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("ZTNur7")),
                        )
                        .into_any_element()
                } else {
                    let any_connected = modal.connections.iter().any(|c| c.connected);
                    let is_last = prompt_count == 0 && !has_rules;
                    let next_label = if any_connected {
                        if is_last {
                            format!("{} 🎉", dc_intl::t("8SuVoE"))
                        } else {
                            dc_intl::t("PDTjLN").to_owned()
                        }
                    } else {
                        dc_intl::t("5Wxrcd").to_owned()
                    };
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(20.))
                                .pb(px(14.))
                                .flex()
                                .items_start()
                                .justify_between()
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
                                                .child(dc_intl::t("eDVMrA")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("BozOXu")),
                                        ),
                                )
                                .child(close_btn),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .pb(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .children(modal.connections.iter().enumerate().map(|(i, conn)| {
                                    let is_conn = conn.connected;
                                    div()
                                        .id(SharedString::from(format!("onboard-conn-{i}")))
                                        .p(px(12.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(if is_conn {
                                            color("--status-positive")
                                        } else {
                                            color("--border-subtle")
                                        })
                                        .bg(color("--background-base-lower"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(conn.provider.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color("--text-muted"))
                                                        .child(conn.description.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .size(px(22.))
                                                .rounded_full()
                                                .bg(if is_conn {
                                                    color("--status-positive")
                                                } else {
                                                    color("--control-secondary-background-default")
                                                })
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(is_conn, |d| {
                                                    d.child(icon("checkbox_check", 12., "--white"))
                                                }),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.onboarding_flow_modal.as_mut()
                                                && let Some(c) = m.connections.get_mut(i)
                                            {
                                                c.connected = !c.connected;
                                            }
                                            cx.notify();
                                        }))
                                })),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .py(px(14.))
                                .bg(color("--background-base-lower"))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .justify_end()
                                .child(
                                    div()
                                        .id("onboard-conn-next")
                                        .h(px(36.))
                                        .px(px(18.))
                                        .rounded(px(8.))
                                        .bg(if any_connected {
                                            color("--control-primary-background-default")
                                        } else {
                                            color("--control-secondary-background-default")
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(if any_connected {
                                            color("--white")
                                        } else {
                                            color("--text-strong")
                                        })
                                        .child(next_label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.onboarding_flow_modal.as_mut() {
                                                if !m.prompts.is_empty() {
                                                    m.step = OnboardingStep::Prompt(0);
                                                } else if !m.rules.is_empty() {
                                                    m.step = OnboardingStep::Rules;
                                                } else {
                                                    this.onboarding_flow_modal = None;
                                                }
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .into_any_element()
                }
            }
            OnboardingStep::Prompt(idx) => {
                let prompt = modal.prompts.get(idx)?;
                let q_label = dc_intl::format(
                    "isV0NW",
                    &[
                        ("currentQuestion", Arg::Num((idx + 1) as f64)),
                        ("questionCount", Arg::Num(prompt_count as f64)),
                    ],
                );
                let empty_sel = prompt.selected.is_empty();
                let blocked_required = empty_sel && prompt.required;
                let is_last = idx + 1 >= prompt_count && !has_rules;
                let next_label = if is_last {
                    format!("{} 🎉", dc_intl::t("8SuVoE"))
                } else if empty_sel {
                    dc_intl::t("5Wxrcd").to_owned()
                } else {
                    dc_intl::t("PDTjLN").to_owned()
                };
                let can_back = idx > 0 || has_conns;

                let next_btn = div()
                    .id("onboard-prompt-next")
                    .h(px(36.))
                    .px(px(18.))
                    .rounded(px(8.))
                    .bg(if empty_sel {
                        color("--control-secondary-background-default")
                    } else {
                        color("--control-primary-background-default")
                    })
                    .when(blocked_required, |d| d.opacity(0.5))
                    .when(!blocked_required, |d| d.cursor_pointer())
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(if empty_sel {
                        color("--text-strong")
                    } else {
                        color("--white")
                    })
                    .child(next_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if blocked_required {
                            return;
                        }
                        if let Some(m) = this.onboarding_flow_modal.as_mut() {
                            if idx + 1 < m.prompts.len() {
                                m.step = OnboardingStep::Prompt(idx + 1);
                            } else if !m.rules.is_empty() {
                                m.step = OnboardingStep::Rules;
                            } else {
                                this.onboarding_flow_modal = None;
                            }
                        }
                        cx.notify();
                    }));

                let next_wrapped = if blocked_required {
                    self.with_tip(
                        "onboard-req-tip",
                        super::tooltip::Tip::text(dc_intl::t("dA1dSf"), super::tooltip::Side::Top),
                        next_btn,
                        cx,
                    )
                    .into_any_element()
                } else {
                    next_btn.into_any_element()
                };

                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-muted"))
                                                    .child(q_label),
                                            )
                                            .when(prompt.required, |row| {
                                                row.child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color("--text-muted"))
                                                        .child("·"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-link"))
                                                        .child(dc_intl::t("Ur8Vrt")),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(prompt.title.clone()),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(20.))
                            .flex()
                            .flex_wrap()
                            .gap(px(10.))
                            .children(prompt.options.iter().enumerate().map(|(oi, opt)| {
                                let is_sel = prompt.selected.contains(&opt.id);
                                let oid = opt.id.clone();
                                div()
                                    .id(SharedString::from(format!("onboard-opt-{idx}-{oi}")))
                                    .w(px(250.))
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(if is_sel {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .bg(if is_sel {
                                        color("--background-mod-subtle")
                                    } else {
                                        color("--background-base-lower")
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(div().text_size(px(20.)).child(opt.emoji.clone()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(opt.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(opt.description.clone()),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.onboarding_flow_modal.as_mut()
                                            && let Some(p) = m.prompts.get_mut(idx)
                                        {
                                            if let Some(pos) =
                                                p.selected.iter().position(|x| x == &oid)
                                            {
                                                p.selected.remove(pos);
                                            } else {
                                                p.selected.push(oid.clone());
                                            }
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().when(can_back, |slot| {
                                slot.child(
                                    div()
                                        .id("onboard-prompt-back")
                                        .h(px(36.))
                                        .px(px(14.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("13/7kX"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.onboarding_flow_modal.as_mut() {
                                                if idx > 0 {
                                                    m.step = OnboardingStep::Prompt(idx - 1);
                                                } else if !m.connections.is_empty() {
                                                    m.step = OnboardingStep::Connections;
                                                }
                                            }
                                            cx.notify();
                                        })),
                                )
                            }))
                            .child(next_wrapped),
                    )
                    .into_any_element()
            }
            OnboardingStep::Rules => {
                let agreed = modal.rules_agreed;
                let finish_text = format!("{} 🎉", dc_intl::t("8SuVoE"));
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("b1JC+l")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("prJqwT")),
                                    ),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .children(modal.rules.iter().enumerate().map(|(i, rule)| {
                                div()
                                    .p(px(10.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(format!("{}.", i + 1)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-default"))
                                            .child(rule.clone()),
                                    )
                            }))
                            .child(
                                div()
                                    .id("onboard-rules-toggle")
                                    .mt(px(4.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(18.))
                                            .rounded(px(4.))
                                            .border_1()
                                            .border_color(if agreed {
                                                color("--control-primary-background-default")
                                            } else {
                                                color("--border-strong")
                                            })
                                            .bg(if agreed {
                                                color("--control-primary-background-default")
                                            } else {
                                                gpui::hsla(0., 0., 0., 0.).into()
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(agreed, |b| {
                                                b.child(icon("checkbox_check", 12., "--white"))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(if agreed {
                                                dc_intl::t("arAe3I")
                                            } else {
                                                dc_intl::t("D0CVAc")
                                            }),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.onboarding_flow_modal.as_mut() {
                                            m.rules_agreed = !m.rules_agreed;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .id("onboard-rules-back")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.onboarding_flow_modal.as_mut() {
                                            if !m.prompts.is_empty() {
                                                m.step =
                                                    OnboardingStep::Prompt(m.prompts.len() - 1);
                                            } else if !m.connections.is_empty() {
                                                m.step = OnboardingStep::Connections;
                                            } else {
                                                m.step = OnboardingStep::Cover;
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("onboard-rules-finish")
                                    .h(px(36.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!agreed, |b| b.opacity(0.5))
                                    .when(agreed, |b| b.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(finish_text)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if agreed {
                                            this.close_onboarding_flow();
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("onboarding-flow-modal")
            .occlude()
            .w(px(560.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(body);

        Some(
            deferred(
                div()
                    .id("onboarding-flow-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_onboarding_flow();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_onboarding_flow(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let step = match arg {
            "cover" => OnboardingStep::Cover,
            "connections" => OnboardingStep::Connections,
            "rules" => OnboardingStep::Rules,
            _ => OnboardingStep::Prompt(0),
        };
        self.onboarding_flow_modal = Some(OnboardingFlowModal {
            guild_name: "gravecord fixture".to_owned(),
            step,
            loading: false,
            not_applicable: false,
            connections: vec![
                OnboardingConnection {
                    provider: "Steam".to_owned(),
                    description: "Verify your game library for LFG channels.".to_owned(),
                    connected: true,
                },
                OnboardingConnection {
                    provider: "GitHub".to_owned(),
                    description: "Unlock contributor channels and dev alerts.".to_owned(),
                    connected: false,
                },
            ],
            prompts: vec![OnboardingPrompt {
                title: "What brings you to gravecord fixture?".to_owned(),
                required: true,
                options: vec![
                    OnboardingOption {
                        id: "opt1".to_owned(),
                        title: "Gaming & Squads".to_owned(),
                        description: "Voice chats and weekend lobbies.".to_owned(),
                        emoji: "🎮".to_owned(),
                    },
                    OnboardingOption {
                        id: "opt2".to_owned(),
                        title: "Rust & GPUI Dev".to_owned(),
                        description: "Commits, reviews, and parity work.".to_owned(),
                        emoji: "🦀".to_owned(),
                    },
                ],
                selected: vec!["opt2".to_owned()],
            }],
            rules: vec![
                "Be respectful and keep channels on topic.".to_owned(),
                "No spam or unsolicited promotions.".to_owned(),
            ],
            rules_agreed: true,
        });
        cx.notify();
        format!("onboarding-flow: {arg}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_18_keys_of_onboarding_flow_215764_resolve() {
        for key in [
            "ZTNur7", "eDVMrA", "BozOXu", "8SuVoE", "PDTjLN", "5Wxrcd", "j59F/c", "Q5ibtV",
            "Alcl/e", "isV0NW", "Ur8Vrt", "13/7kX", "dA1dSf", "b1JC+l", "prJqwT", "arAe3I",
            "D0CVAc", "kI6UoD",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
