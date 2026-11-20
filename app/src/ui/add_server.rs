//! The rail's Add a Server dialog, measured in `design/dom/modals/add-server*.json` and `join-server*.json`: Create Your
//! Server (Create My Own and the templates), Tell Us More About Your Server, Customize Your Server and Join a Server.
//! Creating posts `POST /guilds` as module 943457 `createGuildFromTemplate` does (`name`, `icon`, the template's
//! channels, `system_channel_id`, `guild_template_code`); joining posts `POST /invites/{code}` as the invite card does.
//! The template list and channel names are module 570465 with its en-US strings. Differences: School Club's example
//! role is not sent. The icon upload is module 356343: the dashed ring with "Upload" until a picture is picked (the
//! image input of module 946274: jpg, jpeg, jfif, png, gif, webp, avif, read as a data URL off the UI thread), then
//! the picture in an 80px circle; the data URL goes in the create body's `icon`.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use gpui::Focusable as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, deferred, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// `AGE_GATE` of module 109766: the birthday step an account without a date of birth starts with
    /// (`nsfwAllowed == null`), drawn by `birthday.rs`. Only the harness opens it (see `open_add_server`).
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    AgeGate,
    Pick,
    Purpose,
    Customize,
    Join,
}

pub(crate) struct AddServer {
    pub step: Step,
    pub template: usize,
    pub name: Entity<Editor>,
    pub invite: Entity<Editor>,
    pub error: Option<&'static str>,
    /// The picked icon: its data URL (`icon` of the create body) and the decoded picture.
    pub icon: Option<(String, Arc<gpui::Image>)>,
    scroll: ScrollHandle,
    _events: Vec<Subscription>,
}

/// A guild template of module 570465: its code, label and channels `(id, parent, name, type)` (4 voice, 0 text,
/// 4 is voice and 0 text in the API; categories are 4 and 2 here as `GUILD_CATEGORY` 4, `GUILD_VOICE` 2).
pub(crate) struct Template {
    pub id: &'static str,
    pub code: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub system: Option<&'static str>,
    pub channels: &'static [(&'static str, Option<&'static str>, &'static str, u8)],
}

const CATEGORY: u8 = 4;
const TEXT: u8 = 0;
const VOICE: u8 = 2;

