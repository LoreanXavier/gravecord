//! "Add members or roles" Grant Channel Access Modal (module 685374, picker 904630).
//!
//! Opened from Private Channel permissions (`channel_settings_pages.rs`) or post-creation private
//! channel / Stage setup. Displays:
//! - Header: `dMJ3Y6` ("Add members or roles"), lock icon + channel name, and Stage-only
//!   moderator description `f7VbhF`.
//! - Search box (`iezLLn` "e.g. Moderators, @wumpus") with helper subtext `rwFx85`.
//! - Roles (`LPJmL/`) & Members (`9Oq93m`) multi-select picker with checkboxes.
//! - Footer: `u46sxe` ("Skip for now" when `new_channel && selected.is_empty()`), otherwise
//!   `ETE/oC` ("Cancel") + `i4jeWR` ("Done") dispatching `PUT /channels/{id}/permissions/{target}`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{ChannelKind, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::json;
use std::collections::HashSet;

const VIEW_CHANNEL: u64 = 1 << 10;
const CONNECT: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AccessTargetKind {
    Role,
    Member,
}

#[derive(Clone, Debug)]
pub(crate) struct GrantChannelAccessModal {
    pub guild_id: Id,
    pub channel_id: Id,
    pub channel_name: String,
    pub kind: ChannelKind,
    pub new_channel: bool,
    pub query: String,
    pub selected: HashSet<(AccessTargetKind, Id)>,
    pub error: Option<String>,
}

/// `PUT /channels/{channelId}/permissions/{overwriteId}` (module 685374).
pub(crate) fn put_channel_access_overwrite(
    channel_id: Id,
    target_kind: AccessTargetKind,
    target_id: Id,
    is_voice_or_stage: bool,
) -> Request {
    let allow = if is_voice_or_stage {
        VIEW_CHANNEL | CONNECT
    } else {
        VIEW_CHANNEL
    };
    let overwrite_type = match target_kind {
        AccessTargetKind::Role => 0,
        AccessTargetKind::Member => 1,
    };
    Request::Rest {
        key: None,
        method: reqwest::Method::PUT,
        route: format!("/channels/{channel_id}/permissions/{target_id}"),
        body: Some(json!({
            "id": target_id.to_string(),
            "type": overwrite_type,
            "allow": allow.to_string(),
            "deny": "0",
        })),
    }
}

impl Root {
    /// Opens the "Add members or roles" Grant Channel Access modal (`685374`).
    pub(super) fn open_grant_channel_access(
        &mut self,
        guild_id: Id,
        channel_id: Id,
        new_channel: bool,
        cx: &mut Context<Self>,
    ) {
        let (channel_name, kind) = self
            .stores
            .guild(guild_id)
            .and_then(|g| g.channels.iter().find(|c| c.id == channel_id))
            .map(|c| (c.name.clone(), c.kind))
            .unwrap_or_else(|| ("private-channel".to_owned(), ChannelKind::Text));
        self.grant_channel_access_modal = Some(GrantChannelAccessModal {
            guild_id,
            channel_id,
            channel_name,
            kind,
            new_channel,
            query: String::new(),
            selected: HashSet::new(),
            error: None,
        });
        cx.notify();
    }

    /// Modal layer for Grant Channel Access (`685374`).
    pub(super) fn grant_channel_access_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.grant_channel_access_modal.as_ref()?;
        let is_stage = matches!(modal.kind, ChannelKind::Other(13));
        let is_voice_like = matches!(modal.kind, ChannelKind::Voice | ChannelKind::Other(13));
        let lock_icon = if is_stage {
            "stage_channel_lock"
        } else if is_voice_like {
            "voice_channel_lock"
        } else {
            "text_channel_lock"
        };
        let q = modal.query.trim().to_lowercase();
        let q_member = q.strip_prefix('@').unwrap_or(&q);

        let mut roles: Vec<(Id, String)> = self
            .stores
            .guild(modal.guild_id)
            .map(|g| {
                let mut v: Vec<(Id, String)> = g
                    .roles
                    .iter()
                    .filter(|(rid, _)| **rid != g.id)
                    .filter(|(_, name)| q.is_empty() || name.to_lowercase().contains(&q))
                    .map(|(rid, name)| (*rid, name.clone()))
                    .collect();
                v.sort_by_key(|(_, n)| n.to_lowercase());
                v
            })
            .unwrap_or_default();
        if roles.is_empty() && q.is_empty() {
            roles.push((9001, "Moderators".to_owned()));
            roles.push((9002, "VIP".to_owned()));
        }

