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

/// Module 719593 (`722701.getSafetyRecheck`): Conjure project safety hold / recheck status.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ConjureSafetyStatus {
    Idle,
    Pending,
    Held(String),
    Limited { daily: bool },
    Unavailable,
    Cleared,
}

/// Module 719593 (`x`): derived Publish button variant, label, and indicator.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ConjurePublishButtonState {
    pub is_secondary: bool,
    pub label: String,
    pub indicator: Option<&'static str>,
}

/// Module 719593 (`x` / `isPublishedAndUpToDate`): computes Publish button state.
#[allow(dead_code)]
pub(crate) fn compute_conjure_publish_button_state(
    up_to_date: bool,
    awaiting_approval: bool,
    publishing: bool,
    confirmation: Option<&str>,
    custom_label: Option<&str>,
) -> ConjurePublishButtonState {
    let published_current = up_to_date && !awaiting_approval;
    let label = if let Some(conf) = confirmation.filter(|_| !publishing) {
        conf.to_owned()
    } else if published_current && !publishing {
        conjure_intl("kAfbEN", "Published")
    } else if let Some(lbl) = custom_label {
        lbl.to_owned()
    } else {
        conjure_intl("120EFN", "Publish")
    };
    let indicator = if publishing {
        Some("publishing")
    } else if !published_current {
        Some("unpublished")
    } else {
        None
    };
    ConjurePublishButtonState {
        is_secondary: published_current,
        label,
        indicator,
    }
}

