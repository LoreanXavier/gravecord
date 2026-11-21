//! "Why we hide some media" Sensitive Media Explainer modal (module 743461, false-positive
//! report `POST /attachments/report-false-positive` from 544135 / 135598).
//!
//! Opened from obscured/sensitive media attachments (`media_image.rs`) and embeds (`embed.rs`).
//! Supports all 3 official subtitle modes (`5e0geG`, `RUw0ZC`, `E/oQYL`), `"Mark As Not
//! Sensitive"` (`ZH7P2h`) triggering false-positive submission, `"Learn More"` (`hvVgAZ`), and
//! `"Go to Settings"` (`9D+zGX`) navigating directly to Content & Social settings.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::json;

const EXPLICIT_MEDIA_HELP_URL: &str =
    "https://support.discord.com/hc/en-us/articles/18210995019671";
const AGE_VERIFY_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/30326565624343";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SensitiveExplainerMode {
    CanManageSettings,
    CannotManageSettings,
    AgeRestricted,
}

#[derive(Clone, Debug)]
pub(crate) struct SensitiveMediaModal {
    pub channel_id: Id,
    pub message_id: Id,
    pub attachment_ids: Vec<Id>,
    pub embed_ids: Vec<String>,
    pub mode: SensitiveExplainerMode,
    pub reported: bool,
}

/// `POST /attachments/report-false-positive` (module 743461 / 544135).
pub(crate) fn report_false_positive_request(
    channel_id: Id,
    message_id: Id,
    attachment_ids: &[Id],
    embed_ids: &[String],
) -> Request {
    Request::Rest {
        key: Some("sensitive-media:report-false-positive".to_owned()),
        method: reqwest::Method::POST,
        route: "/attachments/report-false-positive".to_owned(),
        body: Some(json!({
            "channel_id": channel_id.to_string(),
            "message_id": message_id.to_string(),
            "attachment_ids": attachment_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "embed_ids": embed_ids,
        })),
    }
}

impl Root {
    /// Opens the "Why we hide some media" sensitive media explainer (`743461`).
    pub(super) fn open_sensitive_media_modal(
        &mut self,
        channel_id: Id,
        message_id: Id,
        attachment_ids: Vec<Id>,
        embed_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let mode = if !self.stores.account.nsfw_allowed {
            SensitiveExplainerMode::AgeRestricted
        } else if self.stores.current_user.is_none() {
            SensitiveExplainerMode::CannotManageSettings
        } else {
            SensitiveExplainerMode::CanManageSettings
        };
        self.sensitive_media_modal = Some(SensitiveMediaModal {
            channel_id,
            message_id,
            attachment_ids,
            embed_ids,
            mode,
            reported: false,
        });
        cx.notify();
    }

    /// Modal layer for "Why we hide some media" (`743461`).
    pub(super) fn sensitive_media_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.sensitive_media_modal.as_ref()?;
        let subtitle = match modal.mode {
            SensitiveExplainerMode::AgeRestricted => dc_intl::t("5e0geG"),
            SensitiveExplainerMode::CanManageSettings => dc_intl::t("RUw0ZC"),
            SensitiveExplainerMode::CannotManageSettings => dc_intl::t("E/oQYL"),
        };
        let show_mark_not_sensitive =
            modal.mode != SensitiveExplainerMode::AgeRestricted && !modal.reported;
        let show_go_to_settings = modal.mode == SensitiveExplainerMode::CanManageSettings;
        let help_url = if modal.mode == SensitiveExplainerMode::AgeRestricted {
            AGE_VERIFY_HELP_URL
        } else {
            EXPLICIT_MEDIA_HELP_URL
        };
        let channel_id = modal.channel_id;
        let message_id = modal.message_id;
        let attachment_ids = modal.attachment_ids.clone();
        let embed_ids = modal.embed_ids.clone();

        Some(
            deferred(
                div()
                    .id("sensitive-media-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sensitive_media_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("sensitive-media-modal")
                            .w(px(440.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Header
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(10.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(icon("eye_slash", 20., "--icon-strong"))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("sGW77l")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("sensitive-media-modal-close")
                                            .flex_none()
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.sensitive_media_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Subtitle
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(20.))
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(subtitle),
                            )
                            // Footer actions
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(10.))
                                    .when(show_mark_not_sensitive, |ftr| {
                                        ftr.child(
                                            div()
                                                .id("sensitive-media-mark-not-sensitive")
                                                .cursor_pointer()
                                                .px(px(14.))
                                                .h(px(36.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .flex()
                                                .items_center()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("ZH7P2h"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let _ = this.requests.send(
                                                        report_false_positive_request(
                                                            channel_id,
                                                            message_id,
                                                            &attachment_ids,
                                                            &embed_ids,
                                                        ),
                                                    );
                                                    this.sensitive_media_modal = None;
                                                    this.put_app_toast(
                                                        dc_intl::t("ZH7P2h").to_owned(),
                                                        true,
                                                        cx,
                                                    );
                                                })),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("sensitive-media-primary")
                                            .cursor_pointer()
                                            .px(px(16.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .flex()
                                            .items_center()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(if show_go_to_settings {
                                                dc_intl::t("9D+zGX")
                                            } else {
                                                dc_intl::t("hvVgAZ")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.sensitive_media_modal = None;
                                                if show_go_to_settings {
                                                    this.open_settings_section(
                                                        "Content & Social",
                                                        cx,
                                                    );
                                                } else {
                                                    cx.open_url(help_url);
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `sensitive-media [manage|restricted|age|close]` (module 743461).
    #[cfg(feature = "capture")]
    pub(crate) fn script_sensitive_media(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let cid = self.channel.unwrap_or(100);
        match arg.trim() {
            "close" => {
                self.sensitive_media_modal = None;
                cx.notify();
                "sensitive-media: closed".to_owned()
            }
            "age" => {
                self.sensitive_media_modal = Some(SensitiveMediaModal {
                    channel_id: cid,
                    message_id: 200,
                    attachment_ids: vec![300],
                    embed_ids: Vec::new(),
                    mode: SensitiveExplainerMode::AgeRestricted,
                    reported: false,
                });
                cx.notify();
                "sensitive-media: age".to_owned()
            }
            "restricted" => {
                self.sensitive_media_modal = Some(SensitiveMediaModal {
                    channel_id: cid,
                    message_id: 200,
                    attachment_ids: vec![300],
                    embed_ids: Vec::new(),
                    mode: SensitiveExplainerMode::CannotManageSettings,
                    reported: false,
                });
                cx.notify();
                "sensitive-media: restricted".to_owned()
            }
            _ => {
                self.sensitive_media_modal = Some(SensitiveMediaModal {
                    channel_id: cid,
                    message_id: 200,
                    attachment_ids: vec![300],
                    embed_ids: Vec::new(),
                    mode: SensitiveExplainerMode::CanManageSettings,
                    reported: false,
                });
                cx.notify();
                "sensitive-media: manage".to_owned()
            }
        }
    }
}
