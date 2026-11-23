//! Safety Hub Violation Appeal modal (module 894683, `AppealIngestionModal`), with the flagged content preview card
//! (module 351876) and appeal signal / error helpers (module 393033).
//!
//! Four slides (`speed-bump` -> `collect-signal` -> `confirm-submission` -> `request-sent`) triggered from the
//! Violation Detail modal (`violation_detail.rs`, module 163002). Sends `PUT /safety-hub/request-review/{id}` (or
//! `PUT /safety-hub/suspended/request-review/{id}` when a suspended-user token is active).

use super::{Root, color, editor_style, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, Window, deferred, div, prelude::*, px, rgba,
};
use std::ops::Range;

/// Official `239093.Iv` appeal signals, ordered as `239093.Qv`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppealSignal {
    DidntViolatePolicy = 0,
    TooStrictUnfair = 1,
    DontAgreePenalty = 2,
    SomethingElse = 3,
}

impl AppealSignal {
    pub(crate) const ALL: [Self; 4] = [
        Self::DidntViolatePolicy,
        Self::TooStrictUnfair,
        Self::DontAgreePenalty,
        Self::SomethingElse,
    ];

    /// Localized label from module 393033 `l0`.
    pub(crate) fn label(self) -> String {
        match self {
            Self::DidntViolatePolicy => dc_intl::t("mZffAi").to_owned(),
            Self::TooStrictUnfair => dc_intl::t("wgZVAn").to_owned(),
            Self::DontAgreePenalty => dc_intl::t("eu8G4k").to_owned(),
            Self::SomethingElse => dc_intl::t("XU3s6r").to_owned(),
        }
    }
}

/// Official `393033.Tk`: maps a review-request failure error code to the localized message.
pub(crate) fn appeal_error_text(code: Option<u32>) -> String {
    if code == Some(522_001) {
        dc_intl::t("0qyXXH").to_owned()
    } else {
        dc_intl::t("aPmsx3").to_owned()
    }
}

/// Official `239093.yN` slides of `AppealIngestionModal` (module 894683).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppealSlide {
    SpeedBump,
    CollectSignal,
    ConfirmSubmission,
    RequestSent,
}

/// Optional flagged content shown by module 351876 (`YOUR RECENT ACTIVITY`, key `s64CMg`).
#[derive(Clone, Debug)]
pub(crate) struct FlaggedContentPreview {
    pub author: String,
    pub content: String,
    pub timestamp: Option<String>,
    pub guild_name: Option<String>,
}

/// Open state of `AppealIngestionModal` (module 894683).
pub(crate) struct ViolationAppealModal {
    pub classification_id: String,
    pub description: String,
    pub explainer_url: String,
    pub flagged_content: Vec<FlaggedContentPreview>,
    pub is_coppa: bool,
    pub is_spam: bool,
    pub is_developer: bool,
    pub is_dsa_eligible: bool,
    pub in_app_eligible: bool,
    pub suspended_token: Option<String>,
    pub slide: AppealSlide,
    pub signal: Option<AppealSignal>,
    pub free_text: String,
    pub free_text_editor: Option<Entity<crate::editor::Editor>>,
    pub confirm_editing_details: bool,
    pub submitting: bool,
    pub error: Option<String>,
}

impl ViolationAppealModal {
    pub(crate) fn new(
        classification_id: String,
        description: String,
        explainer_url: String,
    ) -> Self {
        Self {
            classification_id,
            description,
            explainer_url,
            flagged_content: Vec::new(),
            is_coppa: false,
            is_spam: false,
            is_developer: false,
            is_dsa_eligible: true,
            in_app_eligible: true,
            suspended_token: None,
            slide: AppealSlide::SpeedBump,
            signal: None,
            free_text: String::new(),
            free_text_editor: None,
            confirm_editing_details: false,
            submitting: false,
            error: None,
        }
    }

    pub(crate) fn can_appeal_in_app(&self) -> bool {
        !self.is_spam && !self.is_coppa && !self.is_developer && self.in_app_eligible
    }

