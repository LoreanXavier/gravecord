//! A stream's "Report a problem" (module 956215), opened by the stream tile's More Options > Report Problem (module
//! 74329): the title and its line, the Streaming issue select (`selectionMode: single`, `maxOptionsVisible: 4`) of the
//! nine problems, Cancel and Submit (dimmed until a problem is picked). Submit records the problem (module 835806
//! only sends the `STREAM_REPORT_PROBLEM` analytics event, which this client does not send) and opens "Feedback
//! Submitted" (module 845671) with the Screen Share thanks.
//!
//! No capture shows this dialog: the select is drawn as the app's other dialog selects (40 px field, 40 px rows in a
//! list four rows high). The blinking window title of `b` is left out: it runs only outside the desktop app
//! (`isPlatformEmbedded`).

use super::chat_notices::{Alert, AlertButton, Content, Then};
use super::modal::Tone;
use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, SharedString, deferred, div, prelude::*, px};

/// The options of `options({isStreamer, isEndStream: false})`, in order: the value (`o.pj`) and the label key.
pub(super) const PROBLEMS: [(&str, &str); 9] = [
    ("BLACK_SCREEN", "fxiRNr"),
    ("BLURRY", "E8jTMN"),
    ("LAGGING", "VoSJEQ"),
    ("OUT_OF_SYNC", "+NluQm"),
    ("AUDIO_MISSING", "G2egzT"),
    ("AUDIO_POOR", "aHOfIo"),
    ("STREAM_STOPPED", "uEoqQp"),
    ("VIBES_OFF", "++JLL0"),
    ("OTHER", "emlT91"),
];
/// `maxOptionsVisible: 4` rows of 40 px.
const VISIBLE_ROWS: f32 = 4.;
const ROW_HEIGHT: f32 = 40.;

pub(super) struct Form {
    /// Whose stream it is (`stream.ownerId`).
    pub owner: Id,
    /// The picked option of [`PROBLEMS`].
    pub problem: Option<usize>,
    /// The option list is open.
    pub open: bool,
}

impl Root {
    /// Report Problem of a stream tile's menu.
    pub(super) fn open_stream_report(&mut self, owner: Id, cx: &mut Context<Self>) {
        self.stream_report = Some(Form {
            owner,
            problem: None,
            open: false,
        });
        cx.notify();
    }

    fn close_stream_report(&mut self, cx: &mut Context<Self>) {
        self.stream_report = None;
        cx.notify();
    }

    /// `handleSubmit`: the report, the dialog closes, then "Feedback Submitted" with `mMTVnv`.
    fn submit_stream_report(&mut self, cx: &mut Context<Self>) {
        let Some(problem) = self
            .stream_report
            .as_ref()
            .and_then(|form| form.problem)
            .and_then(|index| PROBLEMS.get(index))
            .map(|(value, _)| *value)
        else {
            return;
        };
        // GAP: STREAM_REPORT_PROBLEM (module 835806) is an analytics event; analytics are not sent by this client.
        tracing::info!(problem, "stream report: submitted");
        if let Some(form) = self.stream_report.as_ref() {
            // `streamer_user_id` of the event.
            tracing::debug!(streamer = form.owner, "stream report: whose stream");
        }
        self.stream_report = None;
        self.chat_extra.alert = Some(Alert {
            id: "stream-report-submitted",
            title: dc_intl::t("KTjjrG").to_owned(),
            subtitle: None,
            content: Some(Content::Text(dc_intl::t("mMTVnv").to_owned())),
            buttons: vec![AlertButton {
                id: "stream-report-submitted-done",
                label: dc_intl::t("i4jeWR").to_owned(),
                tone: Tone::Primary,
                then: Then::Close,
            }],
        });
        cx.notify();
    }

