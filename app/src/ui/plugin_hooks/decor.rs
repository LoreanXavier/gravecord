//! Decor (Vencord `src/plugins/decor`, GPL-3.0-or-later): avatar decorations from the Decor service. Everyone's choice
//! is read from the service's public list (`/users?ids=…`, no sign-in; at most once per person in four hours, again
//! when their full profile opens) and drawn wherever the app draws avatar decorations (the member list, Active Now,
//! the profile popout and the full profile), over the person's own decoration. The pictures are the service CDN's
//! still PNG, fetched and decoded off the UI thread.
//!
//! Choosing your own needs the service's sign-in (`service_auth.rs`): "Change Decoration" on the plugin card or the
//! Decor tile of the profile editor first opens the consent modal of Decor's application, then "Change Decoration":
//! None, your decorations (a right click: Copy Decoration Hash, Delete Decoration for your own), Create, and the
//! service's presets (public), with the preview, the preset's name, the alt text and its author, Copy Preset ID,
//! Discord Server, Log Out, Cancel and Apply. Create asks the guidelines once ("Hold on"), then the PNG or APNG file
//! and its name, "Submit for Review". Not part of the pinned client.
//!
//! GAP: the avatar decoration of chat messages and of the account panel, which the app does not draw yet (when it
//! does, `decor_decoration` is the hook); the presets' author avatars (`UserSummaryItem`); decorations play still
//! (the app draws every decoration still).

