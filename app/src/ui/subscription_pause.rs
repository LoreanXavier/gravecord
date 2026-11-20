//! Pause or Cancel Nitro Subscription modal (module 318998):
//! `PremiumSubscriptionPauseModalSelect` duration/cancel picker and `cN` pause confirmation step.

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PauseSubStatus {
    Active,
    Paused,
    PastDue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PauseSubStep {
    Select,
    Confirm,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct SubscriptionPauseModal {
    pub status: PauseSubStatus,
    pub step: PauseSubStep,
    pub durations_days: Vec<u32>,
    /// Selected value in days (`0` means "Cancel subscription").
    pub selected_days: u32,
    pub pause_date_label: String,
    pub resume_date_label: String,
    pub has_error: bool,
}

impl Root {
    pub(crate) fn close_subscription_pause(&mut self) -> bool {
        self.subscription_pause_modal.take().is_some()
    }

    pub(in crate::ui) fn subscription_pause_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.subscription_pause_modal.as_ref()?;
        let selected_days = modal.selected_days;

        let close_btn = div()
            .id("sub-pause-close")
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
                this.close_subscription_pause();
                cx.notify();
            }));

        let content: AnyElement = match modal.step {
            PauseSubStep::Select => {
                let subtitle = if modal.status == PauseSubStatus::Paused {
                    dc_intl::t("Lp9WoG")
                } else {
                    dc_intl::t("eSR83U")
                };
                let dur_key = if modal.status == PauseSubStatus::Paused {
                    "o3upfT"
                } else {
                    "dBXZEm"
                };

                let mut rows: Vec<(u32, String, bool)> = modal
                    .durations_days
                    .iter()
                    .map(|&days| {
                        let label = dc_intl::format(dur_key, &[("days", Arg::Num(days as f64))]);
                        (days, label, false)
                    })
                    .collect();
                rows.push((0, dc_intl::t("OCPUM6").to_owned(), true));

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
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("f3nnB/")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
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
                            .gap(px(8.))
                            .children(rows.into_iter().enumerate().map(
                            |(i, (val, label, is_cancel))| {
                                let active = selected_days == val;
                                div()
                                    .id(SharedString::from(format!("sub-pause-opt-{i}")))
                                    .px(px(14.))
                                    .py(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(if active {
                                        if is_cancel {
                                            color("--text-feedback-critical")
                                        } else {
                                            color("--control-primary-background-default")
                                        }
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .bg(if active {
                                        color("--background-mod-subtle")
                                    } else {
                                        color("--background-base-lower")
                                    })
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .size(px(18.))
                                            .rounded_full()
                                            .border_2()
                                            .border_color(if active {
                                                if is_cancel {
                                                    color("--text-feedback-critical")
                                                } else {
                                                    color("--control-primary-background-default")
                                                }
                                            } else if is_cancel {
                                                color("--text-feedback-critical")
                                            } else {
                                                color("--border-strong")
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(active, |d| {
                                                d.child(div().size(px(8.)).rounded_full().bg(
                                                    if is_cancel {
                                                        color("--text-feedback-critical")
                                                    } else {
                                                        color(
                                                            "--control-primary-background-default",
                                                        )
                                                    },
                                                ))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(if is_cancel {
                                                color("--text-feedback-critical")
                                            } else {
                                                color("--text-strong")
                                            })
                                            .child(label),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.subscription_pause_modal.as_mut() {
                                            m.selected_days = val;
                                        }
                                        cx.notify();
                                    }))
                            },
                        )),
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
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("sub-pause-keep")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("rzVN6j"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_subscription_pause();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("sub-pause-continue")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("3PatSz"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.subscription_pause_modal.as_mut() {
                                            if m.selected_days == 0 {
                                                this.close_subscription_pause();
                                            } else {
                                                m.step = PauseSubStep::Confirm;
                                            }
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            PauseSubStep::Confirm => {
                let days = selected_days.max(30);
                let pause_when = Arg::When {
                    date: Cow::Borrowed(&modal.pause_date_label),
                    time: Cow::Borrowed(""),
                };
                let resume_when = Arg::When {
                    date: Cow::Borrowed(&modal.resume_date_label),
                    time: Cow::Borrowed(""),
                };
                let runs = match modal.status {
                    PauseSubStatus::PastDue => dc_intl::format_runs(
                        "xaS18/",
                        &[
                            ("pauseDuration", Arg::Num(days as f64)),
                            ("resumeDate", resume_when),
                        ],
                    ),
                    PauseSubStatus::Paused => {
                        dc_intl::format_runs("Vur3Fc", &[("resumeDate", resume_when)])
                    }
                    PauseSubStatus::Active => dc_intl::format_runs(
                        "W85vFA",
                        &[
                            ("pauseDate", pause_when),
                            ("resumeDate", resume_when),
                            ("pauseDuration", Arg::Num(days as f64)),
                        ],
                    ),
                };

                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(20.))
                            .pt(px(18.))
                            .pb(px(12.))
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
                                    .child(dc_intl::t("AnMG5x")),
                            )
                            .child(close_btn),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .pb(px(18.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .when(modal.has_error, |col| {
                                col.child(
                                    div()
                                        .p(px(12.))
                                        .rounded(px(8.))
                                        .bg(rgba(0xf23f421f))
                                        .border_1()
                                        .border_color(color("--text-feedback-critical"))
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("5mlOCW")),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .children(runs.into_iter().map(|(chunk, hook)| {
                                        div()
                                            .when(hook.as_deref() == Some("$b"), |d| {
                                                d.font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                            })
                                            .child(chunk)
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
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("sub-pause-confirm-keep")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("rzVN6j"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_subscription_pause();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("sub-pause-confirm-ok")
                                    .h(px(38.))
                                    .px(px(16.))
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
                                    .child(dc_intl::t("cY+Oob"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_subscription_pause();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        let dialog = div()
            .id("subscription-pause-modal")
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
                    .id("subscription-pause-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_subscription_pause();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_subscription_pause(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let (status, step, err) = match arg {
            "paused" => (PauseSubStatus::Paused, PauseSubStep::Select, false),
            "confirm" => (PauseSubStatus::Active, PauseSubStep::Confirm, false),
            "error" => (PauseSubStatus::PastDue, PauseSubStep::Confirm, true),
            _ => (PauseSubStatus::Active, PauseSubStep::Select, false),
        };
        self.subscription_pause_modal = Some(SubscriptionPauseModal {
            status,
            step,
            durations_days: vec![30, 60, 90],
            selected_days: 30,
            pause_date_label: "Dec 1, 2026".to_owned(),
            resume_date_label: "Dec 31, 2026".to_owned(),
            has_error: err,
        });
        cx.notify();
        format!("subscription-pause: {arg}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_14_keys_of_subscription_pause_318998_resolve() {
        for key in [
            "3PatSz", "5mlOCW", "AnMG5x", "Lp9WoG", "OCPUM6", "Vur3Fc", "W85vFA", "cY+Oob",
            "dBXZEm", "eSR83U", "f3nnB/", "o3upfT", "rzVN6j", "xaS18/",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
