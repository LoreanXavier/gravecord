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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plus {
    Upload,
    Thread,
    Poll,
    /// "Use Apps": the composer holds a slash, which lists the commands.
    Apps,
    /// "Schedule Message": opens its submenu of times.
    Schedule,
    /// "Record Voice Message": voice messages are not built, the row only closes the menu.
    RecordVoice,
}

pub(super) struct PollForm {
    pub question: Entity<Editor>,
    pub answers: Vec<Entity<Editor>>,
    pub duration: u32,
    pub multiple: bool,
    pub duration_open: bool,
    pub error: Option<&'static str>,
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
            Plus::Schedule | Plus::RecordVoice => {}
        }
        cx.notify();
    }

    fn answer_editor(
        &mut self,
        cx: &mut Context<Self>,
        events: &mut Vec<Subscription>,
    ) -> Entity<Editor> {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Type your answer".to_owned());
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
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("What question do you want to ask?".to_owned());
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
        let Some(body) = poll_body(&question, &answers, form.multiple, form.duration) else {
            form.error = Some(if question.trim().is_empty() {
                "Please enter a question."
            } else {
                "Please enter at least one answer."
            });
            cx.notify();
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/messages"),
            body: Some(body),
        });
        self.poll = None;
        cx.notify();
    }

    /// The menu above the "+" button: 201.44 wide, its bottom 61px above the window's, left edge on the composer's.
    pub(super) fn plus_menu_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.plus_menu {
            return None;
        }
        let row = |id: &'static str,
                   glyph: &'static str,
                   label: &'static str,
                   entry: Option<Plus>,
                   caret: bool,
                   cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(|this| this.bg(color("--background-mod-subtle")))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(icon(glyph, 20., "--interactive-icon-default"))
                .child(div().flex_1().child(label))
                .when(caret, |this| {
                    this.child(icon("pm_chevron", 20., "--interactive-icon-default"))
                })
                // The submenu follows the pointer: the Schedule Message row opens it, any other row closes it.
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let open = entry == Some(Plus::Schedule);
                    if *hovered && this.schedule.submenu != open {
                        this.schedule.submenu = open;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    match entry {
                        Some(entry) => this.run_plus(entry, window, cx),
                        None => this.plus_menu = false,
                    }
                    cx.notify();
                }))
        };
        let menu = div()
            .parity_with_border("plus-menu", 1.)
            .id("composer-plus-menu")
            .occlude()
            .absolute()
            .left(px(395.))
            .bottom(px(61.))
            .w(px(201.44))
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
                "Upload a File",
                Some(Plus::Upload),
                false,
                cx,
            ))
            .child(row(
                "plus-thread",
                "cp_thread",
                "Create Thread",
                Some(Plus::Thread),
                false,
                cx,
            ))
            .child(row(
                "plus-poll",
                "cp_poll",
                "Create Poll",
                Some(Plus::Poll),
                false,
                cx,
            ))
            .child(row(
                "plus-apps",
                "cp_apps",
                "Use Apps",
                Some(Plus::Apps),
                false,
                cx,
            ))
            .children(self.schedule_offered().then(|| {
                row(
                    "plus-schedule",
                    "cp_schedule",
                    "Schedule Message",
                    Some(Plus::Schedule),
                    true,
                    cx,
                )
                .child(self.anchor_probe("plus-schedule-row"))
            }))
            .child(row(
                "plus-record",
                "cp_mic",
                "Record Voice Message",
                Some(Plus::RecordVoice),
                false,
                cx,
            ));
        let layer = div()
            .id("composer-plus-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.plus_menu = false;
                    cx.notify();
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
        let field = |editor: Entity<Editor>, focused: bool| {
            div()
                .min_h(px(47.))
                .px(px(12.))
                .py(px(13.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(if focused {
                    color("--border-focus")
                } else {
                    color("--border-muted")
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
                    .gap(px(8.))
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--input-background-default"))
                    .child(icon("pm_emoji", 24., "--interactive-icon-default"))
                    .child(div().flex_1().min_w(px(0.)).child(editor.clone()))
                    .when(many, |this| {
                        this.child(
                            div()
                                .id(SharedString::from(format!("poll-remove-{index}")))
                                .size(px(32.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(icon("ob_trash", 20., "--interactive-icon-default"))
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
            .map_or("24 hours", |(_, label)| *label);
        let list = form.duration_open.then(|| {
            div()
                .id("poll-duration-list")
                .occlude()
                .mt(px(4.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .flex()
                .flex_col()
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
                        .child(*text)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(form) = this.poll.as_mut() {
                                form.duration = hours;
                                form.duration_open = false;
                            }
                            cx.notify();
                        }))
                }))
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
            .child(label("Question"))
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
            .child(div().mt(px(26.)).child(label("Answers")))
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
                            .child("Add another answer")
                            .on_click(cx.listener(|this, _, _, cx| this.add_poll_answer(cx))),
                    ),
                )
            })
            .child(div().mt(px(18.)).child(label("Duration")))
            .child(
                div()
                    .id("poll-duration")
                    .mt(px(8.))
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(16.))
                    .text_color(color("--text-default"))
                    .child(div().flex_1().child(duration_label))
                    .child(icon("chevron_down", 16., "--interactive-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.poll.as_mut() {
                            form.duration_open = !form.duration_open;
                        }
                        cx.notify();
                    })),
            )
            .children(list);
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
                    .child("Allow Multiple Answers"),
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
            title: "Create a Poll".into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            buttons: vec![modal::button(
                "poll-post",
                "Post",
                Tone::Primary,
                |this, _, cx| this.post_poll(cx),
            )],
            close: Box::new(|this, _, _| this.poll = None),
        };
        Some(modal::render_with_input(spec, checkbox, cx))
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
