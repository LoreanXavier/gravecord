//! The survey after the overlay is switched off (module 387101, opened by `pT` of module 158935 when the global or a
//! game's overlay goes off): the alert of module 189213 with "We’d love to learn more!", its subtitle, "Choose a
//! Reason" over the eight reasons (the radio group of module 771871: 16px apart, 8px above and 4px under), "Leave a
//! comment..." over a box of up to 1000 characters, Skip and Submit. Submit without a reason or a comment turns the
//! reason's label red; otherwise it waits a second (the analytics event is all it sends), says "Submitted" and closes
//! 1.5 seconds later. At most one survey in five days and three in a calendar month (`overlay_survey_timestamps`).

use super::{Root, color, editor_style, modal};
use crate::editor::Editor;
use chrono::{Datelike, TimeZone};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::time::Duration;

const STORAGE_KEY: &str = "overlay_survey_timestamps";
/// `ph` of module 158935.
const FIVE_DAYS_MS: i64 = 5 * 24 * 3_600_000;
const MAX_COMMENT: usize = 1000;
/// The reasons and their strings, in the module's order.
const REASONS: [(&str, &str); 8] = [
    ("not_working", "CHXHFX"),
    ("not_useful", "a95skc"),
    ("game_crashes_occurred", "IFZ0Sa"),
    ("poor_performance", "exRczv"),
    ("too_many_notifications", "XuBFBM"),
    ("multiple_monitors", "p8Ekyz"),
    ("in_the_way_of_game", "YP7Pin"),
    ("other", "YP/Ttl"),
];

pub(super) struct OverlaySurvey {
    reason: Option<usize>,
    comment: Entity<Editor>,
    invalid: bool,
    sending: bool,
    submitted: bool,
}

/// `pT`: whether a survey may open now, given when the earlier ones opened (milliseconds).
pub(super) fn survey_allowed(shown: &[i64], now: i64) -> bool {
    let last = shown.iter().copied().max().unwrap_or(0);
    if last > 0 && now - last < FIVE_DAYS_MS {
        return false;
    }
    let month = |at: i64| {
        chrono::Local
            .timestamp_millis_opt(at)
            .single()
            .map(|date| (date.year(), date.month()))
    };
    let this_month = month(now);
    shown.iter().filter(|at| month(**at) == this_month).count() < 3
}

fn shown() -> Vec<i64> {
    crate::storage::get(STORAGE_KEY)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_i64)
        .collect()
}

impl Root {
    /// The overlay was switched off: the survey opens at the next frame, unless one opened lately.
    pub(in crate::ui) fn overlay_switched_off(&mut self) {
        let now = chrono::Utc::now().timestamp_millis();
        let mut times = shown();
        if !survey_allowed(&times, now) {
            return;
        }
        times.push(now);
        crate::storage::set(STORAGE_KEY, json!(times));
        self.overlay_survey_pending = true;
    }

