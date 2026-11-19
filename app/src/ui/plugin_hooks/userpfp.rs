//! UserPFP (Equicord `src/equicordplugins/userpfp`, GPL-3.0-or-later): avatars of the UserPFP list (`databaseSource`,
//! fetched while the plugin is on) and the ones you set yourself ("Set Avatar" in a person's menu, kept in this
//! client's storage, seen by you only) replace a person's avatar wherever it is drawn (`getUserAvatarURL`), unless
//! `preferNitro` and they have an animated avatar; a UserPFP-hosted picture is asked for still (`animated=false`, a
//! GIF as PNG), and an animated one (a GIF) plays where the app plays animated avatars. With `overrideServerAvatars` a server avatar gives way to the
//! custom one, or else to the person's own avatar. The "Custom Avatar" dialog shows Original → Local, takes a PNG/GIF
//! URL (Enter saves) or a PNG/JPEG dropped or chosen (a GIF or WebP file asks for a URL instead), and has Delete and
//! Save. The plugin card says it is local only and has "Join UserPFP Server" and "Support UserPFP here". The remote
//! list wins over a local entry for the same person, as the original merges them. Not part of the pinned client.
//!
//! GAP: the "Allow Host" notice (the desktop client's content security policy; this client loads any https picture).

use super::super::{Root, color, editor_style, modal};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::State;
use dc_state::{Id, User};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, ExternalPaths, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement,
    Styled, StyledImage as _, Subscription, Window, div, img, px,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

