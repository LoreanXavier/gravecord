//! "Stream failed to start :(" (module 768088 on module 200749) over the focused stream that failed: the watched
//! stream ended as FAILED (here: no server or connection came within the rendezvous bound, or the connection could not
//! be made again) stays on screen, as an ended one does (module 566331), until Close Stream (`STREAM_CLOSE`, the
//! error cleared with `W` of module 37965).
//!
//! Module 200749 without art (768088 passes none): the header (`body-md/semibold`, white, 4 under it), "Error: 2001"
//! (`stream-failed-to-start` of module 487329; `body-sm/semibold`, `text-muted`), "Having issues with your stream? Let
//! us try to help." (`body-sm/medium`, `--primary-200`, not under 400 wide) to the STREAM_FAILED help article, and the
//! secondary Close Stream (`sm` under 400 wide).

use super::Root;
use dc_tokens::Theme;
use gpui::{AnyElement, Context, FontWeight, IntoElement, Rgba, div, prelude::*, px, rgba};

/// `errorCode` of `stream-failed-to-start` (module 487329).
pub(super) const STREAM_FAILED_TO_START: i64 = 2001;
/// `STREAM_FAILED` ("360040816151", module 652215) through `getArticleURL` (module 975571, en-us).
const HELP_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360040816151";

fn mid(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

impl Root {
    /// Module 768088 for a tile `width` wide.
    pub(super) fn stream_failed_content(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        // `J` of module 200749.
        let small = width < 400.;
        let lead = dc_intl::format_runs("sXsgsP", &[("helpUrl", HELP_ARTICLE.to_owned().into())]);
        let mut text = String::new();
        let mut link = 0..0;
        for (run, hook) in lead {
            let start = text.len();
            text.push_str(&run);
            if hook.is_some() {
                link = start..text.len();
            }
        }
        let styled = gpui::StyledText::new(text).with_highlights([(
            link.clone(),
            gpui::HighlightStyle {
                color: Some(mid("--text-link").into()),
                ..Default::default()
            },
        )]);
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(mid("--primary-700"))
            .p(px(4.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .mb(px(4.))
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(mid("--white"))
                    .child(dc_intl::t("rSlOep")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(mid("--text-muted"))
                    .child(dc_intl::format(
                        "ejOT95",
                        &[("errorCode", (STREAM_FAILED_TO_START as f64).into())],
                    )),
            )
            .when(!small, |this| {
                this.child(
                    div()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(mid("--primary-200"))
                        .child(
                            gpui::InteractiveText::new("stream-failed-help", styled).on_click(
                                vec![link],
                                {
                                    let open = cx.listener(|this, _: &usize, _, cx| {
                                        this.follow_link(HELP_ARTICLE, "", cx);
                                    });
                                    move |index, window, cx| open(&index, window, cx)
                                },
                            ),
                        ),
                )
            })
            .child(
                div().mt(px(16.)).child(
                    div()
                        .id("stream-failed-close")
                        .h(px(if small { 32. } else { 40. }))
                        .px(px(if small { 12. } else { 16. }))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(mid("--control-secondary-border-default"))
                        .bg(mid("--control-secondary-background-default"))
                        .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_size(px(if small { 14. } else { 16. }))
                        .line_height(px(if small { 18. } else { 20. }))
                        .font_weight(FontWeight(500.))
                        .text_color(mid("--control-secondary-text-default"))
                        .child(dc_intl::t("4EGMWL"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_ended_stream(cx);
                        })),
                ),
            )
            .into_any_element()
    }
}
