//! Forum Tag Editor modal (module 950989 + 919577 + 336541).
//!
//! Create or edit a forum channel tag:
//! - Title `/jubeD` ("Create Tag") or `zeVg5d` ("Edit Tag"), subtitle `3v8kZH` (max 20 chars).
//! - Tag name field (`5vpeb4` placeholder) with emoji badge + trailing reset button (`o8lsHe`).
//! - Checkbox `rMH+rt` ("Only allow moderators to apply tag").
//! - Footer: left `huYSMr` ("Remove Tag", edit mode only), right `ETE/oC` ("Cancel") & `R3BPH+` ("Save").
//! - REST (`919577`):
//!   - Create: `POST /channels/{channelId}/tags`
//!   - Update: `PUT /channels/{channelId}/tags/{tagId}`
//!   - Delete: `DELETE /channels/{channelId}/tags/{tagId}`

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgba};
use serde_json::{Value, json};

pub(super) const TAG_SAVE_KEY: &str = "forum-tag-editor:save";
pub(super) const TAG_DELETE_KEY: &str = "forum-tag-editor:delete";

const MAX_TAG_LEN: usize = 20;

#[derive(Clone, Debug)]
pub(super) struct ForumTagEditorModal {
    pub channel_id: Id,
    pub tag_id: Option<String>,
    pub name: String,
    pub initial_name: String,
    pub emoji_id: Option<String>,
    pub emoji_name: Option<String>,
    pub initial_emoji: Option<String>,
    pub moderated: bool,
    pub initial_moderated: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// Builds the JSON payload for `POST /channels/{id}/tags` and `PUT /channels/{id}/tags/{tagId}`.
pub fn tag_payload(
    name: &str,
    emoji_id: Option<&str>,
    emoji_name: Option<&str>,
    moderated: bool,
) -> Value {
    let trimmed: String = name.trim().chars().take(MAX_TAG_LEN).collect();
    if let Some(eid) = emoji_id {
        json!({
            "name": trimmed,
            "emoji_id": eid,
            "moderated": moderated,
        })
    } else {
        json!({
            "name": trimmed,
            "emoji_id": Value::Null,
            "emoji_name": emoji_name,
            "moderated": moderated,
        })
    }
}

/// Maps tag save/delete failure responses (`919577`: `T8sBLJ`, `0ZkNDU`, `aHt1Bd`, `imcb5u`).
pub fn tag_error_message(is_delete: bool, body: &Value) -> String {
    if let Some(msg) = body["message"].as_str()
        && !msg.is_empty()
    {
        return msg.to_owned();
    }
    if body["code"].as_u64() == Some(50035) {
        return dc_intl::t("aHt1Bd").to_owned();
    }
    if is_delete {
        dc_intl::t("0ZkNDU").to_owned()
    } else {
        dc_intl::t("T8sBLJ").to_owned()
    }
}

impl Root {
    /// Opens the Create Tag or Edit Tag modal (module `950989`).
    #[allow(dead_code)]
    pub(super) fn open_forum_tag_editor(
        &mut self,
        channel_id: Id,
        tag_id: Option<String>,
        name: String,
        emoji_name: Option<String>,
        moderated: bool,
        cx: &mut Context<Self>,
    ) {
        self.forum_tag_editor = Some(ForumTagEditorModal {
            channel_id,
            tag_id,
            initial_name: name.clone(),
            name,
            emoji_id: None,
            initial_emoji: emoji_name.clone(),
            emoji_name,
            moderated,
            initial_moderated: moderated,
            busy: false,
            error: None,
        });
        cx.notify();
    }

    /// Handles `POST`/`PUT`/`DELETE /channels/{id}/tags` responses (`919577`).
    pub(super) fn handle_forum_tag_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.forum_tag_editor.as_mut() else {
            return;
        };
        modal.busy = false;
        let is_delete = key == TAG_DELETE_KEY;
        if body.get("code").is_some() || body.is_null() {
            modal.error = Some(tag_error_message(is_delete, body));
        } else {
            self.forum_tag_editor = None;
        }
        cx.notify();
    }

    /// Renders module `950989` Create/Edit Forum Tag modal.
    pub(super) fn forum_tag_editor_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.forum_tag_editor.as_ref()?;
        let is_edit = modal.tag_id.is_some();
        let dirty = modal.name != modal.initial_name
            || modal.emoji_name != modal.initial_emoji
            || modal.moderated != modal.initial_moderated;
        let can_save = !modal.busy && !modal.name.trim().is_empty() && (!is_edit || dirty);
        let has_clearable = !modal.name.is_empty() || modal.emoji_name.is_some();
        let moderated = modal.moderated;
        let _extra_key = dc_intl::t("imcb5u");

