//! Server Settings > Emoji (module 989433 `ob`, rows `of`, sections `oN`/`op`/`oA`; styles module 378243).
//!
//! `GET /guilds/{id}/emojis` lists them, split into Emoji and Animated Emoji, newest first. Upload Emoji takes image
//! files and posts each as module 810877 `W` does for a dropped file: the name is the file's stem through
//! `sanitizeEmojiName` (module 690521), the image a data URI (`POST /guilds/{id}/emojis`). A name edited in its row is
//! committed with Enter (`PATCH /guilds/{id}/emojis/{emoji}`), the trash deletes (`DELETE`). The button's Emoji Studio
//! cropper is not drawn: picked files upload as dropped ones do.

use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, PathPromptOptions, SharedString,
    Subscription, div, img, prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const EMOJIS_PREFIX: &str = "guild-emojis:";
const UPLOADED_PREFIX: &str = "guild-emojis:uploaded:";
/// Module 473145 `sN` for a server without the MORE_EMOJI feature or extra slots.
const BASE_SLOTS: usize = 50;

/// Name fields of the listed emoji, by emoji id.
#[derive(Default)]
pub(crate) struct EmojiState {
    names: HashMap<String, (Entity<Editor>, Subscription)>,
    /// The emoji whose name is being edited; the others show `:name:` hugging its text.
    editing: Option<String>,
}

impl EmojiState {
    /// The name fields, for the harness's typing.
    #[cfg(feature = "capture")]
    pub(super) fn fields(&self) -> Vec<Entity<Editor>> {
        self.names
            .values()
            .map(|(field, _)| field.clone())
            .collect()
    }
}

/// Module 690521 `sanitizeEmojiName`: only `[a-zA-Z0-9_]`, at most 32, padded with `_` to 2.
pub(super) fn sanitize_emoji_name(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(32)
        .collect();
    while clean.chars().count() < 2 {
        clean.push('_');
    }
    clean
}

/// `sgL8sI`: "no slots", "1 slot", "N slots", then " available".
fn slots_available(count: usize) -> String {
    match count {
        0 => "no slots available".to_owned(),
        1 => "1 slot available".to_owned(),
        n => format!("{n} slots available"),
    }
}

