//! What the editor draws (stylesheets YPRx7I, wUYaqe, RHyl8o, wRblFq, 6t5fy-, XKaV_y, hNFXXu, NVxEwq, AH5XKr; modules
//! 423699 the timeline, 701266 and 657766 the panes).
use super::super::{Root, color, icon};
use super::editor::{
    CARD, Drag, Editor, HEADER, Layout, SIDEBAR, STRIP_HEIGHT, layout, thumb_cells,
};
use super::model;
use super::widgets::{self, Size};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, ObjectFit, SharedString, Window, canvas, div, img, prelude::*,
    px,
};

/// `Today at 3:04 PM`, `Yesterday at 3:04 PM`, else `11/9/2026 3:04 PM` (module 5377 `mk`).
pub(super) fn when(
    created: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    super::super::timestamps::calendar_time(created, now, false)
}

impl Root {
    /// The editor's two panes at the modal's size.
    pub(super) fn clips_editor_view(
        &self,
        editor: &Editor,
        modal: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        editor.scale.set(window.scale_factor());
        if editor.refocus.replace(false) {
            window.focus(&editor.shell, cx);
        }
        let layout = layout(modal, editor.aspect());
        div()
            .size_full()
            .flex()
            .track_focus(&editor.shell)
            .child(self.clips_editor_main(editor, layout, cx))
            .child(self.clips_editor_sidebar(editor, cx))
            .into_any_element()
    }

