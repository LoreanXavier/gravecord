//! Server Settings > Soundboard (module 989433 `fi`, `fo`, rows `fd`; requests module 796774; dialog module 122998).
//!
//! `GET /guilds/{id}/soundboard-sounds` lists them. Upload Sound opens "Upload a Sound": a file (Browse, or dropped on
//! the dialog), its name (at most 32), a related emoji and the volume, then `POST` with the file as a data URI. A row's
//! trash is `DELETE /guilds/{id}/soundboard-sounds/{sound}`. The official dialog's waveform trimmer is not drawn; a
//! file over the server's limits comes back as the server's error.
//! Measured in `design/dom/settings/guild-soundboard-darker-cozy.json`, `modals/sound-upload-darker-cozy.json`.

use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathPromptOptions, SharedString, Subscription, Window, div, img, prelude::*,
    px,
};
use serde_json::Value;

pub(super) const SOUNDS_PREFIX: &str = "guild-sounds:";
const CHANGED_PREFIX: &str = "guild-sounds:changed:";
/// The dialog's slider track (`track_OnqacX`), inside the 430px field.
const SLIDER_WIDTH: f32 = 420.;
/// Module 473145 `fA` without boosts: the capture's "8 Slots of 8 available" (see `slots_line`).
const BASE_SLOTS: usize = 8;

/// "Upload a Sound" while it is open.
pub(crate) struct SoundDialog {
    pub(super) name: Entity<Editor>,
    file: Option<(String, String)>,
    emoji_name: Option<String>,
    emoji_id: Option<String>,
    volume: f32,
    dragging: bool,
    error: Option<String>,
    /// "Edit Sound": the sound being edited; there is no file to choose then.
    editing: Option<String>,
    _events: Subscription,
}

/// An audio file as the data URI the API takes.
fn audio_uri(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        _ => return None,
    };
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// `7E9Hdz`: "N Slot(s) of M available".
fn slots_line(free: usize, total: usize) -> String {
    let word = if free == 1 { "Slot" } else { "Slots" };
    format!("{free} {word} of {total} available")
}

