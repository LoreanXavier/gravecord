//! E2EE User Verification Modal (module 730713, helper copy 814278, copy button 930840).
//!
//! Displays the 45-digit pairwise MLS fingerprint in 9 chunks of 5 digits (3×3 grid),
//! status badge (`UNUuem` "Verified" or `y2b7CA` "new"), copy button (`OpuAlK`/`t5VZ88`),
//! previously-verified device count (`uZDkz0`), persistence footer note (`FJN+kh`/`p/9PGp`/
//! `qT5z87`/`6JLy+i`), and `ETE/oC` ("Cancel") + `0tvNAn` ("Mark as Verified") / `Osb+/n`
//! ("Clear Verification"), backed by `POST /voice/{userId}/match-public-key` and persisted
//! verification state in `crate::storage`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    prelude::*, px, rgba,
};
use serde_json::json;
use std::borrow::Cow;

pub(crate) const E2EE_MATCH_KEY: &str = "e2ee:match-public-key";
const E2EE_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/25968222946071";
const VERIFIED_STORAGE_KEY: &str = "e2ee-verified-users";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PersistenceNote {
    BothPersistent,
    SelfOnly,
    OtherOnly,
    NeitherPersistent,
}

#[derive(Clone, Debug)]
pub(crate) struct E2eeVerifyModal {
    pub user_id: Id,
    pub username: String,
    pub loading: bool,
    pub verified: bool,
    pub initial_verified: bool,
    pub other_verified_devices: u32,
    pub chunks: Vec<String>,
    pub copied: bool,
    pub persistence: PersistenceNote,
}

impl E2eeVerifyModal {
    /// Generates a deterministic 9×5-digit MLS pairwise fingerprint display from `(me, other)`.
    pub(crate) fn synthetic_chunks(me: Id, other: Id) -> Vec<String> {
        let mut state: u64 =
            me.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ other.wrapping_add(0x517C_C1B7_2722_0A95);
        (0..9)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                format!("{:05}", (state >> 16) % 100_000)
            })
            .collect()
    }

    pub(crate) fn footer_note(&self) -> String {
        match self.persistence {
            PersistenceNote::BothPersistent => dc_intl::format(
                "FJN+kh",
                &[("helpArticle", Arg::Str(Cow::Borrowed(E2EE_HELP_URL)))],
            ),
            PersistenceNote::SelfOnly => dc_intl::format(
                "p/9PGp",
                &[
                    ("username", Arg::Str(Cow::Borrowed(&self.username))),
                    ("helpArticle", Arg::Str(Cow::Borrowed(E2EE_HELP_URL))),
                ],
            ),
            PersistenceNote::OtherOnly => dc_intl::format(
                "qT5z87",
                &[("helpArticle", Arg::Str(Cow::Borrowed(E2EE_HELP_URL)))],
            ),
            PersistenceNote::NeitherPersistent => dc_intl::format(
                "6JLy+i",
                &[("helpArticle", Arg::Str(Cow::Borrowed(E2EE_HELP_URL)))],
            ),
        }
    }
}