    pub(super) fn stream_report_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.stream_report.as_ref()?;
        let picked = form
            .problem
            .and_then(|index| PROBLEMS.get(index))
            .map(|(_, key)| dc_intl::t(key).to_owned());
        let opened = form.open;
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(gpui::FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("6Y1t5P")),
            )
            .child(
                div()
                    .id("stream-report-select")
                    .relative()
                    .child(self.anchor_probe("stream-report-select"))
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if opened {
                        color("--text-brand")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(if picked.is_some() {
                                color("--text-default")
                            } else {
                                color("--input-placeholder-text-default")
                            })
                            .child(picked.unwrap_or_else(|| dc_intl::t("U0kGk6").to_owned())),
                    )
                    .child(
                        gpui::svg()
                            .path("icons/select_chevron.svg")
                            .size(px(24.))
                            .text_color(color("--interactive-text-default"))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                if opened { 0.5 } else { 0. },
                            ))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.stream_report.as_mut() {
                            form.open = !form.open;
                        }
                        cx.notify();
                    })),
            )
            .into_any_element();
        let ready = form.problem.is_some();
        Some(super::modal::render(
            super::modal::Spec {
                id: "stream-report-modal",
                title: dc_intl::t("qnJ9W3").into(),
                subtitle: Some(div().child(dc_intl::t("7vw0h+")).into_any_element()),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "stream-report-cancel",
                        dc_intl::t("ETE/oC"),
                        super::modal::Tone::Secondary,
                        |this, _, cx| this.close_stream_report(cx),
                    ),
                    super::modal::button(
                        "stream-report-submit",
                        dc_intl::t("E48BIc"),
                        if ready {
                            super::modal::Tone::Primary
                        } else {
                            super::modal::Tone::PrimaryDisabled
                        },
                        |this, _, cx| this.submit_stream_report(cx),
                    ),
                ],
                close: Box::new(|this, _, cx| this.close_stream_report(cx)),
            },
            cx,
        ))
    }

    /// The open option list under the select, four rows in view, the picked one checked.
    pub(super) fn stream_report_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.stream_report.as_ref().filter(|form| form.open)?;
        let field = self
            .tip_anchors
            .borrow()
            .get("stream-report-select")
            .copied()?
            .dilate(px(1.));
        let picked = form.problem;
        let rows: Vec<AnyElement> = PROBLEMS
            .iter()
            .enumerate()
            .map(|(index, (value, key))| {
                div()
                    .id(SharedString::from(format!(
                        "stream-report-{}",
                        value.to_ascii_lowercase()
                    )))
                    .flex_none()
                    .h(px(ROW_HEIGHT))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t(key)),
                    )
                    .when(picked == Some(index), |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(form) = this.stream_report.as_mut() {
                            form.problem = Some(index);
                            form.open = false;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("stream-report-list")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(field.bottom() + px(4.))
                    .w(field.size.width)
                    .h(px(ROW_HEIGHT * VISIBLE_ROWS + 10.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.stream_report.as_mut() {
                            form.open = false;
                        }
                        cx.notify();
                    }))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Capture only: `streamreport open <owner>|pick <n>|submit|state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_stream_report(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        match words {
            ["open", owner] => match owner.parse::<Id>() {
                Ok(owner) => {
                    self.open_stream_report(owner, cx);
                    "stream report: open".to_owned()
                }
                Err(_) => "usage: streamreport open <owner>".to_owned(),
            },
            ["pick", n] => {
                let index = n
                    .parse::<usize>()
                    .ok()
                    .filter(|index| *index < PROBLEMS.len());
                match (self.stream_report.as_mut(), index) {
                    (Some(form), Some(index)) => {
                        form.problem = Some(index);
                        cx.notify();
                        format!("stream report: picked {}", PROBLEMS[index].0)
                    }
                    _ => "stream report: not open or no such option".to_owned(),
                }
            }
            ["submit"] => {
                let picked = self
                    .stream_report
                    .as_ref()
                    .and_then(|form| form.problem)
                    .map(|index| PROBLEMS[index].0);
                self.submit_stream_report(cx);
                format!(
                    "stream report: submitted {picked:?}, open={}",
                    self.stream_report.is_some()
                )
            }
            _ => format!(
                "stream report: open={} problem={:?}",
                self.stream_report.is_some(),
                self.stream_report.as_ref().and_then(|form| form.problem)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PROBLEMS;

    #[test]
    fn the_problems_are_the_bundles_nine_in_order() {
        let labels: Vec<&str> = PROBLEMS.iter().map(|(_, key)| dc_intl::t(key)).collect();
        assert_eq!(labels.first(), Some(&"Stream is black"));
        assert_eq!(labels.last(), Some(&"Other"));
        assert_eq!(PROBLEMS.len(), 9);
        assert_eq!(PROBLEMS[7].0, "VIBES_OFF");
    }
}
