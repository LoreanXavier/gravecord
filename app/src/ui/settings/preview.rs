//! The Accessibility page's "Preview" box (`design/golden/windows/settings-full/accessibility.json`): two example
//! messages, reaction pills, three member avatars and an "Example Button", drawn with the chat font size, group spacing,
//! link underline and message display the page's own controls set. It stays pinned 64px below the top of the scroller
//! while the page scrolls under a cover that reaches the box's bottom edge and fades out over 30px.

use super::{Root, color, panel_nudge};
use crate::ui::{format_timestamp, status_dot_sized};
use gpui::{
    AnyElement, FontWeight, IntoElement, ParentElement, Styled, Window, div, img, prelude::*, px,
};

/// Top of the label inside the scroller: the page's 64px top margin.
const TOP: f32 = 64.;
/// The box's top (label 20px plus its gap) and its height (`accessibility-00.png`: 183 to 361 on screen).
const BOX_TOP: f32 = 93.;
const BOX_HEIGHT: f32 = 178.;
/// The fade below the cover.
const FADE: f32 = 30.;
/// The preview's own scroll content.
const CONTENT_HEIGHT: f32 = 208.;
/// Name colour of the example author, as captured in the preview (`username_2i8Bfm`).
const AUTHOR: u32 = 0xdd80f4;

/// The default avatars the preview shows (`avatar_2i8Bfm`, `avatar_XpIUHy`): the author's own and three members.
pub(super) fn requests() -> [(String, String); 4] {
    [0u8, 2, 3, 4].map(|index| {
        (
            format!("avatar:default:{index}"),
            format!("https://cdn.discordapp.com/embed/avatars/{index}.png"),
        )
    })
}

