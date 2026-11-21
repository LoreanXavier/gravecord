//! Discord Research Survey Modal — Qualtrics Embedded Survey (module 215978).
//!
//! Fetches `GET /users/@me/embedded-survey/{surveyId}` and submits answers to
//! `POST /users/@me/embedded-surveys/{surveyId}/responses`.
//! Renders:
//! - Loading state (`MKDeyL` "Loading...")
//! - Multi-step survey pages (`OSqLUF` "Discord Research Survey", radio MC/SAVR, checkbox
//!   MC/MAVR, text entry TE, descriptive DB, ForceResponse validation, `PDTjLN` "Next")
//! - Exit confirmation modal (`T9Sx3z`, `iCK6G0`, `oEAioF` "Never Mind", `p89ACt` "Yes")
//! - Thank-you completion screen (`2scvdw`, `chZxOD`, `i4jeWR` "Done").

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(crate) const SURVEY_FETCH_KEY: &str = "research-survey:fetch";
pub(crate) const SURVEY_SUBMIT_KEY: &str = "research-survey:submit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SurveyQuestionKind {
    SingleChoice,
    MultiChoice,
    TextEntry,
    Descriptive,
}

#[derive(Clone, Debug)]
pub(crate) struct SurveyQuestion {
    pub id: String,
    pub prompt: String,
    pub kind: SurveyQuestionKind,
    pub required: bool,
    pub choices: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurveyPhase {
    Loading,
    Active,
    ConfirmExit,
    Completed,
}

#[derive(Clone, Debug)]
pub(crate) struct ResearchSurveyModal {
    pub survey_id: String,
    pub phase: SurveyPhase,
    pub pages: Vec<Vec<SurveyQuestion>>,
    pub page_index: usize,
    pub single_answers: HashMap<String, String>,
    pub multi_answers: HashMap<String, HashSet<String>>,
    pub text_answers: HashMap<String, String>,
    pub submitting: bool,
}

impl ResearchSurveyModal {
    pub(crate) fn can_advance(&self) -> bool {
        let Some(page) = self.pages.get(self.page_index) else {
            return true;
        };
        for q in page {
            if !q.required {
                continue;
            }
            match q.kind {
                SurveyQuestionKind::SingleChoice => {
                    if !self.single_answers.contains_key(&q.id) {
                        return false;
                    }
                }
                SurveyQuestionKind::MultiChoice => {
                    if self.multi_answers.get(&q.id).is_none_or(HashSet::is_empty) {
                        return false;
                    }
                }
                SurveyQuestionKind::TextEntry => {
                    if self
                        .text_answers
                        .get(&q.id)
                        .is_none_or(|s| s.trim().is_empty())
                    {
                        return false;
                    }
                }
                SurveyQuestionKind::Descriptive => {}
            }
        }
        true
    }

    pub(crate) fn build_values_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        for (qid, cid) in &self.single_answers {
            if let Ok(n) = cid.parse::<i64>() {
                map.insert(qid.clone(), Value::Number(n.into()));
            } else {
                map.insert(qid.clone(), Value::String(cid.clone()));
            }
        }
        for (qid, set) in &self.multi_answers {
            let mut vals: Vec<Value> = set.iter().cloned().map(Value::String).collect();
            vals.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            map.insert(qid.clone(), Value::Array(vals));
        }
        for (qid, text) in &self.text_answers {
            map.insert(format!("{qid}_TEXT"), Value::String(text.clone()));
        }
        Value::Object(map)
    }
}

