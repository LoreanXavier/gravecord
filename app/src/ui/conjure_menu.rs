//! Conjure Project Actions Menu & Destructive Confirmation Modals (module 753329, secondary
//! bundle 191850).
//!
//! Renders the 8-group Conjure project actions menu (`ogxXGq` "Actions", `UKOtz+` "More"):
//! 1. Publish (`120EFN`)
//! 2. Pause/Resume App (`VHxerm`/`ZKd2Cw`) + Pop out/Pop in app (`3Zypbv`/`NKV/MO`, disabled
//!    with subtext `WjdaFw` while Edit & Draw is active)
//! 3. Debug Panel checkbox (`Mt5k9d`)
//! 4. Refresh Profile Widget (`/nOi5n`) + Connect `{label}` (`DEwmI5` / `GnHcWc`)
//! 5. Mute/Unmute Project (`a+i/As`/`s9rCuH`) + Dismiss (`bWzWw9`)
//! 6. Make a Copy (`276ll6`), Export (`WsEEP7`), Import (`rWGY3e`), MCP (`yOIql5`), History (`3hIVou`)
//! 7. Copy Link (`WqhZss` -> toast `L/PwZf`), Copy Project ID (`nm/zuU` -> toast `CmfaZG`)
//! 8. Project Settings (`FzfmQ8`), Remove from `{server}` (`3gKG2j`), Unpublish (`jYAR58`),
//!    Delete (`oyYWHE`), opening confirmation modals (`CJBhb2`/`0OmrVn`/`aC42bN`,
//!    `x6FvsZ`/`8OKM1N`/`BGF8VT`, `m8KWic`/`wve4kB`/`GqcQyH`, cancel `ETE/oC`) backed by
//!    `DELETE /conjure/projects/{id}` and `POST /conjure/projects/{id}/unpublish`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::json;
use std::borrow::Cow;

pub(crate) const CONJURE_DELETE_KEY: &str = "conjure:delete-project";
pub(crate) const CONJURE_UNPUBLISH_KEY: &str = "conjure:unpublish-project";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConjureConfirmKind {
    Delete,
    RemoveFromServer,
    Unpublish,
}

#[derive(Clone, Debug)]
pub(crate) struct ConjureActionsState {
    pub project_id: String,
    pub project_name: String,
    pub server_name: Option<String>,
    pub paused: bool,
    pub popped_out: bool,
    pub edit_draw_active: bool,
    pub debug_panel: bool,
    pub muted: bool,
    pub connection_label: Option<String>,
    pub confirm: Option<ConjureConfirmKind>,
}

/// `DELETE /conjure/projects/{id}` (module 753329 / 233470).
pub(crate) fn delete_project_request(project_id: &str) -> Request {
    Request::Rest {
        key: Some(CONJURE_DELETE_KEY.to_owned()),
        method: reqwest::Method::DELETE,
        route: format!("/conjure/projects/{project_id}"),
        body: None,
    }
}

