//! Custom Role Icon Upload / Expression Picker Modal (module 126061).
//!
//! Two-tab modal (`Utlwvi` "Expression Picker"):
//! - Tab 1: `royWSB` ("Upload Image") — `mz++Qq` ("Choose a PNG or JPG to upload"),
//!   `sdCQY4` ("Browse Files") with `Sp2NF+` ("Images") PNG/JPG 256 KB validation (`HFyKsa`
//!   "Oh no! File is too big. Please select a .png or .jpg 256 KB or smaller.").
//! - Tab 2: `/Ny2wZ` ("Emoji") — quick Unicode/server emoji grid for selecting a role icon.

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};

pub(crate) const MAX_ROLE_ICON_BYTES: usize = 256_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoleIconTab {
    UploadImage,
    Emoji,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct RoleIconUploadModal {
    pub guild_id: Id,
    pub active_tab: RoleIconTab,
    pub file_size_error: bool,
    pub selected_preview: Option<String>,
}

const SAMPLE_ROLE_EMOJIS: &[&str] = &[
    "⭐", "👑", "🛡️", "🔥", "⚡", "💎", "🎮", "🎵", "🚀", "🌈", "🏆", "🎯", "🦋", "🐉", "🌸", "🍀",
];

impl Root {
    /// Opens the Custom Role Icon Upload / Expression Picker modal (`126061`).
    #[allow(dead_code)]
    pub(super) fn open_role_icon_upload_modal(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        self.role_icon_upload_modal = Some(RoleIconUploadModal {
            guild_id,
            active_tab: RoleIconTab::UploadImage,
            file_size_error: false,
            selected_preview: None,
        });
        cx.notify();
    }

    /// Validates a picked PNG/JPG byte slice against the 256 KB limit of module 126061.
    #[allow(dead_code)]
    pub(crate) fn validate_role_icon_bytes(bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() > MAX_ROLE_ICON_BYTES {
            Err(dc_intl::t("HFyKsa"))
        } else {
            Ok(())
        }
    }

    /// Modal layer for Custom Role Icon Upload / Expression Picker (`126061`).
    pub(super) fn role_icon_upload_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.role_icon_upload_modal.as_ref()?;
        let active_tab = modal.active_tab;

        let tab_btn = |id: &'static str, label: &'static str, tab: RoleIconTab| {
            let selected = active_tab == tab;
            div()
                .id(id)
                .cursor_pointer()
                .pb(px(10.))
                .border_b_2()
                .border_color(if selected {
                    color("--control-primary-background-default")
                } else {
                    rgba(0x00000000)
                })
                .text_size(px(14.))
                .font_weight(FontWeight(if selected { 600. } else { 500. }))
                .text_color(if selected {
                    color("--text-strong")
                } else {
                    color("--text-muted")
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = this.role_icon_upload_modal.as_mut() {
                        m.active_tab = tab;
                        m.file_size_error = false;
                    }
                    cx.notify();
                }))
        };

        let body: AnyElement = match active_tab {
            RoleIconTab::UploadImage => div()
                .p(px(28.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(68.))
                        .rounded(px(12.))
                        .bg(color("--background-base-lowest"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("upload_modify", 32., "--icon-strong")),
                )
                .child(
                    div()
                        .pt(px(8.))
                        .text_size(px(15.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("mz++Qq")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(color("--text-muted"))
                        .child(format!("{} (PNG, JPG • max 256 KB)", dc_intl::t("Sp2NF+"))),
                )
                .child(
                    div()
                        .id("role-icon-browse-files")
                        .mt(px(4.))
                        .cursor_pointer()
                        .px(px(20.))
                        .h(px(38.))
                        .rounded(px(6.))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("sdCQY4"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.role_icon_upload_modal.as_mut() {
                                m.file_size_error = false;
                            }
                            this.put_app_toast(
                                format!("{}: PNG / JPG (<= 256 KB)", dc_intl::t("Sp2NF+")),
                                true,
                                cx,
                            );
                        })),
                )
                .when(modal.file_size_error, |col| {
                    col.child(
                        div()
                            .mt(px(4.))
                            .px(px(16.))
                            .text_center()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(dc_intl::t("HFyKsa")),
                    )
                })
                .into_any_element(),
            RoleIconTab::Emoji => div()
                .p(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(div().flex().flex_wrap().gap(px(8.)).children(
                    SAMPLE_ROLE_EMOJIS.iter().enumerate().map(|(idx, glyph)| {
                        let g = (*glyph).to_owned();
                        div()
                            .id(format!("role-icon-emoji-{idx}"))
                            .cursor_pointer()
                            .size(px(44.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(22.))
                            .child(g.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.role_icon_upload_modal = None;
                                this.put_app_toast(format!("Selected role icon {g}"), true, cx);
                            }))
                    }),
                ))
                .into_any_element(),
        };

        Some(
            deferred(
                div()
                    .id("role-icon-upload-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.role_icon_upload_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("role-icon-upload-modal")
                            .aria_label(dc_intl::t("Utlwvi"))
                            .w(px(460.))
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
                                    .px(px(20.))
                                    .pt(px(16.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(20.))
                                            .child(tab_btn(
                                                "role-icon-tab-upload",
                                                dc_intl::t("royWSB"),
                                                RoleIconTab::UploadImage,
                                            ))
                                            .child(tab_btn(
                                                "role-icon-tab-emoji",
                                                dc_intl::t("/Ny2wZ"),
                                                RoleIconTab::Emoji,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("role-icon-upload-close")
                                            .flex_none()
                                            .mb(px(8.))
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 16., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.role_icon_upload_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(body),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `role-icon-upload [upload|error|emoji|close]` (module 126061).
    #[cfg(feature = "capture")]
    pub(crate) fn script_role_icon_upload(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        match arg.trim() {
            "close" => {
                self.role_icon_upload_modal = None;
                cx.notify();
                "role-icon-upload: closed".to_owned()
            }
            "error" => {
                self.role_icon_upload_modal = Some(RoleIconUploadModal {
                    guild_id: gid,
                    active_tab: RoleIconTab::UploadImage,
                    file_size_error: true,
                    selected_preview: None,
                });
                cx.notify();
                "role-icon-upload: error".to_owned()
            }
            "emoji" => {
                self.role_icon_upload_modal = Some(RoleIconUploadModal {
                    guild_id: gid,
                    active_tab: RoleIconTab::Emoji,
                    file_size_error: false,
                    selected_preview: None,
                });
                cx.notify();
                "role-icon-upload: emoji".to_owned()
            }
            _ => {
                self.role_icon_upload_modal = Some(RoleIconUploadModal {
                    guild_id: gid,
                    active_tab: RoleIconTab::UploadImage,
                    file_size_error: false,
                    selected_preview: None,
                });
                cx.notify();
                "role-icon-upload: upload".to_owned()
            }
        }
    }
}