pub(crate) const TEMPLATES: [Template; 7] = [
    Template {
        id: "CREATE",
        code: "2TffvPucqHkN",
        label: "Create My Own",
        icon: "add_server_create_own",
        system: None,
        channels: &[],
    },
    Template {
        id: "GAMING",
        code: "hvtBQMfw6uSJ",
        label: "Gaming",
        icon: "add_server_gaming",
        system: Some("01"),
        channels: &[
            ("00", None, "Text Channels", CATEGORY),
            ("01", Some("00"), "general", TEXT),
            ("02", Some("00"), "clips-and-highlights", TEXT),
            ("10", None, "Voice Channels", CATEGORY),
            ("11", Some("10"), "Lobby", VOICE),
            ("12", Some("10"), "Gaming", VOICE),
        ],
    },
    Template {
        id: "FRIENDS",
        code: "hgM48av5Q69A",
        label: "Friends",
        icon: "add_server_friends",
        system: Some("01"),
        channels: &[
            ("00", None, "Text Channels", CATEGORY),
            ("01", Some("00"), "general", TEXT),
            ("02", Some("00"), "games", TEXT),
            ("03", Some("00"), "music", TEXT),
            ("10", None, "Voice Channels", CATEGORY),
            ("11", Some("10"), "Lounge", VOICE),
            ("12", Some("10"), "Stream Room", VOICE),
        ],
    },
    Template {
        id: "STUDY",
        code: "FbwUwRp4j8Es",
        label: "Study Group",
        icon: "add_server_study",
        system: Some("11"),
        channels: &[
            ("00", None, "Information", CATEGORY),
            ("01", Some("00"), "welcome-and-rules", TEXT),
            ("02", Some("00"), "notes-resources", TEXT),
            ("10", None, "Text Channels", CATEGORY),
            ("11", Some("10"), "general", TEXT),
            ("12", Some("10"), "homework-help", TEXT),
            ("13", Some("10"), "session-planning", TEXT),
            ("14", Some("10"), "off-topic", TEXT),
            ("20", None, "Voice Channels", CATEGORY),
            ("21", Some("20"), "Lounge", VOICE),
            ("22", Some("20"), "Study Room 1", VOICE),
            ("23", Some("20"), "Study Room 2", VOICE),
        ],
    },
    Template {
        id: "SCHOOL_CLUB",
        code: "Ctg7PUHcQmZu",
        label: "School Club",
        icon: "add_server_school",
        system: Some("11"),
        channels: &[
            ("00", None, "Information", CATEGORY),
            ("01", Some("00"), "welcome-and-rules", TEXT),
            ("02", Some("00"), "announcements", TEXT),
            ("03", Some("00"), "resources", TEXT),
            ("10", None, "Text Channels", CATEGORY),
            ("11", Some("10"), "general", TEXT),
            ("12", Some("10"), "meeting-plans", TEXT),
            ("13", Some("10"), "off-topic", TEXT),
            ("20", None, "Voice Channels", CATEGORY),
            ("21", Some("20"), "Lounge", VOICE),
            ("22", Some("20"), "Meeting Room 1", VOICE),
            ("22", Some("20"), "Meeting Room 2", VOICE),
        ],
    },
    Template {
        id: "LOCAL_COMMUNITY",
        code: "64UDvRNCC52Y",
        label: "Local Community",
        icon: "add_server_local",
        system: Some("11"),
        channels: &[
            ("00", None, "Information", CATEGORY),
            ("01", Some("00"), "welcome-and-rules", TEXT),
            ("02", Some("00"), "announcements", TEXT),
            ("03", Some("00"), "resources", TEXT),
            ("10", None, "Text Channels", CATEGORY),
            ("11", Some("10"), "general", TEXT),
            ("12", Some("10"), "meeting-plans", TEXT),
            ("13", Some("10"), "off-topic", TEXT),
            ("20", None, "Voice Channels", CATEGORY),
            ("21", Some("20"), "Lounge", VOICE),
            ("22", Some("20"), "Meeting Room", VOICE),
        ],
    },
    Template {
        id: "CREATORS",
        code: "6exdzMgjZgah",
        label: "Artists & Creators",
        icon: "add_server_artists",
        system: Some("11"),
        channels: &[
            ("00", None, "Information", CATEGORY),
            ("01", Some("00"), "welcome-and-rules", TEXT),
            ("02", Some("00"), "announcements", TEXT),
            ("10", None, "Text Channels", CATEGORY),
            ("11", Some("10"), "general", TEXT),
            ("12", Some("10"), "events", TEXT),
            ("13", Some("10"), "ideas-and-feedback", TEXT),
            ("20", None, "Voice Channels", CATEGORY),
            ("21", Some("20"), "Lounge", VOICE),
            ("22", Some("20"), "Community Hangout", VOICE),
            ("23", Some("20"), "Stream Room", VOICE),
        ],
    },
];

/// The `POST /guilds` body of `createGuildFromTemplate`.
pub(crate) fn create_body(template: &Template, name: &str, icon: Option<&str>) -> Value {
    let channels: Vec<Value> = template
        .channels
        .iter()
        .map(|(id, parent, name, kind)| {
            json!({"id": id, "parent_id": parent, "name": name, "type": kind})
        })
        .collect();
    json!({
        "name": name,
        "icon": icon,
        "channels": channels,
        "system_channel_id": template.system,
        "guild_template_code": template.code,
    })
}

/// The invite code of what was typed: a bare code, `discord.gg/code` or an invite URL.
pub(crate) fn invite_code(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(code) = super::invite_embed::invite_codes(text).into_iter().next() {
        return Some(code);
    }
    text.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        .then(|| text.to_owned())
}

fn picture(bytes: &'static [u8]) -> Arc<gpui::Image> {
    Arc::new(gpui::Image::from_bytes(
        gpui::ImageFormat::Svg,
        bytes.to_vec(),
    ))
}

