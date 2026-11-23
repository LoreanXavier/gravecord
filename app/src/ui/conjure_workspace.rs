//! Conjure AI App Builder Workspace (`ConjureSelectedProjectScreen`, module 164254).
//!
//! Split flex workspace (`.channelView_ajKoHp`):
//! - Top header (`705427.A`): preview mode switcher (`Frame`, `Widget`, `Overlay`, `Bot`),
//!   form-factor toggles (`Desktop`, `Mobile`, `Portrait`/`Landscape`), `Edit and Draw` (`XWU+XF`) /
//!   `Select Mode` (`cl/Jyl`), `Refresh preview` (`FZIHQN`), `Debug` (`Q4FN+H`), `Share` (`RDE0Sc`),
//!   and `Exit` (`/TlGcK`).
//! - Left main preview pane with live frame/widget/overlay/bot modes + annotation mode banner.
//! - Right resizable Chat / 4-tab Debug Panel sidebar (`Logs` `+VRYCm`, `Worker` `50D0FZ`,
//!   `Agent` `UkbTK1`, `Trace`), Tasks checklist (`RtzECX`), Plan Proposal card (`3b6e7o` + `6S+wRM`
//!   "Build it"), and glass composer (`rxW2cl`).

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use serde_json::json;

/// Module 164254 (`ff`): preview pane modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConjurePreviewMode {
    #[default]
    Frame,
    Widget,
    Overlay,
    Bot,
}

impl ConjurePreviewMode {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Frame => cj_copy("I2ucou", "Preview view"),
            Self::Widget => cj_copy("y5GiL1", "Profile Widget"),
            Self::Overlay => cj_copy("2940LX", "Game Overlay"),
            Self::Bot => cj_copy("tjpaGN", "Bot DM"),
        }
    }
}

/// Module 164254: form factor toggle (`tKGF0Q` Desktop vs `peqEOY` Mobile + `tunI2l`/`DoNdjA`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureFormFactor {
    #[default]
    Desktop,
    MobilePortrait,
    MobileLandscape,
}

/// Module 164254: active preview annotation overlay (`None` | `SelectMode` `cl/Jyl` | `EditAndDraw` `XWU+XF`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureOverlayTool {
    #[default]
    None,
    SelectMode,
    EditAndDraw,
}

/// Module 164254 (`mi`): 4-tab developer Debug Panel inside the right sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConjureDebugTab {
    #[default]
    Logs,
    Worker,
    Agent,
    Trace,
}

impl ConjureDebugTab {
    pub(crate) const ALL: [Self; 4] = [Self::Logs, Self::Worker, Self::Agent, Self::Trace];

    pub(crate) fn label(self) -> String {
        match self {
            Self::Logs => cj_copy("+VRYCm", "Logs"),
            Self::Worker => cj_copy("50D0FZ", "Worker"),
            Self::Agent => cj_copy("UkbTK1", "Agent"),
            Self::Trace => "Trace".to_owned(),
        }
    }
}

/// Open state for `ConjureSelectedProjectScreen` (module 164254).
#[derive(Clone, Debug)]
pub(crate) struct ConjureWorkspaceState {
    #[allow(dead_code)]
    pub project_id: String,
    pub project_name: String,
    pub preview_mode: ConjurePreviewMode,
    pub form_factor: ConjureFormFactor,
    pub overlay_tool: ConjureOverlayTool,
    pub chat_open: bool,
    pub debug_open: bool,
    pub debug_tab: ConjureDebugTab,
    pub todos: Vec<(String, bool)>,
    pub plan_title: Option<String>,
    pub plan_steps: Vec<String>,
}

/// Module 273594 (`AgentBrowserControlState`): live control state of a Conjure agent browser session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AgentControlState {
    Viewing,
    Pending,
    Controlling,
    Denied,
}

/// Module 273594: one live or ended agent browser session tab.
#[derive(Clone, Debug)]
pub(crate) struct AgentBrowserSession {
    #[allow(dead_code)]
    pub id: String,
    pub label: String,
    pub ended: bool,
    pub has_frame: bool,
    pub last_seq: u32,
    pub width: u16,
    pub height: u16,
}

/// Module 273594 (`648127.i`): session display label (falls back to `gHsgGe` "Agent browser" for `""`/`"default"`).
#[allow(dead_code)]
pub(crate) fn agent_browser_session_label(session_id: &str) -> String {
    let trimmed = session_id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        cj_copy("gHsgGe", "Agent browser")
    } else {
        trimmed.to_owned()
    }
}

/// Module 273594 (`504054`): parses a binary WS agent browser frame (`ver=1, u32 BE seq, u16 BE w, u16 BE h, jpeg`).
#[allow(dead_code)]
pub(crate) fn parse_agent_browser_frame(bytes: &[u8]) -> Option<(u32, u16, u16, &[u8])> {
    if bytes.len() < 10 || bytes[0] != 1 {
        return None;
    }
    let seq = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    let width = u16::from_be_bytes([bytes[5], bytes[6]]);
    let height = u16::from_be_bytes([bytes[7], bytes[8]]);
    Some((seq, width, height, &bytes[9..]))
}

/// Module 273594: Conjure Agent Live Browser LayerModal state.
#[derive(Clone, Debug)]
pub(crate) struct ConjureAgentBrowserState {
    pub project_id: String,
    pub sessions: Vec<AgentBrowserSession>,
    pub active_idx: usize,
    pub control_state: AgentControlState,
}

/// Module 354155 (`951178.q7` + `589444`): Conjure app surface tag chips in official order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureSurface {
    AppChannel = 1,
    Activity = 8,
    VoiceChannel = 2,
    ProfileWidget = 3,
    Overlay = 7,
    ApplicationCommands = 6,
    Bot = 5,
    AutoMod = 4,
}

impl ConjureSurface {
    pub(crate) const ORDERED: [Self; 8] = [
        Self::AppChannel,
        Self::Activity,
        Self::VoiceChannel,
        Self::ProfileWidget,
        Self::Overlay,
        Self::ApplicationCommands,
        Self::Bot,
        Self::AutoMod,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::AppChannel => dc_intl::t("bHLrO8"),
            Self::Activity => dc_intl::t("hp+8nc"),
            Self::VoiceChannel => dc_intl::t("4V2QWl"),
            Self::ProfileWidget => dc_intl::t("oJ06Po"),
            Self::Overlay => dc_intl::t("Qbnej2"),
            Self::ApplicationCommands => dc_intl::t("pEwndv"),
            Self::Bot => dc_intl::t("HWOzBW"),
            Self::AutoMod => dc_intl::t("gsIXM9"),
        }
    }
}

/// Module 354155 (`529424`): access/load state of a shared Conjure project modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureSharedProjectStatus {
    Loading,
    Unavailable,
    NoAccess,
    InfoOnly,
    Ready,
}

