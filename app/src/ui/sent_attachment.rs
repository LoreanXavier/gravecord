//! The pencil and the trash of a sent attachment's hover group (module 96782 `onEditItem` / `onRemoveItem`, handled in
//! module 679636): "Modify Attachment" (`ModifyMosaicAttachmentModal`, module 507536) and the "Are you sure?" confirm
//! that removes it. Both end in `PATCH /channels/{channel}/messages/{message}` with the message's `attachments`
//! (`patchMessageAttachments`); the gateway's MESSAGE_UPDATE brings the result back.

use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{Attachment, Id, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use serde_json::{Value, json};

/// `Lt(flags, IS_SPOILER)`: bit 3 of the attachment flags.
const IS_SPOILER: u64 = 1 << 3;
/// `h.MO`: a file whose name starts with this is a spoiler already, and the checkbox is checked and disabled.
const SPOILER_PREFIX: &str = "SPOILER_";

pub(super) enum SentEdit {
    Modify {
        message: Message,
        attachment: Attachment,
        spoiler: bool,
    },
    Remove {
        message: Message,
        attachment: Attachment,
    },
}

/// The `attachments` list of the PATCH: every attachment of the message by id, the edited one with its
/// description and spoiler mark (`r=s.attachments.map(e=>({id:e.id}))`, then `a.description=n,a.is_spoiler=i`).
pub(super) fn modify_body(
    message: &Message,
    edited: Id,
    description: &str,
    spoiler: bool,
) -> Value {
    let list: Vec<Value> = message
        .attachments
        .iter()
        .map(|attachment| {
            if attachment.id == edited {
                json!({"id": attachment.id.to_string(), "description": description, "is_spoiler": spoiler})
            } else {
                json!({"id": attachment.id.to_string()})
            }
        })
        .collect();
    json!({ "attachments": list })
}

/// `t.attachments.filter(e=>e.id!==n.id)`: the message's other attachments.
pub(super) fn remove_body(message: &Message, removed: Id) -> Value {
    let list: Vec<Value> = message
        .attachments
        .iter()
        .filter(|attachment| attachment.id != removed)
        .map(|attachment| json!({"id": attachment.id.to_string()}))
        .collect();
    json!({ "attachments": list })
}

impl Root {
    /// The pencil: opens "Modify Attachment" for this attachment.
    pub(super) fn open_modify_sent(
        &mut self,
        message: &Message,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) {
        self.sent_edit = Some(SentEdit::Modify {
            message: message.clone(),
            spoiler: attachment.flags & IS_SPOILER != 0,
            attachment: attachment.clone(),
        });
        cx.notify();
    }

    /// The trash: opens the "Are you sure?" confirm.
    pub(super) fn open_remove_sent(
        &mut self,
        message: &Message,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) {
        self.sent_edit = Some(SentEdit::Remove {
            message: message.clone(),
            attachment: attachment.clone(),
        });
        cx.notify();
    }

    fn patch_attachments(&mut self, message: &Message, body: Value) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/channels/{}/messages/{}", message.channel_id, message.id),
            body: Some(body),
        });
    }

    pub(super) fn sent_edit_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        match self.sent_edit.as_ref()? {
            SentEdit::Remove {
                message,
                attachment,
            } => {
                let body = remove_body(message, attachment.id);
                let message = message.clone();
                let spec = modal::Spec {
                    id: "remove-attachment",
                    title: "Are you sure?".into(),
                    subtitle: None,
                    body: Some(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(
                                "This will remove this attachment from this message permanently.",
                            )
                            .into_any_element(),
                    ),
                    buttons: vec![
                        modal::button(
                            "remove-attachment-cancel",
                            "Cancel",
                            Tone::Secondary,
                            |this, _, _| this.sent_edit = None,
                        ),
                        modal::button(
                            "remove-attachment-confirm",
                            "Remove Attachment",
                            Tone::Primary,
                            move |this, _, _| {
                                this.sent_edit = None;
                                this.patch_attachments(&message, body.clone());
                            },
                        ),
                    ],
                    close: Box::new(|this, _, _| this.sent_edit = None),
                };
                Some(modal::render_small(spec, cx))
            }
            SentEdit::Modify {
                message,
                attachment,
                spoiler,
            } => {
                let forced = attachment.filename.starts_with(SPOILER_PREFIX);
                let checked = *spoiler || forced;
                let label = div()
                    .h(px(20.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Filename");
                // `disableFilename`: the field shows the name and cannot be typed in (disabled look assumed).
                let field = div()
                    .mt(px(8.))
                    .h(px(40.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--input-background-default"))
                    .opacity(0.5)
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(attachment.filename.clone());
                let check = div()
                    .id("modify-sent-spoiler")
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(!forced, |this| this.cursor_pointer())
                    .child(
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .border_2()
                            .border_color(if checked {
                                color("--control-primary-background-default")
                            } else {
                                color("--interactive-icon-default")
                            })
                            .when(checked, |this| {
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if forced {
                            return;
                        }
                        if let Some(SentEdit::Modify { spoiler, .. }) = this.sent_edit.as_mut() {
                            *spoiler = !*spoiler;
                        }
                        cx.notify();
                    }));
                let body = div()
                    .flex()
                    .flex_col()
                    .child(label)
                    .child(field)
                    .child(check);
                // Save sends the description the attachment had (the field only exists for images and videos) and
                // the spoiler mark.
                let description = attachment.description.clone().unwrap_or_default();
                let (message, edited) = (message.clone(), attachment.id);
                let spec = modal::Spec {
                    id: "modify-sent-attachment",
                    title: "Modify Attachment".into(),
                    subtitle: None,
                    body: Some(body.into_any_element()),
                    buttons: vec![
                        modal::button(
                            "modify-sent-cancel",
                            "Cancel",
                            Tone::Secondary,
                            |this, _, _| this.sent_edit = None,
                        ),
                        modal::button(
                            "modify-sent-save",
                            "Save",
                            Tone::Primary,
                            move |this, _, _| {
                                let spoiler = match this.sent_edit.as_ref() {
                                    Some(SentEdit::Modify { spoiler, .. }) => *spoiler || forced,
                                    _ => return,
                                };
                                this.sent_edit = None;
                                let body = modify_body(&message, edited, &description, spoiler);
                                this.patch_attachments(&message, body);
                            },
                        ),
                    ],
                    close: Box::new(|this, _, _| this.sent_edit = None),
                };
                Some(modal::render(spec, cx))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{modify_body, remove_body};
    use serde_json::json;

    fn message() -> dc_state::Message {
        let message = dc_state::Message::from_value(
            &json!({"id": "50", "channel_id": "9", "content": "hi", "timestamp": "2026-11-11T10:00:00+00:00",
                "author": {"id": "3", "username": "me"},
                "attachments": [
                    {"id": "100", "filename": "a.mp3", "url": "u", "proxy_url": "p", "size": 1},
                    {"id": "101", "filename": "b.mp3", "url": "u", "proxy_url": "p", "size": 1}
                ]
            }),
        );
        message.expect("a message")
    }

    #[test]
    fn modify_marks_only_the_edited_attachment() {
        let body = modify_body(&message(), 100, "", true);
        assert_eq!(
            body,
            json!({"attachments": [
                {"id": "100", "description": "", "is_spoiler": true},
                {"id": "101"}
            ]})
        );
    }

    #[test]
    fn remove_keeps_the_others_by_id() {
        let body = remove_body(&message(), 100);
        assert_eq!(body, json!({"attachments": [{"id": "101"}]}));
    }
}