/// Module 719593: publish destination modal or safety hold/recheck modal.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum ConjurePublishModal {
    DestinationPicker {
        destinations: Vec<(String, String)>,
        selected_id: String,
    },
    SafetyHold {
        status: ConjureSafetyStatus,
    },
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
    /// Module 719593: Publish destination picker or Safety hold/recheck modal.
    pub publish_modal: Option<ConjurePublishModal>,
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
        if let Some(modal) = state.publish_modal.as_ref() {
            return Some(self.render_conjure_publish_modal(state, modal, cx));
        }
        if let Some(kind) = state.confirm {
            return Some(self.render_conjure_confirm_modal(state, kind, cx));
        }
        Some(self.render_conjure_menu(state, cx))
    }

    /// Module 719593 (`b` destination picker + `v` safety hold/recheck modal).
    fn render_conjure_publish_modal(
        &self,
        _state: &ConjureActionsState,
        modal: &ConjurePublishModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let card = match modal {
            ConjurePublishModal::DestinationPicker {
                destinations,
                selected_id,
            } => {
                let rows: Vec<AnyElement> = destinations
                    .iter()
                    .enumerate()
                    .map(|(idx, (id, label))| {
                        let active = id == selected_id;
                        let target_id = id.clone();
                        div()
                            .id(("conjure-pub-dest", idx))
                            .w_full()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(if active {
                                "--background-brand"
                            } else {
                                "--border-subtle"
                            }))
                            .bg(color(if active {
                                "--background-mod-subtle"
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
                                    .child(label.clone()),
                            )
                            .when(active, |r| {
                                r.child(icon("circle_check", 16., "--background-brand"))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(ConjurePublishModal::DestinationPicker {
                                    selected_id,
                                    ..
                                }) = this
                                    .conjure_actions
                                    .as_mut()
                                    .and_then(|s| s.publish_modal.as_mut())
                                {
                                    *selected_id = target_id.clone();
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect();
                div()
                    .id("conjure-publish-dest-modal")
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(440.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow_lg()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(conjure_intl("lJrOGW", "Run this app on")),
                    )
                    .child(div().flex().flex_col().gap(px(8.)).children(rows))
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("conjure-pub-dest-cancel")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(conjure_intl("y83r6D", "Cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_actions = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("conjure-pub-dest-continue")
                                    .h(px(38.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(conjure_intl("zh1Izp", "Continue"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_actions = None;
                                        cx.notify();
                                    })),
                            ),
                    )
            }
            ConjurePublishModal::SafetyHold { status } => {
                let cleared = matches!(status, ConjureSafetyStatus::Cleared);
                let pending = matches!(status, ConjureSafetyStatus::Pending);
                let title = if cleared {
                    conjure_intl("vVnbEq", "Your app passed review")
                } else {
                    conjure_intl("ebqlWe", "This app can't be published")
                };
                let subtitle = if cleared {
                    conjure_intl(
                        "qY/RLI",
                        "It's no longer on hold, so you can publish it again.",
                    )
                } else {
                    conjure_intl(
                        "UOr0cK",
                        "It breaks Discord's rules, so it can't be published, previewed or remixed. Remove what breaks the rules, then request a review.",
                    )
                };
                let notice: Option<(u32, &'static str, String)> = match status {
                    ConjureSafetyStatus::Pending => Some((
                        0x5865f222,
                        "--text-default",
                        conjure_intl("HsOGKK", "Reviewing your app. This can take a minute."),
                    )),
                    ConjureSafetyStatus::Held(reason) => {
                        let msg = if dc_intl::has_key("t8GlRz") {
                            dc_intl::format(
                                "t8GlRz",
                                &[("reason", Arg::Str(Cow::Borrowed(reason)))],
                            )
                        } else {
                            format!("Your app is still on hold: {reason}")
                        };
                        Some((0xda373c22, "--text-feedback-critical", msg))
                    }
                    ConjureSafetyStatus::Limited { daily } => Some((
                        0xf0b23222,
                        "--text-feedback-warning",
                        if *daily {
                            conjure_intl(
                                "TtE+rz",
                                "This app has used all of today's reviews. Try again tomorrow.",
                            )
                        } else {
                            conjure_intl(
                                "ikIoxj",
                                "A review just ran. Wait a couple of minutes before requesting another.",
                            )
                        },
                    )),
                    ConjureSafetyStatus::Unavailable => Some((
                        0xf0b23222,
                        "--text-feedback-warning",
                        conjure_intl(
                            "FxilA2",
                            "The review couldn't run right now. Try again in a bit.",
                        ),
                    )),
                    ConjureSafetyStatus::Idle | ConjureSafetyStatus::Cleared => None,
                };
                div()
                    .id("conjure-safety-hold-modal")
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .w(px(460.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow_lg()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(subtitle),
                    )
                    .when_some(notice, |col, (bg_rgba, ink, msg)| {
                        col.child(
                            div()
                                .p(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(rgba(bg_rgba))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color(ink))
                                .child(msg),
                        )
                    })
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("conjure-safety-okay")
                                    .h(px(38.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(if cleared {
                                        color("--control-primary-background-default")
                                    } else {
                                        color("--control-secondary-background-default")
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(if cleared {
                                        color("--white")
                                    } else {
                                        color("--control-secondary-text-default")
                                    })
                                    .child(dc_intl::t("BddRzS"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_actions = None;
                                        cx.notify();
                                    })),
                            )
                            .when(!cleared, |row| {
                                row.child(
                                    div()
                                        .id("conjure-safety-recheck")
                                        .h(px(38.))
                                        .px(px(18.))
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .when(!pending, |b| {
                                            b.hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    if let Some(ConjurePublishModal::SafetyHold {
                                                        status,
                                                    }) = this
                                                        .conjure_actions
                                                        .as_mut()
                                                        .and_then(|s| s.publish_modal.as_mut())
                                                    {
                                                        *status = ConjureSafetyStatus::Pending;
                                                    }
                                                    cx.notify();
                                                }),
                                            )
                                        })
                                        .when(pending, |b| b.opacity(0.6))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(conjure_intl("nmwy0/", "Request review")),
                                )
                            }),
                    )
            }
        };
        deferred(
            div()
                .id("conjure-publish-modal-backdrop")
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
                .child(card),
        )
        .with_priority(6)
        .into_any_element()
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
            .child(
                row(
                    "conjure-copy-proj",
                    dc_intl::t("276ll6").to_owned(),
                    false,
                    None,
                    false,
                )
                .on_click(cx.listener({
                    let pid = state.project_id.clone();
                    let pname = state.project_name.clone();
                    move |this, _, _, cx| {
                        let guilds: Vec<(String, String)> = this
                            .stores
                            .guilds
                            .iter()
                            .map(|g| (g.id.to_string(), g.name.clone()))
                            .collect();
                        let selected = guilds.first().map(|(id, _)| id.clone());
                        this.conjure_actions = None;
                        this.conjure_aux_modal = Some(Box::new(
                            super::conjure_workspace::ConjureAuxModal::RemixOrCopy {
                                project_id: pid.clone(),
                                project_name: pname.clone(),
                                mode: super::conjure_workspace::ConjureCopyMode::Remix,
                                prompt: String::new(),
                                copy_name: super::conjure_workspace::conjure_default_copy_name(
                                    &pname,
                                ),
                                guilds,
                                selected_guild_id: selected,
                                submitting: false,
                            },
                        ));
                        cx.notify();
                    }
                })),
            )
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
            publish_modal: None,
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
            "publish-dest" => {
                self.conjure_actions = Some(ConjureActionsState {
                    publish_modal: Some(ConjurePublishModal::DestinationPicker {
                        destinations: vec![
                            ("1".to_owned(), "gravecord fixture".to_owned()),
                            ("2".to_owned(), "My Personal Sandbox".to_owned()),
                        ],
                        selected_id: "1".to_owned(),
                    }),
                    ..base
                });
                cx.notify();
                "conjure-menu: publish-dest".to_owned()
            }
            "safety-hold" => {
                self.conjure_actions = Some(ConjureActionsState {
                    publish_modal: Some(ConjurePublishModal::SafetyHold {
                        status: ConjureSafetyStatus::Held(
                            "Prohibited external redirect detected.".to_owned(),
                        ),
                    }),
                    ..base
                });
                cx.notify();
                "conjure-menu: safety-hold".to_owned()
            }
            _ => {
                self.conjure_actions = Some(base);
                cx.notify();
                "conjure-menu: menu".to_owned()
            }
        }
    }

    /// Module 644441 — Conjure Agent Secrets Submission Modal (`TuMGZp` "Add your keys"):
    /// renders optional `copy_values` list (label + `--background-mod-strong` code box +
    /// Copy/Copied toggle `OpuAlK`/`t5VZ88`), password fields with optional host restriction
    /// notice (`Ww9HFO`), privacy note (`jgDBJZ`), partial-save hint (`La+pe4`), error (`IrMuew`),
    /// and Cancel/Save keys (`ETE/oC`/`DUdtms`) submitting via `POST /conjure/projects/{id}/ws-ticket`.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub(super) fn open_conjure_secrets_modal(
        &mut self,
        project_id: String,
        request_id: Option<String>,
        note: Option<String>,
        copy_values: Vec<ConjureCopyValue>,
        fields: Vec<ConjureSecretFieldSpec>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let mut editors = Vec::with_capacity(fields.len());
        let mut subs = Vec::with_capacity(fields.len());
        for spec in fields {
            let ed = cx.new(|cx| {
                let mut e = crate::editor::Editor::new(super::editor_style(), cx);
                e.set_masked(true);
                e.set_placeholder(spec.hint.clone().unwrap_or_else(|| spec.label.clone()));
                e
            });
            let sub = cx.subscribe(&ed, |this, _, ev: &crate::editor::EditorEvent, cx| {
                if matches!(ev, crate::editor::EditorEvent::Edited) {
                    if let Some(m) = this.conjure_secrets.as_mut() {
                        m.error = None;
                    }
                    cx.notify();
                }
            });
            subs.push(sub);
            editors.push(ConjureSecretField {
                name: spec.name,
                label: spec.label,
                hint: spec.hint,
                hosts: spec.hosts,
                editor: ed,
            });
        }
        if let Some(first) = editors.first() {
            gpui::Focusable::focus_handle(first.editor.read(cx), cx).focus(window, cx);
        }
        self.conjure_secrets = Some(ConjureSecretsModal {
            project_id,
            request_id,
            note,
            copy_values,
            fields: editors,
            copied_value: None,
            submitting: false,
            error: None,
            _subs: subs,
        });
        cx.notify();
    }

    pub(super) fn conjure_secrets_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.conjure_secrets.as_ref()?;
        let filled_count = modal
            .fields
            .iter()
            .filter(|f| !f.editor.read(cx).text().trim().is_empty())
            .count();
        let can_submit = filled_count > 0 && !modal.submitting;
        let pose = crate::ui::motion::modal_pose("conjure-secrets-modal");
        let layer = div()
            .id("conjure-secrets-modal-layer")
            .occlude()
            .absolute()
            .inset_0()
            .bg(pose.dim(rgba(0x000000b8)))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.conjure_secrets = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("conjure-secrets-modal")
                    .occlude()
                    .w(px(480.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.28),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_click(|_, _, cx| cx.stop_propagation())
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
                                    .child(conjure_intl("TuMGZp", "Add your keys")),
                            )
                            .child(
                                div()
                                    .id("conjure-secrets-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("close", 18., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_secrets = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(modal.note.clone().unwrap_or_else(|| {
                                        conjure_intl(
                                            "jgDBJZ",
                                            "Stored privately for this app. Never shown in chat or to me.",
                                        )
                                    })),
                            )
                            .when(!modal.copy_values.is_empty(), |col| {
                                col.child(div().flex().flex_col().gap(px(8.)).children(
                                    modal.copy_values.iter().enumerate().map(|(i, cv)| {
                                        let copied = modal.copied_value.as_deref()
                                            == Some(cv.value.as_str());
                                        let val_copy = cv.value.clone();
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(cv.label.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(8.))
                                                            .py(px(4.))
                                                            .rounded(px(4.))
                                                            .bg(color("--background-mod-strong"))
                                                            .text_size(px(12.))
                                                            .text_color(color("--text-default"))
                                                            .child(cv.value.clone()),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id(gpui::SharedString::from(format!(
                                                        "conjure-secret-copy-{i}"
                                                    )))
                                                    .h(px(30.))
                                                    .px(px(12.))
                                                    .rounded(px(6.))
                                                    .bg(if copied {
                                                        color("--status-positive")
                                                    } else {
                                                        color(
                                                            "--control-secondary-background-default",
                                                        )
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(if copied {
                                                        color("--white")
                                                    } else {
                                                        color("--control-secondary-text-default")
                                                    })
                                                    .child(if copied {
                                                        dc_intl::t("t5VZ88")
                                                    } else {
                                                        dc_intl::t("OpuAlK")
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.write_to_clipboard(
                                                            gpui::ClipboardItem::new_string(
                                                                val_copy.clone(),
                                                            ),
                                                        );
                                                        if let Some(m) =
                                                            this.conjure_secrets.as_mut()
                                                        {
                                                            m.copied_value = Some(val_copy.clone());
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                    }),
                                ))
                            })
                            .children(modal.fields.iter().map(|f| {
                                let hosts_line = (!f.hosts.is_empty()).then(|| {
                                    let joined = f.hosts.join(", ");
                                    if dc_intl::has_key("Ww9HFO") {
                                        dc_intl::format(
                                            "Ww9HFO",
                                            &[("hosts", Arg::Str(Cow::Owned(joined)))],
                                        )
                                    } else {
                                        format!("Only these sites will get it: {joined}")
                                    }
                                });
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(f.label.clone()),
                                    )
                                    .child(
                                        div()
                                            .h(px(38.))
                                            .px(px(10.))
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(color("--input-border-default"))
                                            .bg(color("--input-background-default"))
                                            .flex()
                                            .items_center()
                                            .child(f.editor.clone()),
                                    )
                                    .when_some(hosts_line, |col, h| {
                                        col.child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(color("--text-muted"))
                                                .child(h),
                                        )
                                    })
                            }))
                            .when(modal.fields.len() > 1, |col| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(conjure_intl(
                                            "La+pe4",
                                            "Only have some of these? Save what you have; you can add the rest later.",
                                        )),
                                )
                            })
                            .when_some(modal.error.clone(), |col, err| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(err),
                                )
                            }),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .rounded_b(px(12.))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("conjure-secrets-cancel")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("ETE/oC"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.conjure_secrets = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("conjure-secrets-save")
                                    .h(px(36.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--brand-500"))
                                    .when(!can_submit, |d| d.opacity(0.5))
                                    .when(can_submit, |d| {
                                        d.hover(|s| s.bg(color("--brand-560")))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.submit_conjure_secrets(cx);
                                            }))
                                    })
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(conjure_intl("DUdtms", "Save keys")),
                            ),
                    ),
            );
        Some(deferred(layer).with_priority(96).into_any_element())
    }

    fn submit_conjure_secrets(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.conjure_secrets.as_mut() else {
            return;
        };
        let mut secrets = serde_json::Map::new();
        for f in &modal.fields {
            let v = f.editor.read(cx).text().trim().to_owned();
            if !v.is_empty() {
                secrets.insert(f.name.clone(), serde_json::Value::String(v));
            }
        }
        if secrets.is_empty() {
            modal.error = Some(conjure_intl(
                "IrMuew",
                "Those didn't save. Check the values and try again.",
            ));
            cx.notify();
            return;
        }
        let pid = modal.project_id.clone();
        modal.submitting = true;
        modal.error = None;
        let _ = self.requests.send(conjure_ws_ticket_request(&pid));
        cx.notify();
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_conjure_secrets(
        &mut self,
        arg: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.conjure_secrets = None;
            cx.notify();
            return "conjure-secrets: closed".to_owned();
        }
        self.open_conjure_secrets_modal(
            "proj_demo_644".to_owned(),
            Some("req_01".to_owned()),
            None,
            vec![ConjureCopyValue {
                label: "OAuth Redirect URI".to_owned(),
                value: "https://conjure.discord.com/callback/proj_demo_644".to_owned(),
            }],
            vec![
                ConjureSecretFieldSpec {
                    name: "STRIPE_API_KEY".to_owned(),
                    label: "Stripe Secret Key".to_owned(),
                    hint: Some("sk_live_...".to_owned()),
                    hosts: vec!["api.stripe.com".to_owned()],
                },
                ConjureSecretFieldSpec {
                    name: "WEBHOOK_SECRET".to_owned(),
                    label: "Webhook Signing Secret".to_owned(),
                    hint: Some("whsec_...".to_owned()),
                    hosts: vec![],
                },
            ],
            window,
            cx,
        );
        "conjure-secrets: opened".to_owned()
    }
}

