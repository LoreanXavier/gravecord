//! Server Settings > Emoji (module 989433 `ob`, rows `of`, sections `oN`/`op`/`oA`; styles module 378243).
//!
//! `GET /guilds/{id}/emojis` lists them, split into Emoji and Animated Emoji, newest first. Upload Emoji takes image
//! files and posts each as module 810877 `W` does for a dropped file: the name is the file's stem through
//! `sanitizeEmojiName` (module 690521), the image a data URI (`POST /guilds/{id}/emojis`). A name edited in its row is
//! committed with Enter (`PATCH /guilds/{id}/emojis/{emoji}`), the trash deletes (`DELETE`). The button's Emoji Studio
//! cropper is not drawn: picked files upload as dropped ones do. Several files post one after the other (`UD` of
//! module 824832) and the ones refused are listed in "Failed to upload files" (module 940372, rows of module 534606).

use super::chat_notices::upload_errors_alert;
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
    bulk: BulkUpload,
}

/// `o` of module 534606: why one file of a bulk upload did not become an emoji.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UploadError {
    TooBig,
    RateLimit,
    TooManyEmoji,
    TooManyAnimatedEmoji,
    WrongType,
    ResizeAnimated,
}

/// `TOO_MANY_EMOJI`, `TOO_MANY_ANIMATED_EMOJI`, `INVALID_FILE_ASSET_SIZE` and its `_RESIZE_ANIMATED` (module 652215).
const TOO_MANY_EMOJI: u64 = 30008;
const TOO_MANY_ANIMATED_EMOJI: u64 = 30018;
const INVALID_FILE_ASSET_SIZE: u64 = 50045;
const INVALID_FILE_ASSET_SIZE_RESIZE_ANIMATED: u64 = 50138;
/// `isFileTooBig` of module 690521: 2 MiB.
const MAX_FILE_BYTES: u64 = 2_097_152;

impl UploadError {
    /// The catch of module 810877 `W`: the body's code (or an `image` field error), then a 429 over all of them.
    pub(super) fn from_answer(status: u64, body: &Value) -> Option<Self> {
        let mut kind = None;
        if !body.is_null() {
            let code = body["code"].as_u64();
            kind = if code == Some(TOO_MANY_EMOJI) {
                Some(Self::TooManyEmoji)
            } else if code == Some(TOO_MANY_ANIMATED_EMOJI) {
                Some(Self::TooManyAnimatedEmoji)
            } else if !body["image"].is_null() || code == Some(INVALID_FILE_ASSET_SIZE) {
                Some(Self::TooBig)
            } else if code == Some(INVALID_FILE_ASSET_SIZE_RESIZE_ANIMATED) {
                Some(Self::ResizeAnimated)
            } else {
                None
            };
        }
        if status == 429 {
            kind = Some(Self::RateLimit);
        }
        kind
    }

    /// `p` of module 534606: the file icon, the description, the error circle's colour and its tooltip.
    fn look(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::TooBig => ("emoji_error_file_warning", "YbdEFK", "--red-400", "PanA4J"),
            Self::WrongType => ("emoji_error_file_deny", "01Papm", "--red-400", "PanA4J"),
            Self::ResizeAnimated => ("emoji_error_resize", "6WN/qk", "--red-400", "PanA4J"),
            Self::RateLimit => (
                "emoji_error_speedometer",
                "4rjikl",
                "--orange-345",
                "dq9aTi",
            ),
            Self::TooManyEmoji => ("circle_info", "5VmbbF", "--red-400", "PanA4J"),
            Self::TooManyAnimatedEmoji => ("circle_info", "rjFOZ2", "--red-400", "PanA4J"),
        }
    }
}