/// Module 354155 (`529424`): `GET /conjure/project-limit`.
#[allow(dead_code)]
pub(crate) fn conjure_project_limit_request() -> Request {
    Request::Rest {
        key: Some("conjure:project-limit".to_owned()),
        method: reqwest::Method::GET,
        route: "/conjure/project-limit".to_owned(),
        body: None,
    }
}

/// Module 354155 (`529424`): `GET /conjure/projects/{project_id}`.
#[allow(dead_code)]
pub(crate) fn conjure_project_fetch_request(project_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("conjure:project:{project_id}")),
        method: reqwest::Method::GET,
        route: format!("/conjure/projects/{project_id}"),
        body: None,
    }
}

/// Module 312405: Conjure Remix vs Install (Copy) modal mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureCopyMode {
    Copy,
    Remix,
}

/// Shared auxiliary modal for Conjure AI App Builder (modules 300874 & 312405).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum ConjureAuxModal {
    /// Module 300874: Patch notes composer modal (`lsYQII`, `r4du8k`, `aYQksU`/`3hV1Gc`,
    /// `Em8bo+`, `Gd63Fl`, `7CvxMC`, `2ECgBx`, `69aIG4`, `6oEjjD`).
    PatchNotes {
        #[allow(dead_code)]
        project_id: String,
        project_name: String,
        app_channel_id: Option<String>,
        channels: Vec<(String, String)>,
        selected_channel_id: Option<String>,
        text: String,
        drafting: bool,
        draft_failed: bool,
        sending: bool,
        max_len: usize,
    },
    /// Module 312405: Remix / Install (Copy) modal (`UsUa/c`/`276ll6`, `GzGb2w`/`QN8T+C`,
    /// `cvvDlE`/`u3W0k0`, `PuVQeX`, `cIWtCk`, `Cn8H0Y`).
    RemixOrCopy {
        project_id: String,
        project_name: String,
        mode: ConjureCopyMode,
        prompt: String,
        copy_name: String,
        guilds: Vec<(String, String)>,
        selected_guild_id: Option<String>,
        submitting: bool,
    },
    /// Module 354155: Conjure Shared Project Preview & Access Gate modal (`ZTNur7`, `mQma36`,
    /// `2DSRxV`, `cF++B3`, `6/x9LD`, `6RGR3m` Remix, `UsUa/c` Install).
    SharedProject {
        status: ConjureSharedProjectStatus,
        project_id: String,
        app_name: String,
        app_description: String,
        surfaces: Vec<ConjureSurface>,
        creator_name: Option<String>,
        can_remix: bool,
    },
}

const CONJURE_COPY_SUFFIX: &str = " (Copy)";

/// Module 312405: default copy name (`"{name} (Copy)"` capped to 128 chars).
pub(crate) fn conjure_default_copy_name(name: &str) -> String {
    let limit = 128usize.saturating_sub(CONJURE_COPY_SUFFIX.len());
    let prefix: String = name.chars().take(limit).collect();
    format!("{prefix}{CONJURE_COPY_SUFFIX}")
}

/// Module 300874 (`w`): builds `\n\nTry it here: <#channelId>` footer appended to sent patch notes.
pub(crate) fn conjure_patch_notes_footer(channel_mention: &str) -> String {
    let line = dc_intl::format("2ECgBx", &[("channel", channel_mention.into())]);
    format!("\n\n{line}")
}

/// Module 300874: `POST /channels/{channel_id}/messages` with optional `Try it here:` footer.
pub(crate) fn conjure_patch_notes_send_request(
    channel_id: &str,
    notes: &str,
    app_channel_id: Option<&str>,
) -> Request {
    let trimmed = notes.trim();
    let content = match app_channel_id {
        Some(app_ch) if !app_ch.is_empty() => {
            format!(
                "{trimmed}{}",
                conjure_patch_notes_footer(&format!("<#{app_ch}>"))
            )
        }
        _ => trimmed.to_owned(),
    };
    Request::Rest {
        key: Some(format!("conjure:patch-notes:{channel_id}")),
        method: reqwest::Method::POST,
        route: format!("/channels/{channel_id}/messages"),
        body: Some(json!({ "content": content })),
    }
}

/// Module 312405 (`233470.iR`): `POST /conjure/projects` for remixing or copying a project.
pub(crate) fn conjure_create_remix_request(
    name: &str,
    guild_id: &str,
    install_scope: &str,
    remix_source_project_id: &str,
    nonce: &str,
) -> Request {
    Request::Rest {
        key: Some(format!("conjure:create-remix:{remix_source_project_id}")),
        method: reqwest::Method::POST,
        route: "/conjure/projects".to_owned(),
        body: Some(json!({
            "name": name.trim(),
            "guild_id": guild_id,
            "install_scope": install_scope,
            "remix_source_project_id": remix_source_project_id,
            "nonce": nonce,
        })),
    }
}

/// Module 164254 (`472119.dv`): `POST /conjure/projects/{id}/ws-ticket`.
#[allow(dead_code)]
pub(crate) fn conjure_ws_ticket_request(project_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("conjure:ws-ticket:{project_id}")),
        method: reqwest::Method::POST,
        route: format!("/conjure/projects/{project_id}/ws-ticket"),
        body: None,
    }
}

/// Module 164254 (`708679.JN`): `PUT /conjure/projects/{id}/install-scope`.
#[allow(dead_code)]
pub(crate) fn conjure_install_scope_request(
    project_id: &str,
    install_scope: &str,
    guild_id: Option<&str>,
) -> Request {
    let mut body = json!({ "install_scope": install_scope });
    if let Some(gid) = guild_id {
        body["guild_id"] = serde_json::Value::String(gid.to_owned());
    }
    Request::Rest {
        key: Some(format!("conjure:install-scope:{project_id}")),
        method: reqwest::Method::PUT,
        route: format!("/conjure/projects/{project_id}/install-scope"),
        body: Some(body),
    }
}

/// Module 164254 (`802905.CN`): `POST /conjure/projects/{id}/preview-dm`.
#[allow(dead_code)]
pub(crate) fn conjure_preview_dm_request(project_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("conjure:preview-dm:{project_id}")),
        method: reqwest::Method::POST,
        route: format!("/conjure/projects/{project_id}/preview-dm"),
        body: None,
    }
}

fn cj_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