impl Root {
    /// The pinned overlay: cover, fade, "Preview" label and the box. Placed over the scroller, under its scrollbar.
    pub(super) fn accessibility_preview(&self, window: &mut Window) -> AnyElement {
        let page = color("--background-base-low");
        let mut clear = page;
        clear.a = 0.;
        let width: f32 = window.viewport_size().width.into();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(BOX_TOP + BOX_HEIGHT + FADE))
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(BOX_TOP + BOX_HEIGHT))
                    .bg(page),
            )
            .child(
                div()
                    .absolute()
                    .top(px(BOX_TOP + BOX_HEIGHT))
                    .left_0()
                    .right_0()
                    .h(px(FADE))
                    .bg(gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(page, 0.),
                        gpui::linear_color_stop(clear, 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .max_w(px(728.))
                            .ml(px(panel_nudge(width)))
                            .px(px(16.))
                            .h(px(BOX_TOP + BOX_HEIGHT))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(TOP))
                                    .left(px(16.))
                                    .right(px(16.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child("Preview"),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(BOX_TOP))
                                    .left(px(16.))
                                    .right(px(16.))
                                    .h(px(BOX_HEIGHT))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .overflow_hidden()
                                    .child(self.preview_scroll()),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn preview_avatar(&self, index: u8, size: f32) -> gpui::Div {
        let image = self.images.get(&format!("avatar:default:{index}")).cloned();
        div()
            .size(px(size))
            .flex_none()
            .children(image.map(|image| img(image).size(px(size)).rounded_full()))
    }

    /// The messages and the right-hand column. Positions are the golden's, relative to the box's inner corner.
    fn preview_scroll(&self) -> gpui::Stateful<gpui::Div> {
        let size = self.chat_font_size();
        let line = (size * 1.375).round();
        let compact = self.compact_messages();
        let spacing = self.message_group_spacing();
        let underline = self.underline_links();
        let time = format_timestamp(chrono::Utc::now().timestamp(), 't');
        let name = |this: gpui::Div| {
            this.text_size(px(16.))
                .line_height(px(22.))
                .font_weight(FontWeight(500.))
                .text_color(gpui::rgb(AUTHOR))
        };
        let stamp = |this: gpui::Div| {
            this.text_size(px(12.))
                .line_height(px(22.))
                .font_weight(FontWeight(500.))
                .text_color(color("--chat-text-muted"))
        };
        let body = |this: gpui::Div| {
            this.text_size(px(size))
                .line_height(px(line))
                .text_color(color("--text-default"))
        };
        let pill = |glyph: &'static str, count: &'static str, me: bool| {
            let image = self.images.get(&format!("emoji:{glyph}")).cloned();
            div()
                .h(px(30.))
                .px(px(6.))
                .rounded(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .border_1()
                .when(me, |this| {
                    this.bg(color("--reaction-background-reacted-default"))
                        .border_color(color("--reaction-border-reacted-default"))
                })
                .when(!me, |this| {
                    this.bg(color("--reaction-background-default"))
                        .border_color(gpui::transparent_black())
                })
                .child(
                    div()
                        .size(px(20.))
                        .children(image.map(|image| img(image).size(px(20.)))),
                )
                .child(
                    div()
                        .ml(px(6.))
                        .min_w(px(12.))
                        .flex()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(if me {
                            color("--reaction-text-reacted-default")
                        } else {
                            color("--text-subtle")
                        })
                        .child(count),
                )
        };
        let link = |this: gpui::Div| {
            let this = this.text_color(color("--text-link"));
            if underline { this.underline() } else { this }
        };
        let first_top = 26.;
        let content = if compact {
            // Compact lines: the time, the name and the text on one line, no avatars.
            let line_top = |index: f32| first_top + index * (line + 2. + spacing);
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(CONTENT_HEIGHT))
                .child(
                    div()
                        .absolute()
                        .left(px(24.))
                        .top(px(line_top(0.)))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(stamp(div()).child(time.clone()))
                        .child(name(div()).child("onugeccen"))
                        .child(body(div()).child("what happened to all the beans")),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(24.))
                        .top(px(line_top(1.)))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(stamp(div()).child(time.clone()))
                        .child(name(div()).child("onugeccen"))
                        .child(body(div()).child("here's a link"))
                        .child(link(body(div())).child("https://discord.com/accessibility")),
                )
        } else {
            // 25px from the pills to the next name at the default 16px of group spacing (golden: 103 to 128).
            let second = first_top + 22. + line + 4. + 28. + 1. + spacing + 9.;
            let header = || {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(name(div()).child("onugeccen"))
                    .child(stamp(div()).child(time.clone()))
            };
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(CONTENT_HEIGHT))
                .child(
                    self.preview_avatar(0, 40.)
                        .absolute()
                        .left(px(24.))
                        .top(px(first_top + 2.)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(80.))
                        .top(px(first_top))
                        .flex()
                        .flex_col()
                        .child(header())
                        .child(body(div()).child("what happened to all the beans"))
                        .child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .gap(px(6.))
                                .child(pill("\u{1f940}", "3", true))
                                .child(pill("\u{1faa4}", "1", false)),
                        ),
                )
                .child(
                    self.preview_avatar(0, 40.)
                        .absolute()
                        .left(px(24.))
                        .top(px(second + 2.)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(80.))
                        .top(px(second))
                        .flex()
                        .flex_col()
                        .child(header())
                        .child(
                            div()
                                .flex()
                                .child(body(div()).child("here's a link "))
                                .child(
                                    link(body(div())).child("https://discord.com/accessibility"),
                                ),
                        ),
                )
        };
        let column = |top: f32, index: u8, status: &'static str| {
            div().absolute().left(px(630.)).top(px(top)).child(
                div()
                    .relative()
                    .size(px(32.))
                    .child(self.preview_avatar(index, 32.))
                    .child(status_dot_sized(status, "--background-base-low", 10., 3.)),
            )
        };
        let button = div()
            .absolute()
            .left(px(542.))
            .top(px(151.))
            .w(px(120.))
            .h(px(32.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .bg(color("--control-primary-background-default"))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-primary-text-default"))
            .child("Example Button");
        div()
            .id("accessibility-preview")
            .size_full()
            .relative()
            .overflow_y_scroll()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(CONTENT_HEIGHT))
                    .child(content)
                    .child(column(23., 2, "idle"))
                    .child(column(61., 3, "dnd"))
                    .child(column(99., 4, "online"))
                    .child(button),
            )
    }
}