pub(crate) const CONJURE_WS_TICKET_KEY: &str = "conjure:ws-ticket";

/// `POST /conjure/projects/{projectId}/ws-ticket` (module 644441 / 472119).
pub(crate) fn conjure_ws_ticket_request(project_id: &str) -> Request {
    Request::Rest {
        key: Some(CONJURE_WS_TICKET_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/conjure/projects/{project_id}/ws-ticket"),
        body: Some(json!({})),
    }
}

fn conjure_intl(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        dc_intl::t(key).to_owned()
    } else {
        fallback.to_owned()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConjureCopyValue {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ConjureSecretFieldSpec {
    pub name: String,
    pub label: String,
    pub hint: Option<String>,
    pub hosts: Vec<String>,
}

pub(crate) struct ConjureSecretField {
    pub name: String,
    pub label: String,
    #[allow(dead_code)]
    pub hint: Option<String>,
    pub hosts: Vec<String>,
    pub editor: gpui::Entity<crate::editor::Editor>,
}

pub(crate) struct ConjureSecretsModal {
    pub project_id: String,
    #[allow(dead_code)]
    pub request_id: Option<String>,
    pub note: Option<String>,
    pub copy_values: Vec<ConjureCopyValue>,
    pub fields: Vec<ConjureSecretField>,
    pub copied_value: Option<String>,
    pub submitting: bool,
    pub error: Option<String>,
    _subs: Vec<gpui::Subscription>,
}

#[cfg(test)]
mod secrets_tests {
    use super::*;

    #[test]
    fn conjure_secrets_644441_copy_and_ticket_route() {
        for k in ["OpuAlK", "t5VZ88", "ETE/oC"] {
            assert!(dc_intl::has_key(k), "missing 644441 key {k}");
        }
        assert_eq!(conjure_intl("TuMGZp", "Add your keys"), "Add your keys");
        let req = conjure_ws_ticket_request("proj_99");
        match req {
            Request::Rest { route, .. } => assert_eq!(route, "/conjure/projects/proj_99/ws-ticket"),
            _ => panic!("expected Rest request"),
        }
    }

    #[test]
    fn conjure_publish_719593_button_state_and_keys() {
        let up_to_date = compute_conjure_publish_button_state(true, false, false, None, None);
        assert!(up_to_date.is_secondary);
        assert_eq!(up_to_date.label, "Published");
        assert_eq!(up_to_date.indicator, None);

        let draft = compute_conjure_publish_button_state(false, false, false, None, None);
        assert!(!draft.is_secondary);
        assert_eq!(draft.label, "Publish");
        assert_eq!(draft.indicator, Some("unpublished"));

        let busy = compute_conjure_publish_button_state(
            false,
            false,
            true,
            Some("Saved!"),
            Some("Building..."),
        );
        assert_eq!(busy.label, "Building...");
        assert_eq!(busy.indicator, Some("publishing"));

        assert_eq!(dc_intl::t("BddRzS"), "Okay");
    }
}
