//! "Who can create a post?" Forum Admin Onboarding Permissions modal (module 698325, called from
//! forum admin onboarding checklist 745431 `nW`).
//!
//! Lets server admins choose which roles can create posts in a Forum/Media channel (`TFGnmk`),
//! toggle `"Let @everyone post"` (`kPwwA/`, bold description `l7Ercq`), pick specific roles
//! (`8kKqCW` "Enter @role..." with colored role dots), and save (`R3BPH+`) via
//! `PUT`/`DELETE /channels/{channelId}/permissions/{roleId}` (showing `mgZKZP` on failure).

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    prelude::*, px, rgba,
};
use serde_json::json;
use std::collections::HashSet;

const SEND_MESSAGES: u64 = 1 << 11;
const VIEW_CHANNEL: u64 = 1 << 10;

#[derive(Clone, Debug)]
pub(crate) struct ForumPostPermsModal {
    pub guild_id: Id,
    pub channel_id: Id,
    pub is_private: bool,
    pub let_everyone_post: bool,
    pub selected_roles: HashSet<Id>,
    pub has_error: bool,
}

/// `PUT /channels/{channelId}/permissions/{roleId}` (module 698325).
pub(crate) fn put_forum_post_perm_request(
    channel_id: Id,
    role_id: Id,
    allow: u64,
    deny: u64,
) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::PUT,
        route: format!("/channels/{channel_id}/permissions/{role_id}"),
        body: Some(json!({
            "id": role_id.to_string(),
            "type": 0,
            "allow": allow.to_string(),
            "deny": deny.to_string(),
        })),
    }
}

/// `DELETE /channels/{channelId}/permissions/{roleId}` (module 698325).
pub(crate) fn delete_forum_post_perm_request(channel_id: Id, role_id: Id) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/channels/{channel_id}/permissions/{role_id}"),
        body: None,
    }
}