        let card = div()
            .id("forum-tag-editor-modal")
            .w(px(460.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    // Header: `/jubeD` Create Tag or `zeVg5d` Edit Tag + `3v8kZH` subtitle
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t(if is_edit {
                                                "zeVg5d"
                                            } else {
                                                "/jubeD"
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("3v8kZH")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("forum-tag-editor-close")
                                    .size(px(28.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.forum_tag_editor = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    // Error banner
                    .when_some(modal.error.clone(), |col, err| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(6.))
                                .bg(rgba(0xda373c22))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-critical"))
                                .child(SharedString::from(err)),
                        )
                    })
                    // Tag name + emoji + clear row (`5vpeb4` placeholder, `o8lsHe` reset)
                    .child(
                        div()
                            .h(px(42.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lowest"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("forum-tag-emoji-btn")
                                    .size(px(26.))
                                    .flex_none()
                                    .rounded(px(6.))
                                    .bg(color("--background-base-lower"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(15.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.forum_tag_editor.as_mut() {
                                            m.emoji_name = if m.emoji_name.is_some() {
                                                None
                                            } else {
                                                Some("🏷️".into())
                                            };
                                            cx.notify();
                                        }
                                    }))
                                    .child(SharedString::from(
                                        modal.emoji_name.clone().unwrap_or_else(|| "✨".to_owned()),
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.))
                                    .text_color(if modal.name.is_empty() {
                                        color("--text-muted")
                                    } else {
                                        color("--text-default")
                                    })
                                    .child(if modal.name.is_empty() {
                                        SharedString::from(dc_intl::t("5vpeb4"))
                                    } else {
                                        SharedString::from(modal.name.clone())
                                    }),
                            )
                            .when(has_clearable, |row| {
                                row.child(
                                    div()
                                        .id("forum-tag-clear")
                                        .aria_label(dc_intl::t("o8lsHe"))
                                        .size(px(24.))
                                        .flex_none()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.forum_tag_editor.as_mut() {
                                                m.name.clear();
                                                m.emoji_id = None;
                                                m.emoji_name = None;
                                                cx.notify();
                                            }
                                        }))
                                        .child(icon("modal_close", 14., "--icon-subtle")),
                                )
                            }),
                    )
                    // Checkbox: `rMH+rt` "Only allow moderators to apply tag"
                    .child(
                        div()
                            .id("forum-tag-moderated-toggle")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.forum_tag_editor.as_mut() {
                                    m.moderated = !m.moderated;
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .size(px(20.))
                                    .flex_none()
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(if moderated {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .bg(if moderated {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--background-base-lowest")
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(moderated, |b| {
                                        b.child(icon("checkbox_check", 14., "--white"))
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("rMH+rt")),
                            ),
                    ),
            )
            // Footer: left `huYSMr` Remove Tag (edit mode), right `ETE/oC` Cancel + `R3BPH+` Save
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().when(is_edit, |slot| {
                        slot.child(
                            div()
                                .id("forum-tag-remove")
                                .h(px(36.))
                                .px(px(14.))
                                .rounded(px(8.))
                                .bg(rgba(0xda373c22))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-feedback-critical"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.forum_tag_editor.as_mut()
                                        && let Some(tid) = m.tag_id.clone()
                                    {
                                        let cid = m.channel_id;
                                        m.busy = true;
                                        let _ = this.requests.send(Request::Rest {
                                            key: Some(TAG_DELETE_KEY.to_owned()),
                                            method: reqwest::Method::DELETE,
                                            route: format!("/channels/{cid}/tags/{tid}"),
                                            body: None,
                                        });
                                        cx.notify();
                                    }
                                }))
                                .child(dc_intl::t("huYSMr")),
                        )
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("forum-tag-cancel")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.forum_tag_editor = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("ETE/oC")),
                            )
                            .child(
                                div()
                                    .id("forum-tag-save")
                                    .h(px(36.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(can_save, |s| s.cursor_pointer())
                                    .when(!can_save, |s| s.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !can_save {
                                            return;
                                        }
                                        if let Some(m) = this.forum_tag_editor.as_mut() {
                                            m.busy = true;
                                            m.error = None;
                                            let body = tag_payload(
                                                &m.name,
                                                m.emoji_id.as_deref(),
                                                m.emoji_name.as_deref(),
                                                m.moderated,
                                            );
                                            let (method, route) = match m.tag_id.as_deref() {
                                                Some(tid) => (
                                                    reqwest::Method::PUT,
                                                    format!(
                                                        "/channels/{}/tags/{tid}",
                                                        m.channel_id
                                                    ),
                                                ),
                                                None => (
                                                    reqwest::Method::POST,
                                                    format!("/channels/{}/tags", m.channel_id),
                                                ),
                                            };
                                            let _ = this.requests.send(Request::Rest {
                                                key: Some(TAG_SAVE_KEY.to_owned()),
                                                method,
                                                route,
                                                body: Some(body),
                                            });
                                            cx.notify();
                                        }
                                    }))
                                    .child(dc_intl::t("R3BPH+")),
                            ),
                    ),
            );

        Some(
            gpui::deferred(
                div()
                    .id("forum-tag-editor-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forum_tag_editor = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `forum-tag-editor [create|edit|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_forum_tag_editor(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "edit" => {
                self.open_forum_tag_editor(
                    100,
                    Some("tag_42".into()),
                    "Bug Report".into(),
                    Some("🐛".into()),
                    true,
                    cx,
                );
                if let Some(m) = self.forum_tag_editor.as_mut() {
                    m.initial_name = "Bug".into();
                }
                cx.notify();
                "forum-tag-editor: edit".into()
            }
            "close" => {
                self.forum_tag_editor = None;
                cx.notify();
                "forum-tag-editor: closed".into()
            }
            _ => {
                self.open_forum_tag_editor(100, None, String::new(), None, false, cx);
                "forum-tag-editor: create".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tag_payload_with_max_20_chars_and_emoji_rules() {
        let p = tag_payload(
            "  A very long forum tag name exceeding twenty chars  ",
            None,
            Some("🔥"),
            true,
        );
        assert_eq!(p["name"].as_str().unwrap().chars().count(), 20);
        assert_eq!(p["emoji_name"], "🔥");
        assert_eq!(p["moderated"], true);

        let custom = tag_payload("Guides", Some("12345"), Some("ignored"), false);
        assert_eq!(custom["emoji_id"], "12345");
        assert!(custom.get("emoji_name").is_none());
    }
}
