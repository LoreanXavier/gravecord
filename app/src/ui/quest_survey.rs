//! Quest dismiss survey modal (modules 321538 + 798899).
//!
//! Opened when dismissing a Quest (`321538` `en()` -> `798899` `QuestSurveyModal`, 467px wide):
//! - Slide 0 (`ugbhDS` "Not interested in this Quest?", `FIYcpC` "Let us know the main reason why!"):
//!   5 clickable reason rows with right chevron (`ChevronSmallRightIcon`):
//!   - `NOT_INTERESTED`: `wHRWkE` ("I'm not interested in this content") for video quests, or
//!     `AZRWVY` ("I'm not interested in {entityName}") for game quests.
//!   - `INVALUABLE`: `ETU4Na` ("The reward isn't valuable to me")
//!   - `TOO_LONG`: `uRIIpD` ("It would take too long to complete")
//!   - `UNSURE_HOW`: `5dxvxs` ("I don't know how to complete it")
//!   - `OTHER`: `RFasgu` ("Something else")
//! - Slide 1 (`KTjjrG` "Feedback Submitted", `AvbrEM` "Thanks for your feedback. We appreciate it!"):
//!   Primary `cpT0Cq` ("Close") button and completion progress bar.

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgba};
use std::borrow::Cow;

/// Official survey ID from module `798899`.
pub const QUEST_SURVEY_ID: &str = "1290718710658629658";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurveyChoiceId {
    NotInterested,
    Invaluable,
    TooLong,
    UnsureHow,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveyChoice {
    pub id: SurveyChoiceId,
    pub label: String,
}

#[derive(Clone, Debug)]
pub(super) struct QuestSurveyModal {
    pub quest_id: String,
    pub entity_name: String,
    pub is_video_quest: bool,
    pub submitted_choice: Option<SurveyChoiceId>,
}

/// Builds the 5 reason rows from module `321538` (`en()`).
pub fn survey_choices(is_video_quest: bool, entity_name: &str) -> Vec<SurveyChoice> {
    let not_interested = if is_video_quest {
        dc_intl::t("wHRWkE").to_owned()
    } else {
        dc_intl::format(
            "AZRWVY",
            &[("entityName", Arg::Str(Cow::Borrowed(entity_name)))],
        )
    };
    vec![
        SurveyChoice {
            id: SurveyChoiceId::NotInterested,
            label: not_interested,
        },
        SurveyChoice {
            id: SurveyChoiceId::Invaluable,
            label: dc_intl::t("ETU4Na").to_owned(),
        },
        SurveyChoice {
            id: SurveyChoiceId::TooLong,
            label: dc_intl::t("uRIIpD").to_owned(),
        },
        SurveyChoice {
            id: SurveyChoiceId::UnsureHow,
            label: dc_intl::t("5dxvxs").to_owned(),
        },
        SurveyChoice {
            id: SurveyChoiceId::Other,
            label: dc_intl::t("RFasgu").to_owned(),
        },
    ]
}

impl Root {
    /// Opens the Quest dismiss survey modal (modules `321538` + `798899`).
    pub(super) fn open_quest_survey_modal(
        &mut self,
        quest_id: String,
        entity_name: String,
        is_video_quest: bool,
        cx: &mut Context<Self>,
    ) {
        self.quest_survey_modal = Some(QuestSurveyModal {
            quest_id,
            entity_name,
            is_video_quest,
            submitted_choice: None,
        });
        cx.notify();
    }

    /// Renders the 467px 2-slide Quest dismiss survey modal (modules `321538` + `798899`).
    pub(super) fn quest_survey_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.quest_survey_modal.as_ref()?;
        let _survey_id = (QUEST_SURVEY_ID, &modal.quest_id);
        let card: AnyElement = if modal.submitted_choice.is_some() {
            // Slide 1 (`798899`): "Feedback Submitted" + "Thanks for your feedback..." + Close + progress bar
            div()
                .id("quest-survey-submitted-modal")
                .w(px(467.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(56.))
                                .rounded(px(28.))
                                .bg(rgba(0x23a55a26))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("circle_check", 28., "--status-positive")),
                        )
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("KTjjrG")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("AvbrEM")),
                        )
                        .child(
                            div()
                                .id("quest-survey-close-btn")
                                .mt(px(8.))
                                .h(px(38.))
                                .px(px(24.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quest_survey_modal = None;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("cpT0Cq")),
                        ),
                )
                // Auto-close progress bar (`798899`)
                .child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .bg(color("--background-base-lowest"))
                        .child(
                            div()
                                .w_1_2()
                                .h_full()
                                .bg(color("--control-primary-background-default")),
                        ),
                )
                .into_any_element()
        } else {
            // Slide 0 (`321538` + `798899`): "Not interested in this Quest?" + 5 rows
            let choices = survey_choices(modal.is_video_quest, &modal.entity_name);
            div()
                .id("quest-survey-modal")
                .w(px(467.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .size(px(40.))
                                        .rounded(px(10.))
                                        .bg(color("--background-base-lower"))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("discover_nav_quests", 22., "--icon-strong")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("ugbhDS")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("FIYcpC")),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("quest-survey-dismiss")
                                .size(px(28.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quest_survey_modal = None;
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--icon-subtle")),
                        ),
                )
                .child(div().flex().flex_col().gap(px(8.)).children(
                    choices.into_iter().enumerate().map(|(i, choice)| {
                        let cid = choice.id;
                        div()
                            .id(SharedString::from(format!("quest-survey-choice-{i}")))
                            .w_full()
                            .px(px(14.))
                            .py(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--interactive-background-hover")))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.quest_survey_modal.as_mut() {
                                    m.submitted_choice = Some(cid);
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(choice.label)),
                            )
                            .child(div().flex_none().child(icon(
                                "bot_chevron_small_right",
                                16.,
                                "--icon-subtle",
                            )))
                    }),
                ))
                .into_any_element()
        };

        Some(
            gpui::deferred(
                div()
                    .id("quest-survey-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quest_survey_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `quest-survey [open|video|submitted|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_survey(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "video" => {
                self.open_quest_survey_modal("q1".into(), "Valorant".into(), true, cx);
                "quest-survey: video".into()
            }
            "submitted" => {
                self.quest_survey_modal = Some(QuestSurveyModal {
                    quest_id: "q1".into(),
                    entity_name: "Valorant".into(),
                    is_video_quest: false,
                    submitted_choice: Some(SurveyChoiceId::TooLong),
                });
                cx.notify();
                "quest-survey: submitted".into()
            }
            "close" => {
                self.quest_survey_modal = None;
                cx.notify();
                "quest-survey: closed".into()
            }
            _ => {
                self.open_quest_survey_modal("q1".into(), "Valorant".into(), false, cx);
                "quest-survey: open".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_game_and_video_survey_choices() {
        let game = survey_choices(false, "Elden Ring");
        assert_eq!(game.len(), 5);
        assert!(game[0].label.contains("Elden Ring"));
        assert_eq!(game[4].id, SurveyChoiceId::Other);

        let video = survey_choices(true, "Elden Ring");
        assert_eq!(video.len(), 5);
        assert_eq!(video[0].label, dc_intl::t("wHRWkE"));
    }
}
