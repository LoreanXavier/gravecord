//! The dialogs the connection buttons open before anything leaves the app (module 80320 `d`).
//!
//! - Crunchyroll, Xbox and PlayStation Network: a 408 wide info dialog (modules 95895, 35352, 158375 over the shared
//!   landing slide 458172, the slides of 52643 run by 670404). Continue goes to the pre-connect slide (873885, "Step 1
//!   of 2"), whose button asks for a two-way link and opens the address it answers; the waiting slide follows until the
//!   gateway's `USER_CONNECTIONS_LINK_CALLBACK`, then the Discord consent slide (910316, "Step 2 of 2") and the success
//!   slide (162185). The requests are in `connection_link.rs`. The landing title offers a relink when the account is
//!   linked one way (`twoWayLink === false`).
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Slide {
    Landing,
    Connect,
    Waiting,
    /// `DISCORD_CONSENT` (module 910316).
    Consent,
    Error,
    Success,
}

pub(in crate::ui) struct Info {
    pub(super) kind: &'static str,
    pub(super) slide: Slide,
    /// `expectedCallbackState`: the `state` of the authorize address the browser opened.
    pub(super) expected: Option<String>,
    /// `callbackData`: the link callback's code and state.
    pub(super) callback: Option<(String, String)>,
    /// `errorCode` of the last failure.
    pub(super) error_code: Option<i64>,
    pub(super) consent: Box<super::connection_link::Consent>,
}

impl Info {
    /// `gotoError(code)` of module 670404.
    pub(super) fn go_error(&mut self, code: Option<i64>) {
        self.expected = None;
        self.error_code = code;
        self.slide = Slide::Error;
    }
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
    /// Module 58829: Add Connection modal (`syl6HS`, `PHjkRE` Apps, `y3ZnnU` Claimed Games).
    AddConnection,
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
            title: dc_intl::tr("Crunchyroll is now on Discord!"),
            body: dc_intl::tr(
                "It\u{2019}s anime time! Sign in to Crunchyroll to connect your accounts.",
            ),
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
                dc_intl::tr("First let's sign in with Crunchyroll"),
                dc_intl::tr(
                    "We\u{2019}ll use this to display what show you\u{2019}re watching on Crunchyroll",
                ),
            ),
            waiting: (
                (
                    "conn-crunchyroll-waiting",
                    "https://discord.com/assets/bc91209df760be4e.png",
                    168.,
                    124.,
                ),
                dc_intl::tr("Waiting for connection..."),
                dc_intl::tr(
                    "Sign in to Crunchyroll in your browser and return here when you're done.",
                ),
            ),
            redirect: Some("https://crunchyroll.com"),
            error: (
                dc_intl::tr("Oops, something went wrong..."),
                dc_intl::tr(
                    "We were unable to link your Crunchyroll account. Please try again or contact support if the issue persists.",
                ),
            ),
        },
        "xbox" => Landing {
            title: dc_intl::tr("Link your Xbox account"),
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
                dc_intl::tr("Sign in with Xbox"),
                dc_intl::tr(
                    "You'll be redirected to the official Xbox sign in page. Log in and click \u{201c}accept.\u{201d}",
                ),
            ),
            waiting: (
                (
                    "conn-xbox-waiting",
                    "https://cdn.discordapp.com/assets/content/2cf087a7f12e3d1d09c0c9416169f2b67f08a27a8642aef5635519900f3e904b.png",
                    231.,
                    172.,
                ),
                dc_intl::tr("Waiting for connection..."),
                dc_intl::tr("Sign in to Xbox in your browser and return here when you're done."),
            ),
            redirect: None,
            error: (
                dc_intl::tr("Oops, something went wrong..."),
                dc_intl::tr(
                    "We were unable to link your Xbox account. Please try again or contact support if the issue persists.",
                ),
            ),
        },
        _ => Landing {
            title: dc_intl::tr("Link your PlayStation Network account"),
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
                dc_intl::tr("Sign in with PlayStation Network"),
                dc_intl::tr(
                    "You'll be redirected to the official PlayStation Network sign in page. Log in and click \u{201c}accept.\u{201d}",
                ),
            ),
            waiting: (
                (
                    "conn-playstation-waiting",
                    "https://cdn.discordapp.com/assets/content/0784339e68320272c6863d3ceb5170eba05ce95a3b5e362086da529fc3f8bfff.png",
                    231.,
                    172.,
                ),
                dc_intl::tr("Waiting for connection..."),
                dc_intl::tr(
                    "Sign in to PlayStation Network in your browser and return here when you're done",
                ),
            ),
            redirect: None,
            error: (
                dc_intl::tr("Oops, something went wrong..."),
                "We were unable to link your PlayStation Network account. Please try again or contact support if the issue persists.",
            ),
        },
    }
}

