//! "Why we hide some media" Sensitive Media Explainer modal (module 743461) and
//! "Mark as not sensitive" False-Positive Confirmation modal (module 961997; `POST
//! /attachments/report-false-positive` & `POST /attachments/sender-report-false-positive`
//! from 544135 / 287002 / 135598).

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(super) const SENSITIVE_REPORT_KEY: &str = "sensitive-media:report-false-positive";

const EXPLICIT_MEDIA_HELP_URL: &str =
    "https://support.discord.com/hc/en-us/articles/18210995019671";
const AGE_VERIFY_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/30326565624343";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SensitiveExplainerMode {
    CanManageSettings,
    CannotManageSettings,
    AgeRestricted,
}

/// Module 974875 / 705833: 14-day window (in ms) for sender explicit-media false-positive reports.
pub(crate) const SENDER_FP_MAX_AGE_MS: u64 = 14 * 86_400_000;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// Module 974875 (`705833.A.canSubmitFpReport`): returns true when the message has not yet been
/// reported and its snowflake timestamp is strictly younger than 14 days.
pub(crate) fn can_submit_sender_fp_report(
    message_id: Id,
    now_ms: u64,
    already_submitted: bool,
) -> bool {
    if already_submitted {
        return false;
    }
    let created_ms = (message_id >> 22).saturating_add(DISCORD_EPOCH_MS);
    now_ms.saturating_sub(created_ms) < SENDER_FP_MAX_AGE_MS
}

#[derive(Clone, Debug)]
pub(crate) struct SensitiveMediaModal {
    pub channel_id: Id,
    pub message_id: Id,
    pub attachment_ids: Vec<Id>,
    pub embed_ids: Vec<String>,
    pub mode: SensitiveExplainerMode,
    pub reported: bool,
    /// Module 961997 confirmation step ("Mark as not sensitive").
    pub confirm_false_positive: bool,
    /// Module 974875 (`iS/eFN` / `YrjcgR` / `BddRzS`): expired/already-submitted alert dialog.
    pub expired_alert: bool,
    pub loading: bool,
    pub preview_url: Option<String>,
    /// When `Some`, uses sender false-positive route (`287002` / `135598.wV`).
    pub sender_filenames: Option<Vec<String>>,
}