use super::super::{Root, avatar_source, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::{Request, ServiceBody, ServicePart};
use crate::plugins::decor::{self as api, Config, Decoration, Preset, Users};
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div, img, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

const ID: &str = "Decor";
/// The sign-in's caller name (`service_auth.rs`) and the credential entry's prefix.
pub(in crate::ui) const SERVICE: &str = "decor";
const KEY: &str = "plugin-decor:";
const IMAGE_PREFIX: &str = "decor:";

#[derive(Default)]
struct State {
    config: Option<Config>,
    /// The account the start ran for (`CONNECTION_OPEN`).
    started: Option<Id>,
    users: Users,
    /// The ids of the `/users` call on its way.
    asked: Vec<Id>,
    /// The full profile the decoration was asked for again (`USER_PROFILE_MODAL_OPEN`).
    profile: Option<Id>,
    /// `CurrentUserDecorationsStore`: `None` until fetched.
    mine: Option<Vec<Decoration>>,
    selected: Option<Decoration>,
    presets: Option<Vec<Preset>>,
    dialog: Option<Dialog>,
    /// After the sign-in: open Change Decoration.
    open_after_sign_in: bool,
    /// The decoration a right click was on.
    menu: Option<Decoration>,
}

enum Dialog {
    /// `undefined` = not trying, `Some(None)` = None, `Some(Some(_))` = one.
    Change {
        trying: Option<Option<Decoration>>,
        applying: bool,
    },
    Guidelines,
    Create(Create),
    LogOut,
    Delete(Decoration),
}

struct Create {
    name: Entity<Editor>,
    file: Option<PathBuf>,
    /// The chosen file as a `data:` URL, for the preview.
    preview: Option<String>,
    submitting: bool,
    error: Option<String>,
    focus: bool,
    _subs: Vec<Subscription>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<R>(read: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| read(&mut state.borrow_mut()))
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("Decor")
}

fn config() -> Config {
    with(|state| {
        state
            .config
            .get_or_insert_with(|| {
                let base = crate::plugins::with(|state| state.value(ID, "baseUrl"));
                Config::new(base.as_str().unwrap_or(api::DEFAULT_BASE))
            })
            .clone()
    })
}

fn image_source(config: &Config, asset: &str) -> (String, String) {
    (
        format!(
            "{IMAGE_PREFIX}{}",
            asset.strip_prefix("a_").unwrap_or(asset)
        ),
        config.asset_url(asset, false),
    )
}

/// The Decor decoration a person wears, as the picture's key and address, in place of their own
/// (`getDecorAvatarDecorationURL`); `None` when the plugin is off or they wear none.
pub(in crate::ui) fn decor_decoration(user: Id) -> Option<(String, String)> {
    if !on() {
        return None;
    }
    let asset = with(|state| state.users.asset(user).map(str::to_owned))?;
    Some(image_source(&config(), &asset))
}

/// A stable number for a Decor picture's key, where the app keys a composed avatar by its decoration's SKU.
pub(in crate::ui) fn decor_tag(key: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() | 1
}

impl Root {
    fn decor_me(&self) -> Option<Id> {
        self.stores.current_user.as_ref().map(|me| me.id)
    }

    fn decor_send(&self, step: &str, method: reqwest::Method, url: String, body: ServiceBody) {
        let bearer = self
            .decor_me()
            .and_then(|me| super::service_auth::token(SERVICE, me));
        let _ = self.requests.send(Request::PluginService {
            key: format!("{KEY}{step}"),
            method,
            url,
            bearer,
            body,
            exchange: false,
        });
    }

    /// The people whose avatars are drawn this frame (`prepare_member_avatars`): the start once per account
    /// (`CONNECTION_OPEN`: the config, the sign-in read, your own decoration), each person queued unless asked in the
    /// last four hours, the open full profile's asked again, and one `/users` call at a time.
    pub(in crate::ui) fn decor_prepare(
        &mut self,
        people: impl Iterator<Item = Id>,
        cx: &mut Context<Self>,
    ) {
        if !on() {
            return;
        }
        let Some(me) = self.decor_me() else {
            return;
        };
        let now = Instant::now();
        if with(|state| state.started.replace(me) != Some(me)) {
            let config = config();
            let _ = self.requests.send(Request::FetchPublicJson {
                key: format!("{KEY}config"),
                url: config.config_url(),
            });
            with(|state| {
                state.mine = None;
                state.selected = None;
                state.users.want(me, now, true);
            });
            self.service_token_load(SERVICE, me, |this, cx| this.decor_signed_in(cx), cx);
        }
        let profile = self.full_profile.as_ref().map(|open| open.user);
        with(|state| {
            if state.profile != profile {
                state.profile = profile;
                if let Some(user) = profile {
                    state.users.want(user, now, true);
                }
            }
            for user in people {
                state.users.want(user, now, false);
            }
        });
        self.decor_ask_users();
    }

    fn decor_ask_users(&mut self) {
        let batch = with(|state| {
            if !state.asked.is_empty() || !state.users.has_queue() {
                return Vec::new();
            }
            let batch = state.users.take_batch();
            state.asked.clone_from(&batch);
            batch
        });
        if batch.is_empty() {
            return;
        }
        let url = config().users_url(&batch);
        let _ = self.requests.send(Request::FetchPublicJson {
            key: format!("{KEY}users"),
            url,
        });
    }

    /// Capture only: the stand-in sign-in changed (`plugin token Decor …`).
    #[cfg(feature = "capture")]
    pub(in crate::ui) fn decor_token_ready(&mut self, cx: &mut Context<Self>) {
        self.decor_signed_in(cx);
    }

    /// Signed in (or known not to be): your decorations, when there is a token.
    fn decor_signed_in(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.decor_me() else {
            return;
        };
        if super::service_auth::token(SERVICE, me).is_none() {
            return;
        }
        let api = config().api();
        self.decor_send(
            "mine",
            reqwest::Method::GET,
            format!("{api}/users/@me/decorations"),
            ServiceBody::None,
        );
        self.decor_send(
            "selected",
            reqwest::Method::GET,
            format!("{api}/users/@me/decoration"),
            ServiceBody::None,
        );
        if with(|state| std::mem::take(&mut state.open_after_sign_in)) {
            self.decor_open_change(cx);
        }
        cx.notify();
    }

    /// The service's answers. True when the key was Decor's.
    pub(in crate::ui) fn decor_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(step) = key.strip_prefix(KEY) else {
            return false;
        };
        let now = Instant::now();
        let ok = body["ok"].as_bool() == Some(true);
        match step {
            "config" => with(|state| {
                if let Some(config) = state.config.as_mut() {
                    config.apply(body);
                }
            }),
            "users" => {
                let answer = body.is_object().then_some(body);
                with(|state| {
                    let asked = std::mem::take(&mut state.asked);
                    state.users.answered(&asked, answer, now);
                });
                self.decor_ask_users();
            }
            "presets" => with(|state| state.presets = Some(api::presets(body))),
            "mine" if ok => with(|state| state.mine = Some(api::decorations(&body["body"]))),
            "selected" if ok => {
                with(|state| state.selected = Decoration::from_value(&body["body"]))
            }
            "token" => self.decor_token(body, cx),
            "create" => self.decor_created(body, cx),
            "select" | "delete" if !ok => {
                self.put_app_toast("Decor did not take the change.".to_owned(), false, cx);
            }
            _ => {}
        }
        if !ok && body["status"].as_u64() == Some(401) {
            // A token the service no longer takes is forgotten, as signing out does.
            if let Some(me) = self.decor_me() {
                self.service_token_forget(SERVICE, me);
            }
        }
        cx.notify();
        true
    }

    /// The grant's code, traded for the service's token (`authorize`'s callback: the redirect with `client=vencord`).
    pub(in crate::ui) fn decor_code(&mut self, code: String, _cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::PluginService {
            key: format!("{KEY}token"),
            method: reqwest::Method::GET,
            url: config().token_url(&code),
            bearer: None,
            body: ServiceBody::None,
            exchange: true,
        });
    }

    fn decor_token(&mut self, body: &Value, cx: &mut Context<Self>) {
        let token = body["body"]
            .as_str()
            .map(str::trim)
            .filter(|token| !token.is_empty());
        match (body["ok"].as_bool() == Some(true), token, self.decor_me()) {
            (true, Some(token), Some(me)) => {
                self.service_token_set(SERVICE, me, token.to_owned());
                self.decor_signed_in(cx);
            }
            _ => {
                with(|state| state.open_after_sign_in = false);
                let why = body["body"].as_str().unwrap_or("Request not OK").to_owned();
                self.put_app_toast(format!("Failed to authorize: {why}"), false, cx);
            }
        }
    }

    fn decor_authorized(&self) -> bool {
        self.decor_me()
            .is_some_and(|me| super::service_auth::token(SERVICE, me).is_some())
    }

    /// `DecorSection`'s button: the sign-in first when there is no token, then Change Decoration.
    fn decor_change(&mut self, cx: &mut Context<Self>) {
        if self.decor_authorized() {
            self.decor_open_change(cx);
            return;
        }
        with(|state| {
            state.open_after_sign_in = true;
            state.dialog = None;
        });
        let config = config();
        self.service_sign_in(
            SERVICE,
            config.client_id.clone(),
            config.authorize_url(),
            cx,
        );
    }

    fn decor_open_change(&mut self, cx: &mut Context<Self>) {
        let api = config().api();
        let _ = self.requests.send(Request::FetchPublicJson {
            key: format!("{KEY}presets"),
            url: format!("{api}/decorations/presets"),
        });
        self.decor_signed_in_reads();
        with(|state| {
            state.dialog = Some(Dialog::Change {
                trying: None,
                applying: false,
            })
        });
        cx.notify();
    }

    fn decor_signed_in_reads(&self) {
        if !self.decor_authorized() {
            return;
        }
        let api = config().api();
        self.decor_send(
            "mine",
            reqwest::Method::GET,
            format!("{api}/users/@me/decorations"),
            ServiceBody::None,
        );
        self.decor_send(
            "selected",
            reqwest::Method::GET,
            format!("{api}/users/@me/decoration"),
            ServiceBody::None,
        );
    }

    /// `select(decoration)`: `PUT /users/@me/decoration` with the hash (`"null"` for none); your own decoration shows
    /// at once.
    fn decor_select(&mut self, decoration: Option<Decoration>, cx: &mut Context<Self>) {
        let Some(me) = self.decor_me() else {
            return;
        };
        let hash = decoration
            .as_ref()
            .map_or_else(|| "null".to_owned(), |decoration| decoration.hash.clone());
        let api = config().api();
        self.decor_send(
            "select",
            reqwest::Method::PUT,
            format!("{api}/users/@me/decoration"),
            ServiceBody::Form(vec![("hash".to_owned(), ServicePart::Text(hash))]),
        );
        with(|state| {
            state.users.set(
                me,
                decoration.as_ref().map(Decoration::asset),
                Instant::now(),
            );
            state.selected = decoration;
        });
        cx.notify();
    }

    fn decor_close(&mut self, cx: &mut Context<Self>) {
        with(|state| state.dialog = None);
        cx.notify();
    }

    /// The Create tile: the guidelines once, then Create Decoration.
    fn decor_create_open(&mut self, cx: &mut Context<Self>) {
        let agreed = crate::plugins::with(|state| state.value(ID, "agreedToGuidelines"))
            == Value::Bool(true);
        if !agreed {
            with(|state| state.dialog = Some(Dialog::Guidelines));
            cx.notify();
            return;
        }
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("Companion Cube".to_owned());
            editor
        });
        let sub = cx.subscribe(
            &name,
            |this: &mut Root, _: Entity<Editor>, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Cancel) {
                    this.decor_close(cx);
                }
                cx.notify();
            },
        );
        with(|state| {
            state.dialog = Some(Dialog::Create(Create {
                name,
                file: None,
                preview: None,
                submitting: false,
                error: None,
                focus: true,
                _subs: vec![sub],
            }))
        });
        cx.notify();
    }

    fn decor_choose_file(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // The file is read off the UI thread for the preview.
            let at = path.clone();
            let preview = cx
                .background_executor()
                .spawn(async move {
                    use base64::Engine as _;
                    let bytes = std::fs::read(&at).ok()?;
                    Some(format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    ))
                })
                .await;
            let _ = this.update(cx, |_, cx| {
                with(|state| {
                    if let Some(Dialog::Create(create)) = state.dialog.as_mut() {
                        create.file = Some(path);
                        create.preview = preview;
                        create.error = None;
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Submit for Review: `PUT /users/@me/decoration` with the image and its name.
    fn decor_submit(&mut self, cx: &mut Context<Self>) {
        let Some((file, name)) = with(|state| match state.dialog.as_mut() {
            Some(Dialog::Create(create)) if !create.submitting => {
                let name = create.name.read(cx).text().trim().to_owned();
                let file = create.file.clone()?;
                if name.is_empty() {
                    return None;
                }
                create.submitting = true;
                Some((file, name))
            }
            _ => None,
        }) else {
            return;
        };
        let filename = file.file_name().map_or_else(
            || "decoration.png".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let api = config().api();
        self.decor_send(
            "create",
            reqwest::Method::PUT,
            format!("{api}/users/@me/decoration"),
            ServiceBody::Form(vec![
                (
                    "image".to_owned(),
                    ServicePart::File {
                        path: file,
                        name: filename,
                        mime: "image/png".to_owned(),
                    },
                ),
                ("alt".to_owned(), ServicePart::Text(name)),
            ]),
        );
        cx.notify();
    }

    fn decor_created(&mut self, body: &Value, cx: &mut Context<Self>) {
        if body["ok"].as_bool() == Some(true) {
            if let Some(decoration) = Decoration::from_value(&body["body"]) {
                with(|state| state.mine.get_or_insert_with(Vec::new).push(decoration));
            }
            with(|state| {
                state.dialog = Some(Dialog::Change {
                    trying: None,
                    applying: false,
                })
            });
        } else {
            let why = body["body"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| "The decoration was not sent.".to_owned());
            with(|state| {
                if let Some(Dialog::Create(create)) = state.dialog.as_mut() {
                    create.submitting = false;
                    create.error = Some(why);
                }
            });
        }
        cx.notify();
    }

    /// `deleteDecoration`.
    fn decor_delete(&mut self, decoration: Decoration, cx: &mut Context<Self>) {
        let api = config().api();
        self.decor_send(
            "delete",
            reqwest::Method::DELETE,
            format!("{api}/decorations/{}", decoration.hash),
            ServiceBody::None,
        );
        let me = self.decor_me();
        with(|state| {
            if let Some(mine) = state.mine.as_mut() {
                mine.retain(|own| own.hash != decoration.hash);
            }
            if state
                .selected
                .as_ref()
                .is_some_and(|own| own.hash == decoration.hash)
            {
                state.selected = None;
                if let Some(me) = me {
                    state.users.set(me, None, Instant::now());
                }
            }
            state.dialog = Some(Dialog::Change {
                trying: None,
                applying: false,
            });
        });
        cx.notify();
    }

    /// The decoration's right-click menu (`DecorationContextMenu`).
    fn decor_menu_open(
        &mut self,
        decoration: Decoration,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        with(|state| state.menu = Some(decoration));
        self.open_context_menu(super::super::context_menu::Target::PluginDecoration, at, cx);
    }

    pub(in crate::ui) fn decor_menu_items(&self) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row};
        let Some(decoration) = with(|state| state.menu.clone()) else {
            return Vec::new();
        };
        let mut items = vec![Entry::Row(
            Row::new(Act::PluginDecorCopyHash, "Copy Decoration Hash").icon("menu_copy_text"),
        )];
        if decoration.author_id.is_some() && decoration.author_id == self.decor_me() {
            items.push(Entry::Row(
                Row::new(Act::PluginDecorDelete, "Delete Decoration")
                    .icon("menu_delete_message")
                    .danger(),
            ));
        }
        items
    }

    pub(in crate::ui) fn decor_menu_run(&mut self, delete: bool, cx: &mut Context<Self>) {
        let Some(decoration) = with(|state| state.menu.take()) else {
            return;
        };
        if delete {
            with(|state| state.dialog = Some(Dialog::Delete(decoration)));
        } else {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(decoration.hash));
        }
        cx.notify();
    }

    /// The service's Discord server: open it when you are in it, else its invite.
    fn decor_server(&mut self, cx: &mut Context<Self>) {
        if self.stores.guild(api::GUILD_ID).is_some() {
            with(|state| state.dialog = None);
            self.select(api::GUILD_ID, None);
            cx.notify();
        } else {
            self.follow_link(
                &format!("https://discord.gg/{}", api::INVITE_KEY),
                "Discord Server",
                cx,
            );
        }
    }

    /// An avatar with a decoration over it, as the decoration modal's preview draws it (the art 1.2 times the avatar).
    fn decor_avatar(
        &self,
        side: f32,
        art: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let me = self.stores.current_user.clone().unwrap_or_default();
        let (key, url) = avatar_source(&me);
        let picture = |key: String, url: String, cx: &mut Context<Self>| {
            let image = self.images.get(&key).cloned();
            if image.is_none() {
                self.want_image(key, url, cx);
            }
            image
        };
        let avatar = picture(key, url, cx);
        let art = art.and_then(|(key, url)| picture(key, url, cx));
        div()
            .relative()
            .size(px(side))
            .flex_none()
            .child(
                div()
                    .size(px(side))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(color("--background-mod-normal"))
                    .children(avatar.map(|image| img(image).size(px(side)).rounded_full())),
            )
            .children(art.map(|image| {
                div()
                    .absolute()
                    .left(px(-side * 0.1))
                    .top(px(-side * 0.1))
                    .child(img(image).size(px(side * 1.2)))
            }))
    }

    /// `DecorSection` on the plugin card: Change Decoration, Remove Decoration while one is chosen and signed in, and
    /// the way to the profile editor.
    pub(in crate::ui) fn decor_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let can_remove = self.decor_authorized() && with(|state| state.selected.is_some());
        let small = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .when(primary, |this| {
                    this.bg(color("--control-primary-background-default"))
                        .text_color(color("--white"))
                        .hover(|style| style.bg(color("--control-primary-background-hover")))
                })
                .when(!primary, |this| {
                    this.bg(color("--control-secondary-background-default"))
                        .text_color(color("--control-secondary-text-default"))
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                })
                .child(label)
        };
        let started = on();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!started, |this| {
                this.child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-default"))
                        .child("Enable Decor to change your avatar decoration."),
                )
            })
            .when(started, |this| {
                this.child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(
                            small("decor-change", "Change Decoration", true)
                                .on_click(cx.listener(|this, _, _, cx| this.decor_change(cx))),
                        )
                        .when(can_remove, |this| {
                            this.child(small("decor-remove", "Remove Decoration", false).on_click(
                                cx.listener(|this, _, _, cx| this.decor_select(None, cx)),
                            ))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .text_color(color("--text-default"))
                        .child("You can also access Decor decorations from the")
                        .child(
                            div()
                                .id("decor-profiles-link")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .hover(|style| style.underline())
                                .child("Profiles")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.open_profile_editor(cx)),
                                ),
                        )
                        .child("page."),
                )
            })
            .into_any_element()
    }

    /// The profile editor's Decor tile (`ExperimentDecorSection`), after Avatar & Decoration; not for a server
    /// profile.
    pub(in crate::ui) fn decor_profile_section(
        &self,
        guild_mode: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if guild_mode || !on() {
            return None;
        }
        let me = self.decor_me()?;
        let asset = with(|state| state.users.asset(me).map(str::to_owned));
        let can_remove = self.decor_authorized() && with(|state| state.selected.is_some());
        let config = config();
        let art = asset.as_deref().map(|asset| image_source(&config, asset));
        let preview = if art.is_some() {
            self.decor_avatar(72., art, cx).into_any_element()
        } else {
            div()
                .size(px(72.))
                .rounded_full()
                .bg(gpui::rgba(0x46494fff))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("plus_small", 24., "--white"))
                .into_any_element()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Decor"),
                )
                .child(
                    div()
                        .id("decor-profile-tile")
                        .group("decor-profile-tile")
                        .relative()
                        .size(px(100.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(preview)
                        .on_click(cx.listener(|this, _, _, cx| this.decor_change(cx)))
                        .when(can_remove, |this| {
                            this.child(
                                div()
                                    .id("decor-profile-remove")
                                    .absolute()
                                    .top(px(4.))
                                    .right(px(4.))
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(color("--control-overlay-secondary-background-default"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "x_small",
                                        16.,
                                        "--control-overlay-secondary-text-default",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.decor_select(None, cx);
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// The open Decor dialog.
    pub(in crate::ui) fn decor_dialog(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        enum Shown {
            Change(Option<Option<Decoration>>, bool),
            Guidelines,
            Create(
                Entity<Editor>,
                Option<PathBuf>,
                Option<String>,
                bool,
                Option<String>,
                bool,
            ),
            LogOut,
            Delete(Decoration),
        }
        let shown = with(|state| {
            Some(match state.dialog.as_mut()? {
                Dialog::Change { trying, applying } => Shown::Change(trying.clone(), *applying),
                Dialog::Guidelines => Shown::Guidelines,
                Dialog::Create(create) => Shown::Create(
                    create.name.clone(),
                    create.file.clone(),
                    create.preview.clone(),
                    create.submitting,
                    create.error.clone(),
                    std::mem::take(&mut create.focus),
                ),
                Dialog::LogOut => Shown::LogOut,
                Dialog::Delete(decoration) => Shown::Delete(decoration.clone()),
            })
        })?;
        Some(match shown {
            Shown::Change(trying, applying) => self.decor_change_modal(trying, applying, cx),
            Shown::Guidelines => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(
                        "By submitting a decoration, you agree to the guidelines. Not reading these guidelines may                          get your account suspended from creating more decorations in the future.",
                    )
                    .child(
                        div()
                            .id("decor-guidelines-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .hover(|style| style.underline())
                            .child("the guidelines")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.follow_link(api::GUIDELINES_URL, "the guidelines", cx)
                            })),
                    )
                    .into_any_element();
                modal::render_small(
                    modal::Spec {
                        id: "decor-guidelines",
                        title: SharedString::from("Hold on"),
                        subtitle: None,
                        body: Some(body),
                        buttons: vec![
                            modal::button(
                                "decor-guidelines-cancel",
                                "Cancel",
                                modal::Tone::Secondary,
                                |this, _, cx| this.decor_back(cx),
                            ),
                            modal::button(
                                "decor-guidelines-continue",
                                "Continue",
                                modal::Tone::Primary,
                                |this, _, cx| {
                                    crate::plugins::update(|state| {
                                        state.set_value(
                                            ID,
                                            "agreedToGuidelines",
                                            &Value::Bool(true),
                                        );
                                    });
                                    this.decor_create_open(cx);
                                },
                            ),
                        ],
                        close: Box::new(|this, _, cx| this.decor_back(cx)),
                    },
                    cx,
                )
            }
            Shown::Create(name, file, preview, submitting, error, focus) => {
                if focus {
                    let handle = gpui::Focusable::focus_handle(name.read(cx), cx);
                    window.focus(&handle, cx);
                }
                self.decor_create_modal(name, file, preview, submitting, error, cx)
            }
            Shown::LogOut => modal::render_small(
                modal::Spec {
                    id: "decor-log-out",
                    title: SharedString::from("Log Out"),
                    subtitle: Some(
                        div()
                            .child("Are you sure you want to log out of Decor?")
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        modal::button(
                            "decor-log-out-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, cx| this.decor_back(cx),
                        ),
                        modal::button(
                            "decor-log-out-confirm",
                            "Log Out",
                            modal::Tone::Critical,
                            |this, _, cx| {
                                if let Some(me) = this.decor_me() {
                                    this.service_token_forget(SERVICE, me);
                                }
                                with(|state| {
                                    state.mine = None;
                                    state.selected = None;
                                });
                                this.decor_close(cx);
                            },
                        ),
                    ],
                    close: Box::new(|this, _, cx| this.decor_back(cx)),
                },
                cx,
            ),
            Shown::Delete(decoration) => {
                let alt = decoration.alt.clone().unwrap_or_default();
                modal::render_small(
                    modal::Spec {
                        id: "decor-delete",
                        title: SharedString::from("Delete Decoration"),
                        subtitle: Some(
                            div()
                                .child(format!("Are you sure you want to delete {alt}?"))
                                .into_any_element(),
                        ),
                        body: None,
                        buttons: vec![
                            modal::button(
                                "decor-delete-cancel",
                                "Cancel",
                                modal::Tone::Secondary,
                                |this, _, cx| this.decor_back(cx),
                            ),
                            modal::button(
                                "decor-delete-confirm",
                                "Delete",
                                modal::Tone::Critical,
                                move |this, _, cx| this.decor_delete(decoration.clone(), cx),
                            ),
                        ],
                        close: Box::new(|this, _, cx| this.decor_back(cx)),
                    },
                    cx,
                )
            }
        })
    }

    /// Back to Change Decoration from a dialog it opened.
    fn decor_back(&mut self, cx: &mut Context<Self>) {
        with(|state| {
            state.dialog = Some(Dialog::Change {
                trying: None,
                applying: false,
            })
        });
        cx.notify();
    }

    fn decor_change_modal(
        &self,
        trying: Option<Option<Decoration>>,
        applying: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (mine, selected, presets) = with(|state| {
            (
                state.mine.clone().unwrap_or_default(),
                state.selected.clone(),
                state.presets.clone().unwrap_or_default(),
            )
        });
        let config = config();
        let active = trying.clone().unwrap_or_else(|| selected.clone());
        let pending = mine.iter().any(Decoration::pending);
        let in_preset = |decoration: &Decoration| {
            presets.iter().any(|preset| {
                preset
                    .decorations
                    .iter()
                    .any(|one| one.hash == decoration.hash)
            })
        };
        let own: Vec<Decoration> = mine.iter().filter(|one| !in_preset(one)).cloned().collect();
        let tile = |id: SharedString, selected: bool| {
            div()
                .id(id)
                .size(px(80.))
                .flex_none()
                .rounded(px(8.))
                .border_2()
                .border_color(if selected {
                    color("--control-brand-foreground")
                } else {
                    gpui::transparent_black().into()
                })
                .bg(color("--background-mod-subtle"))
                .hover(|style| style.bg(color("--background-mod-normal")))
                .cursor_pointer()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
        };
        let label = |text: &str| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-strong"))
                .child(text.to_owned())
        };
        let decoration_tile = |decoration: &Decoration, cx: &mut Context<Self>| {
            let (key, url) = image_source(&config, &decoration.asset());
            let picture = self.images.get(&key).cloned();
            if picture.is_none() {
                self.want_image(key, url, cx);
            }
            let chosen = active
                .as_ref()
                .is_some_and(|one| one.hash == decoration.hash);
            let pick = decoration.clone();
            let menu = decoration.clone();
            let waiting = decoration.pending();
            tile(
                SharedString::from(format!("decor-tile-{}", decoration.hash)),
                chosen,
            )
            .when(waiting, |this| this.opacity(0.5))
            .children(picture.map(|image| img(image).size(px(72.))))
            .on_click(cx.listener(move |_, _, _, cx| {
                if !waiting {
                    with(|state| {
                        if let Some(Dialog::Change { trying, .. }) = state.dialog.as_mut() {
                            *trying = Some(Some(pick.clone()));
                        }
                    });
                }
                cx.notify();
            }))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.decor_menu_open(menu.clone(), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
        };
        let section = |title: String, subtitle: Option<String>, items: Vec<AnyElement>| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .children(subtitle.map(|subtitle| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(subtitle)
                }))
                .child(div().flex().flex_wrap().gap(px(12.)).children(items))
        };
        let mut items = vec![
            tile(SharedString::from("decor-tile-none"), active.is_none())
                .child(icon("deny_circle", 24., "--interactive-text-default"))
                .child(label(dc_intl::tr("None")))
                .on_click(cx.listener(|_, _, _, cx| {
                    with(|state| {
                        if let Some(Dialog::Change { trying, .. }) = state.dialog.as_mut() {
                            *trying = Some(None);
                        }
                    });
                    cx.notify();
                }))
                .into_any_element(),
        ];
        items.extend(own.iter().map(|decoration| decoration_tile(decoration, cx)));
        items.push(
            tile(SharedString::from("decor-tile-create"), false)
                .child(icon("plus_large", 24., "--interactive-text-default"))
                .child(label(dc_intl::tr("Create")))
                .when(pending, |this| this.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if pending {
                        this.put_app_toast(
                            "You already have a decoration pending review".to_owned(),
                            false,
                            cx,
                        );
                    } else {
                        this.decor_create_open(cx);
                    }
                }))
                .into_any_element(),
        );
        let mut sections = vec![section(
            "Your Decorations".to_owned(),
            Some("You can delete your own decorations by right clicking on them.".to_owned()),
            items,
        )];
        for preset in &presets {
            let tiles = preset
                .decorations
                .iter()
                .map(|decoration| decoration_tile(decoration, cx))
                .collect();
            sections.push(section(
                preset.name.clone(),
                preset.description.clone(),
                tiles,
            ));
        }
        let active_preset = active
            .as_ref()
            .and_then(|one| one.preset_id.as_ref())
            .and_then(|id| presets.iter().find(|preset| &preset.id == id))
            .cloned();
        let author = active.as_ref().and_then(|one| one.author_id).map(|id| {
            self.stores
                .users
                .get(&id)
                .map_or_else(|| id.to_string(), |user| user.display_name().to_owned())
        });
        let art = active
            .as_ref()
            .map(|one| image_source(&config, &one.asset()));
        let preview = div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .pt(px(24.))
            .child(self.decor_avatar(96., art, cx))
            .child(div().h(px(16.)))
            .children(active_preset.as_ref().map(|preset| {
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(format!("Part of the {} Preset", preset.name))
            }))
            .children(active.as_ref().and_then(|one| one.alt.clone()).map(|alt| {
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(alt)
            }))
            .children(author.map(|name| {
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(format!("Created by @{name}"))
            }))
            .children(active_preset.map(|preset| {
                let id = preset.id.clone();
                modal::small_footer_button(
                    modal::button(
                        "decor-copy-preset",
                        "Copy Preset ID",
                        modal::Tone::Secondary,
                        move |this, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(id.clone()));
                            this.show_success_toast("Copied to clipboard!".to_owned(), cx);
                        },
                    ),
                    cx,
                )
            }));
        let footer_links = div()
            .flex()
            .items_center()
            .justify_between()
            .mt(px(8.))
            .child(
                div()
                    .id("decor-server")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .text_color(color("--text-link"))
                    .hover(|style| style.underline())
                    .child("Discord Server")
                    .on_click(cx.listener(|this, _, _, cx| this.decor_server(cx))),
            )
            .child(modal::small_footer_button(
                modal::button(
                    "decor-log-out",
                    "Log Out",
                    modal::Tone::CriticalSecondary,
                    |_, _, cx| {
                        with(|state| state.dialog = Some(Dialog::LogOut));
                        cx.notify();
                    },
                ),
                cx,
            ));
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .id("decor-change-sections")
                            .flex_1()
                            .h(px(360.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(20.))
                            .children(sections),
                    )
                    .child(preview),
            )
            .child(footer_links)
            .into_any_element();
        let can_apply = trying.is_some() && !applying;
        let apply = trying.clone().flatten();
        modal::render_large(
            modal::Spec {
                id: "decor-change",
                title: SharedString::from("Change Decoration"),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "decor-change-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, cx| this.decor_close(cx),
                    ),
                    modal::button(
                        "decor-change-apply",
                        "Apply",
                        if can_apply {
                            modal::Tone::Primary
                        } else {
                            modal::Tone::PrimaryDisabled
                        },
                        move |this, _, cx| {
                            this.decor_select(apply.clone(), cx);
                            this.decor_close(cx);
                        },
                    ),
                ],
                close: Box::new(|this, _, cx| this.decor_close(cx)),
            },
            cx,
        )
    }

    fn decor_create_modal(
        &self,
        name: Entity<Editor>,
        file: Option<PathBuf>,
        preview: Option<String>,
        submitting: bool,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field_handle = gpui::Focusable::focus_handle(name.read(cx), cx);
        let filled = file.is_some() && !name.read(cx).text().trim().is_empty();
        let heading = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let note = |text: &'static str| {
            div()
                .mt(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(text)
        };
        let filename = file
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let art = preview.map(|data| (format!("decor-file:{}", data.len()), data));
        let form = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(error.map(|error| {
                div()
                    .text_size(px(12.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
            }))
            .child(
                div()
                    .child(heading("File"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(40.))
                            .pl(px(12.))
                            .pr(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(16.))
                                    .text_color(color(if filename.is_some() {
                                        "--text-default"
                                    } else {
                                        "--text-muted"
                                    }))
                                    .child(filename.unwrap_or_else(|| "Choose a file".to_owned())),
                            )
                            .child(modal::small_footer_button(
                                modal::button(
                                    "decor-create-browse",
                                    "Browse",
                                    modal::Tone::Primary,
                                    |this, _, cx| this.decor_choose_file(cx),
                                ),
                                cx,
                            )),
                    )
                    .child(note("File should be APNG or PNG.")),
            )
            .child(
                div()
                    .child(heading("Name"))
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
                            .bg(color("--input-background-default"))
                            .px(px(12.))
                            .py(px(10.))
                            .child(name),
                    )
                    .child(note(
                        "This name will be used when referring to this decoration.",
                    )),
            );
        let card = |id: &'static str, warning: bool| {
            div()
                .id(id)
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if warning {
                    "--border-feedback-warning"
                } else {
                    "--border-feedback-info"
                }))
                .bg(color(if warning {
                    "--background-feedback-mod-warning"
                } else {
                    "--background-feedback-mod-info"
                }))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .flex()
                .flex_wrap()
        };
        let link = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_color(color("--text-link"))
                .hover(|style| style.underline())
                .child(text)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                card("decor-create-guidelines", true)
                    .child("Make sure your decoration does not violate\u{a0}")
                    .child(
                        link("decor-create-guidelines-link", "the guidelines").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.follow_link(api::GUIDELINES_URL, "the guidelines", cx)
                            }),
                        ),
                    )
                    .child("\u{a0}before submitting it."),
            )
            .child(
                div().flex().gap(px(24.)).child(form).child(
                    div()
                        .pt(px(24.))
                        .px(px(16.))
                        .child(self.decor_avatar(96., art, cx)),
                ),
            )
            .child(
                card("decor-create-server", false)
                    .child("To receive updates on your decoration's review, join\u{a0}")
                    .child(
                        link("decor-create-server-link", "Decor's Discord server")
                            .on_click(cx.listener(|this, _, _, cx| this.decor_server(cx))),
                    )
                    .child("\u{a0}and allow direct messages."),
            )
            .into_any_element();
        modal::render_large(
            modal::Spec {
                id: "decor-create",
                title: SharedString::from("Create Decoration"),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "decor-create-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, cx| this.decor_back(cx),
                    ),
                    modal::button(
                        "decor-create-submit",
                        "Submit for Review",
                        if submitting {
                            modal::Tone::PrimaryLoading
                        } else if filled {
                            modal::Tone::Primary
                        } else {
                            modal::Tone::PrimaryDisabled
                        },
                        |this, _, cx| this.decor_submit(cx),
                    ),
                ],
                close: Box::new(|this, _, cx| this.decor_back(cx)),
            },
            cx,
        )
    }

    /// Capture only: `decor state`: what the plugin holds.
    #[cfg(feature = "capture")]
    pub(crate) fn decor_state_line(&self) -> String {
        let me = self.decor_me();
        with(|state| {
            format!(
                "started={} asked={} mine={:?} selected={:?} presets={:?} me_asset={:?} signed_in={} dialog={}",
                state.started.is_some(),
                state.asked.len(),
                state.mine.as_ref().map(Vec::len),
                state.selected.as_ref().map(|one| one.hash.clone()),
                state.presets.as_ref().map(Vec::len),
                me.and_then(|me| state.users.asset(me).map(str::to_owned)),
                me.is_some_and(|me| super::service_auth::token(SERVICE, me).is_some()),
                match &state.dialog {
                    None => "none",
                    Some(Dialog::Change { .. }) => "change",
                    Some(Dialog::Guidelines) => "guidelines",
                    Some(Dialog::Create(_)) => "create",
                    Some(Dialog::LogOut) => "log-out",
                    Some(Dialog::Delete(_)) => "delete",
                }
            )
        })
    }

    /// Capture only: `decor open`: Change Decoration as after the sign-in.
    #[cfg(feature = "capture")]
    pub(crate) fn decor_script_open(&mut self, cx: &mut Context<Self>) {
        self.decor_change(cx);
    }
}