impl Root {
    /// Renders `ConjureSelectedProjectScreen` (module 164254) workspace overlay when open.
    pub(super) fn conjure_workspace_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ws = self.conjure_workspace.as_ref()?;
        let modes = [
            ConjurePreviewMode::Frame,
            ConjurePreviewMode::Widget,
            ConjurePreviewMode::Overlay,
            ConjurePreviewMode::Bot,
        ];
        let mode_pills = modes.into_iter().map(|m| {
            let active = ws.preview_mode == m;
            div()
                .id(SharedString::from(format!("cj-mode-{m:?}")))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(6.))
                .bg(color(if active {
                    "--background-mod-strong"
                } else {
                    "--background-mod-subtle"
                }))
                .cursor_pointer()
                .text_size(px(12.))
                .font_weight(FontWeight(600.))
                .text_color(color(if active {
                    "--text-strong"
                } else {
                    "--text-muted"
                }))
                .child(m.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(s) = this.conjure_workspace.as_mut() {
                        s.preview_mode = m;
                    }
                    cx.notify();
                }))
        });

        let preview_w = match ws.form_factor {
            ConjureFormFactor::Desktop => px(560.),
            ConjureFormFactor::MobilePortrait => px(320.),
            ConjureFormFactor::MobileLandscape => px(520.),
        };

        let right_sidebar = ws.chat_open.then(|| {
            let body: AnyElement =
                if ws.debug_open {
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(div().flex().gap(px(6.)).children(
                            ConjureDebugTab::ALL.into_iter().map(|tab| {
                                let active = ws.debug_tab == tab;
                                div()
                                    .id(SharedString::from(format!("cj-dbg-{tab:?}")))
                                    .px(px(10.))
                                    .py(px(5.))
                                    .rounded(px(6.))
                                    .bg(color(if active {
                                        "--background-brand"
                                    } else {
                                        "--background-mod-subtle"
                                    }))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color(if active {
                                        "--white"
                                    } else {
                                        "--text-muted"
                                    }))
                                    .child(tab.label())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(s) = this.conjure_workspace.as_mut() {
                                            s.debug_tab = tab;
                                        }
                                        cx.notify();
                                    }))
                            }),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .p(px(12.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lowest"))
                                .font_family("Consolas")
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(format!(
                                    "[{:?}] Preview worker healthy — 0 errors",
                                    ws.debug_tab
                                )),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .p(px(12.))
                        .flex()
                        .flex_col()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                // Tasks checklist (`RtzECX`)
                                .when(!ws.todos.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-surface-highest"))
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(cj_copy("RtzECX", "Tasks")),
                                            )
                                            .children(ws.todos.iter().map(|(label, done)| {
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(icon(
                                                        if *done {
                                                            "circle_check"
                                                        } else {
                                                            "circle_info"
                                                        },
                                                        12.,
                                                        if *done {
                                                            "--status-positive"
                                                        } else {
                                                            "--icon-muted"
                                                        },
                                                    ))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w(px(0.))
                                                            .text_size(px(12.))
                                                            .text_color(color("--text-default"))
                                                            .child(label.clone()),
                                                    )
                                            })),
                                    )
                                })
                                // Plan Proposal card (`3b6e7o` + `6S+wRM` "Build it")
                                .children(ws.plan_title.as_ref().map(|title| {
                                    div()
                                        .p(px(12.))
                                        .rounded(px(10.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-surface-high"))
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.))
                                        .child(
                                            div().flex().items_center().justify_between().child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(format!(
                                                        "{}: {title}",
                                                        cj_copy("3b6e7o", "Plan")
                                                    )),
                                            ),
                                        )
                                        .children(ws.plan_steps.iter().enumerate().map(|(i, s)| {
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(format!("{}. {s}", i + 1))
                                        }))
                                        .child(
                                            div()
                                                .id("cj-build-plan-btn")
                                                .mt(px(4.))
                                                .h(px(34.))
                                                .rounded(px(8.))
                                                .bg(color("--control-primary-background-default"))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(cj_copy("6S+wRM", "Build it")),
                                        )
                                })),
                        )
                        // Glass composer (`.composer_CQ1SDL`)
                        .child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(color("--text-muted"))
                                        .child(cj_copy("wx/o8Y", "What's next?")),
                                )
                                .child(
                                    div()
                                        .id("cj-send-msg")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .bg(color("--control-primary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .aria_label(cj_copy("rxW2cl", "Send message"))
                                        .child(icon("send_message", 14., "--white")),
                                ),
                        )
                        .into_any_element()
                };

            div()
                .w(px(360.))
                .h_full()
                .border_l_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(44.))
                        .px(px(12.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(if ws.debug_open {
                                    cj_copy("Q4FN+H", "Debug")
                                } else {
                                    dc_intl::t("/VQax8").to_owned()
                                }),
                        )
                        .child(
                            div()
                                .id("cj-toggle-debug")
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .bg(color("--background-mod-subtle"))
                                .cursor_pointer()
                                .text_size(px(11.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(cj_copy("Q4FN+H", "Debug"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(s) = this.conjure_workspace.as_mut() {
                                        s.debug_open = !s.debug_open;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(body)
        });

        Some(
            deferred(
                div()
                    .id("conjure-workspace-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000cc))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(20.))
                    .child(
                        div()
                            .id("conjure-workspace-shell")
                            .w(px(1080.))
                            .h(px(640.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lowest"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            // Top header (`705427.A`)
                            .child(
                                div()
                                    .h(px(48.))
                                    .px(px(14.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-high"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(icon("bot_game_controller", 18., "--text-brand"))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(ws.project_name.clone()),
                                            )
                                            .children(mode_pills),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .id("cj-edit-draw-btn")
                                                    .px(px(10.))
                                                    .py(px(5.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        if ws.overlay_tool
                                                            == ConjureOverlayTool::EditAndDraw
                                                        {
                                                            "--background-brand"
                                                        } else {
                                                            "--background-mod-subtle"
                                                        },
                                                    ))
                                                    .cursor_pointer()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(cj_copy("XWU+XF", "Edit and Draw"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(s) =
                                                            this.conjure_workspace.as_mut()
                                                        {
                                                            s.overlay_tool = if s.overlay_tool
                                                                == ConjureOverlayTool::EditAndDraw
                                                            {
                                                                ConjureOverlayTool::None
                                                            } else {
                                                                ConjureOverlayTool::EditAndDraw
                                                            };
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id("cj-share-btn")
                                                    .px(px(10.))
                                                    .py(px(5.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .cursor_pointer()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .child(dc_intl::t("RDE0Sc")),
                                            )
                                            .child(
                                                div()
                                                    .id("cj-exit-btn")
                                                    .px(px(10.))
                                                    .py(px(5.))
                                                    .rounded(px(6.))
                                                    .bg(color("--background-mod-subtle"))
                                                    .cursor_pointer()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-default"))
                                                    .child(cj_copy("/TlGcK", "Exit"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.conjure_workspace = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            // Split workspace body (`.channelView_ajKoHp`)
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .flex()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .bg(color("--background-base-lower"))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .p(px(20.))
                                            .when(
                                                ws.overlay_tool != ConjureOverlayTool::None,
                                                |pane| {
                                                    pane.child(
                                                        div()
                                                            .mb(px(10.))
                                                            .px(px(12.))
                                                            .py(px(6.))
                                                            .rounded(px(8.))
                                                            .bg(color("--background-brand"))
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--white"))
                                                            .child(
                                                                if ws.overlay_tool
                                                                    == ConjureOverlayTool::EditAndDraw
                                                                {
                                                                    cj_copy(
                                                                        "qlgN+X",
                                                                        "Drawing layer over your app.",
                                                                    )
                                                                } else {
                                                                    cj_copy("cl/Jyl", "Select Mode")
                                                                },
                                                            ),
                                                    )
                                                },
                                            )
                                            .child(
                                                div()
                                                    .w(preview_w)
                                                    .h(px(420.))
                                                    .rounded(px(10.))
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .bg(color("--background-surface-high"))
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .justify_center()
                                                    .gap(px(8.))
                                                    .child(icon(
                                                        "bot_game_controller",
                                                        40.,
                                                        "--text-brand",
                                                    ))
                                                    .child(
                                                        div()
                                                            .text_size(px(16.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(format!(
                                                                "{} ({})",
                                                                ws.project_name,
                                                                ws.preview_mode.label()
                                                            )),
                                                    ),
                                            ),
                                    )
                                    .children(right_sidebar),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 273594 (`AgentBrowserModal`, CSS `135422` `3SsYXF`, LayerModal `521489`):
    /// Live multi-session agent browser panel (max 1400px, tab bar w/ 8px live/ended dot, Take Control /
    /// Hand Back / Denied control toggle, stage highlight border when controlling, loading & ended overlays).
    pub(super) fn conjure_agent_browser_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.conjure_agent_browser.as_ref()?;
        let active = state
            .sessions
            .get(state.active_idx)
            .or_else(|| state.sessions.first())?;
        let is_controlling = state.control_state == AgentControlState::Controlling;
        let is_denied = state.control_state == AgentControlState::Denied;
        let is_ended = active.ended;
        let has_frame = active.has_frame;
        let project_id = state.project_id.clone();

        let tabs = div()
            .flex_1()
            .min_w(px(0.))
            .pt(px(12.))
            .flex()
            .items_end()
            .gap(px(4.))
            .children(state.sessions.iter().enumerate().map(|(idx, sess)| {
                let selected = idx == state.active_idx;
                let dot_color = if sess.ended {
                    "--icon-muted"
                } else {
                    "--status-positive"
                };
                let aria = dc_intl::format(
                    if sess.ended { "OO/He7" } else { "FkosiV" },
                    &[("title", sess.label.as_str().into())],
                );
                div()
                    .id(SharedString::from(format!("cj-ab-tab-{idx}")))
                    .aria_label(SharedString::from(aria))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded_t(px(8.))
                    .border_b_2()
                    .border_color(if selected {
                        color("--control-primary-background-default")
                    } else {
                        rgba(0x00000000)
                    })
                    .bg(if selected {
                        color("--background-mod-subtle")
                    } else {
                        rgba(0x00000000)
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .max_w(px(192.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ab) = this.conjure_agent_browser.as_mut() {
                            ab.active_idx = idx;
                        }
                        cx.notify();
                    }))
                    .child(div().size(px(8.)).rounded_full().bg(color(dot_color)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight(if selected { 600. } else { 500. }))
                            .text_color(color(if selected {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            }))
                            .child(sess.label.clone()),
                    )
            }));

        let control_btn = (!is_ended).then(|| {
            let label = if is_controlling {
                cj_copy("7lQQJ5", "Hand Back")
            } else {
                cj_copy("d5Slrv", "Take Control")
            };
            div()
                .id("cj-ab-control-btn")
                .h(px(32.))
                .px(px(12.))
                .rounded(px(6.))
                .bg(color(if is_controlling {
                    "--control-primary-background-default"
                } else {
                    "--control-secondary-background-default"
                }))
                .when(!is_denied, |d| {
                    d.cursor_pointer().hover(move |s| {
                        s.bg(color(if is_controlling {
                            "--control-primary-background-hover"
                        } else {
                            "--control-secondary-background-hover"
                        }))
                    })
                })
                .when(is_denied, |d| d.opacity(0.5))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .font_weight(FontWeight(600.))
                .text_color(color(if is_controlling {
                    "--white"
                } else {
                    "--control-secondary-text-default"
                }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(ab) = this.conjure_agent_browser.as_mut() {
                        if ab.control_state == AgentControlState::Denied {
                            return;
                        }
                        ab.control_state = if ab.control_state == AgentControlState::Controlling {
                            AgentControlState::Viewing
                        } else {
                            let _ = this.requests.send(conjure_ws_ticket_request(&project_id));
                            AgentControlState::Controlling
                        };
                    }
                    cx.notify();
                }))
        });

        let stage_overlay: Option<AnyElement> = if is_ended {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000099))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(cj_copy("ocRH08", "The agent closed this browser.")),
                    )
                    .into_any_element(),
            )
        } else if !has_frame {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .p(px(24.))
                    .child(icon("circle_info", 28., "--text-muted"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(cj_copy("wuE3Ja", "Connecting to the browser…")),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let stage_aria = dc_intl::format("rnwqXb", &[("name", active.label.as_str().into())]);
        let panel_w = (self.viewport.0 - 80.).clamp(640., 1400.);
        let panel_h = (self.viewport.1 - 80.).clamp(420., 840.);

        Some(
            deferred(
                div()
                    .id("conjure-agent-browser-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000c8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.conjure_agent_browser = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("conjure-agent-browser-modal")
                            .aria_label(SharedString::from(cj_copy("sGsShB", "Agent browsers")))
                            .occlude()
                            .w(px(panel_w))
                            .h(px(panel_h))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .overflow_hidden()
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            // `.header_3SsYXF`
                            .child(
                                div()
                                    .pl(px(24.))
                                    .pr(px(16.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(tabs)
                                    .children(control_btn)
                                    .child(
                                        div()
                                            .id("cj-ab-close")
                                            .aria_label(SharedString::from(dc_intl::t("cpT0Cq")))
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                18.,
                                                "--interactive-text-default",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.conjure_agent_browser = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // `.session_3SsYXF` + `.stage_3SsYXF`
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .pt(px(16.))
                                    .px(px(24.))
                                    .pb(px(24.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .id("cj-ab-stage")
                                            .aria_label(SharedString::from(stage_aria))
                                            .relative()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .rounded(px(8.))
                                            .overflow_hidden()
                                            .bg(color("--background-base-lowest"))
                                            .border_2()
                                            .border_color(color(if is_controlling {
                                                "--control-primary-background-default"
                                            } else {
                                                "--border-subtle"
                                            }))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(has_frame && !is_ended, |d| {
                                                d.child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .child(icon(
                                                            "bot_game_controller",
                                                            36.,
                                                            "--brand-500",
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .font_weight(FontWeight(600.))
                                                                .text_color(color("--text-strong"))
                                                                .child(format!(
                                                                    "{} ({}×{} · seq #{})",
                                                                    active.label,
                                                                    active.width,
                                                                    active.height,
                                                                    active.last_seq
                                                                )),
                                                        ),
                                                )
                                            })
                                            .children(stage_overlay),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Renders `ConjureAuxModal` (module 300874 Patch Notes & module 312405 Remix/Install Copy).
    pub(super) fn conjure_aux_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.conjure_aux_modal.as_deref()?;
        let dialog: AnyElement = match modal {
            ConjureAuxModal::PatchNotes {
                project_name,
                app_channel_id,
                channels,
                selected_channel_id,
                text,
                drafting,
                draft_failed,
                sending,
                max_len,
                ..
            } => {
                let title =
                    dc_intl::format("lsYQII", &[("projectName", project_name.as_str().into())]);
                let trimmed = text.trim();
                let can_send = !trimmed.is_empty()
                    && trimmed.chars().count() <= *max_len
                    && selected_channel_id.is_some()
                    && !*sending;
                let selected_label = selected_channel_id
                    .as_deref()
                    .and_then(|id| channels.iter().find(|(cid, _)| cid == id))
                    .map(|(_, name)| format!("#{name}"))
                    .unwrap_or_else(|| dc_intl::t("7CvxMC").to_owned());
                let footer_preview = app_channel_id.as_deref().map(|ch_id| {
                    let ch_label = channels
                        .iter()
                        .find(|(cid, _)| cid == ch_id)
                        .map(|(_, n)| format!("#{n}"))
                        .unwrap_or_else(|| format!("<#{ch_id}>"));
                    dc_intl::format("2ECgBx", &[("channel", ch_label.as_str().into())])
                });
                let send_channel = selected_channel_id.clone();
                let send_notes = text.clone();
                let send_app_ch = app_channel_id.clone();
                let is_drafting = *drafting;
                let is_failed = *draft_failed;
                let is_sending = *sending;
                let char_count = trimmed.chars().count();
                let max_chars = *max_len;

                div()
                    .id("conjure-patch-notes-dialog")
                    .w(px(580.))
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
                            .px(px(20.))
                            .py(px(16.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id("cj-pn-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 16., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("r4du8k")),
                            )
                            .child(
                                div()
                                    .min_h(px(220.))
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .flex_col()
                                    .justify_between()
                                    .gap(px(10.))
                                    .child(if text.is_empty() {
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t(if is_drafting {
                                                "aYQksU"
                                            } else {
                                                "3hV1Gc"
                                            }))
                                    } else {
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(text.clone())
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(8.))
                                            .children(footer_preview.map(|line| {
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-link"))
                                                    .child(line)
                                            }))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(color(if char_count > max_chars {
                                                        "--text-feedback-critical"
                                                    } else {
                                                        "--text-muted"
                                                    }))
                                                    .child(format!("{char_count}/{max_chars}")),
                                            ),
                                    ),
                            )
                            .when(is_failed, |col| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("Em8bo+")),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-subtle"))
                                            .child(dc_intl::t("Gd63Fl")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(6.))
                                            .children(channels.iter().enumerate().map(
                                                |(idx, (cid, cname))| {
                                                    let active =
                                                        selected_channel_id.as_deref() == Some(cid);
                                                    let cid_owned = cid.clone();
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "cj-pn-ch-{idx}"
                                                        )))
                                                        .px(px(10.))
                                                        .py(px(6.))
                                                        .rounded(px(6.))
                                                        .border_1()
                                                        .border_color(color(if active {
                                                            "--control-primary-background-default"
                                                        } else {
                                                            "--border-subtle"
                                                        }))
                                                        .bg(color(if active {
                                                            "--background-mod-strong"
                                                        } else {
                                                            "--background-base-lower"
                                                        }))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .child(icon(
                                                            "channel_type_text",
                                                            14.,
                                                            if active {
                                                                "--text-strong"
                                                            } else {
                                                                "--icon-muted"
                                                            },
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(13.))
                                                                .font_weight(FontWeight(
                                                                    if active {
                                                                        600.
                                                                    } else {
                                                                        500.
                                                                    },
                                                                ))
                                                                .text_color(color(if active {
                                                                    "--text-strong"
                                                                } else {
                                                                    "--text-default"
                                                                }))
                                                                .child(cname.clone()),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(
                                                                    ConjureAuxModal::PatchNotes {
                                                                        selected_channel_id,
                                                                        ..
                                                                    },
                                                                ) = this
                                                                    .conjure_aux_modal
                                                                    .as_deref_mut()
                                                                {
                                                                    *selected_channel_id =
                                                                        Some(cid_owned.clone());
                                                                }
                                                                cx.notify();
                                                            },
                                                        ))
                                                },
                                            ))
                                            .when(channels.is_empty(), |row| {
                                                row.child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-muted"))
                                                        .child(selected_label),
                                                )
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("cj-pn-cancel")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(dc_intl::t("ETE/oC"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("cj-pn-send")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!can_send, |d| d.opacity(0.5))
                                    .when(can_send, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("69aIG4"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !can_send || is_sending {
                                            return;
                                        }
                                        let Some(ch_id) = send_channel.as_deref() else {
                                            this.show_app_toast(dc_intl::t("6oEjjD"), cx);
                                            return;
                                        };
                                        let _ =
                                            this.requests.send(conjure_patch_notes_send_request(
                                                ch_id,
                                                &send_notes,
                                                send_app_ch.as_deref(),
                                            ));
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            ConjureAuxModal::RemixOrCopy {
                project_id,
                project_name,
                mode,
                prompt,
                copy_name,
                guilds,
                selected_guild_id,
                submitting,
            } => {
                let is_copy = *mode == ConjureCopyMode::Copy;
                let title_key = if is_copy { "UsUa/c" } else { "276ll6" };
                let subtitle = dc_intl::format(
                    if is_copy { "GzGb2w" } else { "QN8T+C" },
                    &[("projectName", project_name.as_str().into())],
                );
                let name_valid = is_copy || copy_name.trim().chars().count() >= 2;
                let can_submit = name_valid && selected_guild_id.is_some() && !*submitting;
                let submit_name = if is_copy {
                    project_name.clone()
                } else {
                    copy_name.clone()
                };
                let submit_pid = project_id.clone();
                let submit_gid = selected_guild_id.clone();
                let is_submitting = *submitting;

                div()
                    .id("conjure-remix-dialog")
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
                            .px(px(20.))
                            .pt(px(20.))
                            .pb(px(14.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
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
                                            .text_size(px(18.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t(title_key)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
                                    ),
                            )
                            .child(
                                div()
                                    .id("cj-rmx-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 16., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .when(!is_copy, |col| {
                                col.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("cvvDlE")),
                                        )
                                        .child(
                                            div()
                                                .min_h(px(68.))
                                                .p(px(10.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--input-border-default"))
                                                .bg(color("--input-background-default"))
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color(if prompt.is_empty() {
                                                    "--text-muted"
                                                } else {
                                                    "--text-default"
                                                }))
                                                .child(if prompt.is_empty() {
                                                    dc_intl::t("u3W0k0").to_owned()
                                                } else {
                                                    prompt.clone()
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("PuVQeX")),
                                        )
                                        .child(
                                            div()
                                                .h(px(38.))
                                                .px(px(10.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--input-border-default"))
                                                .bg(color("--input-background-default"))
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .text_color(color("--text-strong"))
                                                        .child(copy_name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(color("--text-muted"))
                                                        .child(format!(
                                                            "{}/128",
                                                            copy_name.chars().count()
                                                        )),
                                                ),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("cIWtCk")),
                                    )
                                    .child(div().flex().flex_col().gap(px(4.)).children(
                                        guilds.iter().enumerate().map(|(idx, (gid, gname))| {
                                            let active = selected_guild_id.as_deref() == Some(gid);
                                            let gid_owned = gid.clone();
                                            div()
                                                .id(SharedString::from(format!(
                                                    "cj-rmx-guild-{idx}"
                                                )))
                                                .h(px(38.))
                                                .px(px(10.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color(if active {
                                                    "--control-primary-background-default"
                                                } else {
                                                    "--border-subtle"
                                                }))
                                                .bg(color(if active {
                                                    "--background-mod-strong"
                                                } else {
                                                    "--background-base-lower"
                                                }))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-strong"))
                                                        .child(gname.clone()),
                                                )
                                                .when(active, |d| {
                                                    d.child(icon(
                                                        "circle_check",
                                                        16.,
                                                        "--brand-400",
                                                    ))
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(ConjureAuxModal::RemixOrCopy {
                                                        selected_guild_id,
                                                        ..
                                                    }) = this.conjure_aux_modal.as_deref_mut()
                                                    {
                                                        *selected_guild_id =
                                                            Some(gid_owned.clone());
                                                    }
                                                    cx.notify();
                                                }))
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("cj-rmx-cancel")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .when(!is_submitting, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(dc_intl::t("ETE/oC"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("cj-rmx-submit")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!can_submit, |d| d.opacity(0.5))
                                    .when(can_submit, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t(title_key))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !can_submit {
                                            return;
                                        }
                                        let Some(gid) = submit_gid.as_deref() else {
                                            this.show_app_toast(dc_intl::t("Cn8H0Y"), cx);
                                            return;
                                        };
                                        let _ = this.requests.send(conjure_create_remix_request(
                                            &submit_name,
                                            gid,
                                            "guild",
                                            &submit_pid,
                                            "nonce-312405",
                                        ));
                                        this.conjure_aux_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            ConjureAuxModal::SharedProject {
                status,
                project_id,
                app_name,
                app_description,
                surfaces,
                creator_name,
                can_remix,
            } => {
                let is_gate = !matches!(
                    status,
                    ConjureSharedProjectStatus::Ready | ConjureSharedProjectStatus::InfoOnly
                );
                if is_gate {
                    let gate_title =
                        dc_intl::t(if *status == ConjureSharedProjectStatus::Loading {
                            "ZTNur7"
                        } else {
                            "mQma36"
                        });
                    div()
                        .id("conjure-shared-gate-dialog")
                        .w(px(440.))
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
                                .p(px(20.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(gate_title),
                                )
                                .child(
                                    div()
                                        .id("cj-sp-gate-x")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "modal_close",
                                            16.,
                                            "--interactive-icon-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.conjure_aux_modal = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .px(px(20.))
                                .py(px(14.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .justify_end()
                                .child(
                                    div()
                                        .id("cj-sp-gate-close")
                                        .h(px(36.))
                                        .px(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(dc_intl::t("cpT0Cq"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.conjure_aux_modal = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .into_any_element()
                } else {
                    let is_ready = *status == ConjureSharedProjectStatus::Ready;
                    let show_remix = is_ready && *can_remix;
                    let pid_remix = project_id.clone();
                    let name_remix = app_name.clone();
                    let pid_copy = project_id.clone();
                    let name_copy = app_name.clone();
                    let initial: String = app_name
                        .chars()
                        .next()
                        .unwrap_or('A')
                        .to_ascii_uppercase()
                        .to_string();

                    div()
                        .id("conjure-shared-project-dialog")
                        .w(px(480.))
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
                                .px(px(20.))
                                .py(px(16.))
                                .border_b_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(app_name.clone()),
                                )
                                .child(
                                    div()
                                        .id("cj-sp-close")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "modal_close",
                                            16.,
                                            "--interactive-icon-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.conjure_aux_modal = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .p(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                // Identity row (`c.PJ` / `.identity_p9RQHW`)
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .flex_none()
                                                .size(px(48.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .bg(color("--brand-500"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(20.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--white"))
                                                .child(initial),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap(px(4.))
                                                .child(
                                                    div()
                                                        .text_size(px(15.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(app_name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(color("--text-default"))
                                                        .child(app_description.clone()),
                                                ),
                                        ),
                                )
                                .child(if is_ready {
                                    let ordered_surfaces: Vec<ConjureSurface> =
                                        ConjureSurface::ORDERED
                                            .into_iter()
                                            .filter(|s| surfaces.contains(s))
                                            .collect();
                                    let creator_line = creator_name.as_deref().map(|cname| {
                                        let runs = dc_intl::format_runs(
                                            "6/x9LD",
                                            &[("name", cname.into())],
                                        );
                                        let mut txt = String::new();
                                        let mut hi = Vec::new();
                                        for (seg, hook) in runs {
                                            let st = txt.len();
                                            txt.push_str(&seg);
                                            if hook.is_some() && st < txt.len() {
                                                hi.push((
                                                    st..txt.len(),
                                                    gpui::HighlightStyle {
                                                        font_weight: Some(FontWeight(700.)),
                                                        color: Some(color("--text-strong").into()),
                                                        ..gpui::HighlightStyle::default()
                                                    },
                                                ));
                                            }
                                        }
                                        (
                                            cname
                                                .chars()
                                                .next()
                                                .unwrap_or('C')
                                                .to_ascii_uppercase()
                                                .to_string(),
                                            gpui::StyledText::new(txt).with_highlights(hi),
                                        )
                                    });
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .id("cj-sp-surfaces")
                                                .aria_label(SharedString::from(dc_intl::t(
                                                    "cF++B3",
                                                )))
                                                .flex()
                                                .flex_wrap()
                                                .gap(px(6.))
                                                .children(ordered_surfaces.into_iter().map(|s| {
                                                    div()
                                                        .px(px(8.))
                                                        .py(px(4.))
                                                        .rounded(px(6.))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .bg(color("--background-mod-subtle"))
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-default"))
                                                        .child(s.label())
                                                })),
                                        )
                                        .children(creator_line.map(|(ch, styled)| {
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .size(px(20.))
                                                        .rounded_full()
                                                        .bg(color("--brand-500"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .text_size(px(10.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--white"))
                                                        .child(ch),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-default"))
                                                        .child(styled),
                                                )
                                        }))
                                        .into_any_element()
                                } else {
                                    div()
                                        .p(px(12.))
                                        .rounded(px(8.))
                                        .bg(color("--background-base-lower"))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("2DSRxV"))
                                        .into_any_element()
                                }),
                        )
                        .child(
                            div()
                                .px(px(20.))
                                .py(px(14.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(8.))
                                .child(if show_remix {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .id("cj-sp-remix")
                                                .h(px(36.))
                                                .px(px(16.))
                                                .rounded(px(8.))
                                                .bg(color("--control-secondary-background-default"))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color(
                                                    "--control-secondary-text-default",
                                                ))
                                                .child(dc_intl::t("6RGR3m"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let guilds: Vec<(String, String)> = this
                                                        .stores
                                                        .guilds
                                                        .iter()
                                                        .map(|g| (g.id.to_string(), g.name.clone()))
                                                        .collect();
                                                    let sel =
                                                        guilds.first().map(|(id, _)| id.clone());
                                                    this.conjure_aux_modal = Some(Box::new(
                                                        ConjureAuxModal::RemixOrCopy {
                                                            project_id: pid_remix.clone(),
                                                            project_name: name_remix.clone(),
                                                            mode: ConjureCopyMode::Remix,
                                                            prompt: String::new(),
                                                            copy_name: conjure_default_copy_name(
                                                                &name_remix,
                                                            ),
                                                            guilds,
                                                            selected_guild_id: sel,
                                                            submitting: false,
                                                        },
                                                    ));
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id("cj-sp-install")
                                                .h(px(36.))
                                                .px(px(16.))
                                                .rounded(px(8.))
                                                .bg(color("--control-primary-background-default"))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("UsUa/c"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let guilds: Vec<(String, String)> = this
                                                        .stores
                                                        .guilds
                                                        .iter()
                                                        .map(|g| (g.id.to_string(), g.name.clone()))
                                                        .collect();
                                                    let sel =
                                                        guilds.first().map(|(id, _)| id.clone());
                                                    this.conjure_aux_modal = Some(Box::new(
                                                        ConjureAuxModal::RemixOrCopy {
                                                            project_id: pid_copy.clone(),
                                                            project_name: name_copy.clone(),
                                                            mode: ConjureCopyMode::Copy,
                                                            prompt: String::new(),
                                                            copy_name: name_copy.clone(),
                                                            guilds,
                                                            selected_guild_id: sel,
                                                            submitting: false,
                                                        },
                                                    ));
                                                    cx.notify();
                                                })),
                                        )
                                        .into_any_element()
                                } else {
                                    div()
                                        .id("cj-sp-close-btn")
                                        .h(px(36.))
                                        .px(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(dc_intl::t("cpT0Cq"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.conjure_aux_modal = None;
                                            cx.notify();
                                        }))
                                        .into_any_element()
                                }),
                        )
                        .into_any_element()
                }
            }
        };

        Some(
            deferred(
                div()
                    .id("conjure-aux-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.conjure_aux_modal = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_conjure_aux_modal(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.conjure_aux_modal = None;
                cx.notify();
                "conjure-aux: closed".to_owned()
            }
            "patch-notes" | "patch-notes-failed" => {
                let failed = arg.trim() == "patch-notes-failed";
                self.conjure_aux_modal = Some(Box::new(ConjureAuxModal::PatchNotes {
                    project_id: "proj-300874".to_owned(),
                    project_name: "Raid Planner App".to_owned(),
                    app_channel_id: Some("9001".to_owned()),
                    channels: vec![
                        ("9001".to_owned(), "raid-announcements".to_owned()),
                        ("9002".to_owned(), "general".to_owned()),
                    ],
                    selected_channel_id: Some("9001".to_owned()),
                    text: if failed {
                        String::new()
                    } else {
                        "- Added tank & healer role signup queues\n- Live voice roster sync for raid nights"
                            .to_owned()
                    },
                    drafting: false,
                    draft_failed: failed,
                    sending: false,
                    max_len: 1960,
                }));
                cx.notify();
                format!("conjure-aux: {}", arg.trim())
            }
            "copy" | "remix" => {
                let mode = if arg.trim() == "copy" {
                    ConjureCopyMode::Copy
                } else {
                    ConjureCopyMode::Remix
                };
                self.conjure_aux_modal = Some(Box::new(ConjureAuxModal::RemixOrCopy {
                    project_id: "proj-312405".to_owned(),
                    project_name: "Raid Planner App".to_owned(),
                    mode,
                    prompt: if mode == ConjureCopyMode::Remix {
                        "Add a weekly boss loot tracker tab".to_owned()
                    } else {
                        String::new()
                    },
                    copy_name: conjure_default_copy_name("Raid Planner App"),
                    guilds: vec![
                        ("1001".to_owned(), "Graveyard Guild".to_owned()),
                        ("1002".to_owned(), "Design Sandbox".to_owned()),
                    ],
                    selected_guild_id: Some("1001".to_owned()),
                    submitting: false,
                }));
                cx.notify();
                format!("conjure-aux: {}", arg.trim())
            }
            "shared-ready" | "shared-info-only" | "shared-no-access" | "shared-loading" => {
                let status = match arg.trim() {
                    "shared-info-only" => ConjureSharedProjectStatus::InfoOnly,
                    "shared-no-access" => ConjureSharedProjectStatus::NoAccess,
                    "shared-loading" => ConjureSharedProjectStatus::Loading,
                    _ => ConjureSharedProjectStatus::Ready,
                };
                self.conjure_aux_modal = Some(Box::new(ConjureAuxModal::SharedProject {
                    status,
                    project_id: "proj-354155".to_owned(),
                    app_name: "Raid Planner App".to_owned(),
                    app_description: "Coordinate weekly raid rosters, voice signups, and loot queues right inside Discord.".to_owned(),
                    surfaces: vec![
                        ConjureSurface::AppChannel,
                        ConjureSurface::Activity,
                        ConjureSurface::VoiceChannel,
                        ConjureSurface::Bot,
                    ],
                    creator_name: Some("Lorean".to_owned()),
                    can_remix: true,
                }));
                cx.notify();
                format!("conjure-aux: {}", arg.trim())
            }
            other => format!("conjure-aux: unknown mode {other}"),
        }
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_conjure_agent_browser(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.conjure_agent_browser = None;
            cx.notify();
            return "conjure-agent-browser: closed".to_owned();
        }
        let control_state = match arg.trim() {
            "control" => AgentControlState::Controlling,
            "denied" => AgentControlState::Denied,
            _ => AgentControlState::Viewing,
        };
        self.conjure_agent_browser = Some(Box::new(ConjureAgentBrowserState {
            project_id: "proj-273594".to_owned(),
            sessions: vec![
                AgentBrowserSession {
                    id: "default".to_owned(),
                    label: agent_browser_session_label("default"),
                    ended: arg.trim() == "ended",
                    has_frame: arg.trim() != "loading",
                    last_seq: 42,
                    width: 1280,
                    height: 720,
                },
                AgentBrowserSession {
                    id: "oauth-check".to_owned(),
                    label: agent_browser_session_label("oauth-check"),
                    ended: true,
                    has_frame: true,
                    last_seq: 19,
                    width: 1280,
                    height: 720,
                },
            ],
            active_idx: 0,
            control_state,
        }));
        cx.notify();
        format!("conjure-agent-browser: opened ({})", arg.trim())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_conjure_workspace(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.conjure_workspace = None;
            cx.notify();
            return "conjure-workspace: closed".to_owned();
        }
        self.conjure_workspace = Some(ConjureWorkspaceState {
            project_id: "proj-164254".to_owned(),
            project_name: "Raid Planner App".to_owned(),
            preview_mode: ConjurePreviewMode::Frame,
            form_factor: ConjureFormFactor::Desktop,
            overlay_tool: if arg.trim() == "draw" {
                ConjureOverlayTool::EditAndDraw
            } else {
                ConjureOverlayTool::None
            },
            chat_open: true,
            debug_open: arg.trim() == "debug",
            debug_tab: ConjureDebugTab::Logs,
            todos: vec![
                ("Create lobby grid layout".to_owned(), true),
                ("Wire role signup buttons".to_owned(), false),
            ],
            plan_title: Some("Voice Lobby Roster".to_owned()),
            plan_steps: vec![
                "Add tank/healer/DPS role columns".to_owned(),
                "Sync active voice participants via SDK".to_owned(),
            ],
        });
        cx.notify();
        format!("conjure-workspace: opened ({})", arg.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conjure_workspace_164254_requests_and_intl() {
        for key in [
            "/VQax8", "RDE0Sc", "cpT0Cq", "XWU+XF", "cl/Jyl", "RtzECX", "3b6e7o", "6S+wRM",
            "Q4FN+H", "+VRYCm", "50D0FZ", "UkbTK1", "/TlGcK",
        ] {
            assert!(dc_intl::has_key(key), "missing 164254 key {key}");
        }
        assert!(matches!(
            conjure_ws_ticket_request("p1"),
            Request::Rest { route, .. } if route == "/conjure/projects/p1/ws-ticket"
        ));
        assert!(matches!(
            conjure_install_scope_request("p1", "guild", Some("99")),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/conjure/projects/p1/install-scope" && b["guild_id"] == "99"
        ));
        assert!(matches!(
            conjure_preview_dm_request("p1"),
            Request::Rest { route, .. } if route == "/conjure/projects/p1/preview-dm"
        ));
    }

    #[test]
    fn conjure_agent_browser_273594_frame_parser_and_intl() {
        for key in [
            "sGsShB", "UjUsph", "gHsgGe", "FkosiV", "OO/He7", "d5Slrv", "7lQQJ5", "Wuww13",
            "rnwqXb", "wuE3Ja", "ocRH08", "cpT0Cq",
        ] {
            assert!(dc_intl::has_key(key), "missing 273594 key {key}");
        }
        assert_eq!(agent_browser_session_label("default"), dc_intl::t("gHsgGe"));
        assert_eq!(agent_browser_session_label(""), dc_intl::t("gHsgGe"));
        assert_eq!(agent_browser_session_label("auth-tab"), "auth-tab");
        // Version 1, seq=7, width=1280, height=720, payload=[0xff, 0xd8]
        let mut buf = vec![1u8];
        buf.extend_from_slice(&7u32.to_be_bytes());
        buf.extend_from_slice(&1280u16.to_be_bytes());
        buf.extend_from_slice(&720u16.to_be_bytes());
        buf.extend_from_slice(&[0xff, 0xd8]);
        let (seq, w, h, jpg) = parse_agent_browser_frame(&buf).expect("valid frame");
        assert_eq!((seq, w, h, jpg), (7, 1280, 720, &[0xff, 0xd8][..]));
        assert!(parse_agent_browser_frame(&[2, 0, 0, 0, 1, 0, 1, 0, 1, 0]).is_none());
    }

    #[test]
    fn conjure_patch_notes_300874_and_remix_312405_requests_and_intl() {
        for key in [
            "lsYQII", "ETE/oC", "69aIG4", "r4du8k", "aYQksU", "3hV1Gc", "Em8bo+", "Gd63Fl",
            "7CvxMC", "2ECgBx", "6oEjjD", "UsUa/c", "276ll6", "GzGb2w", "QN8T+C", "cvvDlE",
            "u3W0k0", "PuVQeX", "cIWtCk", "Cn8H0Y",
        ] {
            assert!(dc_intl::has_key(key), "missing 300874/312405 key {key}");
        }
        assert_eq!(conjure_default_copy_name("My App"), "My App (Copy)");
        assert_eq!(
            conjure_default_copy_name(&"a".repeat(200)).chars().count(),
            128
        );
        assert!(matches!(
            conjure_patch_notes_send_request("55", "v2 shipped", Some("77")),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/channels/55/messages"
                    && b["content"].as_str().is_some_and(|s| s.contains("v2 shipped") && s.contains("<#77>"))
        ));
        assert!(matches!(
            conjure_create_remix_request("My App (Copy)", "99", "guild", "p1", "n1"),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/conjure/projects"
                    && b["guild_id"] == "99"
                    && b["remix_source_project_id"] == "p1"
        ));
    }

    #[test]
    fn conjure_shared_project_354155_surfaces_routes_and_intl() {
        for key in [
            "cpT0Cq", "ZTNur7", "mQma36", "2DSRxV", "6RGR3m", "UsUa/c", "cF++B3", "6/x9LD",
            "bHLrO8", "hp+8nc", "4V2QWl", "oJ06Po", "Qbnej2", "pEwndv", "HWOzBW", "gsIXM9",
        ] {
            assert!(dc_intl::has_key(key), "missing 354155 key {key}");
        }
        assert_eq!(ConjureSurface::ORDERED.len(), 8);
        for s in ConjureSurface::ORDERED {
            assert!(!s.label().is_empty());
        }
        assert!(matches!(
            conjure_project_limit_request(),
            Request::Rest { method, route, .. }
                if method == reqwest::Method::GET && route == "/conjure/project-limit"
        ));
        assert!(matches!(
            conjure_project_fetch_request("proj-1"),
            Request::Rest { method, route, .. }
                if method == reqwest::Method::GET && route == "/conjure/projects/proj-1"
        ));
    }
}