/// The error slide's picture (module 478474: `illustration_N-Y7ZJ`, module 332633, 254 by 127).
const ERROR_ART: (&str, &str, f32, f32) = (
    "conn-error",
    "https://discord.com/assets/a37490430fba083d.png",
    254.,
    127.,
);

/// The success slides' pictures (module 326513 for the consoles, 124 by 160; Crunchyroll's 224 by 104).
const SUCCESS_ART: &[(&str, &str)] = &[
    (
        "conn-console-success",
        "https://cdn.discordapp.com/assets/content/78f63623b9a823fb08b553c956ad16d032c46e4375c55f374e1e6bc791b8a1a7.png",
    ),
    (
        "conn-crunchyroll-success",
        "https://discord.com/assets/f57d187b12fee97a.png",
    ),
];

/// Xbox's `icon.whiteSVG` (module 278922), in the Get the Xbox App box.
const XBOX_WHITE: &str = "c4f09fda61827e19.svg";

/// The Xbox app's page the success slide's code points to (`EY` of module 544105).
const XBOX_APP_URL: &str = "https://www.xbox.com/en-US/apps/xbox-app-for-mobile";

/// Every picture the info dialogs draw, as `(name, url)`: requested when a dialog opens.
fn art_of(kind: &str) -> Vec<(&'static str, &'static str)> {
    let l = landing(kind);
    vec![
        (l.art.0, l.art.1),
        (l.connect.0.0, l.connect.0.1),
        (l.waiting.0.0, l.waiting.0.1),
        (ERROR_ART.0, ERROR_ART.1),
    ]
}