    fn sync_free_text_from_editor(&mut self, cx: &Context<Root>) {
        if let Some(ed) = &self.free_text_editor {
            let text = ed.read(cx).text().to_owned();
            self.free_text = text.chars().take(1024).collect();
        }
    }
}

fn capitalize_first(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Renders a `dc_intl` string with `$link` runs as naturally-wrapping `InteractiveText`.
fn linked_copy(
    id: &'static str,
    runs: Vec<(String, Option<Box<str>>)>,
    urls: Vec<&'static str>,
    text_px: f32,
    line_px: f32,
    cx: &mut Context<Root>,
) -> AnyElement {
    let mut full = String::new();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let link_style = HighlightStyle {
        color: Some(color("--text-link").into()),
        ..Default::default()
    };
    for (chunk, hook) in runs {
        let start = full.len();
        full.push_str(&chunk);
        let end = full.len();
        if hook.as_deref() == Some("$link") && end > start {
            ranges.push(start..end);
            highlights.push((start..end, link_style));
        }
    }
    let styled = StyledText::new(full).with_highlights(highlights);
    let interactive = InteractiveText::new(id, styled).on_click(ranges, move |index, _, cx| {
        if let Some(url) = urls.get(index).or_else(|| urls.first()) {
            cx.open_url(url);
        }
    });
    let _ = cx;
    div()
        .text_size(px(text_px))
        .line_height(px(line_px))
        .text_color(color("--text-muted"))
        .child(interactive)
        .into_any_element()
}

fn link_row(id: SharedString, label: String, url: String, cx: &mut Context<Root>) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .px(px(14.))
        .py(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-base-lower"))
        .hover(|s| s.bg(color("--background-mod-subtle")))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(label),
        )
        .child(icon("report_link_arrow", 16., "--interactive-icon-default"))
        .on_click(cx.listener(move |_, _, _, cx| {
            if !url.is_empty() {
                cx.open_url(&url);
            }
        }))
        .into_any_element()
}

impl Root {
    pub(crate) fn close_violation_appeal(&mut self) -> bool {
        self.violation_appeal_modal.take().is_some()
    }

    pub(crate) fn open_violation_appeal(
        &mut self,
        classification_id: String,
        description: String,
        explainer_url: String,
        is_coppa: bool,
        suspended_token: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut modal = ViolationAppealModal::new(classification_id, description, explainer_url);
        modal.is_coppa = is_coppa;
        modal.suspended_token = suspended_token;
        self.violation_appeal_modal = Some(modal);
        cx.notify();
    }

    fn ensure_appeal_editor(&mut self, cx: &mut Context<Self>) -> Entity<crate::editor::Editor> {
        let existing = self
            .violation_appeal_modal
            .as_ref()
            .and_then(|m| m.free_text_editor.clone());
        if let Some(ed) = existing {
            return ed;
        }
        let initial = self
            .violation_appeal_modal
            .as_ref()
            .map(|m| m.free_text.clone())
            .unwrap_or_default();
        let placeholder = dc_intl::t("bQrZIN").to_owned();
        let editor = cx.new(|cx| {
            let mut style = editor_style();
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            style.line_height = 21.;
            let mut ed = crate::editor::Editor::new(style, cx);
            ed.set_placeholder(placeholder);
            if !initial.is_empty() {
                ed.set_text(&initial);
            }
            ed
        });
        if let Some(m) = self.violation_appeal_modal.as_mut() {
            m.free_text_editor = Some(editor.clone());
        }
        editor
    }