    /// The frame after the switch: the survey with its comment box.
    pub(super) fn open_pending_overlay_survey(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.overlay_survey_pending) {
            return;
        }
        let comment = cx.new(|cx| {
            let mut style = editor_style();
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            style.line_height = 21.;
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(dc_intl::t("gU+bVJ").to_owned());
            editor
        });
        self.overlay_survey = Some(OverlaySurvey {
            reason: None,
            comment,
            invalid: false,
            sending: false,
            submitted: false,
        });
        cx.notify();
    }

    fn submit_overlay_survey(&mut self, cx: &mut Context<Self>) {
        let Some(survey) = self.overlay_survey.as_mut() else {
            return;
        };
        if survey.sending || survey.submitted {
            return;
        }
        let comment = survey.comment.read(cx).text().to_owned();
        if survey.reason.is_none() && comment.trim().is_empty() {
            survey.invalid = true;
            cx.notify();
            return;
        }
        survey.sending = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(survey) = this.overlay_survey.as_mut() {
                    survey.sending = false;
                    survey.submitted = true;
                }
                cx.notify();
            });
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.overlay_survey = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// The radio of module 771871 (`radioIndicator_oJuRbG`: a 20px circle, `--radio-*` tokens, an 8px dot when picked).
    fn survey_radio(picked: bool) -> gpui::Div {
        div()
            .flex_none()
            .size(px(20.))
            .rounded_full()
            .border_1()
            .border_color(color(if picked {
                "--radio-border-selected-default"
            } else {
                "--radio-border-default"
            }))
            .bg(color(if picked {
                "--radio-background-selected-default"
            } else {
                "--radio-background-default"
            }))
            .flex()
            .items_center()
            .justify_center()
            .when(picked, |this| {
                this.child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .bg(color("--radio-thumb-background-active")),
                )
            })
    }

    pub(super) fn overlay_survey_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let survey = self.overlay_survey.as_ref()?;
        let label = |text: &'static str, tint: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color(tint))
                .child(dc_intl::t(text))
        };
        let options: Vec<AnyElement> = REASONS
            .iter()
            .enumerate()
            .map(|(index, (value, text))| {
                let picked = survey.reason == Some(index);
                div()
                    .id(SharedString::from(format!("overlay-survey-{value}")))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .cursor_pointer()
                    .child(Self::survey_radio(picked))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t(text)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(survey) = this.overlay_survey.as_mut() {
                            survey.reason = Some(index);
                            survey.invalid = false;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let length = survey.comment.read(cx).text().chars().count();
        if length > MAX_COMMENT {
            // `maxLength`: the box takes no more.
            survey.comment.update(cx, |editor, cx| {
                let kept: String = editor.text().chars().take(MAX_COMMENT).collect();
                editor.set_text(&kept);
                cx.notify();
            });
        }
        let body = div()
            .flex()
            .flex_col()
            .child(div().pb(px(8.)).child(label(
                "yhUvdd",
                if survey.invalid {
                    "--text-feedback-critical"
                } else {
                    "--text-muted"
                },
            )))
            .child(
                div()
                    .pt(px(8.))
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .children(options),
            )
            .child(
                div()
                    .pt(px(16.))
                    .child(div().pb(px(8.)).child(label("LYnrhc", "--text-muted")))
                    // Module 103557 (`autosize`, `rows` 3, `maxLength` 1000, no counter): `.textArea_uI0xjn`
                    // (`padding: var(--space-12) 10px`) in `.wrapper_UcGqzv`'s 1px border. Autosize adds one pixel
                    // to the rows (`Math.ceil(scrollHeight + 1)` of module 643135): 3 * 21 + 1 + 24 + 2.
                    .child(
                        div()
                            .w_full()
                            .min_h(px(3. * 21. + 1. + 24. + 2.))
                            .px(px(10.))
                            .py(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--input-background-default"))
                            .child(survey.comment.clone()),
                    ),
            );
        let submit_tone = if survey.sending {
            modal::Tone::PrimaryLoading
        } else if survey.submitted {
            modal::Tone::PrimaryDisabled
        } else {
            modal::Tone::Primary
        };
        let spec = modal::Spec {
            id: "overlay-survey-modal",
            title: dc_intl::t("9JKzxe").into(),
            subtitle: Some(div().child(dc_intl::t("2yjnmb")).into_any_element()),
            body: Some(body.into_any_element()),
            buttons: vec![
                modal::button(
                    "overlay-survey-skip",
                    dc_intl::t("5Wxrcd"),
                    modal::Tone::Secondary,
                    |this, _, _| this.overlay_survey = None,
                ),
                modal::button(
                    "overlay-survey-submit",
                    dc_intl::t(if survey.submitted { "bxiZU6" } else { "geKm7t" }),
                    submit_tone,
                    |this, _, cx| this.submit_overlay_survey(cx),
                ),
            ],
            close: Box::new(|this, _, _| this.overlay_survey = None),
        };
        Some(modal::render_small(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_survey_in_five_days_and_three_a_month() {
        let day = 24 * 3_600_000_i64;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 28, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        assert!(survey_allowed(&[], now));
        assert!(!survey_allowed(&[now - 2 * day], now));
        assert!(survey_allowed(&[now - 6 * day], now));
        // Three this month already.
        assert!(!survey_allowed(
            &[now - 20 * day, now - 13 * day, now - 6 * day],
            now
        ));
        // Two this month and one in the last one.
        assert!(survey_allowed(
            &[now - 40 * day, now - 13 * day, now - 6 * day],
            now
        ));
    }
}
