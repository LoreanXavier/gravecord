//! Quest Activity Header Bar, Detail Popout, and "You Started a Mission Quest!" Coachmark
//! (module 131862).
//!
//! Renders above an embedded Activity or Frame stage when a matching active Quest exists:
//! - 48px header bar (`EAYZAr` "{questName} Quest" toggle, floored progress %, 4px green-gradient
//!   progress bar, `l7E81v` "Accept Quest" calling `POST /quests/{id}/enroll` or "Claim Reward")
//! - Dismissible coachmark popover (`y0cRrO` "You Started a Mission Quest!", `tP7w6c` "Your
//!   progress will be tracked here as you play!", `8+TMEJ` "Got It!")
//! - Quest detail popout (`EAYZAr`, `cpT0Cq` Close, circular progress indicator, task copy,
//!   `th2+0j` "View Quest" navigating to Quest Home).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;
use std::borrow::Cow;

pub(crate) const QUEST_ACTIVITY_ENROLL_KEY: &str = "quests:activity-enroll";
const COACHMARK_STORAGE_KEY: &str = "quest-activity-header-intro-seen";

#[derive(Clone, Debug)]
pub(crate) struct QuestActivityHeaderState {
    pub quest_id: String,
    pub quest_name: String,
    pub task_summary: String,
    pub enrolled: bool,
    pub progress_pct: u8,
    pub show_coachmark: bool,
    pub show_popout: bool,
    /// Module 113273: Unenrolled Quest Enrollment Modal (`IrNgN4`, `V3NSJx`, `l7E81v`).
    pub show_unenrolled_modal: bool,
    pub dismissed_unenrolled_modal: bool,
    pub enrolling: bool,
}

