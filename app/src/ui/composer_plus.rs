//! The composer's "+" menu and the poll editor it opens. Rows, sizes and order are from
//! `design/dom/menus/composer-plus-darker-cozy.json`; the poll editor is module 50518 (`createPoll` in module 180662
//! builds the request) and `design/dom/modals/create-poll-darker-cozy.json`.

use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton, SharedString,
    Subscription, Window, deferred, div, prelude::*, px,
};
use serde_json::json;

/// Limits from module 50518: the question 300 characters, an answer 55, ten answers at most.
pub(super) const QUESTION_MAX: usize = 300;
pub(super) const ANSWER_MAX: usize = 55;
pub(super) const ANSWERS_MAX: usize = 10;

/// The durations the poll editor offers, in hours (`N.fT`), with their labels.
pub(super) const DURATIONS: [(u32, &str); 7] = [
    (1, "1 hour"),
    (4, "4 hours"),
    (8, "8 hours"),
    (24, "24 hours"),
    (72, "3 days"),
    (168, "1 week"),
    (336, "2 weeks"),
];

/// A "+" menu row's natural width for a label `label` wide: 8px padding each side, the 20px icon and the 8px gap, and
/// the 20px caret 8px after the label (frame composer-plus: 183.44 for "Schedule Message", 111.44 of text).
pub(super) fn plus_row_width(label: f32, caret: bool) -> f32 {
    8. + 20. + 8. + label + if caret { 8. + 20. } else { 0. } + 8.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plus {
    Upload,
    Thread,
    Poll,
    /// "Use Apps": the composer holds a slash, which lists the commands.
    Apps,
    /// "Schedule Message": opens its submenu of times.
    Schedule,
    /// "Record Voice Message": starts a voice message recording in the composer (module 316576).
    RecordVoice,
    /// "Share a Clip": opens the Clips Gallery to pick a clip to share.
    ShareClip,
    /// Not in the pinned client: VoiceMessages' "Send Voice Message" (`plugin_hooks/voice_messages.rs`).
    PluginVoiceMessage,
    /// Not in the pinned client: ClipUpload's "Upload Clip File" (`plugin_hooks/clip_upload.rs`).
    PluginClipUpload,
}

pub(super) struct PollForm {
    pub question: Entity<Editor>,
    pub answers: Vec<Entity<Editor>>,
    pub duration: u32,
    pub multiple: bool,
    pub duration_open: bool,
    pub error: Option<&'static str>,
    /// The time the poll is scheduled for (module 50518 `Z`), local; `None` posts it at once.
    pub scheduled: Option<chrono::NaiveDateTime>,
    _events: Vec<Subscription>,
}

/// The request body `createPoll` sends: empty answers are dropped, the question is trimmed.
pub(super) fn poll_body(
    question: &str,
    answers: &[String],
    multiple: bool,
    duration: u32,
) -> Option<serde_json::Value> {
    let question = question.trim();
    let answers: Vec<serde_json::Value> = answers
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .map(|text| json!({"poll_media": {"text": text}}))
        .collect();
    if question.is_empty() || answers.is_empty() {
        return None;
    }
    Some(json!({
        "poll": {
            "question": {"text": question},
            "answers": answers,
            "allow_multiselect": multiple,
            "duration": duration,
            "layout_type": 1,
        }
    }))
}

/// `text` cut to `max` characters, or `None` when it already fits.
fn clipped(text: &str, max: usize) -> Option<String> {
    (text.chars().count() > max).then(|| text.chars().take(max).collect())
}

impl Root {
    pub(super) fn toggle_plus_menu(&mut self, cx: &mut Context<Self>) {
        self.plus_menu = !self.plus_menu;
        self.schedule.submenu = false;
        cx.notify();
    }

    pub(super) fn run_plus(&mut self, entry: Plus, window: &mut Window, cx: &mut Context<Self>) {
        // A row with a submenu opens it (a click after the hover keeps it open) and keeps the menu.
        if entry == Plus::Schedule {
            self.schedule.submenu = true;
            cx.notify();
            return;
        }
        if entry == Plus::RecordVoice && self.record_voice_disabled() {
            return;
        }
        self.plus_menu = false;
        match entry {
            Plus::Upload => self.pick_files(cx),
            Plus::Thread => self.open_new_thread(window, cx),
            Plus::Poll => self.open_poll(window, cx),
            Plus::Apps => {
                // `setValue("/")`: the field takes a slash, which opens the command list.
                self.composer.update(cx, |composer, cx| {
                    composer.set_text("");
                    composer.insert("/", cx);
                });
                window.focus(&self.composer.read(cx).focus_handle(cx), cx);
            }
            Plus::RecordVoice => {
                // The bar has no text field; the window takes Enter.
                if self.voice_record_start(cx).starts_with("voice: recording") {
                    window.focus(&self.focus, cx);
                }
            }
            Plus::ShareClip => self.clips_open_gallery(super::clips::Facet::All, cx),
            Plus::PluginVoiceMessage => self.voice_messages_open(cx),
            Plus::PluginClipUpload => self.clip_upload_open(cx),
            Plus::Schedule => {}
        }
        cx.notify();
    }

    fn answer_editor(
        &mut self,
        cx: &mut Context<Self>,
        events: &mut Vec<Subscription>,
    ) -> Entity<Editor> {
        let editor = cx.new(|cx| {
            // Module 95477's input: no inline padding, the placeholder in `--input-placeholder-text-default`.
            let mut style = super::editor_style();
            style.placeholder = super::hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder_message("Type your answer");
            editor
        });
        events.push(
            cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Edited => {
                        if let Some(cut) = clipped(editor.read(cx).text(), ANSWER_MAX) {
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                        if let Some(form) = this.poll.as_mut() {
                            form.error = None;
                        }
                    }
                    // Enter in the last answer adds one, as the official editor does.
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text));
                        this.add_poll_answer(cx)
                    }
                    EditorEvent::Cancel => this.poll = None,
                }
                cx.notify();
            }),
        );
        editor
    }

    pub(super) fn open_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.channel.is_none() {
            return;
        }
        let question = cx.new(|cx| {
            let mut style = super::editor_style();
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            style.line_height = 21.;
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder_message("What question do you want to ask?");
            editor
        });
        let mut events = vec![
            cx.subscribe(&question, |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Edited => {
                        if let Some(cut) = clipped(editor.read(cx).text(), QUESTION_MAX) {
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                        if let Some(form) = this.poll.as_mut() {
                            form.error = None;
                        }
                    }
                    EditorEvent::Submit(text) => {
                        // Only Ctrl+Enter posts, as in the official editor; Enter keeps the question.
                        editor.update(cx, |editor, _| editor.set_text(text));
                    }
                    EditorEvent::Cancel => this.poll = None,
                }
                cx.notify();
            }),
        ];
        let answers = vec![
            self.answer_editor(cx, &mut events),
            self.answer_editor(cx, &mut events),
        ];
        window.focus(&question.read(cx).focus_handle(cx), cx);
        self.poll = Some(PollForm {
            question,
            answers,
            duration: 24,
            multiple: false,
            duration_open: false,
            error: None,
            scheduled: None,
            _events: events,
        });
        cx.notify();
    }

    fn add_poll_answer(&mut self, cx: &mut Context<Self>) {
        let Some(mut form) = self.poll.take() else {
            return;
        };
        if form.answers.len() < ANSWERS_MAX {
            let editor = self.answer_editor(cx, &mut form._events);
            form.answers.push(editor);
        }
        self.poll = Some(form);
        cx.notify();
    }

    fn remove_poll_answer(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(form) = self.poll.as_mut()
            && form.answers.len() > 1
            && index < form.answers.len()
        {
            form.answers.remove(index);
        }
        cx.notify();
    }

    pub(super) fn post_poll(&mut self, cx: &mut Context<Self>) {
        let (Some(channel), Some(form)) = (self.channel, self.poll.as_mut()) else {
            return;
        };
        let answers: Vec<String> = form
            .answers
            .iter()
            .map(|editor| editor.read(cx).text().to_owned())
            .collect();
        let question = form.question.read(cx).text().to_owned();
        let scheduled = form.scheduled;
        let Some(body) = poll_body(&question, &answers, form.multiple, form.duration) else {
            form.error = Some(if question.trim().is_empty() {
                "Please enter a question."
            } else {
                "Please enter at least one answer."
            });
            cx.notify();
            return;
        };
        // `sendPollMessage` with `scheduledTimestamp` (module 180662) creates a scheduled message instead.
        if let Some(when) = scheduled {
            let timestamp = super::guild_events::schedule::iso_utc(when);
            self.send_scheduled_poll(channel, timestamp, &body["poll"]);
        } else {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: format!("/channels/{channel}/messages"),
                body: Some(body),
            });
        }
        self.poll = None;
        cx.notify();
    }

    /// The left edge of the chat column whose composer owns the "+" button: the chat beside the sidebars, or an opened
    /// message request's panel, which sits on the window's right edge.
    pub(super) fn composer_column_left(&self) -> f32 {
        if self.message_requests_active() && self.channel.is_some() {
            self.viewport.0 - self.mr_panel_width(self.viewport.0)
        } else {
            self.combined_sidebar_width()
        }
    }

    /// The menu above the "+" button: as wide as its widest row (201.44 in the capture), its bottom 61px above the
    /// window's, left edge on the composer's.
    pub(super) fn plus_menu_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.plus_menu {
            return None;
        }
        let share_clip = cfg!(windows) && !crate::storage::is_capture_run() && self.clips_listed();
        // `menu_M3qk4j flexible_M3qk4j` (frame composer-plus: 201.44 for "Schedule Message" and its caret): as wide as
        // the widest row, from 144 to 320; a longer label is cut with an ellipsis (`label_M3qk4j`).
        let rows: Vec<(&str, bool)> = [
            Some((dc_intl::tr("Upload a File"), false)),
            Some((dc_intl::tr("Create Thread"), false)),
            Some((dc_intl::tr("Create Poll"), false)),
            Some((dc_intl::tr("Use Apps"), false)),
            self.schedule_offered()
                .then(|| (dc_intl::tr("Schedule Message"), true)),
            self.record_voice_offered()
                .then(|| (dc_intl::tr("Record Voice Message"), false)),
            share_clip.then(|| (dc_intl::tr("Share a Clip"), false)),
            self.voice_messages_plus_label().map(|label| (label, false)),
            self.clip_upload_plus_label().map(|label| (label, false)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let natural = super::menu::flexible_width_unclamped(
            rows.iter()
                .map(|(label, caret)| {
                    plus_row_width(
                        super::threads::measure_text(window, label, 14., 500.),
                        *caret,
                    )
                })
                .fold(0., f32::max),
        );
        let width = super::menu::flexible_width_clamp(natural);
        // Only a menu stopped at its max width has labels to cut; the others fit their widest label by construction
        // (kept on one line, so a fraction of a pixel lost to the layout's pixel snapping cuts nothing).
        let cut = natural > width;
        let row = |id: &'static str,
                   glyph: &'static str,
                   label: &'static str,
                   entry: Option<Plus>,
                   caret: bool,
                   disabled: bool,
                   cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|this| this.bg(color("--background-mod-subtle")))
                })
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(icon(glyph, 20., "--interactive-icon-default"))
                // `label_M3qk4j`: `flex: 1 1 auto`, one line, cut with an ellipsis.
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .when(cut, |label| label.truncate())
                        .child(label),
                )
                .when(caret, |this| {
                    this.child(icon("pm_chevron", 20., "--interactive-icon-default"))
                })
                // The submenu follows the pointer: the Schedule Message row opens it, any other row closes it.
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let open = !disabled && entry == Some(Plus::Schedule);
                    if *hovered && this.schedule.submenu != open {
                        this.schedule.submenu = open;
                        cx.notify();
                    }
                }))
                .when(!disabled, |this| {
                    this.on_click(cx.listener(move |this, _, window, cx| {
                        match entry {
                            Some(entry) => this.run_plus(entry, window, cx),
                            None => this.plus_menu = false,
                        }
                        cx.notify();
                    }))
                })
                .when(!disabled, |row| {
                    self.menu_stop(row, id, color("--background-mod-subtle"), cx)
                })
        };
        let record_finalizing = self.record_voice_disabled();
        let menu = div()
            .parity_with_border("plus-menu", 1.)
            .id("composer-plus-menu")
            .occlude()
            .absolute()
            // The menu keeps 18.56 from the window's right edge (220 for the captured 201.44).
            .left(px(
                (self.composer_column_left() + 20.).min((self.viewport.0 - width - 18.56).max(16.))
            ))
            .bottom(px(61.))
            .w(px(width))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(row(
                "plus-upload",
                "cp_upload",
                dc_intl::tr("Upload a File"),
                Some(Plus::Upload),
                false,
                false,
                cx,
            ))
            .child(row(
                "plus-thread",
                "cp_thread",
                dc_intl::tr("Create Thread"),
                Some(Plus::Thread),
                false,
                false,
                cx,
            ))
            .child(row(
                "plus-poll",
                "cp_poll",
                dc_intl::tr("Create Poll"),
                Some(Plus::Poll),
                false,
                false,
                cx,
            ))
            .child(row(
                "plus-apps",
                "cp_apps",
                dc_intl::tr("Use Apps"),
                Some(Plus::Apps),
                false,
                false,
                cx,
            ))
            .children(self.schedule_offered().then(|| {
                row(
                    "plus-schedule",
                    "cp_schedule",
                    dc_intl::tr("Schedule Message"),
                    Some(Plus::Schedule),
                    true,
                    false,
                    cx,
                )
                .child(self.anchor_probe("plus-schedule-row"))
            }))
            .children(self.record_voice_offered().then(|| {
                row(
                    "plus-record",
                    "cp_mic",
                    dc_intl::tr("Record Voice Message"),
                    Some(Plus::RecordVoice),
                    false,
                    record_finalizing,
                    cx,
                )
            }))
            .children(share_clip.then(|| {
                row(
                    "plus-share-clip",
                    "clips_clips",
                    dc_intl::tr("Share a Clip"),
                    Some(Plus::ShareClip),
                    false,
                    false,
                    cx,
                )
            }))
            // Not in the pinned client: VoiceMessages' row, last (`plugin_hooks/voice_messages.rs`).
            .children(self.voice_messages_plus_label().map(|label| {
                row(
                    "plus-plugin-voice-message",
                    "cp_mic",
                    label,
                    Some(Plus::PluginVoiceMessage),
                    false,
                    false,
                    cx,
                )
            }))
            // Not in the pinned client: ClipUpload's row (`plugin_hooks/clip_upload.rs`).
            .children(self.clip_upload_plus_label().map(|label| {
                row(
                    "plus-plugin-clip-upload",
                    "plugin_cloud_upload",
                    label,
                    Some(Plus::PluginClipUpload),
                    false,
                    false,
                    cx,
                )
            }));
        let layer = div()
            .id("composer-plus-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.press_outside(super::keyboard::Layer::PlusMenu, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| {
                    if this.press_outside(super::keyboard::Layer::PlusMenu, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.press_outside(super::keyboard::Layer::PlusMenu, cx);
                }),
            )
            .child(menu)
            .children(self.schedule_submenu(window, cx));
        Some(deferred(layer).with_priority(1).into_any_element())
    }

    pub(super) fn poll_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.poll.as_ref()?;
        let label = |text: &'static str| {
            div()
                .h(px(20.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        // Module 50518's question box (module 103557, `autosize`, `rows` 1, `maxLength` 300 with the counter): the
        // wrapper is 48px (frame create-poll: a 22px content box, `Math.ceil(scrollHeight + 1)` of module 643135),
        // `.textArea_uI0xjn` pads 12px 10px and `style.paddingRight` is `7.23 * 9 + 10` = 75.07px for the counter.
        // `.wrapper_UcGqzv`: a 1px `--input-border-default` border, and on focus a 2px inset outline in
        // `--input-border-active` (drawn here as a 2px border with the padding one pixel smaller).
        let field = |editor: Entity<Editor>, focused: bool| {
            let inset = if focused { 1. } else { 0. };
            div()
                .min_h(px(48.))
                .pt(px(12. - inset))
                .pb(px(12. - inset))
                .pl(px(10. - inset))
                .pr(px(75.07 - inset))
                .rounded(px(8.))
                .when(focused, |d| d.border_2())
                .when(!focused, |d| d.border_1())
                .border_color(if focused {
                    color("--input-border-active")
                } else {
                    color("--input-border-default")
                })
                .bg(color("--input-background-default"))
                .child(editor)
        };
        let count = form.question.read(cx).text().chars().count();
        let many = form.answers.len() > 1;
        let rows: Vec<AnyElement> = form
            .answers
            .iter()
            .enumerate()
            .map(|(index, editor)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(40.))
                    .px(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    // The leading slot: 40x30 with a transparent 1px border, the 18px `reaction` icon 14px from its
                    // left edge, `--interactive-text-default` and `--interactive-text-active` on hover.
                    .child(
                        div()
                            .id(SharedString::from(format!("poll-reaction-{index}")))
                            .w(px(40.))
                            .h(px(30.))
                            .flex_none()
                            .pl(px(13.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_color(color("--interactive-text-default"))
                            .hover(|this| this.text_color(color("--interactive-text-active")))
                            .child(gpui::svg().path("icons/reaction.svg").size(px(18.))),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
                    .when(many, |this| {
                        this.child(
                            div()
                                .id(SharedString::from(format!("poll-remove-{index}")))
                                .size(px(32.))
                                .flex_none()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(icon("ob_trash", 16., "--icon-strong"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_poll_answer(index, cx)
                                })),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        let duration_label = DURATIONS
            .iter()
            .find(|(hours, _)| *hours == form.duration)
            .map_or(dc_intl::tr("24 hours"), |(_, label)| dc_intl::tr(label));
        let poll_trigger_bounds = self.tip_anchors.borrow().get("poll-duration").copied();
        let list = form.duration_open.then(|| {
            gpui::deferred(
                div()
                    .id("poll-duration-list")
                    .occlude()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(44.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if poll_trigger_bounds.is_none_or(|b| !b.contains(&event.position))
                                && let Some(form) = this.poll.as_mut()
                            {
                                form.duration_open = false;
                                cx.notify();
                            }
                        },
                    ))
                    .children(DURATIONS.iter().map(|(hours, text)| {
                        let hours = *hours;
                        div()
                            .id(SharedString::from(format!("poll-duration-{hours}")))
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|this| this.bg(color("--background-mod-subtle")))
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::tr(text))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(form) = this.poll.as_mut() {
                                    form.duration = hours;
                                    form.duration_open = false;
                                }
                                cx.notify();
                            }))
                    })),
            )
            .with_priority(99)
        });
        let body = div()
            .flex()
            .flex_col()
            .when_some(form.error, |this, message| {
                this.child(
                    div()
                        .mb(px(8.))
                        .text_size(px(14.))
                        .text_color(color("--text-feedback-critical"))
                        .child(message),
                )
            })
            .child(label(dc_intl::tr("Question")))
            .child(div().mt(px(8.)).child(field(form.question.clone(), true)))
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .justify_end()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .child(format!("{count} / {QUESTION_MAX}")),
            )
            .child(div().mt(px(26.)).child(label(dc_intl::tr("Answers"))))
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(rows),
            )
            .when(form.answers.len() < ANSWERS_MAX, |this| {
                this.child(
                    div().mt(px(4.)).flex().justify_end().child(
                        div()
                            .id("poll-add-answer")
                            .h(px(40.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .text_size(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(icon("plus_small", 16., "--control-secondary-text-default"))
                            .child(dc_intl::tr("Add another answer"))
                            .on_click(cx.listener(|this, _, _, cx| this.add_poll_answer(cx))),
                    ),
                )
            })
            .child(div().mt(px(18.)).child(label(dc_intl::tr("Duration"))))
            .child(
                div()
                    .relative()
                    .mt(px(8.))
                    .child(
                        div()
                            .id("poll-duration")
                            .relative()
                            .child(self.anchor_probe("poll-duration"))
                            .h(px(40.))
                            .pl(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .text_color(color("--text-strong"))
                            .child(div().flex_1().child(duration_label))
                            // `.chevronButton_vWC4bk`: 36px wide with `padding-inline: 0 var(--space-4)`.
                            .child(
                                div()
                                    .w(px(36.))
                                    .pr(px(4.))
                                    .flex()
                                    .justify_center()
                                    .child(icon("select_chevron", 20., "--icon-strong")),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = this.poll.as_mut() {
                                    form.duration_open = !form.duration_open;
                                }
                                cx.notify();
                            })),
                    )
                    .children(list),
            );
        let multiple = form.multiple;
        let checkbox = div()
            .id("poll-multiple")
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .child(
                div()
                    .size(px(20.))
                    .rounded(px(4.))
                    .border_2()
                    .border_color(if multiple {
                        color("--control-primary-background-default")
                    } else {
                        color("--interactive-icon-default")
                    })
                    .when(multiple, |this| {
                        this.bg(color("--control-primary-background-default"))
                    }),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::tr("Allow Multiple Answers")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(form) = this.poll.as_mut() {
                    form.multiple = !form.multiple;
                }
                cx.notify();
            }))
            .into_any_element();
        let spec = modal::Spec {
            id: "create-poll",
            title: dc_intl::tr("Create a Poll").into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            // Module 50518 `ek`: the schedule button (`CalendarPlusIcon`, `rlf0tb`) is always there, `active` once a
            // time is set, ahead of Post.
            buttons: vec![
                modal::button(
                    "poll-schedule",
                    dc_intl::t("rlf0tb"),
                    if form.scheduled.is_some() {
                        Tone::Active
                    } else {
                        Tone::Secondary
                    },
                    |this, window, cx| {
                        let value = this.poll.as_ref().and_then(|form| form.scheduled);
                        this.open_poll_schedule_modal(value, window, cx);
                    },
                )
                .icon_only("cp_schedule", 20., "--control-secondary-text-default"),
                modal::button(
                    "poll-post",
                    dc_intl::tr("Post"),
                    Tone::Primary,
                    |this, _, cx| this.post_poll(cx),
                ),
            ],
            close: Box::new(|this, _, _| this.poll = None),
        };
        Some(modal::render_with_input(spec, checkbox, cx))
    }
}

