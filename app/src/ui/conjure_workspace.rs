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
}