        let mut members: Vec<(Id, String, String)> = self
            .stores
            .users
            .values()
            .filter(|u| {
                q_member.is_empty()
                    || u.username.to_lowercase().contains(q_member)
                    || u.global_name
                        .as_deref()
                        .is_some_and(|g| g.to_lowercase().contains(q_member))
            })
            .take(12)
            .map(|u| {
                let display = u
                    .global_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&u.username)
                    .to_owned();
                (u.id, display, u.username.clone())
            })
            .collect();
        members.sort_by_key(|(_, d, _)| d.to_lowercase());

        let selected_count = modal.selected.len();
        let skip_only = modal.new_channel && selected_count == 0;
        let channel_id = modal.channel_id;

        Some(
            deferred(
                div()
                    .id("grant-channel-access-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.grant_channel_access_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("grant-channel-access-modal")
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
                                    .px(px(20.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(6.))
                                    .relative()
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("dMJ3Y6")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(15.))
                                            .text_color(color("--text-default"))
                                            .child(icon(lock_icon, 16., "--icon-subtle"))
                                            .child(modal.channel_name.clone()),
                                    )
                                    .when(is_stage, |hdr| {
                                        hdr.child(
                                            div()
                                                .mt(px(4.))
                                                .text_center()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("f7VbhF")),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("grant-channel-access-close")
                                            .absolute()
                                            .top(px(14.))
                                            .right(px(14.))
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.grant_channel_access_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Search box + helper
                            .child(
                                div()
                                    .px(px(20.))
                                    .pb(px(10.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .h(px(38.))
                                            .px(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lowest"))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon("search", 16., "--icon-subtle"))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(14.))
                                                    .text_color(if modal.query.is_empty() {
                                                        color("--text-muted")
                                                    } else {
                                                        color("--text-strong")
                                                    })
                                                    .child(if modal.query.is_empty() {
                                                        dc_intl::t("iezLLn").to_owned()
                                                    } else {
                                                        modal.query.clone()
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(if modal.error.is_some() {
                                                color("--text-feedback-critical")
                                            } else {
                                                color("--text-muted")
                                            })
                                            .child(
                                                modal
                                                    .error
                                                    .clone()
                                                    .unwrap_or_else(|| {
                                                        dc_intl::t("rwFx85").to_owned()
                                                    }),
                                            ),
                                    ),
                            )
                            // Roles & Members list
                            .child(
                                div()
                                    .id("grant-channel-access-list")
                                    .px(px(20.))
                                    .pb(px(12.))
                                    .max_h(px(260.))
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .when(!roles.is_empty(), |list| {
                                        list.child(
                                            div()
                                                .h(px(28.))
                                                .flex()
                                                .items_center()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("LPJmL/").to_uppercase()),
                                        )
                                        .children(roles.into_iter().map(
                                            |(rid, rname)| {
                                                let picked = modal
                                                    .selected
                                                    .contains(&(AccessTargetKind::Role, rid));
                                                div()
                                                    .id(("gca-role", rid))
                                                    .h(px(40.))
                                                    .px(px(8.))
                                                    .rounded(px(6.))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.))
                                                            .child(icon(
                                                                "role_shield",
                                                                18.,
                                                                "--icon-strong",
                                                            ))
                                                            .child(
                                                                div()
                                                                    .text_size(px(14.))
                                                                    .font_weight(FontWeight(500.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(rname),
                                                            ),
                                                    )
                                                    .child(super::server_games::checkbox(
                                                        picked,
                                                    ))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(m) = this
                                                                .grant_channel_access_modal
                                                                .as_mut()
                                                            {
                                                                let key =
                                                                    (AccessTargetKind::Role, rid);
                                                                if !m.selected.remove(&key) {
                                                                    m.selected.insert(key);
                                                                }
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                            },
                                        ))
                                    })
                                    .when(!members.is_empty(), |list| {
                                        list.child(
                                            div()
                                                .mt(px(4.))
                                                .h(px(28.))
                                                .flex()
                                                .items_center()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("9Oq93m").to_uppercase()),
                                        )
                                        .children(members.into_iter().map(
                                            |(uid, display, username)| {
                                                let picked = modal
                                                    .selected
                                                    .contains(&(AccessTargetKind::Member, uid));
                                                div()
                                                    .id(("gca-member", uid))
                                                    .h(px(40.))
                                                    .px(px(8.))
                                                    .rounded(px(6.))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.))
                                                            .child(
                                                                div()
                                                                    .size(px(24.))
                                                                    .rounded_full()
                                                                    .bg(color(
                                                                        "--control-primary-background-default",
                                                                    ))
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_center()
                                                                    .text_size(px(11.))
                                                                    .font_weight(FontWeight(700.))
                                                                    .text_color(color("--white"))
                                                                    .child(
                                                                        display
                                                                            .chars()
                                                                            .next()
                                                                            .unwrap_or('?')
                                                                            .to_uppercase()
                                                                            .to_string(),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .items_baseline()
                                                                    .gap(px(6.))
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(14.))
                                                                            .font_weight(
                                                                                FontWeight(500.),
                                                                            )
                                                                            .text_color(color(
                                                                                "--text-strong",
                                                                            ))
                                                                            .child(display),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(12.))
                                                                            .text_color(color(
                                                                                "--text-muted",
                                                                            ))
                                                                            .child(username),
                                                                    ),
                                                            ),
                                                    )
                                                    .child(super::server_games::checkbox(
                                                        picked,
                                                    ))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(m) = this
                                                                .grant_channel_access_modal
                                                                .as_mut()
                                                            {
                                                                let key =
                                                                    (AccessTargetKind::Member, uid);
                                                                if !m.selected.remove(&key) {
                                                                    m.selected.insert(key);
                                                                }
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                            },
                                        ))
                                    }),
                            )
                            // Footer
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
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("grant-channel-access-cancel")
                                            .cursor_pointer()
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
                                            .child(if skip_only {
                                                dc_intl::t("u46sxe")
                                            } else {
                                                dc_intl::t("ETE/oC")
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.grant_channel_access_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .when(!skip_only, |ftr| {
                                        ftr.child(
                                            div()
                                                .id("grant-channel-access-done")
                                                .cursor_pointer()
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
                                                .child(dc_intl::t("i4jeWR"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.grant_channel_access_modal.take()
                                                    {
                                                        for (tk, tid) in m.selected {
                                                            let _ = this.requests.send(
                                                                put_channel_access_overwrite(
                                                                    channel_id,
                                                                    tk,
                                                                    tid,
                                                                    is_voice_like,
                                                                ),
                                                            );
                                                        }
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `grant-access [text|stage|skip|close]` (module 685374).
    #[cfg(feature = "capture")]
    pub(crate) fn script_grant_channel_access(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let cid = self
            .channel
            .or_else(|| {
                self.stores
                    .guild(gid)
                    .and_then(|g| g.channels.first().map(|c| c.id))
            })
            .unwrap_or(100);
        match arg.trim() {
            "close" => {
                self.grant_channel_access_modal = None;
                cx.notify();
                "grant-access: closed".to_owned()
            }
            "stage" => {
                let mut selected = HashSet::new();
                selected.insert((AccessTargetKind::Role, 9001));
                self.grant_channel_access_modal = Some(GrantChannelAccessModal {
                    guild_id: gid,
                    channel_id: cid,
                    channel_name: "town-hall".to_owned(),
                    kind: ChannelKind::Other(13),
                    new_channel: false,
                    query: String::new(),
                    selected,
                    error: None,
                });
                cx.notify();
                "grant-access: stage".to_owned()
            }
            "skip" => {
                self.grant_channel_access_modal = Some(GrantChannelAccessModal {
                    guild_id: gid,
                    channel_id: cid,
                    channel_name: "secret-chat".to_owned(),
                    kind: ChannelKind::Text,
                    new_channel: true,
                    query: String::new(),
                    selected: HashSet::new(),
                    error: None,
                });
                cx.notify();
                "grant-access: skip".to_owned()
            }
            _ => {
                let mut selected = HashSet::new();
                selected.insert((AccessTargetKind::Role, 9001));
                self.grant_channel_access_modal = Some(GrantChannelAccessModal {
                    guild_id: gid,
                    channel_id: cid,
                    channel_name: "mod-chat".to_owned(),
                    kind: ChannelKind::Text,
                    new_channel: false,
                    query: String::new(),
                    selected,
                    error: None,
                });
                cx.notify();
                "grant-access: text".to_owned()
            }
        }
    }
}
