//! "Choose a default reaction" Forum Default Reaction Modal (module 939940, preview 235640).
//!
//! Opened from Forum channel onboarding / settings (`forums.rs`). Shows:
//! - Title `XlDE3k` ("Choose a default reaction") & subtitle `lVqhdd`.
//! - Skeleton forum post preview card (`235640`: 4 skeleton bars + thumbnail box + reaction pill
//!   showing the chosen emoji next to count `17`).
//! - `"Select emoji"` (`59QgaD`) quick emoji bar + conditional `"Remove"` (`N86XcP`) button.
//! - Failure error text (`F75nNl` "Failed to save default reaction") and footer `ETE/oC`
//!   ("Cancel") + `R3BPH+` ("Save") dispatching `PATCH /channels/{channelId}` with
//!   `{"default_reaction_emoji": ...}`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{DefaultReactionEmoji, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};

pub(crate) const DEFAULT_REACTION_PATCH_KEY: &str = "forum:default-reaction-patch";

const QUICK_EMOJIS: &[&str] = &["👍", "❤️", "🔥", "🎉", "⭐", "👀", "😂", "✅"];

#[derive(Clone, Debug)]
pub(crate) struct ForumDefaultReactionModal {
    pub channel_id: Id,
    pub initial: Option<DefaultReactionEmoji>,
    pub selected: Option<DefaultReactionEmoji>,
    pub picker_open: bool,
    pub saving: bool,
    pub failed: bool,
}

impl ForumDefaultReactionModal {
    pub(crate) fn is_dirty(&self) -> bool {
        self.initial != self.selected
    }
}

