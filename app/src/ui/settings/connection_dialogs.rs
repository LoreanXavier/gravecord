//! The dialogs the connection buttons open before anything leaves the app (module 80320 `d`).
//!
//! - Crunchyroll, Xbox and PlayStation Network: a 408 wide info dialog (modules 95895, 35352, 158375 over the shared
//!   landing slide 458172). Continue goes to the pre-connect slide (873885, "Step 1 of 2"), whose button starts
//!   `GET /connections/{type}/authorize` and opens the address it answers; the waiting slide follows. The callback that
//!   would finish the link (`USER_CONNECTIONS_LINK_CALLBACK`) is not wired, so the dialog stays on "Waiting for
//!   connection...".
//! - Bluesky (module 343946): Handle form, Next sends the authorize request with `handle=`.
//! - Domain (module 683286): Domain form; Next is `POST /users/@me/connections/domain/{domain}`, a `proof` in the
//!   error answer opens the DNS / HTTPS proof slide whose Verify sends the same request again.
//!
//! Geometry of the landing slides is from `design/dom/settings/connect-{xbox,playstation,crunchyroll}-dialog-darker-cozy.json`
//! and the forms' from the `connect-{bluesky,domain}-dialog` captures; colours are the `.valueProps_N-Y7ZJ` rules of
//! `72338.*.css`. Strings are the bundle's (`design/snapshot/dumps/STRINGS/en-US.json`).

use super::{Root, mask_icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::{color, editor_style, modal};
use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    SharedString, StyledText, Subscription, Window, deferred, div, prelude::*, px, rgba,
};
use serde_json::{Value, json};

/// The answer key of the Domain dialog's request.
pub(super) const DOMAIN_KEY: &str = "settings-connection-domain";

/// The slides of an info dialog (`52643.N`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum Slide {
    Landing,
    Connect,
    Waiting,
    Error,
}

pub(in crate::ui) struct Info {
    kind: &'static str,
    slide: Slide,
}

pub(in crate::ui) struct Handle {
    kind: &'static str,
    editor: Entity<Editor>,
    _sub: Subscription,
    error: Option<String>,
    working: bool,
    focused: bool,
    focus_pending: bool,
}

pub(in crate::ui) struct Domain {
    editor: Entity<Editor>,
    _sub: Subscription,
    error: Option<String>,
    working: bool,
    focused: bool,
    focus_pending: bool,
    /// The proof the server asked for (`body.proof`); set once the proof slide shows.
    proof: Option<String>,
    /// The proof slide's method: DNS record (true) or a file on the website.
    dns: bool,
}

pub(in crate::ui) enum Dlg {
    Info(Info),
    Handle(Handle),
    Domain(Domain),
}

/// What a landing slide shows: the strings and art of each module (`Da+3NJ`, `m8aahn`, `xAWHOy`, ...).
struct Landing {
    title: &'static str,
    body: &'static str,
    /// The "Learn more" article id inside the body, or the separate paragraph's.
    article: &'static str,
    inline_link: bool,
    art: (&'static str, &'static str, f32, f32),
    props: &'static [(&'static str, &'static str, Option<&'static str>)],
    /// The pre-connect slide: art, title, body, and the "You will be redirected to" address.
    connect: (
        (&'static str, &'static str, f32, f32),
        &'static str,
        &'static str,
    ),
    waiting: (
        (&'static str, &'static str, f32, f32),
        &'static str,
        &'static str,
    ),
    redirect: Option<&'static str>,
    error: (&'static str, &'static str),
}

const HELP: &str = "https://support.discord.com/hc/en-us/articles/";

