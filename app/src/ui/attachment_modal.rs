//! Modify Attachment, from the upload card's pencil: the filename, the alt text and the spoiler mark. Rows and sizes
//! are from `design/dom/modals/modify-attachment-darker-cozy.json`; the mark is the `SPOILER_` name prefix.

use super::modal::{self, Tone};
use super::uploads::{SPOILER_PREFIX, toggle_spoiler};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, Subscription, Window, div,
    hsla, img, prelude::*, px, rgba,
};

pub(super) struct ModifyAttachment {
    index: usize,
    name: Entity<Editor>,
    description: Entity<Editor>,
    spoiler: bool,
    _events: [Subscription; 2],
}

/// The name Save puts on the file: an empty field keeps the old name, and the spoiler mark is the name's prefix.
pub(super) fn saved_name(typed: &str, previous: &str, spoiler: bool) -> String {
    let typed = typed.trim();
    let plain = if typed.is_empty() {
        previous.strip_prefix(SPOILER_PREFIX).unwrap_or(previous)
    } else {
        typed.strip_prefix(SPOILER_PREFIX).unwrap_or(typed)
    };
    if spoiler {
        toggle_spoiler(plain)
    } else {
        plain.to_owned()
    }
}

impl Root {
    pub(super) fn open_modify_attachment(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_files.get(index) else {
            return;
        };
        let spoiler = pending.is_spoiler();
        let plain = pending
            .name
            .strip_prefix(SPOILER_PREFIX)
            .unwrap_or(&pending.name)
            .to_owned();
        let described = pending.description.clone();
        let name = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_text(&plain);
            editor
        });
        let description = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Add a description".to_owned());
            if let Some(text) = &described {
                editor.set_text(text);
            }
            editor
        });
        let on_event = |this: &mut Root,
                        editor: Entity<Editor>,
                        event: &EditorEvent,
                        cx: &mut Context<Root>| {
            match event {
                // Enter clears the field before it reports, so the text goes back before it is read.
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.save_modify_attachment(cx)
                }
                EditorEvent::Cancel => this.modify_attachment = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        };
        let events = [
            cx.subscribe(&name, move |this, editor, event: &EditorEvent, cx| {
                on_event(this, editor, event, cx)
            }),
            cx.subscribe(
                &description,
                move |this, editor, event: &EditorEvent, cx| on_event(this, editor, event, cx),
            ),
        ];
        window.focus(&name.read(cx).focus_handle(cx), cx);
        self.modify_attachment = Some(ModifyAttachment {
            index,
            name,
            description,
            spoiler,
            _events: events,
        });
        cx.notify();
    }

    fn save_modify_attachment(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.modify_attachment.take() else {
            return;
        };
        let typed = open.name.read(cx).text().to_owned();
        let described = open.description.read(cx).text().trim().to_owned();
        if let Some(pending) = self.pending_files.get_mut(open.index) {
            pending.name = saved_name(&typed, &pending.name, open.spoiler);
            pending.description = (!described.is_empty()).then_some(described);
        }
        cx.notify();
    }

    #[cfg(feature = "capture")]
    pub(super) fn modify_attachment_editors(&self) -> Vec<Entity<Editor>> {
        self.modify_attachment
            .iter()
            .flat_map(|open| [open.name.clone(), open.description.clone()])
            .collect()
    }

    pub(super) fn modify_attachment_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.modify_attachment.as_ref()?;
        let pending = self.pending_files.get(open.index)?;
        let label = |text: &'static str| {
            div()
                .h(px(20.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        // `wrapper_UcGqzv`: 12% black, a 1px border of the muted grey at 20%, 8px radius; the one-line input pads 12 by
        // 9, the text area 10 by 12 (`input_0PLWhS`, `textArea_uI0xjn` in edit-modal).
        let field = |editor: Entity<Editor>, height: f32, pad: (f32, f32)| {
            div()
                .h(px(height))
                .px(px(pad.0))
                .py(px(pad.1))
                .rounded(px(8.))
                .border_1()
                .border_color(rgba(0x97979f33))
                .bg(hsla(0., 0., 0., 0.12))
                .child(editor)
        };
        let preview = pending.preview_image().map(|(image, width, height)| {
            div()
                .ml(px(16.))
                .w(px(width))
                .h(px(height))
                .child(img(image).w(px(width)).h(px(height)))
        });
        let spoiler = open.spoiler;
        let check = div()
            .id("modify-spoiler")
            .mt(px(24.))
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .child(
                div()
                    .size(px(20.))
                    .rounded(px(4.))
                    .border_1()
                    // `checkboxIndicator_vTUVyc`: 8% black inside a 1px border of the muted grey at 64%.
                    .border_color(if spoiler {
                        color("--control-primary-background-default")
                    } else {
                        rgba(0x97979fa3)
                    })
                    .bg(hsla(0., 0., 0., 0.08))
                    .when(spoiler, |this| {
                        this.bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("checkbox_check", 16., "--white"))
                    }),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child("Mark as spoiler"),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(open) = this.modify_attachment.as_mut() {
                    open.spoiler = !open.spoiler;
                }
                cx.notify();
            }));
        let body = div()
            .flex()
            .flex_col()
            .children(preview)
            .child(div().mt(px(16.)).child(label("Filename")))
            .child(
                div()
                    .mt(px(8.))
                    .child(field(open.name.clone(), 40., (12., 9.))),
            )
            .child(div().mt(px(16.)).child(label("Description (Alt Text)")))
            .child(
                div()
                    .mt(px(8.))
                    .child(field(open.description.clone(), 89., (10., 12.))),
            )
            .child(check);
        let spec = modal::Spec {
            id: "modify-attachment",
            title: "Modify Attachment".into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            buttons: vec![
                modal::button("modify-cancel", "Cancel", Tone::Secondary, |this, _, _| {
                    this.modify_attachment = None
                }),
                modal::button("modify-save", "Save", Tone::Primary, |this, _, cx| {
                    this.save_modify_attachment(cx)
                }),
            ],
            close: Box::new(|this, _, _| this.modify_attachment = None),
        };
        Some(modal::render(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::saved_name;

    #[test]
    fn save_applies_the_name_and_the_spoiler_mark() {
        assert_eq!(saved_name("cat.png", "dog.png", false), "cat.png");
        assert_eq!(saved_name("cat.png", "dog.png", true), "SPOILER_cat.png");
        assert_eq!(saved_name("  ", "SPOILER_dog.png", false), "dog.png");
        assert_eq!(saved_name("", "dog.png", true), "SPOILER_dog.png");
        assert_eq!(
            saved_name("SPOILER_cat.png", "dog.png", true),
            "SPOILER_cat.png"
        );
    }
}