/// A bulk upload in progress (`UD` of module 824832): the files still to post one after the other, the one posting,
/// and the failures so far.
#[derive(Default)]
pub(crate) struct BulkUpload {
    queue: std::collections::VecDeque<std::path::PathBuf>,
    posting: Option<String>,
    errors: Vec<(UploadError, String)>,
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
                let answer = self.fetched.get(key).cloned().unwrap_or(Value::Null);
                self.emoji_upload_answered(guild, &answer, cx);
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
                let _ = this.update(cx, |this, cx| this.upload_emojis(guild, paths, cx));
            }
        })
        .detach();
    }

    fn bulk_upload(&mut self) -> Option<&mut BulkUpload> {
        self.server_settings
            .as_mut()
            .map(|open| &mut open.emoji.get_or_insert_with(EmojiState::default).bulk)
    }

    /// `UD` of module 824832: the files post one after the other; each failure is kept with the file's name, and
    /// "Failed to upload files" lists them once the last one has its answer.
    pub(super) fn upload_emojis(
        &mut self,
        guild: Id,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(bulk) = self.bulk_upload() else {
            return;
        };
        bulk.queue.extend(paths);
        if bulk.posting.is_none() {
            self.next_emoji_upload(guild, cx);
        }
    }

    fn next_emoji_upload(&mut self, guild: Id, cx: &mut Context<Self>) {
        loop {
            let Some(bulk) = self.bulk_upload() else {
                return;
            };
            let Some(path) = bulk.queue.pop_front() else {
                bulk.posting = None;
                let errors = std::mem::take(&mut bulk.errors);
                if !errors.is_empty() {
                    self.chat_extra.alert = Some(upload_errors_alert(errors));
                    cx.notify();
                }
                return;
            };
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            // The image does not read as one: `WRONG_TYPE` before anything is posted.
            let Some(image) = super::server_settings::data_uri(&path) else {
                bulk.errors.push((UploadError::WrongType, filename));
                continue;
            };
            // `W`: a GIF, WebP or AVIF over 2 MiB is too big as it is. GAP: other images over 2 MiB are not resized
            // to 128x128 first (`h_`); they are posted as they are and the server's answer says why.
            let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
            let animated_kind = image.starts_with("data:image/gif")
                || image.starts_with("data:image/webp")
                || image.starts_with("data:image/avif");
            if size > MAX_FILE_BYTES && animated_kind {
                bulk.errors.push((UploadError::TooBig, filename));
                continue;
            }
            bulk.posting = Some(filename);
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
            return;
        }
    }

    /// One file's answer (`Request::Rest`'s `{ok, status, body}`): a refusal becomes its row, then the next file goes.
    fn emoji_upload_answered(&mut self, guild: Id, answer: &Value, cx: &mut Context<Self>) {
        let Some(bulk) = self.bulk_upload() else {
            return;
        };
        let Some(filename) = bulk.posting.take() else {
            return;
        };
        if answer["ok"].as_bool() != Some(true)
            && let Some(kind) =
                UploadError::from_answer(answer["status"].as_u64().unwrap_or(0), &answer["body"])
        {
            bulk.errors.push((kind, filename));
        }
        self.next_emoji_upload(guild, cx);
    }

    /// Capture only: `emoji bulk-errors` lists one row of each kind in the dialog, as a bulk upload ends.
    #[cfg(feature = "capture")]
    pub(crate) fn script_emoji_bulk_errors(&mut self, cx: &mut Context<Self>) -> String {
        let errors = vec![
            (UploadError::TooBig, "huge_party.gif".to_owned()),
            (UploadError::WrongType, "notes.txt".to_owned()),
            (UploadError::ResizeAnimated, "spinning.gif".to_owned()),
            (UploadError::RateLimit, "wave.png".to_owned()),
            (UploadError::TooManyEmoji, "cat.png".to_owned()),
            (UploadError::TooManyAnimatedEmoji, "dance.gif".to_owned()),
        ];
        self.chat_extra.alert = Some(upload_errors_alert(errors));
        cx.notify();
        "emoji: Failed to upload files shown".to_owned()
    }

    /// `A` of module 534606, one row of the dialog (`emojiErrorItem__z-z-q`): `--background-base-lower`, 4px
    /// corners, 16px inside; the file icon in `--primary-300` 18px from the name (16px `--text-default`, lowercase, one
    /// line, at most 256 wide, 4px over the description in body-sm `--text-subtle`), then the 20px error circle whose
    /// tooltip says what to do.
    pub(super) fn emoji_upload_error_row(
        &self,
        index: usize,
        kind: UploadError,
        filename: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (file_icon, description, tint, tip) = kind.look();
        let file_icon = if kind == UploadError::ResizeAnimated {
            gpui::svg()
                .path(format!("icons/{file_icon}.svg"))
                .w(px(18.))
                .h(px(22.))
                .flex_none()
                .text_color(color("--primary-300"))
        } else {
            icon(file_icon, 24., "--primary-300")
        };
        div()
            .p(px(16.))
            .rounded(px(4.))
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .justify_center()
            .child(div().mr(px(18.)).child(file_icon))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .mb(px(4.))
                            .max_w(px(256.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(filename.to_lowercase()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(dc_intl::t(description)),
                    ),
            )
            .child(self.with_tip(
                format!("emoji-upload-error-{index}"),
                super::tooltip::Tip::body(
                    super::tooltip::Body::Text(dc_intl::t(tip).into()),
                    super::tooltip::Side::Right,
                ),
                icon("standing_circle_error", 20., tint),
                cx,
            ))
            .into_any_element()
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
                this.upload_emojis(guild, paths.paths().to_vec(), cx);
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

    #[test]
    fn refusals_map_like_module_810877() {
        use serde_json::json;
        let of = |status, body: Value| UploadError::from_answer(status, &body);
        assert_eq!(
            of(400, json!({"code": 30008})),
            Some(UploadError::TooManyEmoji)
        );
        assert_eq!(
            of(400, json!({"code": 30018})),
            Some(UploadError::TooManyAnimatedEmoji)
        );
        assert_eq!(of(400, json!({"code": 50045})), Some(UploadError::TooBig));
        assert_eq!(
            of(400, json!({"code": 50035, "image": ["too big"]})),
            Some(UploadError::TooBig)
        );
        assert_eq!(
            of(400, json!({"code": 50138})),
            Some(UploadError::ResizeAnimated)
        );
        // A 429 wins over the body; other refusals list nothing.
        assert_eq!(
            of(429, json!({"code": 30008})),
            Some(UploadError::RateLimit)
        );
        assert_eq!(of(429, Value::Null), Some(UploadError::RateLimit));
        assert_eq!(of(403, json!({"code": 50013})), None);
        assert_eq!(of(0, Value::Null), None);
    }
}