    fn submit_violation_appeal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.violation_appeal_modal.as_mut() else {
            return;
        };
        if modal.submitting {
            return;
        }
        modal.sync_free_text_from_editor(cx);
        let Some(signal) = modal.signal else {
            return;
        };
        modal.submitting = true;
        modal.error = None;
        let classification_id = modal.classification_id.clone();
        let user_input = modal.free_text.clone();
        let suspended_token = modal.suspended_token.clone();
        let (route, body) = if let Some(token) = suspended_token {
            (
                format!("/safety-hub/suspended/request-review/{classification_id}"),
                serde_json::json!({
                    "signal": signal as u8,
                    "user_input": user_input,
                    "token": token,
                }),
            )
        } else {
            (
                format!("/safety-hub/request-review/{classification_id}"),
                serde_json::json!({
                    "signal": signal as u8,
                    "user_input": user_input,
                }),
            )
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("safety-hub-appeal:{classification_id}")),
            method: reqwest::Method::PUT,
            route,
            body: Some(body),
        });
        // Optimistically advance to RequestSent in offline/replay captures and mark the parent detail appealed.
        if let Some(detail) = self.violation_detail_modal.as_mut() {
            detail.appeal = super::violation_detail::ViolationAppealState::Appealed;
        }
        if let Some(modal) = self.violation_appeal_modal.as_mut() {
            modal.submitting = false;
            modal.slide = AppealSlide::RequestSent;
        }
        cx.notify();
    }

    /// Handles REST answers for `safety-hub-appeal:{id}` (`393033.Tk`).
    #[allow(dead_code)]
    pub(crate) fn on_violation_appeal_response(
        &mut self,
        ok: bool,
        error_code: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.violation_appeal_modal.as_mut() else {
            return;
        };
        modal.submitting = false;
        if ok {
            modal.error = None;
            modal.slide = AppealSlide::RequestSent;
            if let Some(detail) = self.violation_detail_modal.as_mut() {
                detail.appeal = super::violation_detail::ViolationAppealState::Appealed;
            }
        } else {
            modal.error = Some(appeal_error_text(error_code));
        }
        cx.notify();
    }

    pub(in crate::ui) fn violation_appeal_modal_layer(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let needs_editor = self.violation_appeal_modal.as_ref().is_some_and(|m| {
            (m.slide == AppealSlide::CollectSignal
                && m.is_dsa_eligible
                && m.signal == Some(AppealSignal::SomethingElse))
                || (m.slide == AppealSlide::ConfirmSubmission
                    && m.is_dsa_eligible
                    && m.confirm_editing_details)
        });
        if needs_editor {
            let _ = self.ensure_appeal_editor(cx);
        }
        let modal = self.violation_appeal_modal.as_ref()?;
        let slide = modal.slide;
        let description = capitalize_first(&modal.description);
        let explainer_url = modal.explainer_url.clone();
        let is_coppa = modal.is_coppa;
        let is_spam = modal.is_spam;
        let is_developer = modal.is_developer;
        let is_dsa_eligible = modal.is_dsa_eligible;
        let in_app_eligible = modal.can_appeal_in_app();
        let selected_signal = modal.signal;
        let free_text = modal.free_text.clone();
        let editor = modal.free_text_editor.clone();
        let confirm_editing = modal.confirm_editing_details;
        let submitting = modal.submitting;
        let error_msg = modal.error.clone();
        let flagged = modal.flagged_content.clone();

        let close_btn = |cx: &mut Context<Self>| {
            div()
                .id("appeal-modal-close")
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
                    this.close_violation_appeal();
                    cx.notify();
                }))
        };

        let content: AnyElement = match slide {
            AppealSlide::SpeedBump => {
                // Module 351876: flagged content card (`YOUR RECENT ACTIVITY`, key `s64CMg`).
                let flagged_card = (!flagged.is_empty()).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("s64CMg")),
                        )
                        .children(flagged.iter().map(|item| {
                            div()
                                .p(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(item.author.clone()),
                                        )
                                        .children(item.timestamp.clone().map(|ts| {
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(ts)
                                        })),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(item.content.clone()),
                                )
                                .children(item.guild_name.clone().map(|guild| {
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(guild)
                                }))
                        }))
                });

                let mut links: Vec<AnyElement> = Vec::new();
                if is_coppa {
                    links.push(link_row(
                        "appeal-coppa-link".into(),
                        dc_intl::t("gJs+kf").to_owned(),
                        "https://support.discord.com/hc/requests/new?ticket_form_id=360000029731"
                            .to_owned(),
                        cx,
                    ));
                }
                if is_spam && !is_coppa {
                    links.push(link_row(
                        "appeal-spam-link".into(),
                        dc_intl::t("PaNS+I").to_owned(),
                        "https://support.discord.com/hc/requests/new?ticket_form_id=360000029731&tf_360055270593=__dc.ticket_form-tnsv1_appeals_other_questions__".to_owned(),
                        cx,
                    ));
                }
                if is_developer {
                    links.push(link_row(
                        "appeal-dev-link".into(),
                        dc_intl::t("n9cZTH").to_owned(),
                        "https://dis.gd/app-appeal".to_owned(),
                        cx,
                    ));
                }
                if !is_coppa {
                    links.push(link_row(
                        "appeal-policy-link".into(),
                        dc_intl::t("Vtyn/7").to_owned(),
                        explainer_url,
                        cx,
                    ));
                }

                let dsa_notice = (in_app_eligible && is_dsa_eligible).then(|| {
                    linked_copy(
                        "appeal-dsa-notice",
                        dc_intl::format_runs("WMUgCX", &[]),
                        vec!["https://support.discord.com/hc/articles/17861767909527"],
                        13.,
                        18.,
                        cx,
                    )
                });

                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
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
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("C5q+pW")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("URt7VI")),
                                    ),
                            )
                            .child(close_btn(cx)),
                    )
                    .children(flagged_card)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("xsdcxh")),
                            )
                            .child(
                                div()
                                    .p(px(14.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(description),
                            ),
                    )
                    .child(div().flex().flex_col().gap(px(8.)).children(links))
                    .children(dsa_notice)
                    .when(in_app_eligible, |this| {
                        this.child(
                            div().flex().justify_end().pt(px(4.)).child(
                                div()
                                    .id("appeal-speed-bump-next")
                                    .px(px(20.))
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("PDTjLN"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.violation_appeal_modal.as_mut() {
                                            m.slide = AppealSlide::CollectSignal;
                                        }
                                        cx.notify();
                                    })),
                            ),
                        )
                    })
                    .into_any_element()
            }

            AppealSlide::CollectSignal => {
                let radios = div().flex().flex_col().gap(px(8.)).children(
                    AppealSignal::ALL.into_iter().map(|sig| {
                        let chosen = selected_signal == Some(sig);
                        div()
                            .id(SharedString::from(format!("appeal-signal-{}", sig as u8)))
                            .w_full()
                            .px(px(14.))
                            .py(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if chosen {
                                color("--brand-500")
                            } else {
                                color("--border-subtle")
                            })
                            .bg(if chosen {
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
                                    .size(px(18.))
                                    .rounded(px(9.))
                                    .border_2()
                                    .border_color(if chosen {
                                        color("--brand-500")
                                    } else {
                                        color("--interactive-icon-default")
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(chosen, |d| {
                                        d.child(
                                            div()
                                                .size(px(8.))
                                                .rounded(px(4.))
                                                .bg(color("--brand-500")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(sig.label()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.violation_appeal_modal.as_mut() {
                                    m.signal = Some(sig);
                                    if sig != AppealSignal::SomethingElse {
                                        m.free_text.clear();
                                        if let Some(ed) = m.free_text_editor.clone() {
                                            ed.update(cx, |ed, _| ed.set_text(""));
                                        }
                                    }
                                }
                                cx.notify();
                            }))
                    }),
                );

                let show_textarea =
                    is_dsa_eligible && selected_signal == Some(AppealSignal::SomethingElse);
                let note_section = (show_textarea).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(editor.clone().map(|ed| {
                            // Module 894683's text area (module 103557, `rows` 4, `maxLength` 1024, no override):
                            // `.textArea_uI0xjn` (`padding: var(--space-12) 10px`) in `.wrapper_UcGqzv`'s 1px
                            // border, 4 * 21 + 24 + 2.
                            div()
                                .min_h(px(4. * 21. + 24. + 2.))
                                .px(px(10.))
                                .py(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lowest"))
                                .child(ed)
                        }))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("xfNY3L")),
                        )
                });

                let guidelines_notice = linked_copy(
                    "appeal-guidelines-notice",
                    dc_intl::format_runs("8k9GCW", &[]),
                    vec!["https://discord.com/guidelines"],
                    12.,
                    16.,
                    cx,
                );

                let can_next = selected_signal.is_some();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
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
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("C5q+pW")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("VEcRhw")),
                                    ),
                            )
                            .child(close_btn(cx)),
                    )
                    .child(radios)
                    .children(note_section)
                    .child(guidelines_notice)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt(px(4.))
                            .child(
                                div()
                                    .id("appeal-signal-back")
                                    .px(px(16.))
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.violation_appeal_modal.as_mut() {
                                            m.sync_free_text_from_editor(cx);
                                            m.slide = AppealSlide::SpeedBump;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("appeal-signal-next")
                                    .px(px(20.))
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(can_next, |d| {
                                        d.hover(|s| {
                                            s.bg(color("--control-primary-background-hover"))
                                        })
                                        .cursor_pointer()
                                    })
                                    .when(!can_next, |d| d.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("PDTjLN"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.violation_appeal_modal.as_mut()
                                            && m.signal.is_some()
                                        {
                                            m.sync_free_text_from_editor(cx);
                                            m.confirm_editing_details = false;
                                            m.slide = AppealSlide::ConfirmSubmission;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }

            AppealSlide::ConfirmSubmission => {
                let signal_label = selected_signal.map(AppealSignal::label).unwrap_or_default();
                let has_note = !free_text.trim().is_empty();
                let toggle_label = if has_note {
                    dc_intl::t("tnE3bZ")
                } else {
                    dc_intl::t("uoQFIp")
                };

                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
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
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .line_height(px(28.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("C5q+pW")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("G2g/g5")),
                                    ),
                            )
                            .child(close_btn(cx)),
                    )
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(format!("\u{2022} {signal_label}")),
                            )
                            .when(has_note && !confirm_editing, |d| {
                                d.child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(format!("\u{2022} {}", free_text.trim())),
                                )
                            })
                            .when(is_dsa_eligible, |d| {
                                d.child(
                                    div()
                                        .id("appeal-confirm-toggle-details")
                                        .pt(px(4.))
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-link"))
                                        .hover(|s| s.underline())
                                        .cursor_pointer()
                                        .child(toggle_label)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.violation_appeal_modal.as_mut() {
                                                m.sync_free_text_from_editor(cx);
                                                m.confirm_editing_details =
                                                    !m.confirm_editing_details;
                                            }
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(is_dsa_eligible && confirm_editing, |d| {
                                d.children(editor.clone().map(|ed| {
                                    div()
                                        .mt(px(4.))
                                        .min_h(px(80.))
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lowest"))
                                        .child(ed)
                                }))
                            }),
                    )
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(description),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("d6qgY/")),
                    )
                    .children(error_msg.map(|err| {
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err)
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt(px(4.))
                            .child(
                                div()
                                    .id("appeal-confirm-back")
                                    .px(px(16.))
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .when(!submitting, |d| {
                                        d.hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                    })
                                    .when(submitting, |d| d.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("13/7kX"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.violation_appeal_modal.as_mut()
                                            && !m.submitting
                                        {
                                            m.sync_free_text_from_editor(cx);
                                            m.slide = AppealSlide::CollectSignal;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("appeal-confirm-submit")
                                    .px(px(20.))
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--control-critical-primary-background-default"))
                                    .hover(|s| {
                                        s.bg(color("--control-critical-primary-background-hover"))
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("geKm7t"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_violation_appeal(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }

            AppealSlide::RequestSent => div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .pt(px(8.))
                        .child(
                            div()
                                .size(px(56.))
                                .rounded(px(28.))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("circle_check", 32., "--text-feedback-positive")),
                        )
                        .child(
                            div()
                                .text_size(px(22.))
                                .line_height(px(28.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("QMbTSu")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("Qdx8AP")),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("9BRc1N")),
                        )
                        .child(link_row(
                            "appeal-sent-guidelines".into(),
                            dc_intl::t("PxL38B").to_owned(),
                            "https://discord.com/guidelines".to_owned(),
                            cx,
                        ))
                        .child(link_row(
                            "appeal-sent-enforcement".into(),
                            dc_intl::t("qC3XKa").to_owned(),
                            "https://support.discord.com/hc/articles/18210965981847".to_owned(),
                            cx,
                        )),
                )
                .child(
                    div().flex().justify_end().pt(px(4.)).child(
                        div()
                            .id("appeal-sent-done")
                            .px(px(20.))
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("i4jeWR"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_violation_appeal();
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element(),
        };

        // Official slide container width: 500px (module 894683).
        let dialog = div()
            .id("violation-appeal-modal")
            .occlude()
            .w(px(500.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(24.))
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
                    .id("violation-appeal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_violation_appeal();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(dialog),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_violation_appeal(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let arg = arg.trim();
        if arg == "close" {
            self.close_violation_appeal();
            cx.notify();
            return "violation appeal closed".to_owned();
        }
        let mut modal = ViolationAppealModal::new(
            "cls-demo-1".to_owned(),
            "spam and platform manipulation".to_owned(),
            "https://discord.com/guidelines".to_owned(),
        );
        modal.flagged_content.push(FlaggedContentPreview {
            author: "onugeccen".to_owned(),
            content: "Check out this repeated link message flagged for review.".to_owned(),
            timestamp: Some("Today at 14:22".to_owned()),
            guild_name: Some("gravecord fixture".to_owned()),
        });
        match arg {
            "signal" => {
                modal.slide = AppealSlide::CollectSignal;
                modal.signal = Some(AppealSignal::SomethingElse);
            }
            "confirm" => {
                modal.slide = AppealSlide::ConfirmSubmission;
                modal.signal = Some(AppealSignal::DidntViolatePolicy);
                modal.free_text =
                    "This message was a regular link shared with permission.".to_owned();
            }
            "sent" => {
                modal.slide = AppealSlide::RequestSent;
                modal.signal = Some(AppealSignal::DidntViolatePolicy);
            }
            _ => {
                modal.slide = AppealSlide::SpeedBump;
            }
        }
        self.violation_appeal_modal = Some(modal);
        cx.notify();
        format!("violation appeal opened ({arg})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_of_violation_appeal_894683_and_helpers_resolve() {
        let keys = [
            "C5q+pW", "URt7VI", "xsdcxh", "gJs+kf", "PaNS+I", "n9cZTH", "Vtyn/7", "WMUgCX",
            "PDTjLN", "13/7kX", "VEcRhw", "bQrZIN", "xfNY3L", "8k9GCW", "G2g/g5", "tnE3bZ",
            "uoQFIp", "d6qgY/", "geKm7t", "QMbTSu", "Qdx8AP", "9BRc1N", "PxL38B", "qC3XKa",
            "i4jeWR", "mZffAi", "wgZVAn", "eu8G4k", "XU3s6r", "0qyXXH", "aPmsx3", "s64CMg",
        ];
        for key in keys {
            let value = dc_intl::t(key);
            assert!(!value.is_empty(), "key {key} must resolve");
            assert_ne!(value, key, "key {key} fell back to literal");
        }
        assert_eq!(appeal_error_text(Some(522_001)), dc_intl::t("0qyXXH"));
        assert_eq!(appeal_error_text(Some(50001)), dc_intl::t("aPmsx3"));
        assert_eq!(appeal_error_text(None), dc_intl::t("aPmsx3"));
    }

    #[test]
    fn in_app_eligibility_matches_module_894683_rules() {
        let mut modal = ViolationAppealModal::new("1".into(), "spam".into(), String::new());
        assert!(modal.can_appeal_in_app());
        modal.is_spam = true;
        assert!(!modal.can_appeal_in_app());
        modal.is_spam = false;
        modal.is_coppa = true;
        assert!(!modal.can_appeal_in_app());
        modal.is_coppa = false;
        modal.is_developer = true;
        assert!(!modal.can_appeal_in_app());
    }
}