fn landing(kind: &str) -> Landing {
    match kind {
        "crunchyroll" => Landing {
            title: "Crunchyroll is now on Discord!",
            body: "It\u{2019}s anime time! Sign in to Crunchyroll to connect your accounts.",
            article: "9929188048023",
            inline_link: false,
            art: (
                "conn-crunchyroll",
                "https://discord.com/assets/bb42d0ee2b7d1545.png",
                234.,
                152.,
            ),
            props: &[(
                "conn_tv",
                "Display the Crunchyroll show you\u{2019}re watching on Discord.",
                None,
            )],
            connect: (
                (
                    "conn-crunchyroll-connect",
                    "https://discord.com/assets/8bd88f3ff4b731d0.png",
                    152.,
                    123.,
                ),
                "First let's sign in with Crunchyroll",
                "We\u{2019}ll use this to display what show you\u{2019}re watching on Crunchyroll",
            ),
            waiting: (
                (
                    "conn-crunchyroll-waiting",
                    "https://discord.com/assets/bc91209df760be4e.png",
                    168.,
                    124.,
                ),
                "Waiting for connection...",
                "Sign in to Crunchyroll in your browser and return here when you're done.",
            ),
            redirect: Some("https://crunchyroll.com"),
            error: (
                "Oops, something went wrong...",
                "We were unable to link your Crunchyroll account. Please try again or contact support if the issue persists.",
            ),
        },
        "xbox" => Landing {
            title: "Link your Xbox account",
            body: "Sign in to your Xbox account to start using Discord on your Xbox. ",
            article: "360003953831",
            inline_link: true,
            art: (
                "conn-xbox",
                "https://cdn.discordapp.com/assets/content/87626cb25be78ee682ab4ee85e63b0680a5d90f8a5e6f8d469ffb6889d7959e3.png",
                230.,
                160.,
            ),
            props: &[
                ("conn_voice", "Join voice calls directly from Xbox", None),
                (
                    "conn_screen_stream",
                    "Stream and watch streams on Xbox",
                    None,
                ),
                (
                    "conn_group",
                    "See what Discord friends are currently doing on your Xbox",
                    None,
                ),
                (
                    "conn_gamepad",
                    "Display the Xbox game you're playing on Discord",
                    None,
                ),
            ],
            connect: (
                (
                    "conn-xbox-connect",
                    "https://cdn.discordapp.com/assets/content/abb7826c12c091e08c0ad121b2a6ab48fce6ffd2123f1219b8ef58399908f3ff.png",
                    231.,
                    160.,
                ),
                "Sign in with Xbox",
                "You'll be redirected to the official Xbox sign in page. Log in and click \u{201c}accept.\u{201d}",
            ),
            waiting: (
                (
                    "conn-xbox-waiting",
                    "https://cdn.discordapp.com/assets/content/2cf087a7f12e3d1d09c0c9416169f2b67f08a27a8642aef5635519900f3e904b.png",
                    231.,
                    172.,
                ),
                "Waiting for connection...",
                "Sign in to Xbox in your browser and return here when you're done.",
            ),
            redirect: None,
            error: (
                "Oops, something went wrong...",
                "We were unable to link your Xbox account. Please try again or contact support if the issue persists.",
            ),
        },
        _ => Landing {
            title: "Link your PlayStation Network account",
            body: "Sign in to your PlayStation Network account to start using Discord on your PlayStation. ",
            article: "4419534960919",
            inline_link: true,
            art: (
                "conn-playstation",
                "https://cdn.discordapp.com/assets/content/09152e47812b0b36abe406a49de1cc71ebae44cd025b7f7ce471ecf1bd152285.png",
                230.,
                160.,
            ),
            props: &[
                (
                    "conn_voice",
                    "Join voice calls directly from PlayStation",
                    Some("Only available on PS5"),
                ),
                (
                    "conn_gamepad",
                    "Display the game you're playing on Discord",
                    None,
                ),
            ],
            connect: (
                (
                    "conn-playstation-connect",
                    "https://cdn.discordapp.com/assets/content/e204d2bb762737c619e51cd81d31c6e7fd615fa2ddd459c52ce5fb52a86d44e8.png",
                    231.,
                    160.,
                ),
                "Sign in with PlayStation Network",
                "You'll be redirected to the official PlayStation Network sign in page. Log in and click \u{201c}accept.\u{201d}",
            ),
            waiting: (
                (
                    "conn-playstation-waiting",
                    "https://cdn.discordapp.com/assets/content/0784339e68320272c6863d3ceb5170eba05ce95a3b5e362086da529fc3f8bfff.png",
                    231.,
                    172.,
                ),
                "Waiting for connection...",
                "Sign in to PlayStation Network in your browser and return here when you're done",
            ),
            redirect: None,
            error: (
                "Oops, something went wrong...",
                "We were unable to link your PlayStation Network account. Please try again or contact support if the issue persists.",
            ),
        },
    }
}