/// `POST /attachments/report-false-positive` (module 743461 / 544135 / 961997).
pub(crate) fn report_false_positive_request(
    channel_id: Id,
    message_id: Id,
    attachment_ids: &[Id],
    embed_ids: &[String],
) -> Request {
    Request::Rest {
        key: Some(SENSITIVE_REPORT_KEY.to_owned()),
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

/// `POST /attachments/sender-report-false-positive` (module 287002 / 961997).
pub(crate) fn sender_report_false_positive_request(
    channel_id: Id,
    message_id: Id,
    attachment_ids: &[Id],
    filenames: &[String],
) -> Request {
    Request::Rest {
        key: Some(SENSITIVE_REPORT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/attachments/sender-report-false-positive".to_owned(),
        body: Some(json!({
            "channel_id": channel_id.to_string(),
            "message_id": message_id.to_string(),
            "attachment_ids": attachment_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "filenames": filenames,
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
            confirm_false_positive: false,
            expired_alert: false,
            loading: false,
            preview_url: None,
            sender_filenames: None,
        });
        cx.notify();
    }

    /// Module 974875 (`Q`): sender explicit-media false-positive report launcher.
    /// If `can_submit_sender_fp_report` is false (>= 14 days old or already submitted), opens
    /// the expired alert modal (`iS/eFN` / `YrjcgR` / `BddRzS`); otherwise opens `287002`/`961997`.
    #[allow(dead_code)]
    pub(super) fn open_sender_false_positive(
        &mut self,
        channel_id: Id,
        message_id: Id,
        attachment_ids: Vec<Id>,
        filenames: Vec<String>,
        already_submitted: bool,
        cx: &mut Context<Self>,
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        if !can_submit_sender_fp_report(message_id, now_ms, already_submitted) {
            self.sensitive_media_modal = Some(SensitiveMediaModal {
                channel_id,
                message_id,
                attachment_ids,
                embed_ids: Vec::new(),
                mode: SensitiveExplainerMode::CanManageSettings,
                reported: true,
                confirm_false_positive: false,
                expired_alert: true,
                loading: false,
                preview_url: None,
                sender_filenames: Some(filenames),
            });
            cx.notify();
            return;
        }
        if attachment_ids.is_empty() {
            return;
        }
        self.sensitive_media_modal = Some(SensitiveMediaModal {
            channel_id,
            message_id,
            attachment_ids,
            embed_ids: Vec::new(),
            mode: SensitiveExplainerMode::CanManageSettings,
            reported: false,
            confirm_false_positive: true,
            expired_alert: false,
            loading: false,
            preview_url: None,
            sender_filenames: Some(filenames),
        });
        cx.notify();
    }

    /// Handles `POST /attachments/[sender-]report-false-positive` response (module 961997 `o`:
    /// `gFsTKu` on success, `R0RpRX` on failure).
    pub(super) fn handle_sensitive_media_response(&mut self, body: &Value, cx: &mut Context<Self>) {
        let ok = body["ok"]
            .as_bool()
            .unwrap_or_else(|| body.get("code").is_none());
        if ok {
            self.sensitive_media_modal = None;
            self.put_app_toast(dc_intl::t("gFsTKu").to_owned(), true, cx);
        } else {
            if let Some(m) = self.sensitive_media_modal.as_mut() {
                m.loading = false;
            }
            self.put_app_toast(dc_intl::t("R0RpRX").to_owned(), false, cx);
        }
        cx.notify();
    }

    /// Modal layer for "Why we hide some media" (`743461`), expired sender FP alert (`974875`),
    /// & "Mark as not sensitive" confirm (`961997`).
    pub(super) fn sensitive_media_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.sensitive_media_modal.as_ref()?;
        if modal.expired_alert {
            // Module 974875 (`579872.A.show`): expired / already-submitted single-confirm alert.
            return Some(
                deferred(
                    div()
                        .id("sensitive-fp-expired-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.sensitive_media_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .id("sensitive-fp-expired-modal")
                                .w(px(440.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    div()
                                        .p(px(24.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .text_size(px(20.))
                                                .line_height(px(26.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("iS/eFN")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-default"))
                                                .child(dc_intl::t("YrjcgR")),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(24.))
                                        .py(px(16.))
                                        .border_t_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .flex()
                                        .justify_end()
                                        .child(
                                            div()
                                                .id("sensitive-fp-expired-ok")
                                                .h(px(38.))
                                                .px(px(18.))
                                                .rounded(px(8.))
                                                .bg(color("--control-primary-background-default"))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("BddRzS"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.sensitive_media_modal = None;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                )
                .with_priority(96)
                .into_any_element(),
            );
        }
        if modal.confirm_false_positive {
            let loading = modal.loading;
            let channel_id = modal.channel_id;
            let message_id = modal.message_id;
            let attachment_ids = modal.attachment_ids.clone();
            let embed_ids = modal.embed_ids.clone();
            let sender_filenames = modal.sender_filenames.clone();
            let preview_img = modal
                .preview_url
                .as_ref()
                .and_then(|u| self.images.get(u).cloned());
            return Some(
                deferred(
                    div()
                        .id("sensitive-fp-confirm-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this
                                .sensitive_media_modal
                                .as_ref()
                                .is_some_and(|m| !m.loading)
                            {
                                this.sensitive_media_modal = None;
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .id("sensitive-fp-confirm-modal")
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
                                        .pb(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(20.))
                                                .line_height(px(24.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("TPpVkI")),
                                        )
                                        .child(
                                            div()
                                                .id("sensitive-fp-close")
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
                                .child(
                                    div()
                                        .px(px(24.))
                                        .pb(px(16.))
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("z4du/I")),
                                )
                                .when(
                                    preview_img.is_some() || modal.preview_url.is_some(),
                                    |col| {
                                        col.child(
                                            div()
                                                .mx(px(24.))
                                                .mb(px(16.))
                                                .h(px(160.))
                                                .rounded(px(8.))
                                                .overflow_hidden()
                                                .bg(color("--background-base-lower"))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(match preview_img {
                                                    Some(art) => img(art)
                                                        .max_w_full()
                                                        .max_h(px(160.))
                                                        .into_any_element(),
                                                    None => icon("eye_slash", 32., "--text-muted")
                                                        .into_any_element(),
                                                }),
                                        )
                                    },
                                )
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
                                        .child(
                                            div()
                                                .id("sensitive-fp-cancel")
                                                .when(!loading, |b| b.cursor_pointer())
                                                .when(loading, |b| b.opacity(0.5))
                                                .px(px(16.))
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
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("ETE/oC"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if !loading {
                                                        this.sensitive_media_modal = None;
                                                        cx.notify();
                                                    }
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id("sensitive-fp-confirm")
                                                .when(!loading, |b| b.cursor_pointer())
                                                .when(loading, |b| b.opacity(0.6))
                                                .px(px(18.))
                                                .h(px(36.))
                                                .rounded(px(6.))
                                                .bg(color("--control-primary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-primary-background-hover",
                                                    ))
                                                })
                                                .flex()
                                                .items_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("cY+Oob"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if loading {
                                                        return;
                                                    }
                                                    if let Some(m) =
                                                        this.sensitive_media_modal.as_mut()
                                                    {
                                                        m.loading = true;
                                                    }
                                                    let req =
                                                        if let Some(ref fns) = sender_filenames {
                                                            sender_report_false_positive_request(
                                                                channel_id,
                                                                message_id,
                                                                &attachment_ids,
                                                                fns,
                                                            )
                                                        } else {
                                                            report_false_positive_request(
                                                                channel_id,
                                                                message_id,
                                                                &attachment_ids,
                                                                &embed_ids,
                                                            )
                                                        };
                                                    let _ = this.requests.send(req);
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                )
                .with_priority(96)
                .into_any_element(),
            );
        }
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
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.sensitive_media_modal.as_mut()
                                                    {
                                                        m.confirm_false_positive = true;
                                                        m.loading = false;
                                                    }
                                                    cx.notify();
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

    /// Capture harness: `sensitive-media [manage|restricted|age|confirm|close]` (modules 743461 + 961997).
    #[cfg(feature = "capture")]
    pub(crate) fn script_sensitive_media(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let cid = self.channel.unwrap_or(100);
        let base = |mode, confirm| SensitiveMediaModal {
            channel_id: cid,
            message_id: 200,
            attachment_ids: vec![300],
            embed_ids: Vec::new(),
            mode,
            reported: false,
            confirm_false_positive: confirm,
            expired_alert: false,
            loading: false,
            preview_url: confirm.then(|| "sensitive:preview".to_owned()),
            sender_filenames: None,
        };
        match arg.trim() {
            "close" => {
                self.sensitive_media_modal = None;
                cx.notify();
                "sensitive-media: closed".to_owned()
            }
            "expired" => {
                let mut m = base(SensitiveExplainerMode::CanManageSettings, false);
                m.expired_alert = true;
                self.sensitive_media_modal = Some(m);
                cx.notify();
                "sensitive-media: expired".to_owned()
            }
            "confirm" => {
                self.sensitive_media_modal =
                    Some(base(SensitiveExplainerMode::CanManageSettings, true));
                cx.notify();
                "sensitive-media: confirm".to_owned()
            }
            "age" => {
                self.sensitive_media_modal =
                    Some(base(SensitiveExplainerMode::AgeRestricted, false));
                cx.notify();
                "sensitive-media: age".to_owned()
            }
            "restricted" => {
                self.sensitive_media_modal =
                    Some(base(SensitiveExplainerMode::CannotManageSettings, false));
                cx.notify();
                "sensitive-media: restricted".to_owned()
            }
            _ => {
                self.sensitive_media_modal =
                    Some(base(SensitiveExplainerMode::CanManageSettings, false));
                cx.notify();
                "sensitive-media: manage".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_fp_launcher_974875_14_day_window_and_intl() {
        for key in ["iS/eFN", "YrjcgR", "BddRzS"] {
            assert!(dc_intl::has_key(key), "missing 974875 key {key}");
        }
        let now_ms: u64 = DISCORD_EPOCH_MS + 30 * 86_400_000;
        let fresh_id: Id = (25 * 86_400_000) << 22; // 5 days old
        let expired_id: Id = (10 * 86_400_000) << 22; // 20 days old
        assert!(can_submit_sender_fp_report(fresh_id, now_ms, false));
        assert!(!can_submit_sender_fp_report(fresh_id, now_ms, true));
        assert!(!can_submit_sender_fp_report(expired_id, now_ms, false));
    }
}