/// `GET /users/@me/embedded-survey/{surveyId}` (module 215978 / 669700).
pub(crate) fn fetch_survey_request(survey_id: &str) -> Request {
    Request::Rest {
        key: Some(SURVEY_FETCH_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: format!("/users/@me/embedded-survey/{survey_id}"),
        body: None,
    }
}

/// `POST /users/@me/embedded-surveys/{surveyId}/responses` (module 215978 / 669700).
pub(crate) fn submit_survey_request(survey_id: &str, values: &Value) -> Request {
    Request::Rest {
        key: Some(SURVEY_SUBMIT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/users/@me/embedded-surveys/{survey_id}/responses"),
        body: Some(json!({ "values_json": values.to_string() })),
    }
}

impl Root {
    /// Opens the Discord Research Survey modal (`215978`) and fetches survey details.
    #[allow(dead_code)]
    pub(super) fn open_research_survey(&mut self, survey_id: String, cx: &mut Context<Self>) {
        let _ = self.requests.send(fetch_survey_request(&survey_id));
        self.research_survey_modal = Some(ResearchSurveyModal {
            survey_id,
            phase: SurveyPhase::Loading,
            pages: Vec::new(),
            page_index: 0,
            single_answers: HashMap::new(),
            multi_answers: HashMap::new(),
            text_answers: HashMap::new(),
            submitting: false,
        });
        cx.notify();
    }

    /// Handles REST answers for `SURVEY_FETCH_KEY` and `SURVEY_SUBMIT_KEY`.
    pub(super) fn handle_research_survey_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.research_survey_modal.as_mut() else {
            return;
        };
        if key == SURVEY_FETCH_KEY {
            let mut questions = Vec::new();
            if let Some(obj) = body["body"]["result"]["Questions"].as_object() {
                for (qid, qval) in obj {
                    let prompt = qval["QuestionText"]
                        .as_str()
                        .unwrap_or("Question")
                        .to_owned();
                    let qtype = qval["QuestionType"].as_str().unwrap_or("MC");
                    let selector = qval["Selector"].as_str().unwrap_or("SAVR");
                    let kind = match (qtype, selector) {
                        ("DB", _) => SurveyQuestionKind::Descriptive,
                        ("TE", _) => SurveyQuestionKind::TextEntry,
                        (_, "MAVR") => SurveyQuestionKind::MultiChoice,
                        _ => SurveyQuestionKind::SingleChoice,
                    };
                    let required =
                        qval["Validation"]["Settings"]["ForceResponse"].as_str() == Some("ON");
                    let choices = qval["Choices"]
                        .as_object()
                        .map(|c| {
                            c.iter()
                                .map(|(cid, cv)| {
                                    (
                                        cid.clone(),
                                        cv["Display"].as_str().unwrap_or(cid).to_owned(),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    questions.push(SurveyQuestion {
                        id: qid.clone(),
                        prompt,
                        kind,
                        required,
                        choices,
                    });
                }
            }
            modal.pages = if questions.is_empty() {
                Vec::new()
            } else {
                vec![questions]
            };
            modal.phase = SurveyPhase::Active;
            cx.notify();
        } else if key == SURVEY_SUBMIT_KEY {
            modal.submitting = false;
            modal.phase = SurveyPhase::Completed;
            cx.notify();
        }
    }

    /// Modal layer for Discord Research Survey (`215978`).
    pub(super) fn research_survey_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.research_survey_modal.as_ref()?;
        let can_next = modal.can_advance() && !modal.submitting;
        let is_last_page = modal.page_index + 1 >= modal.pages.len();

        let card = match modal.phase {
            SurveyPhase::Loading => div()
                .id("research-survey-loading")
                .w(px(480.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .p(px(36.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("OSqLUF")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("MKDeyL")),
                ),
            SurveyPhase::ConfirmExit => div()
                .id("research-survey-confirm-exit")
                .w(px(440.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(18.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("T9Sx3z")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("iCK6G0")),
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
                        .justify_end()
                        .gap(px(10.))
                        .child(
                            div()
                                .id("research-survey-stay")
                                .cursor_pointer()
                                .px(px(14.))
                                .h(px(36.))
                                .rounded(px(6.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("oEAioF"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.research_survey_modal.as_mut() {
                                        m.phase = SurveyPhase::Active;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("research-survey-exit-yes")
                                .cursor_pointer()
                                .px(px(18.))
                                .h(px(36.))
                                .rounded(px(6.))
                                .bg(color("--control-critical-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("p89ACt"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.research_survey_modal = None;
                                    cx.notify();
                                })),
                        ),
                ),
            SurveyPhase::Completed => div()
                .id("research-survey-done")
                .w(px(460.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(icon("circle_check", 40., "--status-positive"))
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("OSqLUF")),
                        )
                        .child(
                            div()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("2scvdw")),
                        )
                        .child(
                            div()
                                .text_center()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("chZxOD")),
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
                        .justify_end()
                        .child(
                            div()
                                .id("research-survey-done-btn")
                                .cursor_pointer()
                                .px(px(20.))
                                .h(px(36.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("i4jeWR"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.research_survey_modal = None;
                                    cx.notify();
                                })),
                        ),
                ),
            SurveyPhase::Active => {
                let page_questions = modal
                    .pages
                    .get(modal.page_index)
                    .cloned()
                    .unwrap_or_default();
                let mut body = div().px(px(24.)).py(px(18.)).flex().flex_col().gap(px(20.));
                for (qidx, q) in page_questions.into_iter().enumerate() {
                    let qid = q.id.clone();
                    let mut qblock = div().flex().flex_col().gap(px(10.)).child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(q.prompt),
                    );
                    match q.kind {
                        SurveyQuestionKind::SingleChoice => {
                            let chosen = modal.single_answers.get(&qid).cloned();
                            qblock = qblock.child(div().flex().flex_col().gap(px(4.)).children(
                                q.choices.into_iter().enumerate().map(|(cidx, (cid, label))| {
                                    let sel = chosen.as_deref() == Some(cid.as_str());
                                    let qid_c = qid.clone();
                                    let cid_c = cid.clone();
                                    div()
                                        .id(format!("survey-q{qidx}-r{cidx}"))
                                        .cursor_pointer()
                                        .px(px(10.))
                                        .py(px(8.))
                                        .rounded(px(8.))
                                        .bg(if sel {
                                            rgba(0x5865f226)
                                        } else {
                                            color("--background-base-lower")
                                        })
                                        .flex()
                                        .items_center()
                                        .gap(px(10.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.research_survey_modal.as_mut() {
                                                m.single_answers
                                                    .insert(qid_c.clone(), cid_c.clone());
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex_none()
                                                .size(px(18.))
                                                .rounded_full()
                                                .border_2()
                                                .border_color(if sel {
                                                    color("--control-primary-background-default")
                                                } else {
                                                    color("--border-subtle")
                                                })
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(sel, |d| {
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
                                                .min_w_0()
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .child(label),
                                        )
                                }),
                            ));
                        }
                        SurveyQuestionKind::MultiChoice => {
                            let set = modal.multi_answers.get(&qid).cloned().unwrap_or_default();
                            qblock = qblock.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .children(q.choices.into_iter().enumerate().map(
                                    |(cidx, (cid, label))| {
                                        let checked = set.contains(&cid);
                                        let qid_c = qid.clone();
                                        let cid_c = cid.clone();
                                        div()
                                            .id(format!("survey-q{qidx}-c{cidx}"))
                                            .cursor_pointer()
                                            .px(px(10.))
                                            .py(px(8.))
                                            .rounded(px(8.))
                                            .bg(if checked {
                                                rgba(0x5865f226)
                                            } else {
                                                color("--background-base-lower")
                                            })
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(m) = this.research_survey_modal.as_mut()
                                                {
                                                    let entry = m
                                                        .multi_answers
                                                        .entry(qid_c.clone())
                                                        .or_default();
                                                    if !entry.remove(&cid_c) {
                                                        entry.insert(cid_c.clone());
                                                    }
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .size(px(18.))
                                                    .rounded(px(4.))
                                                    .border_1()
                                                    .border_color(if checked {
                                                        color(
                                                            "--control-primary-background-default",
                                                        )
                                                    } else {
                                                        color("--border-subtle")
                                                    })
                                                    .bg(if checked {
                                                        color(
                                                            "--control-primary-background-default",
                                                        )
                                                    } else {
                                                        color("--background-base-lowest")
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .when(checked, |d| {
                                                        d.child(icon(
                                                            "checkbox_check",
                                                            12.,
                                                            "--white",
                                                        ))
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(14.))
                                                    .text_color(color("--text-default"))
                                                    .child(label),
                                            )
                                    },
                                )),
                            );
                        }
                        SurveyQuestionKind::TextEntry => {
                            let val = modal.text_answers.get(&qid).cloned().unwrap_or_default();
                            qblock = qblock.child(
                                div()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lowest"))
                                    .p(px(12.))
                                    .min_h(px(72.))
                                    .text_size(px(13.))
                                    .text_color(if val.is_empty() {
                                        color("--text-muted")
                                    } else {
                                        color("--text-default")
                                    })
                                    .child(if val.is_empty() {
                                        "Enter your response...".to_owned()
                                    } else {
                                        val
                                    }),
                            );
                        }
                        SurveyQuestionKind::Descriptive => {}
                    }
                    body = body.child(qblock);
                }

                div()
                    .id("research-survey-modal")
                    .w(px(520.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(14.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("OSqLUF")),
                            )
                            .child(
                                div()
                                    .id("research-survey-close")
                                    .flex_none()
                                    .size(px(32.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 18., "--icon-subtle"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.research_survey_modal.as_mut() {
                                            m.phase = SurveyPhase::ConfirmExit;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(body)
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
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(format!(
                                        "Step {} of {}",
                                        modal.page_index + 1,
                                        modal.pages.len().max(1)
                                    )),
                            )
                            .child(
                                div()
                                    .id("research-survey-next")
                                    .when(can_next, |d| d.cursor_pointer())
                                    .when(!can_next, |d| d.opacity(0.5))
                                    .px(px(20.))
                                    .h(px(36.))
                                    .rounded(px(6.))
                                    .bg(color("--control-primary-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("PDTjLN"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !can_next {
                                            return;
                                        }
                                        if is_last_page {
                                            if let Some(m) = this.research_survey_modal.as_mut() {
                                                let vals = m.build_values_json();
                                                let sid = m.survey_id.clone();
                                                m.submitting = true;
                                                let _ = this
                                                    .requests
                                                    .send(submit_survey_request(&sid, &vals));
                                            }
                                        } else if let Some(m) = this.research_survey_modal.as_mut()
                                        {
                                            m.page_index += 1;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
            }
        };

        Some(
            deferred(
                div()
                    .id("research-survey-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `research-survey [active|exit|done|loading|close]` (module 215978).
    #[cfg(feature = "capture")]
    pub(crate) fn script_research_survey(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut single = HashMap::new();
        single.insert("QID1".to_owned(), "2".to_owned());
        let mut multi = HashMap::new();
        multi.insert(
            "QID2".to_owned(),
            HashSet::from(["1".to_owned(), "3".to_owned()]),
        );
        let sample_pages = vec![vec![
            SurveyQuestion {
                id: "QID1".to_owned(),
                prompt: "How satisfied are you with voice and screen share quality this week?"
                    .to_owned(),
                kind: SurveyQuestionKind::SingleChoice,
                required: true,
                choices: vec![
                    ("1".to_owned(), "Very satisfied".to_owned()),
                    ("2".to_owned(), "Satisfied".to_owned()),
                    ("3".to_owned(), "Neutral".to_owned()),
                ],
            },
            SurveyQuestion {
                id: "QID2".to_owned(),
                prompt: "Which features did you use most recently?".to_owned(),
                kind: SurveyQuestionKind::MultiChoice,
                required: false,
                choices: vec![
                    ("1".to_owned(), "Voice channels & Go Live".to_owned()),
                    ("2".to_owned(), "Quests & Orbs".to_owned()),
                    ("3".to_owned(), "Direct Messages".to_owned()),
                ],
            },
        ]];
        let make = |phase| ResearchSurveyModal {
            survey_id: "SV_demo_2026".to_owned(),
            phase,
            pages: sample_pages.clone(),
            page_index: 0,
            single_answers: single.clone(),
            multi_answers: multi.clone(),
            text_answers: HashMap::new(),
            submitting: false,
        };
        match arg.trim() {
            "close" => {
                self.research_survey_modal = None;
                cx.notify();
                "research-survey: closed".to_owned()
            }
            "exit" => {
                self.research_survey_modal = Some(make(SurveyPhase::ConfirmExit));
                cx.notify();
                "research-survey: exit".to_owned()
            }
            "done" => {
                self.research_survey_modal = Some(make(SurveyPhase::Completed));
                cx.notify();
                "research-survey: done".to_owned()
            }
            "loading" => {
                self.research_survey_modal = Some(make(SurveyPhase::Loading));
                cx.notify();
                "research-survey: loading".to_owned()
            }
            _ => {
                self.research_survey_modal = Some(make(SurveyPhase::Active));
                cx.notify();
                "research-survey: active".to_owned()
            }
        }
    }
}