/// `POST /quests/{questId}/enroll` (module 131862 / 343559).
pub(crate) fn enroll_quest_request(quest_id: &str) -> Request {
    Request::Rest {
        key: Some(QUEST_ACTIVITY_ENROLL_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/quests/{quest_id}/enroll"),
        body: Some(json!({ "location": 11 })),
    }
}

impl Root {
    /// Renders the Quest Activity Header + Coachmark / Detail Popout overlay (`131862`).
    pub(super) fn quest_activity_header_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.quest_activity_header.as_ref()?;
        let quest_id = state.quest_id.clone();
        let quest_id_claim = state.quest_id.clone();
        let title = dc_intl::format(
            "EAYZAr",
            &[("questName", Arg::Str(Cow::Borrowed(&state.quest_name)))],
        );
        let pct = state.progress_pct.min(100);
        let fill_w = (260. * (f32::from(pct) / 100.)).clamp(0., 260.);

        // 48px Quest Activity Header bar (`header_Sh_Ht6`)
        let header_bar = div()
            .id("quest-activity-header-bar")
            .w(px(740.))
            .h(px(48.))
            .px(px(12.))
            .rounded(px(10.))
            .bg(color("--background-base-lowest"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .on_click(|_, _, cx| cx.stop_propagation())
            // Left: 32x32 Quest icon + "{questName} Quest" toggle button
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(32.))
                            .rounded(px(6.))
                            .bg(rgba(0x5865f233))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("home_quests", 18., "--white")),
                    )
                    .child(
                        div()
                            .id("quest-activity-title-btn")
                            .cursor_pointer()
                            .px(px(10.))
                            .h(px(30.))
                            .rounded(px(6.))
                            .bg(color("--control-secondary-background-default"))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(title.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(s) = this.quest_activity_header.as_mut() {
                                    s.show_popout = !s.show_popout;
                                    s.show_coachmark = false;
                                }
                                cx.notify();
                            })),
                    ),
            )
            // Right: progress % + green gradient bar + Accept/Claim CTA
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(format!("{pct}%")),
                            )
                            .child(
                                div()
                                    .w(px(260.))
                                    .h(px(5.))
                                    .rounded_full()
                                    .bg(rgba(0xffffff1f))
                                    .overflow_hidden()
                                    .child(div().h_full().w(px(fill_w)).rounded_full().bg(
                                        linear_gradient(
                                            90.,
                                            linear_color_stop(rgba(0x5865f2ff), 0.),
                                            linear_color_stop(rgba(0x44fc7eff), 1.),
                                        ),
                                    )),
                            ),
                    )
                    .child(if state.enrolled {
                        self.quest_claim_reward_button(
                            "quest-activity-claim-cta",
                            quest_id_claim,
                            pct >= 100,
                            false,
                            false,
                            false,
                            false,
                            cx,
                        )
                    } else {
                        div()
                            .id("quest-activity-accept-cta")
                            .cursor_pointer()
                            .px(px(12.))
                            .h(px(30.))
                            .rounded(px(6.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("l7E81v"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.requests.send(enroll_quest_request(&quest_id));
                                if let Some(s) = this.quest_activity_header.as_mut() {
                                    s.enrolled = true;
                                    s.show_coachmark = true;
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            );

        // Coachmark popover (`y0cRrO`, `tP7w6c`, `8+TMEJ`)
        let coachmark = (state.enrolled && state.show_coachmark).then(|| {
            div()
                .id("quest-activity-coachmark")
                .w(px(320.))
                .rounded(px(12.))
                .border_1()
                .border_color(rgba(0x5865f288))
                .bg(color("--background-surface-high"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .h(px(72.))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(rgba(0x5865f266), 0.),
                            linear_color_stop(rgba(0x44fc7e33), 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("home_quests", 34., "--white")),
                )
                .child(
                    div()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(22.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("y0cRrO")),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("tP7w6c")),
                        )
                        .child(
                            div()
                                .id("quest-activity-coachmark-got-it")
                                .mt(px(4.))
                                .cursor_pointer()
                                .w_full()
                                .h(px(34.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("8+TMEJ"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(s) = this.quest_activity_header.as_mut() {
                                        s.show_coachmark = false;
                                    }
                                    if !crate::storage::is_capture_run() {
                                        crate::storage::set(
                                            COACHMARK_STORAGE_KEY,
                                            serde_json::Value::Bool(true),
                                        );
                                    }
                                    cx.notify();
                                })),
                        ),
                )
        });

        // Quest Detail Popout (`container_stq7kb`: `EAYZAr`, `cpT0Cq`, `th2+0j`)
        let popout = state.show_popout.then(|| {
            div()
                .id("quest-activity-popout")
                .w(px(320.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(15.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .truncate()
                                .child(title),
                        )
                        .child(
                            div()
                                .id("quest-activity-popout-close")
                                .aria_label(dc_intl::t("cpT0Cq"))
                                .flex_none()
                                .size(px(26.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 14., "--icon-subtle"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(s) = this.quest_activity_header.as_mut() {
                                        s.show_popout = false;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_none()
                                .size(px(48.))
                                .rounded_full()
                                .border_2()
                                .border_color(color("--status-positive"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(format!("{pct}%")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(state.task_summary.clone()),
                        ),
                )
                .child(
                    div()
                        .id("quest-activity-view-quest")
                        .cursor_pointer()
                        .w_full()
                        .h(px(34.))
                        .rounded(px(6.))
                        .bg(color("--control-secondary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-secondary-text-default"))
                        .child(dc_intl::t("th2+0j"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quest_activity_header = None;
                            this.open_quests(cx);
                        })),
                )
        });

        // Module 113273: Unenrolled Quest Enrollment Modal (`IrNgN4`, `V3NSJx`, `l7E81v`,
        // `UuzHh8`/`VYwSSu`, 480px blue gradient modal with 105x105 app + reward tiles).
        let unenrolled_modal = (!state.enrolled
            && state.show_unenrolled_modal
            && !state.dismissed_unenrolled_modal)
            .then(|| {
                let quest_id_enroll = state.quest_id.clone();
                let enrolling = state.enrolling;
                let lead = dc_intl::format(
                    "V3NSJx",
                    &[("questName", Arg::Str(Cow::Borrowed(&state.quest_name)))],
                );
                let subtitle = format!("{lead} {}", state.task_summary);
                div()
                    .id("quest-activity-unenrolled-modal")
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .relative()
                    .w(px(480.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .h(px(176.))
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(rgba(0x3b82f6ff), 0.),
                                linear_color_stop(rgba(0x5865f2ff), 1.),
                            ))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(105.))
                                    .rounded(px(16.))
                                    .border_1()
                                    .border_color(rgba(0xffffff3d))
                                    .bg(rgba(0x00000033))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("home_quests", 44., "--white")),
                            )
                            .child(
                                div()
                                    .size(px(105.))
                                    .rounded(px(16.))
                                    .border_1()
                                    .border_color(rgba(0xffffff3d))
                                    .bg(rgba(0xffffff22))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("gift", 44., "--white")),
                            ),
                    )
                    .child(
                        div()
                            .id("quest-activity-unenrolled-close")
                            .absolute()
                            .top(px(10.))
                            .right(px(10.))
                            .size(px(28.))
                            .rounded(px(6.))
                            .bg(rgba(0x00000044))
                            .hover(|s| s.bg(rgba(0x00000066)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(s) = this.quest_activity_header.as_mut() {
                                    s.dismissed_unenrolled_modal = true;
                                    s.show_unenrolled_modal = false;
                                }
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--white")),
                    )
                    .child(
                        div()
                            .px(px(32.))
                            .pt(px(24.))
                            .pb(px(28.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(12.))
                            .text_center()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .line_height(px(28.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("IrNgN4")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(subtitle),
                            )
                            .child(
                                div()
                                    .id("quest-activity-unenrolled-accept")
                                    .mt(px(8.))
                                    .w_full()
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .when(enrolling, |d| d.opacity(0.65))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("l7E81v"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ = this
                                            .requests
                                            .send(enroll_quest_request(&quest_id_enroll));
                                        if let Some(s) = this.quest_activity_header.as_mut() {
                                            s.enrolled = true;
                                            s.enrolling = false;
                                            s.show_unenrolled_modal = false;
                                            s.show_coachmark = true;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
            });

        Some(
            deferred(
                div()
                    .id("quest-activity-header-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000080))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quest_activity_header = None;
                        cx.notify();
                    }))
                    .child(header_bar)
                    .children(coachmark)
                    .children(popout)
                    .children(unenrolled_modal),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `quest-activity-header [coachmark|popout|unenrolled|unenrolled-modal|close]`
    /// (modules 131862 & 113273).
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_activity_header(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let base = QuestActivityHeaderState {
            quest_id: "quest_activity_131".to_owned(),
            quest_name: "Starfall Odyssey".to_owned(),
            task_summary: dc_intl::format(
                "UuzHh8",
                &[
                    ("activityName", Arg::Str(Cow::Borrowed("Starfall Odyssey"))),
                    ("streamingDurationRequirement", Arg::Num(15.0)),
                    ("questReward", Arg::Str(Cow::Borrowed("500 Orbs"))),
                ],
            ),
            enrolled: true,
            progress_pct: 40,
            show_coachmark: false,
            show_popout: false,
            show_unenrolled_modal: false,
            dismissed_unenrolled_modal: false,
            enrolling: false,
        };
        match arg.trim() {
            "close" => {
                self.quest_activity_header = None;
                cx.notify();
                "quest-activity-header: closed".to_owned()
            }
            "popout" => {
                self.quest_activity_header = Some(QuestActivityHeaderState {
                    show_popout: true,
                    ..base
                });
                cx.notify();
                "quest-activity-header: popout".to_owned()
            }
            "unenrolled" => {
                self.quest_activity_header = Some(QuestActivityHeaderState {
                    enrolled: false,
                    progress_pct: 0,
                    ..base
                });
                cx.notify();
                "quest-activity-header: unenrolled".to_owned()
            }
            "unenrolled-modal" => {
                self.quest_activity_header = Some(QuestActivityHeaderState {
                    enrolled: false,
                    progress_pct: 0,
                    show_unenrolled_modal: true,
                    ..base
                });
                cx.notify();
                "quest-activity-header: unenrolled-modal".to_owned()
            }
            _ => {
                self.quest_activity_header = Some(QuestActivityHeaderState {
                    show_coachmark: true,
                    ..base
                });
                cx.notify();
                "quest-activity-header: coachmark".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quest_unenrolled_113273_intl_keys() {
        for k in ["IrNgN4", "V3NSJx", "l7E81v", "UuzHh8", "VYwSSu"] {
            assert!(dc_intl::has_key(k), "missing 113273 key {k}");
        }
    }
}
