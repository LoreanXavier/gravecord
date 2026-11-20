//! Search Feedback Survey modal (module 770950): 1-step (`Good`) or 2-step (`Bad`) radio + freeform survey
//! opened from the search results feedback prompt.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, prelude::*, px, rgba,
};

pub(crate) const OTHER_VALUE: &str = "other";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SearchRating {
    Good,
    Bad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FeedbackOption {
    pub value: &'static str,
    pub key: &'static str,
}

/// Unsatisfied options (`_` of module 770950).
pub(crate) const UNSATISFIED_OPTIONS: &[FeedbackOption] = &[
    FeedbackOption {
        value: "missing_expected_result",
        key: "4DEniX",
    },
    FeedbackOption {
        value: "no_results",
        key: "jlh29e",
    },
    FeedbackOption {
        value: "required_multiple_searches",
        key: "TpGBIB",
    },
    FeedbackOption {
        value: "buried_expected_result",
        key: "Zh6bNN",
    },
    FeedbackOption {
        value: "irrelevant_results",
        key: "WCiDBF",
    },
    FeedbackOption {
        value: "inadequate_filters",
        key: "mf36b7",
    },
    FeedbackOption {
        value: "hard_to_scan_results",
        key: "ECI0Of",
    },
    FeedbackOption {
        value: OTHER_VALUE,
        key: "oxbtmN",
    },
];

/// Search-intent options (`m` of module 770950).
pub(crate) const DESCRIBE_OPTIONS: &[FeedbackOption] = &[
    FeedbackOption {
        value: "looking_for_known_answer",
        key: "Str6/o",
    },
    FeedbackOption {
        value: "looking_for_unknown_answer",
        key: "DePZNY",
    },
    FeedbackOption {
        value: "message_from_specific_author",
        key: "sGKG/Y",
    },
    FeedbackOption {
        value: "images_and_files_results",
        key: "DYLXHx",
    },
    FeedbackOption {
        value: "link_results",
        key: "a1ZtE8",
    },
    FeedbackOption {
        value: "specific_topic",
        key: "EIHXbc",
    },
    FeedbackOption {
        value: "navigation_results",
        key: "6wKpiV",
    },
    FeedbackOption {
        value: OTHER_VALUE,
        key: "RBmOLj",
    },
];

pub(crate) struct FeedbackQuestion {
    pub title_key: &'static str,
    pub options: &'static [FeedbackOption],
    pub selected: Option<&'static str>,
    pub freeform: Entity<Editor>,
    _sub: Subscription,
}

impl FeedbackQuestion {
    /// Validity rule `f` of module 770950: an option must be picked, and `"other"` requires non-empty trimmed text.
    pub(crate) fn is_valid(&self, text: &str) -> bool {
        match self.selected {
            None => false,
            Some(v) if v == OTHER_VALUE => !text.trim().is_empty(),
            Some(_) => true,
        }
    }
}

pub(crate) struct SearchFeedbackModal {
    #[allow(dead_code)]
    pub rating: SearchRating,
    pub questions: Vec<FeedbackQuestion>,
    pub step: usize,
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_search_feedback(
        &mut self,
        rating: SearchRating,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut make_q = |title_key: &'static str, options: &'static [FeedbackOption]| {
            let freeform = cx.new(|cx| {
                let mut ed = Editor::new(editor_style(), cx);
                ed.set_placeholder(dc_intl::t("oV10fv").to_owned());
                ed
            });
            let sub = cx.subscribe_in(&freeform, window, |_, _, _: &EditorEvent, _, cx| {
                cx.notify();
            });
            FeedbackQuestion {
                title_key,
                options,
                selected: None,
                freeform,
                _sub: sub,
            }
        };
        let mut questions = Vec::new();
        if rating == SearchRating::Bad {
            questions.push(make_q("UyBQF7", UNSATISFIED_OPTIONS));
        }
        questions.push(make_q("LhMLCk", DESCRIBE_OPTIONS));
        self.search_feedback_modal = Some(SearchFeedbackModal {
            rating,
            questions,
            step: 0,
        });
        cx.notify();
    }

    pub(crate) fn close_search_feedback(&mut self) -> bool {
        self.search_feedback_modal.take().is_some()
    }

    pub(in crate::ui) fn search_feedback_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.search_feedback_modal.as_ref()?;
        let step = modal.step.min(modal.questions.len().saturating_sub(1));
        let q = modal.questions.get(step)?;
        let is_first = step == 0;
        let is_last = step + 1 >= modal.questions.len();
        let current_text = q.freeform.read(cx).text().to_owned();
        let current_valid = q.is_valid(&current_text);
        let all_valid = modal
            .questions
            .iter()
            .all(|item| item.is_valid(item.freeform.read(cx).text()));
        let freeform_enabled = q.selected.is_some();
        let char_count = current_text.chars().count().min(1024);

        let radio_rows: Vec<AnyElement> = q
            .options
            .iter()
            .enumerate()
            .map(|(idx, opt)| {
                let selected = q.selected == Some(opt.value);
                let val = opt.value;
                div()
                    .id(SharedString::from(format!("search-fb-opt-{step}-{idx}")))
                    .p(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if selected {
                        color("--control-primary-background-default")
                    } else {
                        color("--border-subtle")
                    })
                    .bg(if selected {
                        color("--background-mod-subtle")
                    } else {
                        color("--background-base-lower")
                    })
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(18.))
                            .rounded_full()
                            .border_2()
                            .border_color(if selected {
                                color("--control-primary-background-default")
                            } else {
                                color("--border-strong")
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |dot| {
                                dot.child(
                                    div()
                                        .size(px(8.))
                                        .rounded_full()
                                        .bg(color("--control-primary-background-default")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t(opt.key)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = this.search_feedback_modal.as_mut()
                            && let Some(cur) = m.questions.get_mut(step)
                        {
                            cur.selected = Some(val);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        let dialog = div()
            .id("search-feedback-modal")
            .occlude()
            .w(px(500.))
            .max_h(px(650.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("LRGdVy")),
                    )
                    .child(
                        div()
                            .id("search-fb-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_search_feedback();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("search-fb-scroll")
                    .px(px(24.))
                    .pb(px(20.))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t(q.title_key)),
                            )
                            .children(radio_rows),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("oV10fv")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(format!("{char_count}/1024")),
                                    ),
                            )
                            .child(
                                div()
                                    .min_h(px(72.))
                                    .p(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .when(!freeform_enabled, |d| d.opacity(0.5))
                                    .child(q.freeform.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .rounded_b(px(12.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("search-fb-secondary")
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
                            .child(if is_first {
                                dc_intl::t("ETE/oC")
                            } else {
                                dc_intl::t("13/7kX")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if is_first {
                                    this.close_search_feedback();
                                } else if let Some(m) = this.search_feedback_modal.as_mut() {
                                    m.step = m.step.saturating_sub(1);
                                }
                                cx.notify();
                            })),
                    )
                    .child({
                        let enabled = if is_last { all_valid } else { current_valid };
                        div()
                            .id("search-fb-primary")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(enabled, |b| {
                                b.hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                            })
                            .when(!enabled, |b| b.opacity(0.5))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(if is_last {
                                dc_intl::t("4Zpxtl")
                            } else {
                                dc_intl::t("PDTjLN")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !enabled {
                                    return;
                                }
                                if is_last {
                                    this.close_search_feedback();
                                } else if let Some(m) = this.search_feedback_modal.as_mut() {
                                    m.step += 1;
                                }
                                cx.notify();
                            }))
                    }),
            );

        Some(
            deferred(
                div()
                    .id("search-feedback-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_search_feedback();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_search_feedback(
        &mut self,
        arg: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let rating = if arg == "good" {
            SearchRating::Good
        } else {
            SearchRating::Bad
        };
        self.open_search_feedback(rating, window, cx);
        if let Some(m) = self.search_feedback_modal.as_mut() {
            if let Some(q0) = m.questions.first_mut() {
                q0.selected = Some(q0.options[0].value);
            }
            if arg == "step2" && m.questions.len() > 1 {
                m.step = 1;
                if let Some(q1) = m.questions.get_mut(1) {
                    q1.selected = Some(q1.options[1].value);
                }
            }
        }
        cx.notify();
        format!("search-feedback: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_24_keys_of_search_feedback_770950_resolve() {
        assert_eq!(UNSATISFIED_OPTIONS.len(), 8);
        assert_eq!(DESCRIBE_OPTIONS.len(), 8);
        for opt in UNSATISFIED_OPTIONS.iter().chain(DESCRIBE_OPTIONS.iter()) {
            assert!(!dc_intl::t(opt.key).is_empty(), "empty key {}", opt.key);
        }
        for key in [
            "LRGdVy", "UyBQF7", "LhMLCk", "oV10fv", "ETE/oC", "13/7kX", "PDTjLN", "4Zpxtl",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