impl Root {
    pub(super) fn request_emojis(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{EMOJIS_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/emojis"),
        });
    }

    /// The list arrived: images for the emoji and their uploaders, and a name field for each.
    pub(super) fn emojis_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(UPLOADED_PREFIX) {
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_emojis(guild);
            }
            return;
        }
        let list: Vec<Value> = self
            .fetched
            .get(key)
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        for emoji in &list {
            let Some(id) = emoji["id"].as_str() else {
                continue;
            };
            let extension = if emoji["animated"].as_bool() == Some(true) {
                "gif"
            } else {
                "webp"
            };
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.{extension}?size=32"),
            );
            if let Some(user) = User::from_value(&emoji["user"]) {
                let (key, url) = avatar_source(&user);
                self.request_image(key, url);
            }
        }
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let guild = open.guild;
        let state = open.emoji.get_or_insert_with(EmojiState::default);
        for emoji in &list {
            let Some(id) = emoji["id"].as_str().map(str::to_owned) else {
                continue;
            };
            let name = emoji["name"].as_str().unwrap_or_default().to_owned();
            if let Some((field, _)) = state.names.get(&id) {
                field.update(cx, |editor, _| editor.set_text(&name));
                continue;
            }
            let field = cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_text(&name);
                editor
            });
            let emoji_id = id.clone();
            let events = cx.subscribe(&field, move |this, editor, event: &EditorEvent, cx| {
                match event {
                    // Enter empties the editor; the submitted text comes with the event.
                    EditorEvent::Submit(typed) => {
                        let typed = typed.clone();
                        editor.update(cx, |editor, _| editor.set_text(&typed));
                        this.rename_emoji(guild, &emoji_id, &typed, cx);
                        if let Some(state) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.emoji.as_mut())
                        {
                            state.editing = None;
                        }
                    }
                    EditorEvent::Cancel => {
                        if let Some(state) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.emoji.as_mut())
                        {
                            state.editing = None;
                        }
                    }
                    EditorEvent::Edited => {}
                }
                cx.notify();
            });
            state.names.insert(id, (field, events));
        }
        cx.notify();
    }

    /// `of`'s onCommit: unchanged names are left alone, others sanitized and sent.
    fn rename_emoji(&mut self, guild: Id, emoji: &str, typed: &str, cx: &mut Context<Self>) {
        let key = format!("{EMOJIS_PREFIX}{guild}");
        let current = self
            .fetched
            .get(&key)
            .and_then(|body| body.as_array())
            .and_then(|list| {
                list.iter()
                    .find(|known| known["id"].as_str() == Some(emoji))
            })
            .and_then(|known| known["name"].as_str().map(str::to_owned));
        if current.as_deref() == Some(typed) {
            return;
        }
        let name = sanitize_emoji_name(typed);
        if let Some(field) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.emoji.as_ref())
            .and_then(|state| state.names.get(emoji))
        {
            field.0.update(cx, |editor, _| editor.set_text(&name));
        }
        if current.as_deref() == Some(name.as_str()) {
            return;
        }
        if let Some(list) = self
            .fetched
            .get_mut(&key)
            .and_then(|body| body.as_array_mut())
            && let Some(known) = list
                .iter_mut()
                .find(|known| known["id"].as_str() == Some(emoji))
        {
            known["name"] = Value::String(name.clone());
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/emojis/{emoji}"),
            body: Some(serde_json::json!({"name": name})),
        });
    }

    fn delete_emoji(&mut self, guild: Id, emoji: &str, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/guilds/{guild}/emojis/{emoji}"),
            body: None,
        });
        if let Some(list) = self
            .fetched
            .get_mut(&format!("{EMOJIS_PREFIX}{guild}"))
            .and_then(|body| body.as_array_mut())
        {
            list.retain(|known| known["id"].as_str() != Some(emoji));
        }
        cx.notify();
    }

    /// Upload Emoji: image files, each posted under its file name.
    fn pick_emoji_files(&mut self, guild: Id, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |this, _| {
                    for path in paths {
                        this.upload_emoji(guild, &path);
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn upload_emoji(&mut self, guild: Id, path: &std::path::Path) {
        let Some(image) = super::server_settings::data_uri(path) else {
            return;
        };
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        let name = sanitize_emoji_name(stem.split('.').next().unwrap_or_default());
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{UPLOADED_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/emojis"),
            body: Some(serde_json::json!({"image": image, "name": name})),
        });
    }

    /// One `of` row (`emojiRow_XTvD56`): the picture, the `:name:` field, who uploaded it, and the trash on hover.
    /// Columns measure 48, 245 and 245 in the row; the last row has no divider.
    fn emoji_row(
        &self,
        guild: Id,
        emoji: &Value,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = emoji["id"].as_str().unwrap_or_default().to_owned();
        let name = emoji["name"].as_str().unwrap_or_default().to_owned();
        let picture = self.images.get(&format!("custom:{id}")).cloned();
        let state = self
            .server_settings
            .as_ref()
            .and_then(|open| open.emoji.as_ref());
        let field = state
            .and_then(|state| state.names.get(&id))
            .map(|(field, _)| field.clone());
        let editing = state.is_some_and(|state| state.editing.as_deref() == Some(id.as_str()));
        let user = User::from_value(&emoji["user"]);
        let avatar = user
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let group = SharedString::from(format!("server-emoji-row-{id}"));
        let colon = || {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(":")
        };
        let target = id.clone();
        let edit = id.clone();
        // `emojiAliasInputContainer`: colons 2px inside the field, the name 7px in; the field hugs the name.
        let name_field = if editing {
            div()
                .w(px(200.))
                .h(px(32.))
                .px(px(7.))
                .rounded(px(8.))
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .child(div().flex_1().min_w(px(0.)).children(field))
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("server-emoji-name-{id}")))
                .h(px(32.))
                .pl(px(2.))
                .pr(px(2.))
                .rounded(px(8.))
                .cursor_text()
                .hover(|style| style.bg(color("--input-background-default")))
                .flex()
                .items_center()
                .child(colon())
                .child(
                    div()
                        .px(px(1.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(name),
                )
                .child(colon())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let Some(state) = this
                        .server_settings
                        .as_mut()
                        .and_then(|open| open.emoji.as_mut())
                    else {
                        return;
                    };
                    state.editing = Some(edit.clone());
                    if let Some((field, _)) = state.names.get(&edit) {
                        let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
                        window.focus(&handle, cx);
                    }
                    cx.notify();
                }))
                .into_any_element()
        };
        div()
            .group(group.clone())
            .relative()
            .min_h(px(64.))
            .when(!last, |this| {
                this.border_b_1().border_color(color("--border-subtle"))
            })
            .flex()
            .items_center()
            .child(
                div()
                    .w(px(48.))
                    .flex_none()
                    .children(picture.map(|picture| img(picture).size(px(32.)))),
            )
            .child(div().w(px(245.)).flex_none().flex().child(name_field))
            .child(
                div()
                    .w(px(245.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div().size(px(20.)).flex_none().rounded_full().children(
                            avatar.map(|avatar| img(avatar).size(px(20.)).rounded_full()),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(
                                user.map(|user| user.display_name().to_owned())
                                    .unwrap_or_default(),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .child(
                        div()
                            .id(SharedString::from(format!("server-emoji-delete-{id}")))
                            .size(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-critical-secondary-border-default"))
                            .bg(color("--control-critical-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                "channel_settings_trash",
                                16.,
                                "--control-critical-secondary-icon-default",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_emoji(guild, &target, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// `op` (a fieldset: 20px legend, the free slots under it) over `oA`'s table, or its centred "None".
    fn emoji_section(
        &self,
        guild: Id,
        title: &'static str,
        emojis: &[Value],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let count = emojis.len();
        let rows: Vec<AnyElement> = emojis
            .iter()
            .rev()
            .enumerate()
            .map(|(index, emoji)| self.emoji_row(guild, emoji, index + 1 == count, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(
                div()
                    .mt(px(-8.))
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(slots_available(BASE_SLOTS.saturating_sub(count))),
            )
            .child(if rows.is_empty() {
                div()
                    .mb(px(48.))
                    .flex()
                    .justify_center()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-muted"))
                    .child("None")
                    .into_any_element()
            } else {
                div()
                    .mb(px(48.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        div()
                            .mb(px(4.))
                            .py(px(8.))
                            .px(px(16.))
                            .rounded_t(px(12.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-mod-muted"))
                            .flex()
                            .child(div().w(px(49.)).flex_none().child(label("Image")))
                            .child(div().w(px(257.)).flex_none().child(label("Name")))
                            .child(div().w(px(257.)).flex_none().child(label("Uploaded By"))),
                    )
                    .child(div().py(px(4.)).px(px(24.)).children(rows))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// The page: heading, Upload Emoji, the drag-and-drop note under a divider, then the two sections or No Emoji.
    pub(super) fn server_emoji_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let loaded = self
            .fetched
            .get(&format!("{EMOJIS_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned());
        let (animated, still): (Vec<Value>, Vec<Value>) = loaded
            .clone()
            .unwrap_or_default()
            .into_iter()
            .partition(|emoji| emoji["animated"].as_bool() == Some(true));
        let list = match &loaded {
            None => div().into_any_element(),
            Some(all) if all.is_empty() => div()
                .mt(px(20.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child("No Emoji"),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child("Get the party started by uploading an emoji"),
                )
                .into_any_element(),
            Some(_) => div()
                .flex()
                .flex_col()
                .child(self.emoji_section(guild, "Emoji", &still, cx))
                .child(self.emoji_section(guild, "Animated Emoji", &animated, cx))
                .into_any_element(),
        };
        div()
            .id("server-emoji-page")
            // Files dropped on the page upload under their names (`a6`'s onDrop).
            .on_drop(cx.listener(move |this, paths: &gpui::ExternalPaths, _, cx| {
                for path in paths.paths() {
                    this.upload_emoji(guild, path);
                }
                cx.notify();
            }))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Emoji"),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(format!("Add up to {BASE_SLOTS} custom emoji that anyone can use in this server. Animated GIF emoji may be used by members with Discord Nitro.")),
                    ),
            )
            .child(
                div()
                    .pb(px(24.))
                    .mb(px(24.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(
                        div().flex().child(
                            div()
                                .id("server-emoji-upload")
                                .h(px(40.))
                                .px(px(15.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--control-primary-border-default"))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child("Upload Emoji")
                                .on_click(cx.listener(move |this, _, _, cx| this.pick_emoji_files(guild, cx))),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child("If you want to upload multiple emojis or skip the editor, drag and drop the file(s) onto this page. The emojis will be named using the file name."),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sanitize_like_module_690521() {
        assert_eq!(sanitize_emoji_name("party-parrot!"), "partyparrot");
        assert_eq!(sanitize_emoji_name("a"), "a_");
        assert_eq!(sanitize_emoji_name("é"), "__");
        assert_eq!(sanitize_emoji_name(&"x".repeat(40)).len(), 32);
        assert_eq!(slots_available(49), "49 slots available");
        assert_eq!(slots_available(1), "1 slot available");
    }
}