impl Root {
    pub(super) fn close_poll_duration_select(&mut self) -> bool {
        if let Some(m) = self.poll.as_mut()
            && m.duration_open
        {
            m.duration_open = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn the_poll_request_is_the_create_poll_body() {
        let body = poll_body(" Lunch? ", &texts(&["Pizza", "  ", "Soup "]), true, 72).unwrap();
        assert_eq!(
            body,
            json!({"poll": {
                "question": {"text": "Lunch?"},
                "answers": [
                    {"poll_media": {"text": "Pizza"}},
                    {"poll_media": {"text": "Soup"}},
                ],
                "allow_multiselect": true,
                "duration": 72,
                "layout_type": 1,
            }})
        );
    }

    #[test]
    fn the_plus_menu_is_as_wide_as_its_widest_row_from_144_to_320() {
        use super::super::menu::flexible_width;
        // Frame composer-plus: "Schedule Message" (111.44 of text) and its caret make the 201.44 menu, 183.44 rows.
        assert!((plus_row_width(111.44, true) - 183.44).abs() < 1e-3);
        assert!((flexible_width(plus_row_width(111.44, true)) - 201.44).abs() < 1e-3);
        // A short menu keeps the 144 minimum; a long plugin label stops the menu at 320 and is cut.
        assert_eq!(flexible_width(plus_row_width(40., false)), 144.);
        assert_eq!(flexible_width(plus_row_width(400., false)), 320.);
        // VoiceMessages' "Send Audio File as Voice Message" (about 215 of text) widens it past 201.44, not past 320.
        let wide = flexible_width(plus_row_width(215., false));
        assert!(wide > 201.44 && wide < 320.);
    }

    #[test]
    fn a_poll_needs_a_question_and_an_answer() {
        assert!(poll_body("", &texts(&["a"]), false, 24).is_none());
        assert!(poll_body("q", &texts(&["", " "]), false, 24).is_none());
    }

    #[test]
    fn long_text_is_cut_to_its_limit() {
        assert_eq!(clipped("abc", 3), None);
        assert_eq!(clipped("abcd", 3).as_deref(), Some("abc"));
        assert!(DURATIONS.iter().any(|(hours, _)| *hours == 24));
    }
}