/// Every picture the info dialogs draw, as `(name, url)`: requested when a dialog opens.
fn art_of(kind: &str) -> Vec<(&'static str, &'static str)> {
    let l = landing(kind);
    vec![
        (l.art.0, l.art.1),
        (l.connect.0.0, l.connect.0.1),
        (l.waiting.0.0, l.waiting.0.1),
    ]
}

/// The platform's name in the forms' strings (`ImMhq+`, `7TByKh`).
fn service_name(kind: &str) -> &'static str {
    match kind {
        "bluesky" => "Bluesky",
        "mastodon" => "Mastodon",
        _ => "Account",
    }
}

/// `/^.+\.[^.@]{2,}$/` (module 343946): something, a dot, and an ending of two or more characters without a dot or an
/// at sign.
fn handle_is_valid(text: &str) -> bool {
    match text.rsplit_once('.') {
        Some((before, after)) => {
            !before.is_empty()
                && after.chars().count() >= 2
                && !after.contains('@')
                && !text.contains('\n')
        }
        None => false,
    }
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn new_editor(
    placeholder: &str,
    max: usize,
    cx: &mut Context<Root>,
) -> (Entity<Editor>, Subscription) {
    let editor = cx.new(|cx| {
        let mut editor = Editor::new(editor_style(), cx);
        editor.set_single_line(true);
        editor.set_keeps_text(true);
        editor.set_placeholder(placeholder.to_owned());
        editor
    });
    let _ = max;
    let sub = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
        match event {
            EditorEvent::Submit(_) => this.connection_dialog_next(cx),
            EditorEvent::Cancel => this.settings_extra.conn = None,
            EditorEvent::Edited => {
                // Typing clears the field's error (module 95477 keeps it until the next try; the official forms
                // rebuild their `error` only on submit), so nothing else changes here.
            }
        }
        cx.notify();
    });
    (editor, sub)
}

impl Root {
    /// A connection button whose first step is a dialog of its own (module 80320).
    pub(in crate::ui) fn open_connection_dialog(
        &mut self,
        kind: &'static str,
        cx: &mut Context<Self>,
    ) {
        match kind {
            "bluesky" => {
                let (editor, sub) = new_editor("example.bsky.social", 0, cx);
                self.settings_extra.conn = Some(Dlg::Handle(Handle {
                    kind,
                    editor,
                    _sub: sub,
                    error: None,
                    working: false,
                    focused: false,
                    focus_pending: true,
                }));
            }
            "domain" => {
                let (editor, sub) = new_editor("example.com", 253, cx);
                self.settings_extra.conn = Some(Dlg::Domain(Domain {
                    editor,
                    _sub: sub,
                    error: None,
                    working: false,
                    focused: false,
                    focus_pending: false,
                    proof: None,
                    dns: true,
                }));
            }
            _ => {
                for (name, url) in art_of(kind) {
                    self.request_image(crate::ui::nitro::art_key(name), url.to_owned());
                }
                self.settings_extra.conn = Some(Dlg::Info(Info {
                    kind,
                    slide: Slide::Landing,
                }));
            }
        }
        cx.notify();
    }