fn template_icon(name: &str) -> Arc<gpui::Image> {
    picture(match name {
        "add_server_create_own" => include_bytes!("../../assets/icons/add_server_create_own.svg"),
        "add_server_gaming" => include_bytes!("../../assets/icons/add_server_gaming.svg"),
        "add_server_friends" => include_bytes!("../../assets/icons/add_server_friends.svg"),
        "add_server_study" => include_bytes!("../../assets/icons/add_server_study.svg"),
        "add_server_school" => include_bytes!("../../assets/icons/add_server_school.svg"),
        "add_server_local" => include_bytes!("../../assets/icons/add_server_local.svg"),
        "add_server_artists" => include_bytes!("../../assets/icons/add_server_artists.svg"),
        "add_server_friends_group" => {
            include_bytes!("../../assets/icons/add_server_friends_group.svg")
        }
        "add_server_community" => include_bytes!("../../assets/icons/add_server_community.svg"),
        "add_server_discover" => include_bytes!("../../assets/icons/add_server_discover.svg"),
        "add_server_upload" => include_bytes!("../../assets/icons/add_server_upload.svg"),
        _ => include_bytes!("../../assets/icons/add_server_arrow.svg"),
    })
}

fn muted_card() -> u32 {
    0x97979f14
}

/// `gA` of module 946274: the file types the image input takes.
const ICON_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "jfif", "png", "gif", "webp", "avif"];