impl Root {
    pub(super) fn request_sounds(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{SOUNDS_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/soundboard-sounds"),
        });
    }

    pub(super) fn sounds_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(CHANGED_PREFIX) {
            let report = self.fetched.get(key).cloned().unwrap_or_default();
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_sounds(guild);
            }
            if let Some(open) = self.server_settings.as_mut() {
                if report["ok"] == true {
                    open.sound_dialog = None;
                } else if let Some(dialog) = open.sound_dialog.as_mut() {
                    dialog.error = report["body"]["message"].as_str().map(str::to_owned);
                }
            }
        } else if let Some(guild) = key
            .strip_prefix(SOUNDS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            for sound in self.sounds_of(guild) {
                if let Some(user) = sound["user"]
                    .as_object()
                    .map(|_| &sound["user"])
                    .and_then(User::from_value)
                {
                    let (key, url) = avatar_source(&user);
                    self.request_image(key, url);
                }
                if let Some(id) = sound["emoji_id"].as_str() {
                    self.request_image(
                        format!("custom:{id}"),
                        format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
                    );
                } else if let Some(name) = sound["emoji_name"].as_str() {
                    let name = name.to_owned();
                    self.request_emoji_image(&name);
                }
            }
        }
        cx.notify();
    }

    fn sounds_of(&self, guild: Id) -> Vec<Value> {
        self.fetched
            .get(&format!("{SOUNDS_PREFIX}{guild}"))
            .and_then(|body| body["items"].as_array().cloned())
            .unwrap_or_default()
    }

    pub(super) fn open_sound_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Sound Name".to_owned());
            editor
        });
        let events = cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Cancel => {
                    if let Some(open) = this.server_settings.as_mut() {
                        open.sound_dialog = None;
                    }
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Edited => {
                    // `maxLength: 32`.
                    let text = editor.read(cx).text().to_owned();
                    if text.chars().count() > 32 {
                        let cut: String = text.chars().take(32).collect();
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                }
            }
            cx.notify();
        });
        let handle = gpui::Focusable::focus_handle(name.read(cx), cx);
        window.focus(&handle, cx);
        if let Some(open) = self.server_settings.as_mut() {
            open.sound_dialog = Some(SoundDialog {
                name,
                file: None,
                emoji_name: None,
                emoji_id: None,
                volume: 1.,
                dragging: false,
                error: None,
                editing: None,
                _events: events,
            });
        }
        cx.notify();
    }

    /// Edit on a row: the same dialog with the sound's values, saved with `PATCH` (module 796774 `xV`).
    fn edit_sound(&mut self, sound: &Value, window: &mut Window, cx: &mut Context<Self>) {
        self.open_sound_dialog(window, cx);
        let name = sound["name"].as_str().unwrap_or_default().to_owned();
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sound_dialog.as_mut())
        {
            dialog.name.update(cx, |editor, _| editor.set_text(&name));
            dialog.volume = sound["volume"].as_f64().unwrap_or(1.) as f32;
            dialog.emoji_id = sound["emoji_id"].as_str().map(str::to_owned);
            dialog.emoji_name = sound["emoji_name"].as_str().map(str::to_owned);
            dialog.editing = sound["sound_id"].as_str().map(str::to_owned);
        }
        cx.notify();
    }

    /// A chosen file: its data, and its name for the name field while that is empty.
    fn set_sound_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(uri) = audio_uri(path) else {
            return;
        };
        let shown = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let stem: String = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .chars()
            .take(32)
            .collect();
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sound_dialog.as_mut())
        {
            dialog.file = Some((shown, uri));
            if dialog.name.read(cx).text().is_empty() {
                dialog.name.update(cx, |editor, _| editor.set_text(&stem));
            }
        }
        cx.notify();
    }

    fn browse_sound_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let _ = this.update(cx, |this, cx| this.set_sound_file(path, cx));
            }
        })
        .detach();
    }

    pub(super) fn set_sound_emoji(
        &mut self,
        id: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if id.is_none() {
            self.request_emoji_image(&name);
        }
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sound_dialog.as_mut())
        {
            dialog.emoji_id = id;
            dialog.emoji_name = Some(name);
        }
        cx.notify();
    }

    fn upload_sound(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.sound_dialog.as_ref())
        else {
            return;
        };
        let name = dialog.name.read(cx).text().trim().to_owned();
        // `onChangeEnd`: the volume is kept to two places.
        let volume = (dialog.volume * 100.).round() / 100.;
        if let Some(sound) = dialog.editing.clone() {
            if name.is_empty() {
                return;
            }
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{CHANGED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/soundboard-sounds/{sound}"),
                body: Some(serde_json::json!({
                    "name": name,
                    "volume": volume,
                    "emoji_id": dialog.emoji_id,
                    "emoji_name": if dialog.emoji_id.is_some() { None } else { dialog.emoji_name.clone() },
                })),
            });
            return;
        }
        let Some((_, sound)) = dialog.file.as_ref().filter(|_| !name.is_empty()) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{CHANGED_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/soundboard-sounds"),
            body: Some(serde_json::json!({
                "name": name,
                "sound": sound,
                "volume": volume,
                "emoji_id": dialog.emoji_id,
                "emoji_name": if dialog.emoji_id.is_some() { None } else { dialog.emoji_name.clone() },
            })),
        });
    }

    fn set_sound_volume(&mut self, x: gpui::Pixels, left: f32, cx: &mut Context<Self>) {
        let fraction = ((f32::from(x) - left) / SLIDER_WIDTH).clamp(0., 1.);
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sound_dialog.as_mut())
        {
            dialog.volume = fraction;
        }
        cx.notify();
    }

    /// "Upload a Sound".
    pub(super) fn sound_dialog_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let dialog = open.sound_dialog.as_ref()?;
        let guild = open.guild;
        let editing = dialog.editing.is_some();
        let ready =
            (editing || dialog.file.is_some()) && !dialog.name.read(cx).text().trim().is_empty();
        let required = |text: &'static str| {
            div()
                .mb(px(8.))
                .flex()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
                .child(
                    div()
                        .font_weight(FontWeight(400.))
                        .text_color(color("--text-feedback-critical"))
                        .child("*"),
                )
        };
        let plain = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let picked = dialog
            .emoji_name
            .as_ref()
            .and_then(|name| self.images.get(&format!("emoji:{name}")))
            .cloned();
        let emoji_button = self.images.get("emoji_button").cloned();
        let shown_file = dialog.file.as_ref().map(|(name, _)| name.clone());
        let fraction = dialog.volume.clamp(0., 1.);
        // The dialog is 480 wide and centred; its track starts 30px inside.
        let body = div()
            .flex()
            .flex_col()
            .when(!editing, |this| {
                this.child(required("File")).child(
                    div()
                        .id("sound-file")
                        .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                            if let Some(path) = paths.paths().first() {
                                this.set_sound_file(path, cx);
                            }
                        }))
                        .h(px(40.))
                        .pl(px(10.))
                        .pr(px(4.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("sound_file_upload", 16., "--interactive-text-default"))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(if shown_file.is_some() {
                                    color("--text-default")
                                } else {
                                    color("--input-placeholder-text-default")
                                })
                                .child(shown_file.unwrap_or_else(|| "Choose a file".to_owned())),
                        )
                        .child(
                            div()
                                .id("sound-browse")
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--control-secondary-border-default"))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child("Browse")
                                .on_click(cx.listener(|this, _, _, cx| this.browse_sound_file(cx))),
                        ),
                )
            })
            .child(
                div()
                    .when(!editing, |this| this.mt(px(24.)))
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(266.))
                            .flex_none()
                            .child(required("Sound Name"))
                            .child(
                                div()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .child(div().w_full().child(dialog.name.clone())),
                            ),
                    )
                    .child(
                        div().flex_1().child(plain("Related Emoji")).child(
                            div()
                                .id("sound-emoji")
                                .relative()
                                .child(self.anchor_probe("sound-emoji"))
                                .h(px(40.))
                                .pl(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .children(
                                    picked
                                        .clone()
                                        .or(emoji_button)
                                        .map(|picture| img(picture).size(px(18.))),
                                )
                                .when(picked.is_none(), |this| {
                                    this.child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--input-placeholder-text-default"))
                                            .child("Click to select"),
                                    )
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let anchor =
                                        this.tip_anchors.borrow().get("sound-emoji").copied();
                                    if let Some(bounds) = anchor {
                                        this.open_picker(
                                            super::picker::Target::SoundEmoji,
                                            gpui::point(bounds.left() + px(12.), bounds.top()),
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                        ),
                    ),
            )
            .child(div().mt(px(24.)).child(plain("Sound Volume")))
            .child(
                div()
                    .id("sound-volume")
                    .relative()
                    .h(px(24.))
                    .cursor_pointer()
                    .child(self.anchor_probe("sound-volume"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            let left = this
                                .tip_anchors
                                .borrow()
                                .get("sound-volume")
                                .map_or(0., |bounds| f32::from(bounds.left()) + 5.);
                            if let Some(dialog) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.sound_dialog.as_mut())
                            {
                                dialog.dragging = true;
                            }
                            this.set_sound_volume(event.position.x, left, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        let dragging = this
                            .server_settings
                            .as_ref()
                            .and_then(|open| open.sound_dialog.as_ref())
                            .is_some_and(|dialog| dialog.dragging);
                        if dragging && event.dragging() {
                            let left = this
                                .tip_anchors
                                .borrow()
                                .get("sound-volume")
                                .map_or(0., |bounds| f32::from(bounds.left()) + 5.);
                            this.set_sound_volume(event.position.x, left, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if let Some(dialog) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.sound_dialog.as_mut())
                            {
                                dialog.dragging = false;
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(10.))
                            .w(px(430.))
                            .h(px(4.))
                            .rounded(px(4.))
                            .bg(color("--background-mod-strong"))
                            .child(
                                div()
                                    .h_full()
                                    .w(px(10. + SLIDER_WIDTH * fraction))
                                    .rounded(px(4.))
                                    .bg(color("--background-brand")),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(5. + SLIDER_WIDTH * fraction - 8.))
                            .top(px(4.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(gpui::rgb(0xffffff)),
                    ),
            )
            .children(dialog.error.clone().map(|error| {
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
            }))
            .into_any_element();
        let close = |this: &mut Root| {
            if let Some(open) = this.server_settings.as_mut() {
                open.sound_dialog = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "sound-upload-modal",
                title: if editing {
                    "Edit Sound".into()
                } else {
                    "Upload a Sound".into()
                },
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "sound-cancel",
                        "Never mind",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "sound-upload",
                        if editing { "Save" } else { "Upload" },
                        if ready {
                            super::modal::Tone::Primary
                        } else {
                            super::modal::Tone::PrimaryDisabled
                        },
                        move |this, _, cx| this.upload_sound(guild, cx),
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// One sound (`fd`, `row_doBhuY`): 60, 328 and 200 wide, 8 apart, the emoji or a placeholder, the name with its
    /// speaker, the uploader, and Edit over Remove.
    fn sound_row(&self, guild: Id, sound: &Value, cx: &mut Context<Self>) -> AnyElement {
        let id = sound["sound_id"].as_str().unwrap_or_default().to_owned();
        let picture = sound["emoji_id"]
            .as_str()
            .and_then(|emoji| self.images.get(&format!("custom:{emoji}")))
            .or_else(|| {
                sound["emoji_name"]
                    .as_str()
                    .and_then(|name| self.images.get(&format!("emoji:{name}")))
            })
            .cloned();
        let user = User::from_value(&sound["user"]);
        let avatar = user
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let target = id.clone();
        let edited = sound.clone();
        let group = SharedString::from(format!("server-sound-{id}"));
        let tool = |key: String, glyph: &'static str, tint: &'static str| {
            div()
                .id(SharedString::from(key))
                .size(px(24.))
                .rounded_full()
                .bg(color("--background-base-low"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph, 16., tint))
        };
        div()
            .group(group.clone())
            .w(px(636.))
            .py(px(4.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(60.)).flex_none().child(match picture {
                Some(picture) => img(picture).size(px(24.)).into_any_element(),
                None => {
                    icon("sound_no_emoji", 24., "--interactive-text-default").into_any_element()
                }
            }))
            .child(
                div()
                    .w(px(328.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(sound["name"].as_str().unwrap_or_default().to_owned()),
                    )
                    .child(
                        div()
                            .opacity(0.)
                            .group_hover(group.clone(), |style| style.opacity(1.))
                            .child(icon("sound_speaker", 16., "--interactive-text-default")),
                    ),
            )
            .child(
                div()
                    .w(px(200.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div().size(px(24.)).flex_none().rounded_full().children(
                            avatar.map(|avatar| img(avatar).size(px(24.)).rounded_full()),
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
                    .w(px(24.))
                    .h(px(52.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .child(
                        tool(
                            format!("server-sound-edit-{id}"),
                            "sound_edit",
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.edit_sound(&edited, window, cx);
                            },
                        )),
                    )
                    .child(
                        tool(
                            format!("server-sound-remove-{id}"),
                            "sound_remove",
                            "--text-feedback-critical",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this.requests.send(Request::Rest {
                                key: Some(format!("{CHANGED_PREFIX}{guild}")),
                                method: reqwest::Method::DELETE,
                                route: format!("/guilds/{guild}/soundboard-sounds/{target}"),
                                body: None,
                            });
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }

    /// The page: heading, intro, then No Sounds with its button, or the button, the free slots and the list.
    pub(super) fn server_sounds_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let sounds = self.sounds_of(guild);
        let upload = |cx: &mut Context<Self>| {
            div()
                .id("server-sound-upload")
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
                .child("Upload Sound")
                .on_click(cx.listener(|this, _, window, cx| this.open_sound_dialog(window, cx)))
        };
        let mut page = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Soundboard"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Upload custom sound reactions that anyone in this server can use. Nitro members will be able to access these sounds in any server on Discord."),
            );
        if sounds.is_empty() {
            page = page.child(
                div()
                    .mt(px(64.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child("No Sounds"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .mb(px(40.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child("Get the party started by uploading a sound"),
                    )
                    .child(upload(cx)),
            );
        } else {
            let head = |text: &'static str| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(text)
            };
            let rows: Vec<AnyElement> = sounds
                .iter()
                .map(|sound| self.sound_row(guild, sound, cx))
                .collect();
            page = page
                .child(div().mt(px(16.)).flex().child(upload(cx)))
                .child(
                    div()
                        .mt(px(32.))
                        .mb(px(32.))
                        .h(px(1.))
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .mb(px(32.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(slots_line(
                            BASE_SLOTS.saturating_sub(sounds.len()),
                            BASE_SLOTS,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(div().w(px(60.)).flex_none().child(head("Emoji")))
                        .child(div().w(px(328.)).flex_none().child(head("Name")))
                        .child(div().w(px(200.)).flex_none().child(head("Uploaded By"))),
                )
                .children(rows);
        }
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_read_like_the_string() {
        assert_eq!(slots_line(8, 8), "8 Slots of 8 available");
        assert_eq!(slots_line(1, 8), "1 Slot of 8 available");
        assert_eq!(audio_uri(std::path::Path::new("x.txt")), None);
    }
}
