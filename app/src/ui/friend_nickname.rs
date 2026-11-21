//! Relationship nickname editor and private-channel names (609422/47167).
use super::{Root, color, editor_style, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_state::{Id, PrivateChannel, User};
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Subscription,
    Window, div, prelude::*, px,
};
use serde_json::{Value, json};

const KEY: &str = "friend-nickname:";
const LIMIT: usize = 32;
#[cfg(feature = "capture")]
const FIXTURE_DM: Id = 1568693333864480939;

pub(super) struct Dialog {
    pub input: Entity<Editor>,
    _events: Subscription,
    user: Id,
    editing: bool,
    reset: bool,
    pending: bool,
    key: String,
    save_focus: FocusHandle,
    #[cfg(feature = "capture")]
    suppressed_saves: usize,
}

fn patch(user: Id, key: String, nickname: Option<&str>, writes_allowed: bool) -> Option<Request> {
    if !writes_allowed || nickname.is_some_and(|text| text.encode_utf16().count() > LIMIT) {
        return None;
    }
    Some(Request::Rest {
        key: Some(key),
        method: reqwest::Method::PATCH,
        route: format!("/users/@me/relationships/{user}"),
        body: Some(json!({"nickname": nickname})),
    })
}

impl Root {
    pub(super) fn nickname_for(&self, user: Id) -> Option<&str> {
        #[cfg(feature = "capture")]
        if self.capture_friend_nickname
            && self
                .stores
                .private_channels
                .iter()
                .find(|channel| channel.id == FIXTURE_DM)
                .is_some_and(|channel| channel.recipients.first() == Some(&user))
        {
            return Some("fixture nickname");
        }
        self.stores.friend_nickname(user)
    }

    pub(super) fn dm_display_name(&self, user: &User) -> String {
        self.nickname_for(user.id)
            .map(str::to_owned)
            .unwrap_or_else(|| self.concealed_display_name(user).0)
    }

    pub(super) fn private_channel_name(&self, channel: &PrivateChannel) -> String {
        if let Some(name) = channel.name.as_ref().filter(|name| !name.is_empty()) {
            return name.clone();
        }
        let names: Vec<_> = channel
            .recipients
            .iter()
            .filter_map(|id| self.stores.users.get(id))
            .map(|user| self.dm_display_name(user))
            .collect();
        if channel.kind == 1 {
            return names.into_iter().next().unwrap_or_else(|| "???".into());
        }
        if names.is_empty() {
            format!(
                "{}'s Group",
                self.stores
                    .current_user
                    .as_ref()
                    .map_or("???", User::display_name)
            )
        } else {
            names.join(", ")
        }
    }