/// `readAsDataURL` of the picked file (`p` of module 946274), with the type its extension names.
pub(crate) fn icon_data_url(extension: &str, bytes: &[u8]) -> Option<String> {
    use base64::Engine as _;
    let mime = match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => return None,
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

impl Root {
    /// Module 356343 (`iconContainer_YUKqn_`, 80px): the picked icon in an 80px circle on `--background-mod-normal`
    /// (`filledIcon_YUKqn_`, "Your icon"), else the dashed ring with its camera and "+" badge and "Upload"
    /// (`uploadLabel_YUKqn_`: 44px from the top, centred, `body-xs/semibold`, the ring's own colour). A click opens the
    /// file dialog (`aria-label` "Upload a Server Icon").
    fn add_server_icon_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let picked = self
            .add_server
            .as_ref()
            .and_then(|add| add.icon.as_ref())
            .map(|(_, image)| image.clone());
        div()
            .id("add-server-icon")
            .relative()
            .size(px(80.))
            .cursor_pointer()
            .map(|control| match picked {
                Some(image) => control.child(
                    div()
                        .size(px(80.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(color("--background-mod-normal"))
                        // The `img` is sized to 80 by 80 with no `object-fit`: it fills the circle.
                        .child(
                            img(image)
                                .size(px(80.))
                                .rounded_full()
                                .object_fit(gpui::ObjectFit::Fill),
                        ),
                ),
                None => control
                    .child(img(template_icon("add_server_upload")).size(px(80.)))
                    .child(
                        div()
                            .absolute()
                            .top(px(44.))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            // The ring's `currentColor` (`--text-muted` in the measured frame).
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("3UB9ad")),
                    ),
            })
            .on_click(cx.listener(|this, _, _, cx| this.pick_add_server_icon(cx)))
            .into_any_element()
    }

    /// The file dialog of the image input; the file is read and decoded off the UI thread.
    fn pick_add_server_icon(&mut self, cx: &mut Context<Self>) {
        if crate::storage::is_capture_run() {
            // A capture run never opens the dialog: `add-server-icon <path>` hands the file in.
            return;
        }
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.add_server_icon_file(path, cx));
        })
        .detach();
    }

    /// Reads the picked file in the background (`readAsDataURL`), then shows it once it decodes as a picture (an
    /// unreadable one is dropped, as `onerror` does without `onChange`).
    pub(crate) fn add_server_icon_file(
        &mut self,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !ICON_EXTENSIONS.contains(&extension.as_str()) {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            let bytes = std::fs::read(&path).ok()?;
            let format = match extension.as_str() {
                "jpg" | "jpeg" | "jfif" => gpui::ImageFormat::Jpeg,
                "png" => gpui::ImageFormat::Png,
                "gif" => gpui::ImageFormat::Gif,
                "webp" => gpui::ImageFormat::Webp,
                _ => return None,
            };
            // `h` of module 946274 loads the picture before `onChange`; one that does not load (an avif here: no
            // decoder) takes `onerror`, the upload error.
            image::load_from_memory(&bytes).ok()?;
            let uri = icon_data_url(&extension, &bytes)?;
            Some((uri, Arc::new(gpui::Image::from_bytes(format, bytes))))
        });
        cx.spawn(async move |this, cx| {
            let picked = task.await;
            let _ = this.update(cx, |this, cx| {
                match picked {
                    Some(picked) => {
                        if let Some(add) = this.add_server.as_mut() {
                            add.icon = Some(picked);
                        }
                    }
                    // `handleFileError`: "Oops, something went wrong...", "Unable to process image".
                    None => {
                        this.upload_error = Some(super::upload_error::UploadError::Plain {
                            title: dc_intl::t("0egKg3"),
                            help: dc_intl::t("7PnXqu").to_owned(),
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_add_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.username.clone())
            .unwrap_or_default();
        let mut events = Vec::new();
        let mut field = |placeholder: &str, text: &str, cx: &mut Context<Self>| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder(placeholder.to_owned());
                editor.set_text(text);
                editor
            });
            events.push(cx.subscribe_in(
                &editor,
                window,
                |this, _, event: &EditorEvent, window, cx| {
                    match event {
                        EditorEvent::Submit(_) => this.add_server_submit(window, cx),
                        EditorEvent::Cancel => this.add_server = None,
                        EditorEvent::Edited => {
                            if let Some(add) = this.add_server.as_mut() {
                                add.error = None;
                            }
                        }
                    }
                    cx.notify();
                },
            ));
            editor
        };
        let name = field("", &format!("{username}'s server"), cx);
        let invite = field("https://discord.gg/hTKzmak", "", cx);
        // GAP: module 109766's flow (`ea`) starts at the birthday step for an account without a date of birth, but
        // module 596339 opens that NUF modal only for an account with no servers outside the desktop app
        // (`!isPlatformEmbedded`), so the desktop rail never reaches it; only the harness (`birthday nuf`) shows it.
        self.add_server = Some(AddServer {
            step: Step::Pick,
            template: 0,
            name,
            invite,
            error: None,
            icon: None,
            scroll: ScrollHandle::new(),
            _events: events,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn add_server_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(add) = self.add_server.as_mut() {
            add.step = step;
            add.error = None;
            let target = match step {
                Step::Customize => Some(add.name.clone()),
                Step::Join => Some(add.invite.clone()),
                _ => None,
            };
            match target {
                Some(editor) => window.focus(&editor.read(cx).focus_handle(cx), cx),
                None => window.focus(&self.focus, cx),
            }
        }
        cx.notify();
    }

    /// `onComplete` of 109766's birthday step: on to Create Your Server.
    /// GAP: an unclaimed account that came from an invite (`INVITE_UNCLAIMED`) closes the flow instead.
    pub(super) fn add_server_age_gate_done(&mut self) {
        if let Some(add) = self.add_server.as_mut()
            && add.step == Step::AgeGate
        {
            add.step = Step::Pick;
            add.error = None;
        }
    }

    fn add_server_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(add) = self.add_server.as_ref() else {
            return;
        };
        match add.step {
            Step::Customize => {
                let name = add.name.read(cx).text().trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let icon = add.icon.as_ref().map(|(uri, _)| uri.as_str());
                let body = create_body(&TEMPLATES[add.template], &name, icon);
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::POST,
                    route: "/guilds".to_owned(),
                    body: Some(body),
                });
                self.add_server = None;
            }
            Step::Join => {
                let text = add.invite.read(cx).text().to_owned();
                match invite_code(&text) {
                    Some(code) => {
                        self.add_server = None;
                        self.open_invite_accept(code, cx);
                    }
                    None => {
                        if let Some(add) = self.add_server.as_mut() {
                            add.error = Some("Please enter a valid invite link or invite code.");
                        }
                    }
                }
            }
            _ => {}
        }
        let _ = window;
        cx.notify();
    }

    fn choice(
        &self,
        id: SharedString,
        label: &'static str,
        symbol: &str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_none()
            .w(px(402.))
            .h(px(54.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgba(0x97979f1f))
            .bg(rgba(muted_card()))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0x97979f1f)))
            .flex()
            .items_center()
            .child(
                img(template_icon(symbol))
                    .flex_none()
                    .size(px(20.))
                    .mx(px(16.)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(label),
            )
            .child(
                img(template_icon("add_server_arrow"))
                    .flex_none()
                    .size(px(20.))
                    .mr(px(16.)),
            )
            .on_click(cx.listener(move |this, _, window, cx| run(this, window, cx)))
            .into_any_element()
    }

    fn add_header(
        title: &'static str,
        subtitle: AnyElement,
        gap_below: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .relative()
            .flex_none()
            .pt(px(24.))
            .pb(px(gap_below))
            .px(px(16.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .h(px(32.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(title),
            )
            .child(div().mt(px(8.)).w_full().child(subtitle))
            .child(
                div()
                    .id("add-server-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(16.))
                    .size(px(32.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 16., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.add_server = None;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn subtitle(text: &'static str, size: f32, line: f32) -> AnyElement {
        div()
            .w_full()
            .text_center()
            .text_size(px(size))
            .line_height(px(line))
            .text_color(color("--text-default"))
            .child(text)
            .into_any_element()
    }

    fn add_text_button(
        id: &'static str,
        label: &'static str,
        brand: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .cursor_pointer()
            .text_size(px(if brand { 14. } else { 16. }))
            .line_height(px(if brand { 18. } else { 20. }))
            .font_weight(FontWeight(500.))
            .text_color(if brand {
                color("--text-brand")
            } else {
                color("--text-strong")
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| run(this, window, cx)))
            .into_any_element()
    }

    fn add_primary(
        id: &'static str,
        label: &'static str,
        width: f32,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .w(px(width))
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgba(0xffffff14))
            .bg(color("--control-primary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| run(this, window, cx)))
            .into_any_element()
    }

    fn field(&self, editor: Entity<Editor>) -> AnyElement {
        div()
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-active"))
            .bg(color("--input-background-default"))
            .px(px(12.))
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(editor))
            .into_any_element()
    }

    pub(super) fn add_server_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let add = self.add_server.as_ref()?;
        let (name, invite) = (add.name.clone(), add.invite.clone());
        let step = add.step;
        if step == Step::AgeGate {
            return Some(self.add_server_age_gate_layer(cx));
        }
        let body: AnyElement = match step {
            // `AgeGate` returned above.
            Step::Pick | Step::AgeGate => {
                let mut rows: Vec<AnyElement> = Vec::new();
                for (index, template) in TEMPLATES.iter().enumerate() {
                    if index == 1 {
                        rows.push(
                            div()
                                .mt(px(8.))
                                .h(px(19.))
                                .text_size(px(14.))
                                .line_height(px(18.67))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child("Start from a template")
                                .into_any_element(),
                        );
                    }
                    rows.push(self.choice(
                        SharedString::from(format!("add-server-{}", template.id)),
                        template.label,
                        template.icon,
                        cx,
                        move |this, window, cx| {
                            if let Some(add) = this.add_server.as_mut() {
                                add.template = index;
                            }
                            this.add_server_step(Step::Purpose, window, cx);
                        },
                    ));
                }
                let scroll = add.scroll.clone();
                div()
                    .flex_none()
                    .relative()
                    .w_full()
                    .h(px(330.))
                    .child(
                        div()
                            .id("add-server-list")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .pl(px(24.))
                            .py(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(rows),
                    )
                    .child(super::settings::auto_scrollbar(scroll))
                    .into_any_element()
            }
            Step::Purpose => div()
                .w_full()
                .px(px(16.))
                .pb(px(8.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(self.choice(
                    "add-server-friends-purpose".into(),
                    "For me and my friends",
                    "add_server_friends_group",
                    cx,
                    |this, window, cx| this.add_server_step(Step::Customize, window, cx),
                ))
                .child(self.choice(
                    "add-server-community-purpose".into(),
                    "For a club or community",
                    "add_server_community",
                    cx,
                    |this, window, cx| this.add_server_step(Step::Customize, window, cx),
                ))
                .child(
                    div()
                        .mt(px(10.))
                        .h(px(18.))
                        .flex()
                        .justify_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child("Not sure? You can")
                        .child(
                            div()
                                .id("add-server-skip")
                                .cursor_pointer()
                                .text_color(color("--text-link"))
                                .child("skip this question")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.add_server_step(Step::Customize, window, cx)
                                })),
                        )
                        .child("for now."),
                )
                .into_any_element(),
            Step::Customize => div()
                .w_full()
                .px(px(24.))
                .pt(px(8.))
                .pb(px(16.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(84.))
                        .pt(px(4.))
                        .flex()
                        .justify_center()
                        .child(self.add_server_icon_control(cx)),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(20.))
                                .flex()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child("Server Name")
                                .child(
                                    div()
                                        .ml(px(4.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child("*"),
                                ),
                        )
                        .child(div().mt(px(8.)).child(self.field(name)))
                        .child(
                            div()
                                .mt(px(12.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child("By creating a server, you agree to Discord's ")
                                .child(
                                    div()
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-link"))
                                        .child("Community Guidelines"),
                                )
                                .child("."),
                        ),
                )
                .into_any_element(),
            Step::Join => {
                let sample = |text: &'static str| {
                    div()
                        .h(px(28.))
                        .px(px(5.))
                        .rounded(px(3.))
                        .bg(rgba(muted_card()))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-strong"))
                        .child(text)
                };
                div()
                    .w_full()
                    .px(px(24.))
                    .pt(px(8.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Invite link")
                            .child(
                                div()
                                    .ml(px(4.))
                                    .text_color(color("--text-feedback-critical"))
                                    .child("*"),
                            ),
                    )
                    .child(div().mt(px(8.)).child(self.field(invite)))
                    .children(add.error.map(|message| {
                        div().mt(px(8.)).child(super::motion::messages::status_message(
                            message,
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-feedback-critical"))
                                .child(message),
                        ))
                    }))
                    .child(
                        div()
                            .mt(px(16.))
                            .h(px(18.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child("Invites should look like"),
                    )
                    .child(
                        div()
                            .mt(px(0.))
                            .p(px(4.))
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .child(sample("hTKzmak"))
                            .child(sample("https://discord.gg/hTKzmak"))
                            .child(sample("https://discord.gg/wumpus-friends")),
                    )
                    .child(
                        div()
                            .id("add-server-discover")
                            .mt(px(0.))
                            .h(px(76.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-higher"))
                            .px(px(12.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(img(template_icon("add_server_discover")).size(px(40.)))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .h(px(20.))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child("Don't have an invite?"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(
                                                "Check out Discoverable communities in Server Discovery.",
                                            ),
                                    ),
                            )
                            .child(img(template_icon("add_server_arrow")).size(px(20.)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add_server = None;
                                cx.open_url("https://discord.com/discovery");
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        };
        let header = match step {
            Step::Pick | Step::AgeGate => Self::add_header(
                "Create Your Server",
                Self::subtitle(
                    "Your server is where you and your friends hang out. Make yours and start talking.",
                    16.,
                    20.,
                ),
                24.,
                cx,
            ),
            Step::Purpose => Self::add_header(
                "Tell Us More About Your Server",
                Self::subtitle(
                    "In order to help you with your setup, is your new server for just a few friends or a larger community?",
                    16.,
                    20.,
                ),
                24.,
                cx,
            ),
            Step::Customize => Self::add_header(
                "Customize Your Server",
                Self::subtitle(
                    "Give your new server a personality with a name and an icon. You can always change it later.",
                    16.,
                    20.,
                ),
                16.,
                cx,
            ),
            Step::Join => Self::add_header(
                "Join a Server",
                Self::subtitle("Enter an invite below to join an existing server", 14., 18.),
                0.,
                cx,
            ),
        };
        let footer = match step {
            Step::Pick | Step::AgeGate => div()
                .flex_none()
                .px(px(24.))
                .py(px(16.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(24.))
                        .text_center()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Have an invite already?"),
                )
                .child(
                    div()
                        .id("add-server-join")
                        .mt(px(8.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--control-secondary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .child("Join a Server")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_server_step(Step::Join, window, cx)
                        })),
                )
                .into_any_element(),
            Step::Purpose => div()
                .flex_none()
                .px(px(24.))
                .py(px(16.))
                .h(px(70.))
                .flex()
                .items_center()
                .child(Self::add_text_button(
                    "add-server-back",
                    "Back",
                    true,
                    cx,
                    |this, window, cx| this.add_server_step(Step::Pick, window, cx),
                ))
                .into_any_element(),
            Step::Customize => div()
                .flex_none()
                .px(px(24.))
                .py(px(16.))
                .h(px(72.))
                .flex()
                .items_center()
                .justify_between()
                .child(Self::add_text_button(
                    "add-server-back",
                    "Back",
                    false,
                    cx,
                    |this, window, cx| this.add_server_step(Step::Purpose, window, cx),
                ))
                .child(Self::add_primary(
                    "add-server-create",
                    "Create",
                    100.,
                    cx,
                    |this, window, cx| this.add_server_submit(window, cx),
                ))
                .into_any_element(),
            Step::Join => div()
                .flex_none()
                .px(px(24.))
                .py(px(16.))
                .h(px(72.))
                .flex()
                .items_center()
                .justify_between()
                .child(Self::add_text_button(
                    "add-server-back",
                    "Back",
                    true,
                    cx,
                    |this, window, cx| this.add_server_step(Step::Pick, window, cx),
                ))
                .child(Self::add_primary(
                    "add-server-join-go",
                    "Join Server",
                    110.,
                    cx,
                    |this, window, cx| this.add_server_submit(window, cx),
                ))
                .into_any_element(),
        };
        let modal = div()
            .parity_with_border("add-server", 1.)
            .id("add-server")
            .occlude()
            .w(px(444.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(body)
            .child(footer);
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose = crate::ui::motion::legacy_modal_pose("add-server", "add-server-layer", false);
        Some(
            deferred(
                div()
                    .id("add-server-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.add_server = None;
                        cx.notify();
                    }))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The birthday step of module 109766 in this dialog's shell (no close button on that step: `O !== AGE_GATE`).
    /// GAP: the official step is a slide of the 680px NUF modal (`modal_3h-1-Q`), forced to the light theme, with
    /// the step illustration in its sidebar (`es`); only the 440 by 464 slide (`container_3h-1-Q`) is drawn here, in
    /// the current theme. No DOM capture of it exists.
    fn add_server_age_gate_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let modal = div()
            .parity_with_border("add-server-age-gate", 1.)
            .id("add-server-age-gate")
            .occlude()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .overflow_hidden()
            .child(self.birthday_nuf_content(cx));
        let pose = crate::ui::motion::legacy_modal_pose("add-server", "add-server-layer", false);
        deferred(
            div()
                .id("add-server-layer")
                .occlude()
                .absolute()
                .inset_0()
                .bg(pose.dim(rgba(0x000000b8)))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.add_server = None;
                    cx.notify();
                }))
                .child(modal.posed(pose)),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_become_the_guild_request_of_module_943457() {
        let body = create_body(&TEMPLATES[0], "fixture throwaway", None);
        assert_eq!(body["name"], "fixture throwaway");
        assert_eq!(body["guild_template_code"], "2TffvPucqHkN");
        assert_eq!(body["channels"], json!([]));
        assert!(body["system_channel_id"].is_null());
        assert!(body["icon"].is_null());
        // Module 356343: a picked icon goes as the data URL the image input read.
        let uri = icon_data_url("PNG", &[1, 2, 3]).unwrap();
        assert_eq!(uri, "data:image/png;base64,AQID");
        assert_eq!(
            icon_data_url("jfif", &[0]).unwrap(),
            "data:image/jpeg;base64,AA=="
        );
        assert!(icon_data_url("bmp", &[0]).is_none());
        let with_icon = create_body(&TEMPLATES[0], "fixture throwaway", Some(&uri));
        assert_eq!(with_icon["icon"], uri.as_str());
        assert_eq!(dc_intl::t("3UB9ad"), "Upload");
        assert_eq!(dc_intl::t("DWB2YZ"), "Upload a Server Icon");
        let gaming = create_body(&TEMPLATES[1], "g", None);
        assert_eq!(gaming["guild_template_code"], "hvtBQMfw6uSJ");
        assert_eq!(gaming["system_channel_id"], "01");
        let channels = gaming["channels"].as_array().unwrap();
        assert_eq!(channels.len(), 6);
        assert_eq!(
            channels[1],
            json!({"id": "01", "parent_id": "00", "name": "general", "type": 0})
        );
        assert!(channels[0]["parent_id"].is_null());
        assert_eq!(channels[0]["type"], 4);
        assert_eq!(channels[4]["type"], 2);
    }

    #[test]
    fn invites_are_read_from_codes_and_links() {
        assert_eq!(invite_code("hTKzmak").as_deref(), Some("hTKzmak"));
        assert_eq!(
            invite_code(" https://discord.gg/hTKzmak ").as_deref(),
            Some("hTKzmak")
        );
        assert_eq!(
            invite_code("https://discord.gg/wumpus-friends").as_deref(),
            Some("wumpus-friends")
        );
        assert_eq!(
            invite_code("discord.com/invite/abc123").as_deref(),
            Some("abc123")
        );
        assert_eq!(invite_code(""), None);
        assert_eq!(invite_code("not an invite!"), None);
    }

    #[test]
    fn the_list_keeps_the_official_order() {
        let labels: Vec<&str> = TEMPLATES.iter().map(|template| template.label).collect();
        assert_eq!(
            labels,
            [
                "Create My Own",
                "Gaming",
                "Friends",
                "Study Group",
                "School Club",
                "Local Community",
                "Artists & Creators"
            ]
        );
    }
}