fn bold_everyone_desc() -> StyledText {
    let runs = dc_intl::format_runs("l7Ercq", &[]);
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
    /// Opens the "Who can create a post?" forum permissions modal (`698325`).
    pub(super) fn open_forum_post_perms(
        &mut self,
        guild_id: Id,
        channel_id: Id,
        cx: &mut Context<Self>,
    ) {
        self.forum_post_perms_modal = Some(ForumPostPermsModal {
            guild_id,
            channel_id,
            is_private: false,
            let_everyone_post: true,
            selected_roles: HashSet::new(),
            has_error: false,
        });
        cx.notify();
    }

    /// Modal layer for "Who can create a post?" (`698325`).
    pub(super) fn forum_post_perms_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.forum_post_perms_modal.as_ref()?;
        let can_save = modal.let_everyone_post || !modal.selected_roles.is_empty();
        let guild_id = modal.guild_id;
        let channel_id = modal.channel_id;
        let perm_bits = if modal.is_private {
            VIEW_CHANNEL | SEND_MESSAGES
        } else {
            SEND_MESSAGES
        };

        let mut roles: Vec<(Id, String, u32)> = self
            .stores
            .guild(guild_id)
            .map(|g| {
                let mut v: Vec<(Id, String, u32)> = g
                    .roles
                    .iter()
                    .filter(|(rid, _)| **rid != g.id)
                    .map(|(rid, name)| (*rid, name.clone(), 0x5865f2))
                    .collect();
                v.sort_by_key(|(_, n, _)| n.to_lowercase());
                v
            })
            .unwrap_or_default();
        if roles.is_empty() {
            roles = vec![
                (8001, "Moderators".to_owned(), 0x57f287),
                (8002, "Contributors".to_owned(), 0xfee75c),
                (8003, "Members".to_owned(), 0x5865f2),
            ];
        }
        let all_role_ids: Vec<Id> = roles.iter().map(|(id, _, _)| *id).collect();

        Some(
            deferred(
                div()
                    .id("forum-post-perms-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forum_post_perms_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("forum-post-perms-modal")
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
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(icon("forum_channel", 20., "--icon-strong"))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("TFGnmk")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("forum-post-perms-close")
                                            .flex_none()
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.forum_post_perms_modal = None;
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
                                    .gap(px(14.))
                                    // Role multi-select box (`.roleSelector_nKMVXR`)
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--background-base-lowest"))
                                            .bg(color("--background-base-lower"))
                                            .p(px(10.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("8kKqCW")),
                                            )
                                            .children(roles.into_iter().take(5).map(
                                                |(rid, rname, col)| {
                                                    let picked =
                                                        modal.selected_roles.contains(&rid);
                                                    div()
                                                        .id(("fpp-role", rid))
                                                        .h(px(34.))
                                                        .px(px(8.))
                                                        .rounded(px(6.))
                                                        .cursor_pointer()
                                                        .bg(if picked {
                                                            color("--background-mod-subtle")
                                                        } else {
                                                            rgba(0x00000000)
                                                        })
                                                        .hover(|s| {
                                                            s.bg(color("--background-mod-subtle"))
                                                        })
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_center()
                                                                .gap(px(8.))
                                                                .child(
                                                                    div()
                                                                        .size(px(8.))
                                                                        .rounded_full()
                                                                        .bg(rgba(
                                                                            (col << 8) | 0xff,
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(14.))
                                                                        .text_color(color(
                                                                            "--text-strong",
                                                                        ))
                                                                        .child(format!("@{rname}")),
                                                                ),
                                                        )
                                                        .child(super::server_games::checkbox(
                                                            picked,
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(m) = this
                                                                    .forum_post_perms_modal
                                                                    .as_mut()
                                                                {
                                                                    if !m
                                                                        .selected_roles
                                                                        .remove(&rid)
                                                                    {
                                                                        m.selected_roles
                                                                            .insert(rid);
                                                                    }
                                                                    m.has_error = false;
                                                                }
                                                                cx.notify();
                                                            },
                                                        ))
                                                },
                                            )),
                                    )
                                    // Let @everyone post toggle row (`.row_nKMVXR`)
                                    .child(
                                        div()
                                            .id("forum-post-perms-everyone")
                                            .cursor_pointer()
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(12.))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) =
                                                    this.forum_post_perms_modal.as_mut()
                                                {
                                                    m.let_everyone_post = !m.let_everyone_post;
                                                    m.has_error = false;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("kPwwA/")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-muted"))
                                                            .child(bold_everyone_desc()),
                                                    ),
                                            )
                                            .child(super::create_channel::switch(
                                                modal.let_everyone_post,
                                            )),
                                    )
                                    .when(modal.has_error, |col| {
                                        col.child(
                                            div()
                                                .text_center()
                                                .text_size(px(13.))
                                                .text_color(color("--text-feedback-critical"))
                                                .child(dc_intl::t("mgZKZP")),
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
                                            .id("forum-post-perms-cancel")
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
                                                this.forum_post_perms_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("forum-post-perms-save")
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
                                                if let Some(m) = this.forum_post_perms_modal.take()
                                                {
                                                    if m.let_everyone_post {
                                                        let _ = this.requests.send(
                                                            put_forum_post_perm_request(
                                                                channel_id, guild_id, perm_bits, 0,
                                                            ),
                                                        );
                                                    } else {
                                                        let _ = this.requests.send(
                                                            put_forum_post_perm_request(
                                                                channel_id, guild_id, 0, perm_bits,
                                                            ),
                                                        );
                                                    }
                                                    for rid in &all_role_ids {
                                                        if m.selected_roles.contains(rid) {
                                                            let _ = this.requests.send(
                                                                put_forum_post_perm_request(
                                                                    channel_id, *rid, perm_bits, 0,
                                                                ),
                                                            );
                                                        } else {
                                                            let _ = this.requests.send(
                                                                delete_forum_post_perm_request(
                                                                    channel_id, *rid,
                                                                ),
                                                            );
                                                        }
                                                    }
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

    /// Capture harness: `forum-post-perms [open|error|close]` (module 698325).
    #[cfg(feature = "capture")]
    pub(crate) fn script_forum_post_perms(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let cid = self.channel.unwrap_or(100);
        match arg.trim() {
            "close" => {
                self.forum_post_perms_modal = None;
                cx.notify();
                "forum-post-perms: closed".to_owned()
            }
            "error" => {
                let mut selected = HashSet::new();
                selected.insert(8001);
                self.forum_post_perms_modal = Some(ForumPostPermsModal {
                    guild_id: gid,
                    channel_id: cid,
                    is_private: false,
                    let_everyone_post: false,
                    selected_roles: selected,
                    has_error: true,
                });
                cx.notify();
                "forum-post-perms: error".to_owned()
            }
            _ => {
                let mut selected = HashSet::new();
                selected.insert(8001);
                self.forum_post_perms_modal = Some(ForumPostPermsModal {
                    guild_id: gid,
                    channel_id: cid,
                    is_private: false,
                    let_everyone_post: true,
                    selected_roles: selected,
                    has_error: false,
                });
                cx.notify();
                "forum-post-perms: open".to_owned()
            }
        }
    }
}