    fn clips_editor_main(
        &self,
        editor: &Editor,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let meta = &editor.clip.meta;
        let title = editor
            .edit
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| model::title(meta));
        let created = chrono::DateTime::from_timestamp_millis(meta.created_at as i64)
            .map(|time| time.with_timezone(&chrono::Local));
        let date = created
            .map(|created| when(created, chrono::Local::now()))
            .unwrap_or_default();
        let meta_item = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(text)
        };
        let header = div()
            .parity("clips-editor-header")
            .flex_none()
            .h(px(HEADER))
            .px(px(16.))
            .pt(px(16.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .h(px(24.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .overflow_hidden()
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(meta_item(meta.application_name.clone()))
                    .child(meta_item(date)),
            );
        let (video_w, video_h) = layout.video;
        let video = div()
            .flex_1()
            .min_h(px(0.))
            .p(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .parity("clips-editor-video")
                    .w(px(video_w))
                    .h(px(video_h))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--black-500"))
                    .children(editor.frame.clone().map(|frame| {
                        img(frame)
                            .size_full()
                            .object_fit(ObjectFit::Fill)
                            .into_any_element()
                    }))
                    .children(editor.failed.clone().map(|error| {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .text_color(color("--text-muted"))
                            .child(error)
                    })),
            );
        div()
            .parity("clips-editor-main")
            .flex_none()
            .w(px(layout.left))
            .h_full()
            .flex()
            .flex_col()
            .bg(color("--background-base-lowest"))
            .child(header)
            .child(video)
            .child(self.clips_timeline(editor, layout, cx))
            .into_any_element()
    }

    /// The timeline card: the header with the clock and the transport buttons, the ruler and the strip.
    fn clips_timeline(
        &self,
        editor: &Editor,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let crop = editor.edit.crop;
        let duration = editor.duration();
        let strip_w = layout.strip;
        let at = |seconds: f64| model::fraction(seconds, duration) * strip_w;
        let pill = div()
            .absolute()
            .left(px(16.))
            .top_0()
            .h_full()
            .flex()
            .items_center()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-muted"))
            .child(
                div()
                    .text_color(color("--text-default"))
                    .child(model::clock(editor.position - crop.start)),
            )
            .child(format!(" / {}", model::clock(crop.length())));
        let transport = div()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                widgets::icon_button("clips-skip-back", "clips_skip_back", Size::Sm, false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(editor) = this.clips_editor_mut() {
                            editor.skip(-10.);
                        }
                        cx.notify();
                    })),
            )
            .child(
                widgets::icon_button(
                    "clips-play",
                    if editor.playing {
                        "clips_pause"
                    } else {
                        "clips_play"
                    },
                    Size::Sm,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(editor) = this.clips_editor_mut() {
                        editor.toggle();
                    }
                    cx.notify();
                })),
            )
            .child(
                widgets::icon_button("clips-skip-forward", "clips_skip_forward", Size::Sm, false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(editor) = this.clips_editor_mut() {
                            editor.skip(10.);
                        }
                        cx.notify();
                    })),
            );
        let card_header = div()
            .relative()
            .flex_none()
            .h(px(46.))
            .bg(color("--background-mod-subtle"))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .child(pill)
            .child(transport);

        // The ruler: long notches with `Ns` labels, short ones between (`eY`).
        let notches = model::notches(duration);
        let mut ruler = div().relative().flex_none().h(px(24.)).w(px(strip_w));
        for (time, percent) in &notches.long {
            let x = strip_w * (*percent as f32) / 100.;
            ruler = ruler
                .child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top_0()
                        .h(px(24.))
                        .w(px(1.))
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(x + 4.))
                        .bottom(px(1.))
                        .whitespace_nowrap()
                        .text_size(px(10.))
                        .line_height(px(12.))
                        .text_color(color("--text-muted"))
                        .child(format!("{}s", time.round() as i64)),
                );
        }
        for percent in &notches.short {
            ruler = ruler.child(
                div()
                    .absolute()
                    .left(px(strip_w * (*percent as f32) / 100.))
                    .bottom_0()
                    .h(px(8.))
                    .w(px(1.))
                    .bg(color("--border-subtle")),
            );
        }

        // The strip: thumbnails, the dimmed parts outside the range, the range's box with its handles, the playhead.
        let (cell_w, _) = thumb_cells(strip_w, editor.aspect());
        let cells = editor.thumbs.iter().map(|thumb| {
            div()
                .flex_none()
                .w(px(cell_w as f32))
                .h(px(STRIP_HEIGHT))
                .children(thumb.clone().map(|thumb| {
                    img(thumb)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                }))
        });
        let dim = |left: f32, width: f32| {
            div()
                .absolute()
                .top_0()
                .left(px(left))
                .w(px(width.max(0.)))
                .h_full()
                .bg(gpui::Rgba {
                    a: 0.7,
                    ..color("--black-500")
                })
        };
        let (left, right) = (at(crop.start), at(crop.end));
        let handle = |id: &'static str, x: f32, which: Drag| {
            div()
                .id(id)
                .absolute()
                .top_0()
                .left(px(x))
                .w(px(16.))
                .h_full()
                .bg(color("--brand-500"))
                .cursor_ew_resize()
                .flex()
                .items_center()
                .justify_center()
                // 503535 / cropArrow_wUYaqe: the visible grip inside each crop handle.
                .child(div().w(px(2.)).h(px(16.)).rounded(px(100.)).bg(gpui::Rgba {
                    a: 0.7,
                    ..color("--white")
                }))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(editor) = this.clips_editor_mut() {
                            editor.begin_drag(which, f32::from(event.position.x));
                        }
                        cx.notify();
                    }),
                )
        };
        let bounds_cell = editor.strip.clone();
        let strip = div()
            .id("clips-strip")
            .parity("clips-editor-strip")
            .relative()
            .flex_none()
            .w(px(strip_w))
            .h(px(STRIP_HEIGHT))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    if let Some(editor) = this.clips_editor_mut() {
                        editor.begin_drag(Drag::Playhead, f32::from(event.position.x));
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .overflow_hidden()
                    .rounded(px(8.))
                    .children(cells),
            )
            .child(dim(0., left))
            .child(dim(right, strip_w - right))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(left))
                    .w(px((right - left).max(0.)))
                    .h_full()
                    .border_t(px(4.))
                    .border_b(px(4.))
                    .border_color(color("--brand-500")),
            )
            .child(handle("clips-handle-start", left, Drag::Start))
            .child(handle("clips-handle-end", (right - 16.).max(0.), Drag::End))
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let playhead = div()
            .absolute()
            .left(px(at(editor.position) - 1.))
            .top(px(-32.))
            .w(px(2.))
            .h(px(32. + STRIP_HEIGHT))
            .bg(color("--red-430"));
        let body = div()
            .relative()
            .p(px(16.))
            .flex()
            .flex_col()
            .child(ruler)
            .child(div().relative().child(strip).child(playhead));
        div()
            .parity("clips-editor-timeline")
            .flex_none()
            .mx(px(16.))
            .mb(px(16.))
            .h(px(CARD))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .child(card_header)
            .child(body)
            .into_any_element()
    }

    /// The sidebar `clipForm`: the favourite star and the close button, Clip Info and the audio switches.
    fn clips_editor_sidebar(&self, editor: &Editor, cx: &mut Context<Self>) -> AnyElement {
        let meta = &editor.clip.meta;
        let favourite = meta.is_favorite.unwrap_or(false);
        let header = div()
            .flex_none()
            .h(px(48.))
            .px(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        widgets::icon_button(
                            "clips-favourite",
                            if favourite {
                                "clips_star"
                            } else {
                                "clips_star_outline"
                            },
                            Size::Sm,
                            favourite,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.clips_toggle_favourite(cx))),
                    )
                    .child(
                        widgets::icon_button("clips-delete", "clips_trash", Size::Sm, false)
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                this.clips_ask_delete(event.modifiers().shift, cx);
                            })),
                    )
                    .child(
                        widgets::icon_button("clips-more", "clips_more", Size::Sm, false)
                            .relative()
                            .child(self.anchor_probe("clips-more"))
                            .on_click(cx.listener(|this, _, _, cx| this.clips_toggle_menu(cx))),
                    ),
            )
            .child(
                widgets::icon_button("clips-close", "clips_close", Size::Sm, false)
                    .on_click(cx.listener(|this, _, _, cx| this.clips_editor_closed(cx))),
            );
        let label = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let card = || {
            div()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(color("--background-mod-subtle"))
                .p(px(12.))
        };
        let title_text = editor
            .edit
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| model::title(meta));
        let thumbnail = self
            .clips
            .engine
            .ui
            .pictures
            .borrow_mut()
            .entry(meta.id.clone())
            .or_insert_with(|| super::panels::decode(&meta.thumbnail))
            .clone();
        let title_view: AnyElement = if editor.title_editing {
            div()
                .id("clips-title-field")
                .flex_1()
                .min_w(px(0.))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.clips_end_title_edit(cx)))
                .child(editor.title.clone())
                .into_any_element()
        } else {
            div()
                .id("clips-title")
                .group("clips-title")
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.clips_edit_title(window, cx)))
                .child(
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(title_text),
                )
                .child(
                    icon("clips_pencil", 16., "--text-muted")
                        .invisible()
                        .group_hover("clips-title", |style| style.visible()),
                )
                .into_any_element()
        };
        let info = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label("Clip Info"))
            .child(
                card()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_none()
                            .w(px(40.))
                            .h(px(30.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--black-500"))
                            .children(thumbnail.map(|thumbnail| {
                                img(thumbnail)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element()
                            })),
                    )
                    .child(title_view),
            );
        // The audio rows: one per track the file holds. Our files have `application` and `voice` only, so there is no
        // soundboard row and no per-source count.
        let tracks = editor
            .player
            .shared
            .loaded
            .get()
            .map(|loaded| loaded.tracks.clone())
            .unwrap_or_default();
        let has_audio = !tracks.is_empty();
        let mut audio = div().flex().flex_col().gap(px(8.));
        for (name, _) in tracks {
            let (text, glyph, on, key): (String, &'static str, bool, &'static str) =
                match name.as_str() {
                    "voice" => (
                        "Speaker".to_owned(),
                        "clips_mic",
                        editor.edit.voice_audio,
                        "voice",
                    ),
                    _ => (
                        meta.application_name.clone(),
                        "clips_screen",
                        editor.edit.application_audio,
                        "application",
                    ),
                };
            audio = audio.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(glyph, 20., "--text-muted"))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(text),
                    )
                    .child(
                        widgets::icon_button(
                            SharedString::from(format!("clips-audio-{key}")),
                            if on {
                                "clips_voice_on"
                            } else {
                                "clips_voice_off"
                            },
                            Size::Sm,
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.clips_toggle_audio(key, cx);
                        })),
                    ),
            );
        }
        let audio_section = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label("Audio"))
            .child(card().child(audio));
        div()
            .parity("clips-editor-sidebar")
            .flex_none()
            .w(px(SIDEBAR))
            .h_full()
            .flex()
            .flex_col()
            .bg(color("--background-base-lower"))
            .border_l_1()
            .border_color(color("--border-muted"))
            .child(header)
            .child(info)
            .children(has_audio.then_some(audio_section))
            // `clipFormFooter`: 16 px padding, `--border-muted` above, pushed to the bottom; one primary button,
            // `Share` (module 686320 for no picker action).
            .child(
                div()
                    .mt_auto()
                    .flex_none()
                    .p(px(16.))
                    .border_t_1()
                    .border_color(color("--border-muted"))
                    .child(
                        widgets::button(
                            "clips-share",
                            "Share",
                            Some("clips_share"),
                            widgets::Variant::Primary,
                            Size::Md,
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.clips_share(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn dates_read_today_yesterday_or_in_full() {
        let at = |day, hour| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, day, hour, 5, 0)
                .unwrap()
        };
        assert_eq!(when(at(11, 15), at(11, 18)), "Today at 3:05 PM");
        assert_eq!(when(at(10, 0), at(11, 18)), "Yesterday at 12:05 AM");
        assert_eq!(when(at(3, 9), at(11, 18)), "11/3/2026 9:05 AM");
    }
}