    pub(super) fn open_friend_nickname(
        &mut self,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stores.relationships.get(&user) != Some(&1) {
            return;
        }
        let Some(known) = self.stores.users.get(&user) else {
            return;
        };
        let placeholder = known.display_name().to_owned();
        let initial = self.nickname_for(user).map(str::to_owned);
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            let mut input = Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_reports_all_edits(true);
            input.set_max_utf16(LIMIT);
            input.set_single_line(true);
            input.set_placeholder(placeholder);
            input.set_text(initial.as_deref().unwrap_or_default());
            input
        });
        let events = cx.subscribe(&input, |this, editor, event: &EditorEvent, cx| {
            if !this
                .friend_nickname
                .as_ref()
                .is_some_and(|dialog| dialog.input.entity_id() == editor.entity_id())
            {
                return;
            }
            match event {
                EditorEvent::Edited => {
                    if let Some(dialog) = this.friend_nickname.as_mut() {
                        dialog.reset = false;
                    }
                }
                EditorEvent::Submit(_) => this.save_friend_nickname(cx),
                EditorEvent::Cancel => this.friend_nickname = None,
            }
            cx.notify();
        });
        let save_focus = cx.focus_handle();
        let key = format!("{KEY}{}", input.entity_id());
        self.friend_nickname = Some(Dialog {
            input,
            _events: events,
            user,
            editing: initial.is_some(),
            reset: initial.is_none(),
            pending: false,
            key,
            save_focus: save_focus.clone(),
            #[cfg(feature = "capture")]
            suppressed_saves: 0,
        });
        // 696208 gives the last primary action autofocus after the field mounts.
        window.focus(&save_focus, cx);
        cx.notify();
    }

    fn save_friend_nickname(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .friend_nickname
            .as_ref()
            .filter(|dialog| !dialog.pending)
        else {
            return;
        };
        let text = dialog.input.read(cx).text();
        let nickname = (!(dialog.reset && text.is_empty())).then_some(text);
        let writes_allowed = std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() != Ok("1");
        let Some(request) = patch(dialog.user, dialog.key.clone(), nickname, writes_allowed) else {
            if !writes_allowed {
                tracing::info!("friend nickname write suppressed by settings-write guard");
                #[cfg(feature = "capture")]
                if let Some(dialog) = self.friend_nickname.as_mut() {
                    dialog.suppressed_saves += 1;
                }
            }
            return;
        };
        if self.requests.send(request).is_ok()
            && let Some(dialog) = self.friend_nickname.as_mut()
        {
            dialog.pending = true;
        }
        cx.notify();
    }

    pub(super) fn friend_nickname_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(KEY) {
            return false;
        }
        if let Some(dialog) = self
            .friend_nickname
            .as_mut()
            .filter(|dialog| dialog.key == key)
        {
            if body["ok"] == true {
                self.friend_nickname = None;
            } else {
                dialog.pending = false;
            }
            cx.notify();
        }
        true
    }

    pub(super) fn friend_nickname_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.friend_nickname.as_ref()?;
        let focused = dialog.input.read(cx).focus_handle(cx).is_focused(window);
        let input = dialog.input.clone();
        let field = div()
            .parity_with_border("friend-nickname-input", if focused { 2. } else { 1. })
            .id("friend-nickname-input")
            .h(px(40.))
            .w_full()
            .rounded(px(8.))
            .bg(color("--input-background-default"))
            .when(focused, |d| d.border_2())
            .when(!focused, |d| d.border_1())
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
            .px(px(if focused { 11. } else { 12. }))
            .flex()
            .items_center()
            .on_click(cx.listener(move |_, _, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx);
                cx.notify();
            }))
            .child(div().flex_1().min_w(px(0.)).child(dialog.input.clone()));
        let reset = div()
            .parity("friend-nickname-reset")
            .id("friend-nickname-reset")
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-brand"))
            .cursor_pointer()
            .child("Reset Friend Nickname")
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(dialog) = this.friend_nickname.as_mut() {
                    dialog.input.update(cx, |input, _| input.set_text(""));
                    dialog.reset = true;
                    window.focus(&dialog.input.read(cx).focus_handle(cx), cx);
                }
                cx.notify();
            }));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .parity("friend-nickname-label")
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Friend Nickname"),
            )
            .child(field)
            .child(div().flex().child(reset));
        let mut save = modal::button(
            "friend-nickname-save",
            "Save",
            if dialog.pending {
                modal::Tone::PrimaryDisabled
            } else {
                modal::Tone::Primary
            },
            |this, _, cx| this.save_friend_nickname(cx),
        );
        save.focus = Some(dialog.save_focus.clone());
        Some(modal::render(modal::Spec {
            id: "friend-nickname-modal",
            title: if dialog.editing {"Change Friend Nickname"} else {"Add Friend Nickname"}.into(),
            subtitle: Some(div().child("Find a friend faster with a personal nickname. It will only be visible to you in your direct messages.").into_any_element()),
            body: Some(body.into_any_element()),
            buttons: vec![modal::button("friend-nickname-cancel", "Cancel", modal::Tone::Secondary,
                |this, _, _| this.friend_nickname = None), save],
            close: Box::new(|this, _, _| this.friend_nickname = None),
        }, cx))
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_nickname_preview(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        if (self.channel != Some(FIXTURE_DM) && !self.capture_friend_nickname)
            || std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() != Ok("1")
        {
            return "nickname preview requires fixture DM and settings-write guard".into();
        }
        if mode == "state" {
            return self.friend_nickname.as_ref().map_or_else(
                || "nickname modal closed".into(),
                |dialog| {
                    format!(
                        "nickname modal open; pending={}; suppressed_saves={}; input_utf16={}",
                        dialog.pending,
                        dialog.suppressed_saves,
                        dialog.input.read(cx).text().encode_utf16().count()
                    )
                },
            );
        }
        match mode {
            "preview" => self.capture_friend_nickname = true,
            "clear" => self.capture_friend_nickname = false,
            _ => return "usage: nickname preview | clear | state".into(),
        }
        cx.notify();
        format!(
            "fixture nickname preview: {}; view only",
            self.capture_friend_nickname
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nickname_patch_keeps_null_distinct_from_empty_and_honors_write_guard() {
        for nickname in [None, Some(""), Some(" private name ")] {
            let Some(Request::Rest {
                key,
                method,
                route,
                body,
            }) = patch(41, "response-key".into(), nickname, true)
            else {
                panic!("missing PATCH")
            };
            assert_eq!(key.as_deref(), Some("response-key"));
            assert_eq!(method, reqwest::Method::PATCH);
            assert_eq!(route, "/users/@me/relationships/41");
            assert_eq!(body, Some(json!({"nickname":nickname})));
            assert!(patch(41, "response-key".into(), nickname, false).is_none());
        }
        assert!(patch(41, "k".into(), Some(&"😀".repeat(16)), true).is_some());
        assert!(patch(41, "k".into(), Some(&"😀".repeat(17)), true).is_none());
    }
}