    /// Render hook: the box takes the focus once, and the active border follows it.
    pub(in crate::ui) fn connection_dialog_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.gift_focus(window, cx);
        let (editor, focused, pending) = match self.settings_extra.conn.as_mut() {
            Some(Dlg::Handle(form)) => (
                form.editor.clone(),
                &mut form.focused,
                std::mem::take(&mut form.focus_pending),
            ),
            Some(Dlg::Domain(form)) if form.proof.is_none() => (
                form.editor.clone(),
                &mut form.focused,
                std::mem::take(&mut form.focus_pending),
            ),
            _ => return,
        };
        let handle = gpui::Focusable::focus_handle(editor.read(cx), cx);
        if pending {
            window.focus(&handle, cx);
        }
        let now = handle.is_focused(window);
        if now != *focused {
            *focused = now;
            cx.notify();
        }
    }

    /// Enter in a box, or the Next button.
    pub(in crate::ui) fn connection_dialog_next(&mut self, cx: &mut Context<Self>) {
        match self.settings_extra.conn.as_mut() {
            Some(Dlg::Handle(form)) => {
                let text = form.editor.read(cx).text().to_owned();
                if form.working || !handle_is_valid(&text) {
                    return;
                }
                form.working = true;
                form.error = None;
                let kind = form.kind;
                let route = format!(
                    "/connections/{kind}/authorize?handle={}",
                    percent_encode(&text)
                );
                self.connection_authorize_route(kind, route, cx);
            }
            Some(Dlg::Domain(form)) => {
                if form.working {
                    return;
                }
                let domain = form.editor.read(cx).text().to_owned();
                if form.proof.is_none() && domain.is_empty() {
                    form.error = Some("This field is required.".to_owned());
                    return;
                }
                form.working = true;
                form.error = None;
                let route = format!("/users/@me/connections/domain/{domain}");
                if crate::storage::is_capture_run() {
                    // The request creates a verification at the server; a harness run only notes it.
                    tracing::info!("connection domain: POST {route} not sent in a harness run");
                    form.working = false;
                    self.settings_extra.last_url = Some(format!("POST {route}"));
                    self.settings_extra.conn = None;
                    cx.notify();
                    return;
                }
                let _ = self.requests.send(Request::Rest {
                    key: Some(DOMAIN_KEY.to_owned()),
                    method: reqwest::Method::POST,
                    route,
                    body: Some(json!({})),
                });
            }
            _ => {}
        }
        cx.notify();
    }

    /// The Domain request's answer (`onNext` of the two steps).
    pub(super) fn connection_domain_answered(&mut self, body: &Value, cx: &mut Context<Self>) {
        let Some(Dlg::Domain(form)) = self.settings_extra.conn.as_mut() else {
            return;
        };
        form.working = false;
        if body["ok"].as_bool() == Some(true) {
            // `n()`: the dialog closes, the list refreshes.
            self.settings_extra.conn = None;
            self.settings_extra.connections_asked = false;
            cx.notify();
            return;
        }
        let answer = &body["body"];
        if let Some(proof) = answer["proof"].as_str() {
            form.proof = Some(proof.to_owned());
            form.error = None;
        } else {
            // `v(t)`: the domain field's first error, else the message.
            form.error = Some(
                answer["errors"]["domain"]["_errors"][0]["message"]
                    .as_str()
                    .or(answer["message"].as_str())
                    .unwrap_or("An unknown error occurred.")
                    .to_owned(),
            );
        }
        cx.notify();
    }

    /// `GET route` and open the address it answers (`738104`); `kind` names the answer.
    pub(super) fn connection_authorize_route(
        &mut self,
        kind: &'static str,
        route: String,
        cx: &mut Context<Self>,
    ) {
        if crate::storage::is_capture_run() {
            // The route creates an authorization state at the server; a harness run never starts one.
            tracing::info!("connection {kind}: hand-off skipped, GET {route} then the browser");
            self.settings_extra.last_url = Some(route);
            self.connection_authorized(true, cx);
            return;
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{}{kind}", super::connections::AUTHORIZE_KEY),
            route,
        });
    }

    /// The authorize request ended: the info dialog moves on (waiting, or the error slide); the Bluesky form closes,
    /// or shows `7wbPNl` under its box.
    pub(super) fn connection_authorized(&mut self, ok: bool, cx: &mut Context<Self>) {
        match self.settings_extra.conn.as_mut() {
            Some(Dlg::Info(info)) => {
                info.slide = if ok { Slide::Waiting } else { Slide::Error };
            }
            Some(Dlg::Handle(form)) => {
                form.working = false;
                if ok {
                    self.settings_extra.conn = None;
                } else {
                    form.error =
                        Some("An unknown error occurred. Maybe check your handle?".to_owned());
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// The dialog, drawn over the settings layer.
    pub(super) fn connection_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        match self.settings_extra.conn.as_ref()? {
            Dlg::Info(info) => Some(self.info_dialog(info, cx)),
            Dlg::Handle(form) => Some(self.handle_dialog(form, cx)),
            Dlg::Domain(form) => Some(self.domain_dialog(form, cx)),
        }
    }

    fn close_connection_dialog(&mut self) {
        self.settings_extra.conn = None;
    }

    fn field_box(
        &self,
        label: &'static str,
        required: bool,
        editor: &Entity<Editor>,
        focused: bool,
        error: Option<&str>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = if error.is_some() {
            color("--border-feedback-critical")
        } else if focused {
            color("--input-border-active")
        } else {
            color("--input-border-default")
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label)
                    .when(required, |this| {
                        this.child(
                            div()
                                .text_color(color("--text-feedback-critical"))
                                .child("*"),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .when(focused && error.is_none(), |this| {
                        this.relative().child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(7.))
                                .border_1()
                                .border_color(color("--input-border-active")),
                        )
                    })
                    .child(div().flex_1().min_w(px(0.)).child(editor.clone())),
            )
            .children(error.map(|message| {
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message.to_owned())
            }))
            .into_any_element()
    }

    /// Module 343946: the Handle form.
    fn handle_dialog(&self, form: &Handle, cx: &mut Context<Self>) -> AnyElement {
        let name = service_name(form.kind);
        let text = form.editor.read(cx).text().to_owned();
        let valid = handle_is_valid(&text);
        let body = self.field_box(
            "Handle",
            false,
            &form.editor,
            form.focused,
            form.error.as_deref(),
            cx,
        );
        modal::render(
            modal::Spec {
                id: "settings-connect-handle",
                title: format!("Connect your {name} Account").into(),
                subtitle: Some(
                    div()
                        .child(format!(
                            "Verify your {name} account and show it off on your profile!"
                        ))
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "settings-connect-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.close_connection_dialog(),
                    ),
                    modal::button(
                        "settings-connect-next",
                        "Next",
                        if !valid {
                            modal::Tone::PrimaryDisabled
                        } else if form.working {
                            modal::Tone::PrimaryLoading
                        } else {
                            modal::Tone::Primary
                        },
                        |this, _, cx| this.connection_dialog_next(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.close_connection_dialog()),
            },
            cx,
        )
    }

    /// Module 683286: the Domain form and its proof slide.
    fn domain_dialog(&self, form: &Domain, cx: &mut Context<Self>) -> AnyElement {
        let title = "Connect your Domain";
        let Some(proof) = form.proof.clone() else {
            let body = self.field_box(
                "Domain",
                true,
                &form.editor,
                form.focused,
                form.error.as_deref(),
                cx,
            );
            return modal::render(
                modal::Spec {
                    id: "settings-connect-domain",
                    title: title.into(),
                    subtitle: Some(
                        div()
                            .child(
                                "Verify ownership of your domain and show it off on your profile!",
                            )
                            .into_any_element(),
                    ),
                    body: Some(body),
                    buttons: vec![
                        modal::button(
                            "settings-connect-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, _| this.close_connection_dialog(),
                        ),
                        modal::button(
                            "settings-connect-next",
                            "Next",
                            if form.working {
                                modal::Tone::PrimaryLoading
                            } else {
                                modal::Tone::Primary
                            },
                            |this, _, cx| this.connection_dialog_next(cx),
                        ),
                    ],
                    close: Box::new(|this, _, _| this.close_connection_dialog()),
                },
                cx,
            );
        };
        let domain = form.editor.read(cx).text().to_owned();
        let text = |value: String| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(value)
        };
        let copy_row = |name: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(name.to_uppercase()),
                )
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
                        .text_size(px(16.))
                        .text_color(color("--text-default"))
                        .child(value),
                )
        };
        let dns = form.dns;
        let content = if dns {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(text("1. Sign in to your domain name provider".to_owned()))
                .child(text("2. Create a new DNS record:".to_owned()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(copy_row("Name", format!("_discord.{domain}")))
                        .child(copy_row("Type", "TXT".to_owned()))
                        .child(copy_row("Content", proof)),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child("It may take a few minutes for the DNS record to propagate."),
                )
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(text("Create a new file on your website:".to_owned()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(copy_row(
                            "Name",
                            format!("https://{domain}/.well-known/discord"),
                        ))
                        .child(copy_row("Content", proof)),
                )
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(form.error.as_ref().map(|message| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message.clone())
            }))
            .child(content)
            .into_any_element();
        modal::render(
            modal::Spec {
                id: "settings-connect-domain-proof",
                title: title.into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "settings-connect-method",
                        if dns {
                            "Verify using HTTPS"
                        } else {
                            "Verify using DNS"
                        },
                        modal::Tone::Secondary,
                        |this, _, cx| {
                            if let Some(Dlg::Domain(form)) = this.settings_extra.conn.as_mut() {
                                form.dns = !form.dns;
                                form.error = None;
                            }
                            cx.notify();
                        },
                    ),
                    modal::button(
                        "settings-connect-verify",
                        "Verify",
                        if form.working {
                            modal::Tone::PrimaryLoading
                        } else {
                            modal::Tone::Primary
                        },
                        |this, _, cx| this.connection_dialog_next(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.close_connection_dialog()),
            },
            cx,
        )
    }

    /// The body line with an inline "Learn more" link (`kqZQNe`, `CIc3IN`: the link is the sentence's last word).
    fn connect_learn_more(
        &self,
        before: &'static str,
        article: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = format!("{before}Learn more.");
        let start = before.len();
        let end = start + "Learn more".len();
        let styled = StyledText::new(shown).with_highlights([(
            start..end,
            HighlightStyle {
                color: Some(color("--text-link").into()),
                ..HighlightStyle::default()
            },
        )]);
        let root = cx.weak_entity();
        let range = start..end;
        InteractiveText::new(SharedString::from("connect-learn-more"), styled)
            .on_click(vec![range], move |_, _, cx| {
                let _ = root.update(cx, |this, cx| {
                    this.settings_open_url(&format!("{HELP}{article}"), cx);
                    cx.notify();
                });
            })
            .into_any_element()
    }

    /// The landing, pre-connect, waiting and error slides of a platform's info dialog.
    fn info_dialog(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let kind = info.kind;
        let data = landing(kind);
        // Crunchyroll, PlayStation and Xbox are on the legacy modal root (`root__KoJTP` in
        // design/dom/settings/connect-{crunchyroll,playstation,xbox}-dialog, `EO` in module 935462).
        let pose = crate::ui::motion::legacy_modal_pose(
            "settings-connect-info",
            "settings-connect-info-layer",
            false,
        );
        // The links are built before the button closure below borrows `cx`.
        let mut inline_link = Some(if data.inline_link {
            self.connect_learn_more(data.body, data.article, cx)
        } else {
            div().into_any_element()
        });
        let mut separate_link = Some(self.connect_learn_more("", data.article, cx));
        let primary =
            |id: &'static str, label: &'static str, run: fn(&mut Root, &mut Context<Root>)| {
                div()
                    .id(id)
                    .h(px(40.))
                    .px(px(17.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-primary-border-default"))
                    .bg(color("--control-primary-background-default"))
                    .hover(|style| style.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        run(this, cx);
                        cx.notify();
                    }))
            };
        let art = |spec: (&'static str, &'static str, f32, f32), this: &Root| {
            div()
                .h(px(spec.3 + 3.))
                .mb(px(16.))
                .flex()
                .justify_center()
                .child(this.nitro_art(spec.0, spec.2, spec.3))
        };
        let title = |value: &'static str| {
            div()
                .text_size(px(24.))
                .line_height(px(32.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .text_center()
                .child(value)
        };
        let paragraph = |value: &'static str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .text_center()
                .child(value)
        };
        let (header, content, footer): (AnyElement, AnyElement, AnyElement) = match info.slide {
            Slide::Landing => {
                let rows = data.props.len();
                let props = data
                    .props
                    .iter()
                    .enumerate()
                    .map(|(index, (glyph, label, sub))| {
                        div()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .p(px(12.))
                                    .child(
                                        div()
                                            .size(px(32.))
                                            .p(px(4.))
                                            .mr(px(12.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(mask_icon(
                                                glyph,
                                                24.,
                                                "--interactive-text-default",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-default"))
                                                    .child(*label),
                                            )
                                            .children(sub.map(|sub| {
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-subtle"))
                                                    .child(sub)
                                            })),
                                    ),
                            )
                            .when(index + 1 != rows, |this| {
                                this.child(
                                    div()
                                        .h(px(1.))
                                        .ml(px(56.))
                                        .mr(px(12.))
                                        .bg(color("--border-subtle")),
                                )
                            })
                    });
                let body = if data.inline_link {
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .text_center()
                        .children(inline_link.take())
                        .into_any_element()
                } else {
                    paragraph(data.body).into_any_element()
                };
                (
                    div()
                        .child(art(data.art, self))
                        .child(title(data.title))
                        .into_any_element(),
                    div()
                        .flex()
                        .flex_col()
                        .child(body)
                        .child(
                            div()
                                .mt(px(24.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lower"))
                                .children(props),
                        )
                        .when(!data.inline_link, |this| {
                            this.child(
                                div()
                                    .mt(px(16.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .text_center()
                                    .children(separate_link.take()),
                            )
                        })
                        .into_any_element(),
                    primary("settings-connect-continue", "Continue", |this, cx| {
                        if let Some(Dlg::Info(info)) = this.settings_extra.conn.as_mut() {
                            info.slide = Slide::Connect;
                        }
                        cx.notify();
                    })
                    .w(px(100.))
                    .into_any_element(),
                )
            }
            slide => {
                let (spec, heading, text) = match slide {
                    Slide::Connect => data.connect,
                    Slide::Waiting => data.waiting,
                    _ => (data.art, data.error.0, data.error.1),
                };
                let waiting = slide == Slide::Waiting;
                let label = match slide {
                    Slide::Waiting => "Retry",
                    Slide::Error => "Continue",
                    _ => "Continue",
                };
                let start: fn(&mut Root, &mut Context<Root>) = |this, cx| {
                    let Some(Dlg::Info(info)) = this.settings_extra.conn.as_mut() else {
                        return;
                    };
                    if info.slide == Slide::Error {
                        info.slide = Slide::Connect;
                        return;
                    }
                    let kind = info.kind;
                    this.connection_authorize_route(
                        kind,
                        format!("/connections/{kind}/authorize"),
                        cx,
                    );
                };
                (
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .when(slide != Slide::Error, |this| {
                            this.child(
                                div()
                                    .mb(px(8.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child("Step 1 of 2"),
                            )
                        })
                        .child(art(spec, self))
                        .child(title(heading))
                        .into_any_element(),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(paragraph(text))
                        .when(!waiting && slide == Slide::Connect, |this| {
                            this.children(data.redirect.map(|address| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .text_center()
                                    .child(format!("You will be redirected to: {address}"))
                            }))
                        })
                        .into_any_element(),
                    primary("settings-connect-start", label, start)
                        .w_full()
                        // `variant: n ? "secondary" : "primary"` (module 873885): Retry is the secondary button.
                        .when(waiting, |this| {
                            this.border_color(color("--border-muted"))
                                .bg(color("--control-secondary-background-default"))
                                .text_color(color("--control-secondary-text-default"))
                                .hover(|style| {
                                    style.bg(color("--control-secondary-background-hover"))
                                })
                        })
                        .into_any_element(),
                )
            }
        };
        let modal = div()
            .id("settings-connect-info")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .relative()
            .w(px(410.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(div().pt(px(24.)).px(px(16.)).child(header))
            .child(div().px(px(24.)).py(px(16.)).child(content))
            .child(
                div()
                    .h(px(72.))
                    .px(px(24.))
                    .flex()
                    .items_center()
                    .bg(color("--background-base-lowest"))
                    .child(footer),
            )
            .child(
                div()
                    .id("settings-connect-info-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(mask_icon(
                        "modal_close",
                        20.,
                        "--control-icon-only-icon-default",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_connection_dialog();
                        cx.notify();
                    })),
            );
        deferred(
            div()
                .id("settings-connect-info-layer")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::Rgba {
                    a: f32::from(0xb8u8) / 255. * pose.scrim,
                    ..rgba(0x000000ff)
                })
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_connection_dialog();
                    cx.notify();
                }))
                .child(modal),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::handle_is_valid;

    #[test]
    fn handle_needs_a_dotted_ending() {
        assert!(handle_is_valid("example.bsky.social"));
        assert!(handle_is_valid("a.bc"));
        assert!(!handle_is_valid(""));
        assert!(!handle_is_valid("example"));
        assert!(!handle_is_valid("example.b"));
        assert!(!handle_is_valid(".com"));
        assert!(!handle_is_valid("a.b@c"));
    }
}