/// `POST /voice/{userId}/match-public-key` (module 730713).
pub(crate) fn match_public_key_request(user_id: Id, public_key: &str) -> Request {
    Request::Rest {
        key: Some(E2EE_MATCH_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/voice/{user_id}/match-public-key"),
        body: Some(json!({ "public_key": public_key, "key_version": 1 })),
    }
}

fn bold_username_subtitle(username: &str) -> StyledText {
    let runs = dc_intl::format_runs("oc2kce", &[("username", Arg::Str(Cow::Borrowed(username)))]);
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$b") && start < text.len() {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(color("--text-strong").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Opens the E2EE User Verification modal (`730713`) for `user_id`.
    pub(super) fn open_e2ee_verify_modal(&mut self, user_id: Id, cx: &mut Context<Self>) {
        let me = self.stores.me().map_or(1, |u| u.id);
        let username = self
            .stores
            .users
            .get(&user_id)
            .map(|u| {
                u.global_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&u.username)
                    .to_owned()
            })
            .unwrap_or_else(|| format!("User {user_id}"));
        let verified = self.e2ee_verified_users.contains(&user_id);
        let chunks = E2eeVerifyModal::synthetic_chunks(me, user_id);
        let _ = self
            .requests
            .send(match_public_key_request(user_id, &chunks.join("")));
        self.e2ee_verify_modal = Some(E2eeVerifyModal {
            user_id,
            username,
            loading: false,
            verified,
            initial_verified: verified,
            other_verified_devices: 0,
            chunks,
            copied: false,
            persistence: PersistenceNote::BothPersistent,
        });
        cx.notify();
    }

    /// Modal layer for E2EE User Verification (`730713`).
    pub(super) fn e2ee_verify_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.e2ee_verify_modal.as_ref()?;
        let user_id = modal.user_id;
        let verified = modal.verified;
        let loading = modal.loading;
        let full_code = modal.chunks.join(" ");

        let grid = if loading {
            div()
                .min_h(px(104.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("ZTNur7"))
                .into_any_element()
        } else {
            let mut rows = div().flex().flex_col();
            for row_idx in 0..3 {
                if row_idx > 0 {
                    rows = rows.child(div().h(px(1.)).bg(color("--border-subtle")));
                }
                let slice = modal
                    .chunks
                    .get(row_idx * 3..(row_idx * 3 + 3).min(modal.chunks.len()))
                    .unwrap_or(&[]);
                rows = rows.child(
                    div()
                        .py(px(10.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .children(slice.iter().map(|chunk| {
                            div()
                                .flex_1()
                                .text_center()
                                .font_family("Consolas")
                                .text_size(px(15.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(chunk.clone())
                        })),
                );
            }
            rows.into_any_element()
        };

        Some(
            deferred(
                div()
                    .id("e2ee-verify-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.e2ee_verify_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("e2ee-verify-modal")
                            .w(px(460.))
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
                                    .pb(px(12.))
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
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("/WPGnF")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-default"))
                                                    .child(bold_username_subtitle(&modal.username)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("e2ee-verify-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.e2ee_verify_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Body
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.))
                                    .when(modal.other_verified_devices > 0, |col| {
                                        col.child(
                                            div()
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .bg(color("--background-base-lower"))
                                                .px(px(12.))
                                                .py(px(8.))
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(icon("circle_info", 16., "--icon-strong"))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-default"))
                                                        .child(dc_intl::format(
                                                            "uZDkz0",
                                                            &[(
                                                                "count",
                                                                Arg::Num(f64::from(
                                                                    modal.other_verified_devices,
                                                                )),
                                                            )],
                                                        )),
                                                ),
                                        )
                                    })
                                    // Verification box
                                    .child(
                                        div()
                                            .w_full()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .overflow_hidden()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .px(px(14.))
                                                    .py(px(8.))
                                                    .bg(color("--background-mod-subtle"))
                                                    .border_b_1()
                                                    .border_color(color("--border-subtle"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(8.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(13.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(dc_intl::t("/WPGnF")),
                                                            )
                                                            .child(
                                                                div()
                                                                    .id("e2ee-copy-code")
                                                                    .cursor_pointer()
                                                                    .px(px(8.))
                                                                    .py(px(2.))
                                                                    .rounded(px(4.))
                                                                    .bg(color(
                                                                        "--control-secondary-background-default",
                                                                    ))
                                                                    .text_size(px(11.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--control-secondary-text-default",
                                                                    ))
                                                                    .child(if modal.copied {
                                                                        dc_intl::t("t5VZ88")
                                                                    } else {
                                                                        dc_intl::t("OpuAlK")
                                                                    })
                                                                    .on_click(cx.listener(
                                                                        move |this, _, _, cx| {
                                                                            cx.write_to_clipboard(
                                                                                gpui::ClipboardItem::new_string(
                                                                                    full_code.clone(),
                                                                                ),
                                                                            );
                                                                            if let Some(m) = this
                                                                                .e2ee_verify_modal
                                                                                .as_mut()
                                                                            {
                                                                                m.copied = true;
                                                                            }
                                                                            cx.notify();
                                                                        },
                                                                    )),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(8.))
                                                            .py(px(2.))
                                                            .rounded_full()
                                                            .bg(if modal.initial_verified {
                                                                color(
                                                                    "--control-primary-background-default",
                                                                )
                                                            } else {
                                                                color(
                                                                    "--control-critical-primary-background-default",
                                                                )
                                                            })
                                                            .text_size(px(11.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--white"))
                                                            .child(if modal.initial_verified {
                                                                dc_intl::t("UNUuem")
                                                            } else {
                                                                dc_intl::t("y2b7CA")
                                                            }),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .bg(color("--background-base-lower"))
                                                    .child(grid),
                                            ),
                                    )
                                    // Persistence note
                                    .child(
                                        div()
                                            .id("e2ee-footer-note")
                                            .cursor_pointer()
                                            .text_center()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(modal.footer_note())
                                            .on_click(|_, _, cx| cx.open_url(E2EE_HELP_URL)),
                                    ),
                            )
                            // Footer actions
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
                                            .id("e2ee-verify-cancel")
                                            .cursor_pointer()
                                            .px(px(14.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.e2ee_verify_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("e2ee-verify-toggle")
                                            .when(!loading, |d| d.cursor_pointer())
                                            .when(loading, |d| d.opacity(0.5))
                                            .px(px(18.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(if verified {
                                                color("--control-critical-primary-background-default")
                                            } else {
                                                color("--control-primary-background-default")
                                            })
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(if verified {
                                                dc_intl::t("Osb+/n")
                                            } else {
                                                dc_intl::t("0tvNAn")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if loading {
                                                    return;
                                                }
                                                if verified {
                                                    this.e2ee_verified_users.remove(&user_id);
                                                } else {
                                                    this.e2ee_verified_users.insert(user_id);
                                                }
                                                if !crate::storage::is_capture_run() {
                                                    let ids: Vec<String> = this
                                                        .e2ee_verified_users
                                                        .iter()
                                                        .map(ToString::to_string)
                                                        .collect();
                                                    crate::storage::set(
                                                        VERIFIED_STORAGE_KEY,
                                                        serde_json::Value::from(ids),
                                                    );
                                                }
                                                this.e2ee_verify_modal = None;
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

    /// Capture harness: `e2ee-verify [new|verified|devices|close]` (module 730713 + 144485 launchers).
    #[cfg(feature = "capture")]
    pub(crate) fn script_e2ee_verify(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let uid = self
            .stores
            .users
            .keys()
            .copied()
            .find(|id| Some(*id) != self.stores.me().map(|u| u.id))
            .unwrap_or(2);
        match arg.trim() {
            "close" => {
                self.e2ee_verify_modal = None;
                cx.notify();
                "e2ee-verify: closed".to_owned()
            }
            "verified" => {
                self.e2ee_verified_users.insert(uid);
                self.open_e2ee_verify_modal(uid, cx);
                "e2ee-verify: verified".to_owned()
            }
            "devices" => {
                self.e2ee_verified_users.remove(&uid);
                self.open_e2ee_verify_modal(uid, cx);
                if let Some(m) = self.e2ee_verify_modal.as_mut() {
                    m.other_verified_devices = 2;
                }
                cx.notify();
                "e2ee-verify: devices".to_owned()
            }
            _ => {
                self.e2ee_verified_users.remove(&uid);
                self.open_e2ee_verify_modal(uid, cx);
                "e2ee-verify: new".to_owned()
            }
        }
    }
}

/// Module 144485 (`openSecureFramesUpdateConfirmation`): confirmation payload for Secure Frames
/// (DAVE E2EE) update prompts (`confirmText` defaults to `cY+Oob` `"Confirm"`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct SecureFramesUpdateConfirmation {
    pub title: String,
    pub subtitle: String,
    pub confirm_text: String,
}

#[allow(dead_code)]
impl SecureFramesUpdateConfirmation {
    pub(crate) fn new(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        confirm_text: Option<&str>,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            confirm_text: confirm_text
                .unwrap_or_else(|| dc_intl::t("cY+Oob"))
                .to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_frames_launchers_144485_defaults_and_intl() {
        assert_eq!(dc_intl::t("cY+Oob"), "Confirm");
        let cfg = SecureFramesUpdateConfirmation::new(
            "Update Secure Frames",
            "Reconnect to apply protocol update.",
            None,
        );
        assert_eq!(cfg.confirm_text, "Confirm");
        let custom = SecureFramesUpdateConfirmation::new("Title", "Sub", Some("Continue"));
        assert_eq!(custom.confirm_text, "Continue");
    }
}