/// `POST /conjure/projects/{id}/unpublish` (module 753329 / 664619).
pub(crate) fn unpublish_project_request(project_id: &str, remove_bot: bool) -> Request {
    Request::Rest {
        key: Some(CONJURE_UNPUBLISH_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/conjure/projects/{project_id}/unpublish"),
        body: Some(json!({ "also_remove_preview_bot": remove_bot })),
    }
}

impl Root {
    /// Renders the Conjure Project Actions Menu & confirmation dialog layer (`753329`).
    pub(super) fn conjure_actions_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.conjure_actions.as_ref()?;
        if let Some(kind) = state.confirm {
            return Some(self.render_conjure_confirm_modal(state, kind, cx));
        }
        Some(self.render_conjure_menu(state, cx))
    }

    fn render_conjure_menu(
        &self,
        state: &ConjureActionsState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sep = || div().my(px(3.)).h(px(1.)).bg(color("--border-subtle"));
        let pid = state.project_id.clone();
        let pid_id = state.project_id.clone();
        let pop_disabled = state.edit_draw_active;
        let pop_label = if state.popped_out {
            dc_intl::t("NKV/MO")
        } else {
            dc_intl::t("3Zypbv")
        };
        let pause_label = if state.paused {
            dc_intl::t("ZKd2Cw")
        } else {
            dc_intl::t("VHxerm")
        };
        let mute_label = if state.muted {
            dc_intl::t("s9rCuH")
        } else {
            dc_intl::t("a+i/As")
        };

        let row = |id: &'static str,
                   label: String,
                   danger: bool,
                   sub: Option<&'static str>,
                   disabled: bool| {
            div()
                .id(id)
                .when(!disabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                })
                .when(disabled, |d| d.opacity(0.45))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(if danger {
                            color("--text-feedback-critical")
                        } else {
                            color("--text-default")
                        })
                        .child(label),
                )
                .children(sub.map(|s| {
                    div()
                        .text_size(px(11.))
                        .text_color(color("--text-muted"))
                        .child(s)
                }))
        };

        let menu = div()
            .id("conjure-actions-menu")
            .aria_label(dc_intl::t("ogxXGq"))
            .w(px(250.))
            .rounded(px(10.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(6.))
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Group 1: Publish
            .child(
                row(
                    "conjure-publish",
                    dc_intl::t("120EFN").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.conjure_actions = None;
                    this.put_app_toast("Published project.".to_owned(), true, cx);
                })),
            )
            .child(sep())
            // Group 2: Pause/Resume + Pop out/Pop in app
            .child(
                row("conjure-pause", pause_label.to_owned(), false, None, false).on_click(
                    cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.conjure_actions.as_mut() {
                            s.paused = !s.paused;
                        }
                        cx.notify();
                    }),
                ),
            )
            .child(
                row(
                    "conjure-popout",
                    pop_label.to_owned(),
                    false,
                    pop_disabled.then(|| dc_intl::t("WjdaFw")),
                    pop_disabled,
                )
                .when(!pop_disabled, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.conjure_actions.as_mut() {
                            s.popped_out = !s.popped_out;
                        }
                        cx.notify();
                    }))
                }),
            )
            .child(sep())
            // Group 3: Debug Panel checkbox
            .child(
                div()
                    .id("conjure-debug-panel")
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.conjure_actions.as_mut() {
                            s.debug_panel = !s.debug_panel;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("Mt5k9d")),
                    )
                    .child(
                        div()
                            .size(px(16.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if state.debug_panel {
                                color("--control-primary-background-default")
                            } else {
                                color("--border-subtle")
                            })
                            .bg(if state.debug_panel {
                                color("--control-primary-background-default")
                            } else {
                                color("--background-base-lowest")
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(state.debug_panel, |d| {
                                d.child(icon("checkbox_check", 11., "--white"))
                            }),
                    ),
            )
            .child(sep())
            // Group 4: Refresh Profile Widget + Connect
            .child(
                row(
                    "conjure-refresh-widget",
                    dc_intl::t("/nOi5n").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.conjure_actions = None;
                    cx.notify();
                })),
            )
            .children(state.connection_label.as_ref().map(|lbl| {
                let text = dc_intl::format("DEwmI5", &[("label", Arg::Str(Cow::Borrowed(lbl)))]);
                row("conjure-connect", text, false, None, false)
            }))
            .child(sep())
            // Group 5: Mute/Unmute + Dismiss
            .child(
                row("conjure-mute", mute_label.to_owned(), false, None, false).on_click(
                    cx.listener(|this, _, _, cx| {
                        if let Some(s) = this.conjure_actions.as_mut() {
                            s.muted = !s.muted;
                        }
                        cx.notify();
                    }),
                ),
            )
            .child(
                row(
                    "conjure-dismiss",
                    dc_intl::t("bWzWw9").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.conjure_actions = None;
                    cx.notify();
                })),
            )
            .child(sep())
            // Group 6: Make a Copy, Export, Import, MCP, History
            .child(row(
                "conjure-copy-proj",
                dc_intl::t("276ll6").to_owned(),
                false,
                None,
                false,
            ))
            .child(row(
                "conjure-export",
                dc_intl::t("WsEEP7").to_owned(),
                false,
                None,
                false,
            ))
            .child(row(
                "conjure-import",
                dc_intl::t("rWGY3e").to_owned(),
                false,
                None,
                false,
            ))
            .child(row(
                "conjure-mcp",
                dc_intl::t("yOIql5").to_owned(),
                false,
                None,
                false,
            ))
            .child(row(
                "conjure-history",
                dc_intl::t("3hIVou").to_owned(),
                false,
                None,
                false,
            ))
            .child(sep())
            // Group 7: Copy Link + Copy Project ID
            .child(
                row(
                    "conjure-copy-link",
                    dc_intl::t("WqhZss").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                        "https://discord.com/apps/{pid}"
                    )));
                    this.conjure_actions = None;
                    this.put_app_toast(dc_intl::t("L/PwZf").to_owned(), true, cx);
                })),
            )
            .child(
                row(
                    "conjure-copy-id",
                    dc_intl::t("nm/zuU").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(pid_id.clone()));
                    this.conjure_actions = None;
                    this.put_app_toast(dc_intl::t("CmfaZG").to_owned(), true, cx);
                })),
            )
            .child(sep())
            // Group 8: Project Settings + Danger actions
            .child(row(
                "conjure-settings",
                dc_intl::t("FzfmQ8").to_owned(),
                false,
                None,
                false,
            ))
            .children(state.server_name.as_ref().map(|srv| {
                let text = dc_intl::format("3gKG2j", &[("server", Arg::Str(Cow::Borrowed(srv)))]);
                row("conjure-remove-srv", text, true, None, false).on_click(cx.listener(
                    |this, _, _, cx| {
                        if let Some(s) = this.conjure_actions.as_mut() {
                            s.confirm = Some(ConjureConfirmKind::RemoveFromServer);
                        }
                        cx.notify();
                    },
                ))
            }))
            .child(
                row(
                    "conjure-unpublish",
                    dc_intl::t("jYAR58").to_owned(),
                    true,
                    None,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(s) = this.conjure_actions.as_mut() {
                        s.confirm = Some(ConjureConfirmKind::Unpublish);
                    }
                    cx.notify();
                })),
            )
            .child(
                row(
                    "conjure-delete",
                    dc_intl::t("oyYWHE").to_owned(),
                    true,
                    None,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(s) = this.conjure_actions.as_mut() {
                        s.confirm = Some(ConjureConfirmKind::Delete);
                    }
                    cx.notify();
                })),
            );

        deferred(
            div()
                .id("conjure-actions-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x00000080))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.conjure_actions = None;
                    cx.notify();
                }))
                .child(menu),
        )
        .with_priority(96)
        .into_any_element()
    }

    fn render_conjure_confirm_modal(
        &self,
        state: &ConjureActionsState,
        kind: ConjureConfirmKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = state.project_name.as_str();
        let (title, body, cta) = match kind {
            ConjureConfirmKind::Delete => (
                dc_intl::format("CJBhb2", &[("name", Arg::Str(Cow::Borrowed(name)))]),
                dc_intl::t("0OmrVn").to_owned(),
                dc_intl::t("aC42bN"),
            ),
            ConjureConfirmKind::RemoveFromServer => (
                dc_intl::format("x6FvsZ", &[("app", Arg::Str(Cow::Borrowed(name)))]),
                dc_intl::t("8OKM1N").to_owned(),
                dc_intl::t("BGF8VT"),
            ),
            ConjureConfirmKind::Unpublish => (
                dc_intl::format("m8KWic", &[("app", Arg::Str(Cow::Borrowed(name)))]),
                dc_intl::t("wve4kB").to_owned(),
                dc_intl::t("GqcQyH"),
            ),
        };
        let pid = state.project_id.clone();
        let pname = state.project_name.clone();

        deferred(
            div()
                .id("conjure-confirm-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.conjure_actions = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("conjure-confirm-modal")
                        .w(px(440.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(20.))
                                .pb(px(16.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(body),
                                ),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .py(px(14.))
                                .bg(color("--background-base-lower"))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .justify_end()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .id("conjure-confirm-cancel")
                                        .cursor_pointer()
                                        .px(px(14.))
                                        .h(px(36.))
                                        .rounded(px(6.))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("ETE/oC"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(s) = this.conjure_actions.as_mut() {
                                                s.confirm = None;
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("conjure-confirm-submit")
                                        .cursor_pointer()
                                        .px(px(18.))
                                        .h(px(36.))
                                        .rounded(px(6.))
                                        .bg(color("--control-critical-primary-background-default"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(cta)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            match kind {
                                                ConjureConfirmKind::Delete => {
                                                    let _ = this
                                                        .requests
                                                        .send(delete_project_request(&pid));
                                                }
                                                ConjureConfirmKind::RemoveFromServer => {
                                                    let _ = this.requests.send(
                                                        unpublish_project_request(&pid, true),
                                                    );
                                                }
                                                ConjureConfirmKind::Unpublish => {
                                                    let _ = this.requests.send(
                                                        unpublish_project_request(&pid, false),
                                                    );
                                                    this.put_app_toast(
                                                        dc_intl::format(
                                                            "b8LN7N",
                                                            &[(
                                                                "app",
                                                                Arg::Str(Cow::Borrowed(&pname)),
                                                            )],
                                                        ),
                                                        true,
                                                        cx,
                                                    );
                                                }
                                            }
                                            this.conjure_actions = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                ),
        )
        .with_priority(96)
        .into_any_element()
    }

    /// Capture harness: `conjure-menu [menu|delete|unpublish|remove|close]` (module 753329).
    #[cfg(feature = "capture")]
    pub(crate) fn script_conjure_menu(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let base = ConjureActionsState {
            project_id: "proj_demo_753".to_owned(),
            project_name: "Star Tracker".to_owned(),
            server_name: Some("gravecord fixture".to_owned()),
            paused: false,
            popped_out: false,
            edit_draw_active: false,
            debug_panel: true,
            muted: false,
            connection_label: Some("GitHub".to_owned()),
            confirm: None,
        };
        match arg.trim() {
            "close" => {
                self.conjure_actions = None;
                cx.notify();
                "conjure-menu: closed".to_owned()
            }
            "delete" => {
                self.conjure_actions = Some(ConjureActionsState {
                    confirm: Some(ConjureConfirmKind::Delete),
                    ..base
                });
                cx.notify();
                "conjure-menu: delete".to_owned()
            }
            "unpublish" => {
                self.conjure_actions = Some(ConjureActionsState {
                    confirm: Some(ConjureConfirmKind::Unpublish),
                    ..base
                });
                cx.notify();
                "conjure-menu: unpublish".to_owned()
            }
            "remove" => {
                self.conjure_actions = Some(ConjureActionsState {
                    confirm: Some(ConjureConfirmKind::RemoveFromServer),
                    ..base
                });
                cx.notify();
                "conjure-menu: remove".to_owned()
            }
            _ => {
                self.conjure_actions = Some(base);
                cx.notify();
                "conjure-menu: menu".to_owned()
            }
        }
    }
}