/// `PATCH /channels/{channelId}` updating `default_reaction_emoji` (module 939940 / 435183).
pub(crate) fn patch_default_reaction_request(
    channel_id: Id,
    reaction: Option<&DefaultReactionEmoji>,
) -> Request {
    let payload = match reaction {
        Some(r) => json!({
            "default_reaction_emoji": {
                "emoji_id": r.emoji_id.map(|id| id.to_string()),
                "emoji_name": r.emoji_name,
            }
        }),
        None => json!({ "default_reaction_emoji": Value::Null }),
    };
    Request::Rest {
        key: Some(DEFAULT_REACTION_PATCH_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: format!("/channels/{channel_id}"),
        body: Some(payload),
    }
}

impl Root {
    /// Opens the "Choose a default reaction" modal (`939940`) for `channel_id`.
    pub(super) fn open_forum_default_reaction(&mut self, channel_id: Id, cx: &mut Context<Self>) {
        let initial = self
            .stores
            .guilds
            .iter()
            .flat_map(|g| g.channels.iter())
            .find(|c| c.id == channel_id)
            .and_then(|c| c.default_reaction_emoji.clone());
        self.forum_default_reaction_modal = Some(ForumDefaultReactionModal {
            channel_id,
            initial: initial.clone(),
            selected: initial,
            picker_open: false,
            saving: false,
            failed: false,
        });
        cx.notify();
    }

    /// Handles REST answer for `PATCH /channels/{id}` default reaction (`939940`).
    pub(super) fn handle_forum_default_reaction_response(
        &mut self,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.forum_default_reaction_modal.as_mut() else {
            return;
        };
        modal.saving = false;
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if failed {
            modal.failed = true;
        } else {
            self.forum_default_reaction_modal = None;
        }
        cx.notify();
    }

    /// Modal layer for "Choose a default reaction" (`939940`).
    pub(super) fn forum_default_reaction_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.forum_default_reaction_modal.as_ref()?;
        let dirty = modal.is_dirty();
        let can_save = dirty && !modal.saving;
        let has_selected = modal.selected.is_some();
        let emoji_display = modal
            .selected
            .as_ref()
            .and_then(|r| r.emoji_name.clone())
            .unwrap_or_default();
        let channel_id = modal.channel_id;
        let selected_clone = modal.selected.clone();

        // Skeleton forum post preview card (`235640.A`).
        let skel = |w_pct: f32, h_px: f32| {
            div()
                .w(gpui::relative(w_pct))
                .h(px(h_px))
                .rounded(px(4.))
                .bg(color("--background-mod-strong"))
        };
        let preview_card = div()
            .p(px(14.))
            .rounded(px(10.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(skel(0.95, 14.))
                    .child(skel(0.78, 10.))
                    .child(skel(0.55, 10.))
                    .child(
                        div()
                            .mt(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .px(px(8.))
                                    .h(px(26.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-primary-background-default"))
                                    .bg(rgba(0x5865f226))
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .child(if !emoji_display.is_empty() {
                                        div()
                                            .text_size(px(14.))
                                            .child(emoji_display)
                                            .into_any_element()
                                    } else {
                                        icon("menu_add_reaction", 14., "--interactive-text-default")
                                            .into_any_element()
                                    })
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--interactive-text-default"))
                                            .child("17"),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(52.))
                                    .h(px(10.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-subtle")),
                            )
                            .child(
                                div()
                                    .w(px(36.))
                                    .h(px(10.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-subtle")),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .size(px(72.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-strong")),
            );

        Some(
            deferred(
                div()
                    .id("forum-default-reaction-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forum_default_reaction_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("forum-default-reaction-modal")
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
                                                    .child(dc_intl::t("XlDE3k")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("lVqhdd")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("forum-default-reaction-close")
                                            .flex_none()
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.forum_default_reaction_modal = None;
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
                                    .child(preview_card)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .id("forum-default-reaction-select-emoji")
                                                    .cursor_pointer()
                                                    .px(px(14.))
                                                    .h(px(32.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-primary-background-default",
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(dc_intl::t("59QgaD"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(m) = this
                                                            .forum_default_reaction_modal
                                                            .as_mut()
                                                        {
                                                            m.picker_open = !m.picker_open;
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .when(has_selected, |row| {
                                                row.child(
                                                    div()
                                                        .id("forum-default-reaction-remove")
                                                        .cursor_pointer()
                                                        .px(px(14.))
                                                        .h(px(32.))
                                                        .rounded(px(6.))
                                                        .bg(color(
                                                            "--control-critical-secondary-background-default",
                                                        ))
                                                        .border_1()
                                                        .border_color(color(
                                                            "--control-critical-secondary-border-default",
                                                        ))
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color(
                                                            "--control-critical-secondary-text-default",
                                                        ))
                                                        .child(dc_intl::t("N86XcP"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(m) = this
                                                                .forum_default_reaction_modal
                                                                .as_mut()
                                                            {
                                                                m.selected = None;
                                                                m.failed = false;
                                                            }
                                                            cx.notify();
                                                        })),
                                                )
                                            }),
                                    )
                                    .when(modal.picker_open, |col| {
                                        col.child(
                                            div()
                                                .p(px(8.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .bg(color("--background-base-lower"))
                                                .flex()
                                                .flex_wrap()
                                                .gap(px(6.))
                                                .children(QUICK_EMOJIS.iter().enumerate().map(
                                                    |(idx, &em)| {
                                                        div()
                                                            .id(("fdr-quick-emoji", idx))
                                                            .cursor_pointer()
                                                            .size(px(34.))
                                                            .rounded(px(6.))
                                                            .hover(|s| {
                                                                s.bg(color(
                                                                    "--background-mod-subtle",
                                                                ))
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(18.))
                                                            .child(em)
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    if let Some(m) = this
                                                                        .forum_default_reaction_modal
                                                                        .as_mut()
                                                                    {
                                                                        m.selected = Some(
                                                                            DefaultReactionEmoji {
                                                                                emoji_id: None,
                                                                                emoji_name: Some(
                                                                                    em.to_owned(),
                                                                                ),
                                                                            },
                                                                        );
                                                                        m.picker_open = false;
                                                                        m.failed = false;
                                                                    }
                                                                    cx.notify();
                                                                },
                                                            ))
                                                    },
                                                )),
                                        )
                                    })
                                    .when(modal.failed, |col| {
                                        col.child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(color("--text-feedback-critical"))
                                                .child(dc_intl::t("F75nNl")),
                                        )
                                    }),
                            )
                            // Footer
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
                                            .id("forum-default-reaction-cancel")
                                            .cursor_pointer()
                                            .px(px(16.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.forum_default_reaction_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("forum-default-reaction-save")
                                            .when(can_save, |b| b.cursor_pointer())
                                            .when(!can_save, |b| b.opacity(0.5))
                                            .px(px(18.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("R3BPH+"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !can_save {
                                                    return;
                                                }
                                                if let Some(m) =
                                                    this.forum_default_reaction_modal.as_mut()
                                                {
                                                    m.saving = true;
                                                    m.failed = false;
                                                }
                                                let _ = this.requests.send(
                                                    patch_default_reaction_request(
                                                        channel_id,
                                                        selected_clone.as_ref(),
                                                    ),
                                                );
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

    /// Capture harness: `forum-default-reaction [open|picker|error|close]` (module 939940).
    #[cfg(feature = "capture")]
    pub(crate) fn script_forum_default_reaction(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let cid = self.channel.unwrap_or(100);
        match arg.trim() {
            "close" => {
                self.forum_default_reaction_modal = None;
                cx.notify();
                "forum-default-reaction: closed".to_owned()
            }
            "picker" => {
                self.forum_default_reaction_modal = Some(ForumDefaultReactionModal {
                    channel_id: cid,
                    initial: None,
                    selected: Some(DefaultReactionEmoji {
                        emoji_id: None,
                        emoji_name: Some("🔥".to_owned()),
                    }),
                    picker_open: true,
                    saving: false,
                    failed: false,
                });
                cx.notify();
                "forum-default-reaction: picker".to_owned()
            }
            "error" => {
                self.forum_default_reaction_modal = Some(ForumDefaultReactionModal {
                    channel_id: cid,
                    initial: None,
                    selected: Some(DefaultReactionEmoji {
                        emoji_id: None,
                        emoji_name: Some("👍".to_owned()),
                    }),
                    picker_open: false,
                    saving: false,
                    failed: true,
                });
                cx.notify();
                "forum-default-reaction: error".to_owned()
            }
            _ => {
                self.forum_default_reaction_modal = Some(ForumDefaultReactionModal {
                    channel_id: cid,
                    initial: None,
                    selected: Some(DefaultReactionEmoji {
                        emoji_id: None,
                        emoji_name: Some("🔥".to_owned()),
                    }),
                    picker_open: false,
                    saving: false,
                    failed: false,
                });
                cx.notify();
                "forum-default-reaction: open".to_owned()
            }
        }
    }
}