/// The platform's name in the forms' strings (`ImMhq+`, `7TByKh`).
fn service_name(kind: &str) -> &'static str {
    match kind {
        "bluesky" => dc_intl::tr("Bluesky"),
        "mastodon" => "Mastodon",
        _ => dc_intl::tr("Account"),
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
    /// Harness only: `connect-error <xbox|playstation|crunchyroll>` opens that dialog with its authorize request
    /// failed, as a refused `GET /connections/<type>/authorize` leaves it (the error slide of module 478474).
    #[cfg(feature = "capture")]
    pub(crate) fn script_connect_error(&mut self, kind: &str, cx: &mut Context<Self>) -> String {
        let kind: &'static str = match kind {
            "xbox" => "xbox",
            "playstation" => "playstation",
            "crunchyroll" => "crunchyroll",
            _ => return "usage: connect-error xbox|playstation|crunchyroll".to_owned(),
        };
        self.open_settings_page("connected-apps", cx);
        self.open_connection_dialog(kind, cx);
        self.connection_authorized(false, cx);
        format!("connect-error: {kind} on its error slide")
    }

    /// Harness only: `connect-open <xbox|playstation|crunchyroll>` opens Connections with that platform's dialog on
    /// its landing slide, as its connect button does.
    #[cfg(feature = "capture")]
    pub(crate) fn script_connect_open(&mut self, kind: &str, cx: &mut Context<Self>) -> String {
        let kind: &'static str = match kind {
            "xbox" => "xbox",
            "playstation" => "playstation",
            "crunchyroll" => "crunchyroll",
            _ => return "usage: connect-open xbox|playstation|crunchyroll".to_owned(),
        };
        self.open_settings_page("connected-apps", cx);
        self.open_connection_dialog(kind, cx);
        format!("connect-open: {kind} on its landing slide")
    }

    /// Harness only: the open info dialog's slide, its expected state and its last request.
    #[cfg(feature = "capture")]
    pub(crate) fn script_connect_state(&self) -> String {
        match self.settings_extra.conn.as_ref() {
            Some(Dlg::Info(info)) => format!(
                "connect-state: {} slide={:?} expected={:?} consent_loaded={} loading={} failed={:?} last={:?}",
                info.kind,
                info.slide,
                info.expected,
                info.consent.get.is_some(),
                info.consent.loading,
                info.consent.failed,
                self.settings_extra.last_url
            ),
            Some(_) => "connect-state: a form dialog".to_owned(),
            None => format!(
                "connect-state: closed last={:?}",
                self.settings_extra.last_url
            ),
        }
    }

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
                for (name, url) in SUCCESS_ART {
                    self.request_image(crate::ui::nitro::art_key(name), (*url).to_owned());
                }
                self.request_image(
                    crate::ui::platform_presence::asset_key(XBOX_WHITE),
                    crate::ui::platform_presence::asset_url(XBOX_WHITE),
                );
                self.settings_extra.conn = Some(Dlg::Info(Info {
                    kind,
                    slide: Slide::Landing,
                    expected: None,
                    callback: None,
                    error_code: None,
                    consent: Default::default(),
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
                    form.error = Some(dc_intl::tr("This field is required.").to_owned());
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
            // The route creates an authorization state at the server; a harness run never starts one. An info
            // dialog waits for its answer (`fetched settings-connection-authorize-<type> {"url": ...}`).
            tracing::info!("connection {kind}: hand-off skipped, GET {route} then the browser");
            self.settings_extra.last_url = Some(route);
            if !matches!(self.settings_extra.conn, Some(Dlg::Info(_))) {
                self.connection_authorized(true, cx);
            }
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
                if ok {
                    info.slide = Slide::Waiting;
                } else {
                    info.go_error(None);
                }
            }
            Some(Dlg::Handle(form)) => {
                form.working = false;
                if ok {
                    self.settings_extra.conn = None;
                } else {
                    form.error = Some(
                        dc_intl::tr("An unknown error occurred. Maybe check your handle?")
                            .to_owned(),
                    );
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
            Dlg::AddConnection => Some(self.add_connection_dialog(cx)),
        }
    }

    /// Capture helper for module 58829 (`add-connection open|close`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_add_connection(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.settings_extra.conn = None;
            cx.notify();
            return "add-connection: closed".to_owned();
        }
        self.settings_extra.conn = Some(Dlg::AddConnection);
        cx.notify();
        "add-connection: open".to_owned()
    }

    /// Renders Add Connection modal (module 58829: `syl6HS`, platform grid, `PHjkRE` Apps cards,
    /// `y3ZnnU` Claimed Games cards).
    fn add_connection_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        const PLATFORMS: [(&str, &str, u32); 8] = [
            ("spotify", "Spotify", 0x1db954),
            ("steam", "Steam", 0x171a21),
            ("twitch", "Twitch", 0x9146ff),
            ("youtube", "YouTube", 0xff0000),
            ("github", "GitHub", 0x24292e),
            ("xbox", "Xbox", 0x107c10),
            ("playstation", "PlayStation", 0x003791),
            ("crunchyroll", "Crunchyroll", 0xf47521),
        ];
        const APPS: [(&str, &str); 2] = [
            (
                "Linked Roles Bot",
                "Verify server roles with external accounts.",
            ),
            ("Creator Hub", "Connect your community storefront & perks."),
        ];
        const CLAIMED_GAMES: [(&str, &str); 2] = [
            ("League of Legends", "Display your rank and summoner stats."),
            ("Destiny 2", "Link your Bungie.net Guardian profile."),
        ];

        let card = div()
            .id("add-connection-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(620.))
            .max_h(px(620.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("syl6HS")),
                    )
                    .child(
                        div()
                            .id("add-connection-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_connection_dialog();
                                cx.notify();
                            }))
                            .child(mask_icon(
                                "modal_close",
                                18.,
                                "--control-icon-only-icon-default",
                            )),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        // Section 1: Platform & App connection buttons grid (`connectionsContainer_g9DJn9`).
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(10.))
                            .children(PLATFORMS.iter().map(|&(kind, name, col)| {
                                div()
                                    .id(SharedString::from(format!("add-conn-plat-{kind}")))
                                    .w(px(134.))
                                    .h(px(44.))
                                    .px(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().size(px(22.)).rounded(px(6.)).bg(gpui::rgb(col)))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.close_connection_dialog();
                                        if matches!(kind, "xbox" | "playstation" | "crunchyroll") {
                                            this.open_connection_dialog(kind, cx);
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        // Section 2: `PHjkRE` ("Apps") — 184px Role Connection Integrations cards.
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("PHjkRE")),
                            )
                            .child(div().flex().flex_wrap().gap(px(12.)).children(
                                APPS.iter().enumerate().map(|(i, &(name, desc))| {
                                    div()
                                        .id(SharedString::from(format!("add-conn-app-{i}")))
                                        .w(px(184.))
                                        .p(px(14.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .size(px(32.))
                                                        .rounded(px(8.))
                                                        .bg(color("--brand-500"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--white"))
                                                        .child(&name[..1]),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(name),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-muted"))
                                                .child(desc),
                                        )
                                }),
                            )),
                    )
                    .child(
                        // Section 3: `y3ZnnU` ("Claimed Games") — 184px identity game cards.
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("y3ZnnU")),
                            )
                            .child(div().flex().flex_wrap().gap(px(12.)).children(
                                CLAIMED_GAMES.iter().enumerate().map(|(i, &(name, desc))| {
                                    div()
                                        .id(SharedString::from(format!("add-conn-game-{i}")))
                                        .w(px(184.))
                                        .p(px(14.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .size(px(32.))
                                                        .rounded(px(8.))
                                                        .bg(color("--background-surface-higher"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(crate::ui::icon(
                                                            "activity_game",
                                                            18.,
                                                            "--text-strong",
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(name),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(color("--text-muted"))
                                                .child(desc),
                                        )
                                }),
                            )),
                    ),
            );

        deferred(
            div()
                .id("add-connection-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_connection_dialog();
                        cx.notify();
                    }),
                )
                .child(card),
        )
        .with_priority(95)
        .into_any_element()
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
            dc_intl::tr("Handle"),
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
                        dc_intl::tr("Cancel"),
                        modal::Tone::Secondary,
                        |this, _, _| this.close_connection_dialog(),
                    ),
                    modal::button(
                        "settings-connect-next",
                        dc_intl::tr("Next"),
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
        let title = dc_intl::tr("Connect your Domain");
        let Some(proof) = form.proof.clone() else {
            let body = self.field_box(
                dc_intl::tr("Domain"),
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
                            .child(dc_intl::tr(
                                "Verify ownership of your domain and show it off on your profile!",
                            ))
                            .into_any_element(),
                    ),
                    body: Some(body),
                    buttons: vec![
                        modal::button(
                            "settings-connect-cancel",
                            dc_intl::tr("Cancel"),
                            modal::Tone::Secondary,
                            |this, _, _| this.close_connection_dialog(),
                        ),
                        modal::button(
                            "settings-connect-next",
                            dc_intl::tr("Next"),
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
                        .child(copy_row(dc_intl::tr("Name"), format!("_discord.{domain}")))
                        .child(copy_row(dc_intl::tr("Type"), "TXT".to_owned()))
                        .child(copy_row(dc_intl::tr("Content"), proof)),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::tr(
                            "It may take a few minutes for the DNS record to propagate.",
                        )),
                )
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(text(
                    dc_intl::tr("Create a new file on your website:").to_owned(),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(copy_row(
                            dc_intl::tr("Name"),
                            format!("https://{domain}/.well-known/discord"),
                        ))
                        .child(copy_row(dc_intl::tr("Content"), proof)),
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
                            dc_intl::tr("Verify using HTTPS")
                        } else {
                            dc_intl::tr("Verify using DNS")
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
                        dc_intl::tr("Verify"),
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

    /// The full-width footer button of a slide (`footerButton_N-Y7ZJ`: `align-self: stretch`).
    fn slide_button(
        id: &'static str,
        label: &'static str,
        tone: modal::Tone,
        run: fn(&mut Root, &mut Context<Root>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .flex()
            .child(modal::footer_button(
                modal::button(id, label, tone, move |this, _, cx| {
                    run(this, cx);
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }

    /// `DISCORD_CONSENT` (module 910316): "Step 2 of 2" and the close button; the hook's header in
    /// `consentHeader_N-Y7ZJ` (32px above and below), its scope list and details; one Authorize and Continue.
    fn consent_slide(
        &self,
        info: &Info,
        step: AnyElement,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement, AnyElement) {
        let consent = &info.consent;
        let header = div()
            .flex()
            .flex_col()
            .items_center()
            .child(step)
            .into_any_element();
        let body: AnyElement = if let Some(message) = consent.failed.clone() {
            // The hook's error for a two-way consent: `Error` (body-lg/semibold, text-strong) over the message.
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .py(px(32.))
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("j2d6Km")),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(message),
                )
                .into_any_element()
        } else if let Some(get) = consent.get.clone() {
            let requested = super::connection_link::consent_args(info.kind)
                .map(|args| crate::ui::oauth::requested_scopes(&args.scopes))
                .unwrap_or_default();
            let data = crate::ui::oauth::ConsentData {
                get,
                disclosures: crate::ui::oauth::disclosure_ids(consent.disclosures.as_ref()),
                requested,
                integration: crate::ui::oauth::GUILD_INSTALL,
                fake: consent.fake,
                trusted_name: true,
            };
            let (head, scopes, details) = self.oauth_consent_parts(&data, cx);
            div()
                .flex()
                .flex_col()
                .child(div().py(px(32.)).child(head))
                .child(scopes)
                .child(details)
                .into_any_element()
        } else {
            // `e9`: the hook's spinner while its request is out.
            div()
                .h(px(120.))
                .flex()
                .items_center()
                .justify_center()
                .child(crate::ui::login::wandering_cubes())
                .into_any_element()
        };
        let footer = Self::slide_button(
            "settings-connect-authorize",
            dc_intl::t("ZN4hkc"),
            if consent.loading {
                modal::Tone::PrimaryLoading
            } else {
                modal::Tone::Primary
            },
            |this, cx| this.connection_consent_authorize(cx),
            cx,
        );
        (header, body, footer)
    }

    /// `SUCCESS` (module 162185 with each platform's picture, title, body and extra content); Done closes.
    fn success_slide(
        &self,
        kind: &'static str,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement, AnyElement) {
        #[allow(clippy::type_complexity)]
        let (art, heading, text): ((&str, f32, f32), &str, &str) = match kind {
            "xbox" => (
                ("conn-console-success", 124., 160.),
                dc_intl::t("7QsHmh"),
                dc_intl::t("byYNPX"),
            ),
            "playstation" => (
                ("conn-console-success", 124., 160.),
                dc_intl::t("e6SOl0"),
                dc_intl::t("QjAZAQ"),
            ),
            _ => (
                ("conn-crunchyroll-success", 224., 104.),
                dc_intl::t("Fnvxvk"),
                dc_intl::t("YwXceg"),
            ),
        };
        let header = div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .mb(px(16.))
                    .flex()
                    .justify_center()
                    .child(self.nitro_art(art.0, art.1, art.2)),
            )
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .text_center()
                    .child(heading),
            )
            .into_any_element();
        // `Anchor` (body-sm/medium) to the platform's help article.
        let article = if kind == "xbox" {
            "360003953831"
        } else {
            "9929188048023"
        };
        let learn_more = div()
            .id("settings-connect-success-learn")
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-link"))
            .cursor_pointer()
            .child(dc_intl::t("hvVgAZ"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings_open_url(&format!("{HELP}{article}"), cx);
                cx.notify();
            }));
        let extra: Option<AnyElement> = match kind {
            // `w` of module 35352 (`getXboxApp_LGWSzq`): the Xbox mark, Get the Xbox App, its text and link, and the
            // app's QR code (90px, 16 from the text).
            "xbox" => Some(
                div()
                    .mt(px(24.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-normal"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(24.))
                                            .mr(px(10.))
                                            .rounded(px(4.))
                                            .bg(color("--platform-xbox"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .children(
                                                self.images
                                                    .get(&crate::ui::platform_presence::asset_key(
                                                        XBOX_WHITE,
                                                    ))
                                                    .cloned()
                                                    .map(|picture| {
                                                        gpui::img(picture).size(px(20.))
                                                    }),
                                            ),
                                    )
                                    .child(dc_intl::t("12Kx2v")),
                            )
                            .child(
                                div()
                                    .my(px(8.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("M5yEcb")),
                            )
                            .child(learn_more),
                    )
                    .children(
                        crate::ui::login::qr_code_card(XBOX_APP_URL, 90., 8., 4.)
                            .map(|code| div().ml(px(16.)).child(code)),
                    )
                    .into_any_element(),
            ),
            "crunchyroll" => Some(
                div()
                    .flex()
                    .justify_center()
                    .child(learn_more)
                    .into_any_element(),
            ),
            _ => None,
        };
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .my(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .text_center()
                    .child(text),
            )
            .children(extra)
            .into_any_element();
        let footer = Self::slide_button(
            "settings-connect-done",
            dc_intl::t("i4jeWR"),
            modal::Tone::Primary,
            |this, _| this.close_connection_dialog(),
            cx,
        );
        (header, body, footer)
    }

    /// The landing, pre-connect, waiting and error slides of a platform's info dialog.
    fn info_dialog(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let kind = info.kind;
        let data = landing(kind);
        // `headerReconnect` when the account is linked one way (`getAccount(null, type)?.twoWayLink === false`).
        let one_way = self
            .settings_extra
            .connections
            .iter()
            .flatten()
            .any(|item| item["type"] == kind && item["two_way_link"] == Value::Bool(false));
        let landing_title = match (kind, one_way) {
            ("xbox", true) => dc_intl::t("z3rAhq"),
            ("playstation", true) => dc_intl::t("ZJ/vBh"),
            _ => data.title,
        };
        // `stepHeader_N-Y7ZJ` (14px, medium, 16 below) over the Text's `body-xs/semibold`: the dialog's stylesheet is
        // the later one, and only `line-height: 1.333` of the variant stays.
        let step = |number: u32| {
            div()
                .mb(px(16.))
                .text_size(px(14.))
                .line_height(px(14. * 4. / 3.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(dc_intl::format(
                    "fHz6eR",
                    &[("number", number.to_string().into()), ("total", "2".into())],
                ))
        };
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
        // `variant: n ? "secondary" : "primary"` (module 873885): the waiting slide's Retry is the secondary button
        // (gpui takes one hover style per element, so the variant is chosen here).
        let primary = |id: &'static str,
                       label: &'static str,
                       secondary: bool,
                       run: fn(&mut Root, &mut Context<Root>)| {
            let family = if secondary { "secondary" } else { "primary" };
            let token = move |part: &str| color(&format!("--control-{family}-{part}"));
            div()
                .id(id)
                .h(px(40.))
                .px(px(17.))
                .rounded(px(8.))
                .border_1()
                .border_color(if secondary {
                    color("--border-muted")
                } else {
                    token("border-default")
                })
                .bg(token("background-default"))
                .hover(move |style| style.bg(token("background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(if secondary {
                    token("text-default")
                } else {
                    color("--white")
                })
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
                                                    .child(dc_intl::tr(label)),
                                            )
                                            .children(sub.map(|sub| {
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-subtle"))
                                                    .child(dc_intl::tr(sub))
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
                        .child(title(landing_title))
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
                    primary(
                        "settings-connect-continue",
                        dc_intl::tr("Continue"),
                        false,
                        |this, cx| {
                            if let Some(Dlg::Info(info)) = this.settings_extra.conn.as_mut() {
                                info.slide = Slide::Connect;
                            }
                            cx.notify();
                        },
                    )
                    .w(px(100.))
                    .into_any_element(),
                )
            }
            Slide::Consent => self.consent_slide(info, step(2).into_any_element(), cx),
            Slide::Success => self.success_slide(kind, cx),
            slide => {
                let (spec, heading, text) = match slide {
                    Slide::Connect => data.connect,
                    Slide::Waiting => data.waiting,
                    _ => (
                        ERROR_ART,
                        data.error.0,
                        super::connection_link::error_body(kind, info.error_code)
                            .unwrap_or(data.error.1),
                    ),
                };
                let waiting = slide == Slide::Waiting;
                // The error slide (module 478474) offers "Try Again" with the window launch icon, back to the
                // pre-connect slide (`gotoNext` of module 670404 from ERROR).
                let label = match slide {
                    Slide::Waiting => dc_intl::tr("Retry"),
                    // `4X7vPo`: "Try Again " and the icon's hook.
                    Slide::Error => dc_intl::tr("Try Again"),
                    _ => dc_intl::tr("Continue"),
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
                        super::connection_link::authorize_route(kind),
                        cx,
                    );
                };
                (
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .when(slide != Slide::Error, |this| this.child(step(1)))
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
                    primary("settings-connect-start", label, waiting, start)
                        .w_full()
                        // `launchIcon_N-Y7ZJ`: the 16px WindowLaunchIcon after the text's space, 1px up; `6623tt`
                        // (Continue) and `8Laby+` (Retry) carry it too, in the button's text colour.
                        .child(div().ml(px(4.)).mt(px(-1.)).child(mask_icon(
                            "access_launch",
                            16.,
                            if waiting {
                                "--control-secondary-text-default"
                            } else {
                                "--white"
                            },
                        )))
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
            // `slideContainer_N-Y7ZJ`: at most `clamp(200px, 100vh - 80px, 720px)`. The consent's list is longer than
            // that, so its slide takes the whole height and its content scrolls.
            .max_h(px(720.))
            .min_h(px(0.))
            .when(info.slide == Slide::Consent, |this| {
                this.h(px((self.viewport.1 - 80.).clamp(200., 720.)))
            })
            .child(div().flex_none().pt(px(24.)).px(px(16.)).child(header))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .id("settings-connect-info-content")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&info.consent.scroll)
                            .px(px(24.))
                            .py(px(16.))
                            .child(content),
                    )
                    .when(info.slide == Slide::Consent, |this| {
                        this.child(super::auto_scrollbar(info.consent.scroll.clone()))
                    }),
            )
            .child(
                div()
                    .flex_none()
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
                .py(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_connection_dialog();
                    cx.notify();
                }))
                .child(
                    div()
                        .max_h_full()
                        .min_h(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(modal),
                ),
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