const ID: &str = "UserPFP";
const STORAGE: &str = "plugin-userpfp-avatars";
const USERPFP_IMG_URL: &str = "https://raw.githubusercontent.com/UserPFP/img";
const INVITE_LINK: &str = "https://discord.gg/userpfp-1129784704267210844";
/// UserPFP's own donation page, as the plugin card links it (not this project's support link).
const DONATION_URL: &str = "https://ko-fi.com/coolesding";
pub(in crate::ui) const KEY: &str = "plugin-userpfp-database";

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    override_server: bool,
    prefer_nitro: bool,
    source: String,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    Some(Settings {
        override_server: flag("overrideServerAvatars"),
        prefer_nitro: key("preferNitro") != Value::Bool(false),
        source: key("databaseSource")
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

thread_local! {
    /// `data.avatars`: the local ones, then the remote list over them.
    static AVATARS: RefCell<Option<HashMap<Id, String>>> = const { RefCell::new(None) };
    static ASKED: Cell<bool> = const { Cell::new(false) };
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

struct Dialog {
    user: Id,
    url: Entity<Editor>,
    /// An uploaded file's `data:` URL (the field shows "(uploaded file)").
    upload: Option<String>,
    focus: bool,
    _subs: Vec<Subscription>,
}

fn local() -> HashMap<Id, String> {
    crate::storage::get(STORAGE)
        .and_then(|value| {
            Some(
                value
                    .as_object()?
                    .iter()
                    .filter_map(|(id, url)| Some((id.parse().ok()?, url.as_str()?.to_owned())))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn avatars<R>(read: impl FnOnce(&HashMap<Id, String>) -> R) -> R {
    AVATARS.with(|cell| {
        let mut cell = cell.borrow_mut();
        read(cell.get_or_insert_with(local))
    })
}

/// `getAvatarHook`: the picture to show for `user`, or `None` for their own.
fn custom_url(url: &str, animated: bool) -> String {
    if url.starts_with("data:") {
        return url.to_owned();
    }
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_owned();
    };
    if url.starts_with(USERPFP_IMG_URL) {
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(key, _)| key != "animated")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        parsed
            .query_pairs_mut()
            .clear()
            .extend_pairs(pairs)
            .append_pair("animated", if animated { "true" } else { "false" });
        if !animated {
            let path = parsed
                .path()
                .replace(".gifv", ".png")
                .replace(".gif", ".png");
            parsed.set_path(&path);
        }
    }
    parsed.to_string()
}

/// The playing file of a custom avatar that moves (a GIF), where the app plays animated avatars: `Some(None)` when
/// the custom avatar is still (the person's own animation must not play over it), `None` without a custom avatar.
pub(in crate::ui) fn userpfp_live_avatar(user: &User) -> Option<Option<(String, String)>> {
    let url = userpfp_avatar(user, true)?;
    let moving = url::Url::parse(&url).is_ok_and(|parsed| {
        let path = parsed.path().to_ascii_lowercase();
        path.ends_with(".gif") || path.ends_with(".gifv")
    });
    Some(moving.then(|| (format!("userpfp-live:{url}"), url)))
}

/// The UserPFP avatar of `user`, as `getUserAvatarURL` would give it.
pub(in crate::ui) fn userpfp_avatar(user: &User, animated: bool) -> Option<String> {
    let settings = current()?;
    if settings.prefer_nitro
        && user
            .avatar
            .as_deref()
            .is_some_and(|hash| hash.starts_with("a_"))
    {
        return None;
    }
    let url = avatars(|list| list.get(&user.id).cloned())?;
    Some(custom_url(&url, animated))
}

/// `getAvatarServerHook` with `overrideServerAvatars`: the custom avatar, else (for a member with a server avatar)
/// the person's own avatar; `None` keeps the server avatar.
pub(in crate::ui) fn userpfp_server_avatar(
    user: &User,
    has_server_avatar: bool,
) -> Option<(String, String)> {
    current().filter(|settings| settings.override_server)?;
    if let Some(url) = avatars(|list| list.get(&user.id).cloned()) {
        let url = custom_url(&url, false);
        return Some((format!("userpfp:{url}"), url));
    }
    (has_server_avatar && user.avatar.is_some()).then(|| super::super::avatar_source(user))
}

impl Root {
    /// `start`: the list is fetched the first time the plugin is seen on.
    pub(in crate::ui) fn userpfp_prepare(&mut self) {
        if ASKED.with(Cell::get) {
            return;
        }
        let Some(settings) = current() else {
            return;
        };
        ASKED.with(|asked| asked.set(true));
        if settings.source.is_empty() {
            return;
        }
        let _ = self.requests.send(crate::net::Request::FetchPublicJson {
            key: KEY.to_owned(),
            url: settings.source,
        });
    }

    /// The list arrived: `Object.assign(data.avatars, remote.avatars)`. True when the key was ours.
    pub(in crate::ui) fn userpfp_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if key != KEY {
            return false;
        }
        if let Some(remote) = body["avatars"].as_object() {
            avatars(|_| ());
            AVATARS.with(|cell| {
                if let Some(list) = cell.borrow_mut().as_mut() {
                    for (id, url) in remote {
                        if let (Ok(id), Some(url)) = (id.parse::<Id>(), url.as_str()) {
                            list.insert(id, url.to_owned());
                        }
                    }
                }
            });
            cx.notify();
        }
        true
    }

    /// "Set Avatar" in a person's menu, after a separator.
    pub(in crate::ui) fn with_userpfp_row(
        &self,
        mut items: Vec<super::super::context_menu::Entry>,
        target: super::super::context_menu::Target,
    ) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row, Target};
        if current().is_none() || !matches!(target, Target::User { .. }) {
            return items;
        }
        items.push(Entry::Separator);
        items.push(Entry::Row(
            Row::new(Act::PluginSetAvatar, "Set Avatar").icon("clips_pencil"),
        ));
        items
    }

    pub(in crate::ui) fn userpfp_open(&mut self, user: Id, cx: &mut Context<Self>) {
        let saved = avatars(|list| list.get(&user).cloned()).unwrap_or_default();
        let (shown, upload) = if saved.starts_with("data:") {
            ("(uploaded file)".to_owned(), Some(saved))
        } else {
            (saved, None)
        };
        let url = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("https://example.com/image.png".to_owned());
            editor.set_text(&shown);
            editor
        });
        let sub = cx.subscribe(
            &url,
            |this: &mut Root, _: Entity<Editor>, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Submit(_) => this.userpfp_save(cx),
                    EditorEvent::Cancel => this.userpfp_close(cx),
                    EditorEvent::Edited => {
                        // Typing replaces an uploaded file with the URL typed.
                        DIALOG.with(|dialog| {
                            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                                dialog.upload = None;
                            }
                        });
                    }
                }
                cx.notify();
            },
        );
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                user,
                url,
                upload,
                focus: true,
                _subs: vec![sub],
            });
        });
        cx.notify();
    }

    fn userpfp_close(&mut self, cx: &mut Context<Self>) {
        DIALOG.with(|dialog| dialog.borrow_mut().take());
        cx.notify();
    }

    fn userpfp_store(&mut self, user: Id, url: Option<String>) {
        avatars(|_| ());
        AVATARS.with(|cell| {
            if let Some(list) = cell.borrow_mut().as_mut() {
                match url {
                    Some(url) => {
                        list.insert(user, url);
                    }
                    None => {
                        list.remove(&user);
                    }
                }
            }
        });
        // Only the ones set here are kept (the remote list is fetched again at start).
        let mut kept = local();
        match avatars(|list| list.get(&user).cloned()) {
            Some(url) => {
                kept.insert(user, url);
            }
            None => {
                kept.remove(&user);
            }
        }
        let value: serde_json::Map<String, Value> = kept
            .into_iter()
            .map(|(id, url)| (id.to_string(), json!(url)))
            .collect();
        crate::storage::set(STORAGE, Value::Object(value));
    }

    /// `saveUserAvatar`: an empty field deletes.
    fn userpfp_save(&mut self, cx: &mut Context<Self>) {
        let Some((user, url)) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            let typed = dialog.url.read(cx).text().trim().to_owned();
            Some((dialog.user, dialog.upload.clone().unwrap_or(typed)))
        }) else {
            return;
        };
        self.userpfp_store(
            user,
            (!url.trim().is_empty()).then(|| url.trim().to_owned()),
        );
        self.userpfp_close(cx);
    }

    fn userpfp_delete(&mut self, cx: &mut Context<Self>) {
        if let Some(user) = DIALOG.with(|dialog| dialog.borrow().as_ref().map(|dialog| dialog.user))
        {
            self.userpfp_store(user, None);
        }
        self.userpfp_close(cx);
    }

    /// `handleFile`: a PNG or JPEG becomes the field's `data:` URL; a GIF or WebP asks for a URL.
    fn userpfp_file(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let mime = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" | "webp" => {
                self.put_app_toast(
                    "GIFs/WebP must be added via URL. Imgur, GitHub, Tenor, Giphy and Discord links work without extra \
                     setup."
                        .to_owned(),
                    false,
                    cx,
                );
                return;
            }
            _ => return,
        };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read(path) })
                .await;
            let Ok(bytes) = read else {
                return;
            };
            use base64::Engine as _;
            let url = format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            let _ = this.update(cx, |this, cx| {
                let editor = DIALOG.with(|dialog| {
                    let mut dialog = dialog.borrow_mut();
                    let dialog = dialog.as_mut()?;
                    dialog.upload = Some(url.clone());
                    Some(dialog.url.clone())
                });
                if let Some(editor) = editor {
                    editor.update(cx, |editor, cx| {
                        editor.set_text("(uploaded file)");
                        cx.notify();
                    });
                }
                let _ = this;
                cx.notify();
            });
        })
        .detach();
    }

    fn userpfp_choose(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.userpfp_file(path, cx));
            }
        })
        .detach();
    }

    /// `SetAvatarModal`, while open.
    pub(in crate::ui) fn userpfp_dialog(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (user, url, upload) = DIALOG.with(|dialog| {
            let mut dialog = dialog.borrow_mut();
            let dialog = dialog.as_mut()?;
            if std::mem::take(&mut dialog.focus) {
                let handle = gpui::Focusable::focus_handle(dialog.url.read(cx), cx);
                window.focus(&handle, cx);
            }
            Some((dialog.user, dialog.url.clone(), dialog.upload.clone()))
        })?;
        current()?;
        let person = self.stores.users.get(&user).cloned().unwrap_or_default();
        let (original_key, original_url) = super::super::avatar_source(&User {
            id: user,
            ..person.clone()
        });
        let original = if avatars(|list| list.contains_key(&user)) {
            // `IconUtils.getUserAvatarURL` is hooked too; the original box shows the person's own picture.
            match &person.avatar {
                Some(hash) => {
                    let url =
                        format!("https://cdn.discordapp.com/avatars/{user}/{hash}.webp?size=128");
                    (format!("avatar:{user}:{hash}:128"), url)
                }
                None => (original_key, original_url),
            }
        } else {
            (original_key, original_url)
        };
        let typed = url.read(cx).text().trim().to_owned();
        let preview = upload.clone().unwrap_or(typed);
        let preview_source =
            (!preview.is_empty()).then(|| (format!("userpfp-preview:{preview}"), preview.clone()));
        let picture = |source: &(String, String), cx: &mut Context<Self>| {
            let image = self.images.get(&source.0).cloned();
            if image.is_none() {
                self.want_image(source.0.clone(), source.1.clone(), cx);
            }
            image
        };
        let original_image = picture(&original, cx);
        let local_image = match &preview_source {
            Some(source) => picture(source, cx),
            None => original_image.clone(),
        };
        let active = preview_source.is_some();
        let avatar = |image: Option<std::sync::Arc<gpui::Image>>, active: bool| {
            div()
                .size(px(80.))
                .rounded_full()
                .overflow_hidden()
                .border_3()
                .border_color(color(if active {
                    "--brand-500"
                } else {
                    "--background-mod-normal"
                }))
                .children(
                    image.map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                )
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(11.))
                .text_color(color("--text-muted"))
                .child(text.to_uppercase())
        };
        let field_handle = gpui::Focusable::focus_handle(url.read(cx), cx);
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .my(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.))
                            .child(label("Original"))
                            .child(avatar(original_image, false)),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .text_size(px(24.))
                            .text_color(color("--text-muted"))
                            .child("→"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.))
                            .child(label("Local"))
                            .child(avatar(local_image, active)),
                    ),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Enter PNG/GIF URL"),
            )
            .child(
                div()
                    .cursor_text()
                    .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                        window.focus(&field_handle, cx)
                    })
                    .min_h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(gpui::rgba(0x0000001f))
                    .px(px(12.))
                    .py(px(10.))
                    .mb(px(8.))
                    .child(url),
            )
            .child(
                div()
                    .id("userpfp-dropzone")
                    .mt(px(8.))
                    .p(px(20.))
                    .rounded(px(10.))
                    .border_2()
                    .border_dashed()
                    .border_color(color("--background-mod-normal"))
                    .text_center()
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .cursor_pointer()
                    .drag_over::<ExternalPaths>(|style, _, _, _| {
                        style
                            .border_color(color("--brand-500"))
                            .bg(gpui::hsla(0.65, 0.8, 0.6, 0.1))
                    })
                    .child(
                        "⬆ Drag an image or click to upload (for GIFs or WebP use a URL instead)",
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.userpfp_choose(cx)))
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        if let Some(path) = paths.paths().first().cloned() {
                            this.userpfp_file(path, cx);
                        }
                    })),
            )
            .into_any_element();
        let mut buttons = Vec::new();
        if avatars(|list| list.contains_key(&user)) {
            buttons.push(modal::button(
                "userpfp-delete",
                "Delete",
                modal::Tone::Critical,
                |this, _, cx| this.userpfp_delete(cx),
            ));
        }
        buttons.push(modal::button(
            "userpfp-save",
            "Save",
            modal::Tone::Primary,
            |this, _, cx| this.userpfp_save(cx),
        ));
        let spec = modal::Spec {
            id: "userpfp-dialog",
            title: SharedString::from("Custom Avatar"),
            subtitle: None,
            body: Some(body),
            buttons,
            close: Box::new(|this, _, cx| this.userpfp_close(cx)),
        };
        Some(modal::render_small(spec, cx))
    }

    /// The plugin card's `settingsAboutComponent`.
    pub(in crate::ui) fn userpfp_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(50.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .cursor_pointer()
                .border_1()
                .border_color(color("--control-secondary-border-default"))
                .bg(color("--control-secondary-background-default"))
                .text_color(color("--control-secondary-text-default"))
                .text_size(px(16.))
                .child(label)
                .when(primary, |this| {
                    this.child(super::super::icon(
                        "link_external_small",
                        16.,
                        "--control-secondary-text-default",
                    ))
                })
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-feedback-mod-info"))
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child("Using the set avatar feature is local only meaning only you see it change."),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button("userpfp-join", "Join UserPFP Server", true)
                            .on_click(cx.listener(|this, _, _, cx| this.follow_link(INVITE_LINK, "Join UserPFP Server", cx))),
                    )
                    .child(
                        button("userpfp-support", "Support UserPFP here ♥", false)
                            .on_click(cx.listener(|_, _, _, cx| cx.open_url(DONATION_URL))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userpfp_pictures_are_asked_for_still() {
        assert_eq!(
            custom_url(
                "https://raw.githubusercontent.com/UserPFP/img/main/a.gif",
                false
            ),
            "https://raw.githubusercontent.com/UserPFP/img/main/a.png?animated=false"
        );
        assert_eq!(
            custom_url(
                "https://raw.githubusercontent.com/UserPFP/img/main/a.gif?x=1",
                true
            ),
            "https://raw.githubusercontent.com/UserPFP/img/main/a.gif?x=1&animated=true"
        );
        assert_eq!(
            custom_url("https://example.com/a.gif", false),
            "https://example.com/a.gif"
        );
        assert_eq!(
            custom_url("data:image/png;base64,AA==", false),
            "data:image/png;base64,AA=="
        );
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        let settings = settings_of(&state).unwrap();
        assert!(settings.override_server && settings.prefer_nitro);
        assert_eq!(
            settings.source,
            "https://userpfp.github.io/UserPFP/source/data.json"
        );
    }
}
