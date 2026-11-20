//! Invite Accept header and collapsible Server Settings expander (module 700156 `H`, `K`, `M`,
//! `O`, `Q`; join settings store module 960736; accordion module 844330; CSS modules 88861,
//! 884182).
//!
//! Renders the resolved invite header for server, stream, embedded application, group DM, friend,
//! loading, and error states, along with the collapsible "Server Settings" accordion (Server
//! Nickname, Allow direct messages, Show your activity status) applied when accepting the invite.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, IntoElement, Subscription, Window, div,
    prelude::*, px, rgba,
};
use serde_json::{Value, json};

/// `IS_GUEST_INVITE` flag bit (module 821418).
const IS_GUEST_INVITE: u64 = 1;
/// Invite target types (module 294630 `y`).
const TARGET_STREAM: u64 = 1;
const TARGET_EMBEDDED_APPLICATION: u64 = 2;

/// Fetch key prefix for resolving an invite preview (`GET /invites/{code}?with_counts=true`).
pub(super) const PREVIEW_KEY_PREFIX: &str = "invite-accept-preview:";

/// Invite kind resolved from the `/invites/{code}` payload (module 700156 `H`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InviteTarget {
    Loading,
    Error(String),
    Guild {
        guild_id: Option<Id>,
        guild_name: String,
        inviter_name: Option<String>,
        online: Option<u64>,
        total: Option<u64>,
        target_type: Option<u64>,
        target_user_name: Option<String>,
        target_app_name: Option<String>,
        is_nickname_changeable: bool,
        is_guest: bool,
        roles: Vec<String>,
    },
    GroupDm {
        channel_name: Option<String>,
        inviter_name: String,
    },
    Friend {
        inviter_name: String,
        is_register: bool,
    },
}

/// Join settings state (`960736`).
#[derive(Clone, Debug)]
pub(crate) struct JoinSettings {
    pub expanded: bool,
    pub nickname: String,
    pub dms_allowed: bool,
    pub show_activity: bool,
}

impl JoinSettings {
    /// Default join settings per module 960736 (`guildSize > 200` disables activity for large guilds).
    pub(crate) fn defaults_for(guild_size: Option<u64>) -> Self {
        let large_guild = guild_size.is_some_and(|n| n > 200);
        Self {
            expanded: false,
            nickname: String::new(),
            dms_allowed: true,
            show_activity: !large_guild,
        }
    }
}

pub(crate) struct InviteAcceptModal {
    pub code: String,
    pub target: InviteTarget,
    pub settings: JoinSettings,
    pub nickname_editor: Entity<Editor>,
    _events: Subscription,
    pub accepting: bool,
    focus: FocusHandle,
}

/// Parses a `GET /invites/{code}` JSON response into `InviteTarget` (`H` of module 700156).
pub(crate) fn parse_invite_target(body: &Value) -> InviteTarget {
    if let Some(msg) = body["message"].as_str()
        && body["code"].is_u64()
        && body["guild"].is_null()
        && body["channel"].is_null()
        && body["inviter"].is_null()
    {
        return InviteTarget::Error(msg.to_owned());
    }
    let inviter_name = body["inviter"]["global_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| body["inviter"]["username"].as_str())
        .map(str::to_owned);
    if let Some(guild) = body["guild"].as_object() {
        let guild_id = guild
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<Id>().ok());
        let guild_name = guild
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Server")
            .to_owned();
        let flags = body["flags"].as_u64().unwrap_or(0);
        let target_type = body["target_type"].as_u64();
        let target_user_name = body["target_user"]["global_name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| body["target_user"]["username"].as_str())
            .map(str::to_owned);
        let target_app_name = body["target_application"]["name"]
            .as_str()
            .map(str::to_owned);
        let roles = body["roles"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        return InviteTarget::Guild {
            guild_id,
            guild_name,
            inviter_name,
            online: body["approximate_presence_count"].as_u64(),
            total: body["approximate_member_count"].as_u64(),
            target_type,
            target_user_name,
            target_app_name,
            is_nickname_changeable: body["is_nickname_changeable"].as_bool().unwrap_or(true),
            is_guest: (flags & IS_GUEST_INVITE) != 0,
            roles,
        };
    }
    if let Some(channel) = body["channel"].as_object()
        && channel.get("type").and_then(|v| v.as_u64()) == Some(3)
    {
        return InviteTarget::GroupDm {
            channel_name: channel
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            inviter_name: inviter_name.unwrap_or_else(|| "Someone".to_owned()),
        };
    }
    if let Some(inviter) = inviter_name {
        return InviteTarget::Friend {
            inviter_name: inviter,
            is_register: false,
        };
    }
    InviteTarget::Error(dc_intl::t("mDFGFj").to_owned())
}

/// Computes the header eyebrow, main title, and optional subtitle for an invite (`H` of 700156).
pub(crate) fn format_invite_header(
    target: &InviteTarget,
) -> (Option<String>, String, Option<String>) {
    match target {
        InviteTarget::Loading => (
            Some(dc_intl::t("3rE1P8").to_owned()),
            dc_intl::t("ZTNur7").to_owned(),
            None,
        ),
        InviteTarget::Error(err) => (Some(dc_intl::t("mDFGFj").to_owned()), err.clone(), None),
        InviteTarget::Guild {
            guild_name,
            inviter_name,
            target_type,
            target_user_name,
            target_app_name,
            ..
        } => {
            let mut eyebrow = match inviter_name.as_deref() {
                Some(name) => dc_intl::t!("5u47va", username = name),
                None => dc_intl::t("3rE1P8").to_owned(),
            };
            if *target_type == Some(TARGET_STREAM)
                && let Some(user) = target_user_name.as_deref()
            {
                eyebrow = dc_intl::t!("x2L32Q", username = user);
            }
            if *target_type == Some(TARGET_EMBEDDED_APPLICATION)
                && let Some(app_name) = target_app_name.as_deref()
            {
                eyebrow = match inviter_name.as_deref() {
                    Some(name) => dc_intl::t!("UW1Cav", username = name),
                    None => dc_intl::t("ENSuN3").to_owned(),
                };
                let sub = format!("{} {guild_name}", dc_intl::t("3gg9fF"));
                return (Some(eyebrow), app_name.to_owned(), Some(sub));
            }
            (Some(eyebrow), guild_name.clone(), None)
        }
        InviteTarget::GroupDm {
            channel_name,
            inviter_name,
        } => match channel_name.as_deref() {
            Some(name) if !name.is_empty() => (
                Some(dc_intl::t!("5u47va", username = inviter_name.as_str())),
                name.to_owned(),
                None,
            ),
            _ => (
                Some(dc_intl::t("OsdY8B").to_owned()),
                inviter_name.clone(),
                None,
            ),
        },
        InviteTarget::Friend {
            inviter_name,
            is_register,
        } => {
            let title = dc_intl::t!("4aF92R", username = inviter_name.as_str());
            let sub = if *is_register {
                dc_intl::t!("6r4JiR", username = inviter_name.as_str())
            } else {
                dc_intl::t!("Quj7HX", username = inviter_name.as_str())
            };
            (None, title, Some(sub))
        }
    }
}

impl Root {
    pub(super) fn invite_accept_open(&self) -> bool {
        self.invite_accept.is_some()
    }

    /// Opens the Invite Accept preview modal (module 700156) and resolves `GET /invites/{code}`.
    pub(super) fn open_invite_accept(&mut self, code: String, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder(dc_intl::t("09Q8yp").to_owned());
            ed
        });
        let events = cx.subscribe(&editor, |this, ed, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let mut text = ed.read(cx).text().to_owned();
                if text.chars().count() > 32 {
                    text = text.chars().take(32).collect();
                    ed.update(cx, |e, _| e.set_text(&text));
                }
                if let Some(m) = this.invite_accept.as_mut() {
                    m.settings.nickname = text;
                }
                cx.notify();
            }
        });
        self.invite_accept = Some(InviteAcceptModal {
            code: code.clone(),
            target: InviteTarget::Loading,
            settings: JoinSettings::defaults_for(None),
            nickname_editor: editor,
            _events: events,
            accepting: false,
            focus: cx.focus_handle(),
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{PREVIEW_KEY_PREFIX}{code}"),
            route: format!("/invites/{code}?with_counts=true&with_expiration=true"),
        });
        cx.notify();
    }

    pub(super) fn close_invite_accept(&mut self, cx: &mut Context<Self>) {
        self.invite_accept = None;
        cx.notify();
    }

    pub(super) fn invite_accept_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.invite_accept.is_none() {
            return false;
        }
        if key.key == "escape" {
            self.close_invite_accept(cx);
            return true;
        }
        false
    }

    /// Handles the resolved invite preview payload (`GET /invites/{code}`).
    pub(super) fn invite_accept_preview_fetched(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(code) = key.strip_prefix(PREVIEW_KEY_PREFIX) else {
            return false;
        };
        let body = self.fetched.get(key).cloned().unwrap_or(Value::Null);
        let target = parse_invite_target(&body);
        if let Some(m) = self.invite_accept.as_mut()
            && m.code == code
        {
            if let InviteTarget::Guild { total, .. } = &target {
                m.settings = JoinSettings::defaults_for(*total);
            }
            m.target = target;
        }
        cx.notify();
        true
    }

    /// Accepts the currently previewed invite and sends optional join settings.
    fn submit_invite_accept(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.invite_accept.as_mut() else {
            return;
        };
        if modal.accepting {
            return;
        }
        modal.accepting = true;
        let code = modal.code.clone();
        let nick = modal.settings.nickname.trim().to_owned();
        let guild_id = match &modal.target {
            InviteTarget::Guild { guild_id, .. } => *guild_id,
            _ => None,
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{}{code}", super::invite_embed::ACCEPT_KEY)),
            method: reqwest::Method::POST,
            route: format!("/invites/{code}"),
            body: Some(json!({})),
        });
        if let Some(gid) = guild_id
            && !nick.is_empty()
        {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{gid}/members/@me"),
                body: Some(json!({ "nick": nick })),
            });
        }
        self.invite_accept = None;
        cx.notify();
    }

    /// Capture helper: `invite-accept [expanded|stream|app|group|friend|error]` opens the modal.
    #[cfg(feature = "capture")]
    pub(crate) fn script_invite_accept(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        self.open_invite_accept("gravecord-demo".to_owned(), cx);
        if let Some(m) = self.invite_accept.as_mut() {
            m.target = if arg.contains("stream") {
                InviteTarget::Guild {
                    guild_id: self.guild,
                    guild_name: "gravecord fixture".to_owned(),
                    inviter_name: Some("LoreanXavier".to_owned()),
                    online: Some(12),
                    total: Some(48),
                    target_type: Some(TARGET_STREAM),
                    target_user_name: Some("LoreanXavier".to_owned()),
                    target_app_name: None,
                    is_nickname_changeable: true,
                    is_guest: false,
                    roles: vec!["Members".to_owned()],
                }
            } else if arg.contains("app") {
                InviteTarget::Guild {
                    guild_id: self.guild,
                    guild_name: "gravecord fixture".to_owned(),
                    inviter_name: Some("LoreanXavier".to_owned()),
                    online: Some(12),
                    total: Some(48),
                    target_type: Some(TARGET_EMBEDDED_APPLICATION),
                    target_user_name: None,
                    target_app_name: Some("Watch Together".to_owned()),
                    is_nickname_changeable: true,
                    is_guest: false,
                    roles: Vec::new(),
                }
            } else if arg.contains("group") {
                InviteTarget::GroupDm {
                    channel_name: None,
                    inviter_name: "LoreanXavier".to_owned(),
                }
            } else if arg.contains("friend") {
                InviteTarget::Friend {
                    inviter_name: "LoreanXavier".to_owned(),
                    is_register: false,
                }
            } else if arg.contains("error") {
                InviteTarget::Error("Invite Invalid".to_owned())
            } else {
                InviteTarget::Guild {
                    guild_id: self.guild,
                    guild_name: "gravecord fixture".to_owned(),
                    inviter_name: Some("LoreanXavier".to_owned()),
                    online: Some(18),
                    total: Some(64),
                    target_type: None,
                    target_user_name: None,
                    target_app_name: None,
                    is_nickname_changeable: true,
                    is_guest: false,
                    roles: vec!["Community".to_owned()],
                }
            };
            m.settings.expanded = arg.contains("expanded") || arg.is_empty();
        }
        format!("invite-accept: opened ({arg})")
    }

    /// Renders the Invite Accept modal (`700156` + `844330` collapsible Server Settings).
    pub(super) fn invite_accept_layer(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.invite_accept.as_ref()?;
        let (eyebrow, title, subtitle) = format_invite_header(&modal.target);
        let is_error = matches!(modal.target, InviteTarget::Error(_));
        let is_loading = matches!(modal.target, InviteTarget::Loading);

        let initial = title
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        let (counts, roles, show_settings, nickname_changeable) = match &modal.target {
            InviteTarget::Guild {
                online,
                total,
                roles,
                is_guest,
                is_nickname_changeable,
                ..
            } => (
                (*online, *total),
                roles.clone(),
                !*is_guest,
                *is_nickname_changeable,
            ),
            _ => ((None, None), Vec::new(), false, false),
        };

        let expanded = modal.settings.expanded;
        let dms_allowed = modal.settings.dms_allowed;
        let show_activity = modal.settings.show_activity;

        let settings_accordion = show_settings.then(|| {
            let mut body = div().px(px(14.)).pb(px(14.)).flex().flex_col().gap(px(12.));

            if nickname_changeable {
                body = body
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("me1lRk")),
                            )
                            .child(
                                div()
                                    .h(px(38.))
                                    .px(px(10.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .child(modal.nickname_editor.clone()),
                            ),
                    )
                    .child(div().h(px(1.)).bg(color("--border-subtle")));
            }

            body = body
                .child(
                    div()
                        .id("invite-accept-toggle-dms")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("/2ed37")),
                        )
                        .child(super::create_channel::switch(dms_allowed))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.invite_accept.as_mut() {
                                m.settings.dms_allowed = !m.settings.dms_allowed;
                            }
                            cx.notify();
                        })),
                )
                .child(div().h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .id("invite-accept-toggle-activity")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("bN4m1G")),
                        )
                        .child(super::create_channel::switch(show_activity))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.invite_accept.as_mut() {
                                m.settings.show_activity = !m.settings.show_activity;
                            }
                            cx.notify();
                        })),
                );

            div()
                .w_full()
                .mt(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("invite-accept-settings-header")
                        .px(px(14.))
                        .py(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.invite_accept.as_mut() {
                                m.settings.expanded = !m.settings.expanded;
                            }
                            cx.notify();
                        }))
                        .child(icon(
                            "guild_menu_settings",
                            18.,
                            "--interactive-text-default",
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("A5wHQ/")),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-subtle"))
                                        .child(dc_intl::t("UKCSEd")),
                                ),
                        )
                        .child(
                            gpui::svg()
                                .path("icons/bot_chevron_small_right.svg")
                                .size(px(18.))
                                .text_color(color("--interactive-text-default"))
                                .with_transformation(gpui::Transformation::rotate(
                                    gpui::percentage(if expanded { 0.25 } else { 0. }),
                                )),
                        ),
                )
                .when(expanded, |d| d.child(body))
        });

        Some(
            div()
                .id("invite-accept-backdrop")
                .track_focus(&modal.focus)
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.close_invite_accept(cx)))
                .child(
                    div()
                        .id("invite-accept-modal")
                        .w(px(440.))
                        .p(px(24.))
                        .rounded(px(12.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_xl()
                        .flex()
                        .flex_col()
                        .items_center()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .size(px(64.))
                                .rounded(px(16.))
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(26.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(initial),
                        )
                        .children(eyebrow.map(|line| {
                            div()
                                .mt(px(12.))
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child(line)
                        }))
                        .child(
                            div()
                                .mt(px(4.))
                                .text_center()
                                .text_size(px(22.))
                                .line_height(px(28.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .children(subtitle.map(|sub| {
                            div()
                                .mt(px(4.))
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(sub)
                        }))
                        .when(counts.0.is_some() || counts.1.is_some(), |d| {
                            d.child(
                                div()
                                    .mt(px(8.))
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .children(counts.0.map(|n| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(gpui::rgb(0x23a55a)),
                                            )
                                            .child(format!("{n} Online"))
                                    }))
                                    .children(counts.1.map(|n| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(color("--text-muted")),
                                            )
                                            .child(format!("{n} Members"))
                                    })),
                            )
                        })
                        .when(!roles.is_empty(), |d| {
                            d.child(
                                div()
                                    .mt(px(8.))
                                    .flex()
                                    .flex_wrap()
                                    .justify_center()
                                    .gap(px(6.))
                                    .children(roles.into_iter().map(|role| {
                                        div()
                                            .px(px(8.))
                                            .py(px(3.))
                                            .rounded(px(4.))
                                            .bg(color("--background-mod-subtle"))
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(role)
                                    })),
                            )
                        })
                        .children(settings_accordion)
                        .child(
                            div()
                                .mt(px(20.))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .id("invite-accept-cancel")
                                        .h(px(40.))
                                        .px(px(16.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .border_1()
                                        .border_color(color("--control-secondary-border-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(dc_intl::t("cpT0Cq"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_invite_accept(cx);
                                        })),
                                )
                                .when(!is_error, |d| {
                                    d.child(
                                        div()
                                            .id("invite-accept-submit")
                                            .flex_1()
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(color("--brand-500"))
                                            .when(is_loading, |b| b.opacity(0.5))
                                            .when(!is_loading, |b| {
                                                b.cursor_pointer()
                                                    .hover(|s| s.bg(color("--brand-560")))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.submit_invite_accept(cx);
                                                    }))
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child("Accept Invite"),
                                    )
                                }),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_invite_accept_strings_resolve() {
        for key in [
            "A5wHQ/", "UKCSEd", "me1lRk", "09Q8yp", "/2ed37", "bN4m1G", "3rE1P8", "ZTNur7",
            "5u47va", "x2L32Q", "UW1Cav", "ENSuN3", "3gg9fF", "OsdY8B", "4aF92R", "6r4JiR",
            "Quj7HX", "mDFGFj",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in invite_accept"
            );
        }
    }

    #[test]
    fn invite_header_and_join_settings_cover_all_variants() {
        assert!(JoinSettings::defaults_for(Some(50)).show_activity);
        assert!(!JoinSettings::defaults_for(Some(250)).show_activity);

        let stream = InviteTarget::Guild {
            guild_id: Some(1),
            guild_name: "Test".into(),
            inviter_name: Some("Alice".into()),
            online: Some(5),
            total: Some(10),
            target_type: Some(TARGET_STREAM),
            target_user_name: Some("Bob".into()),
            target_app_name: None,
            is_nickname_changeable: true,
            is_guest: false,
            roles: Vec::new(),
        };
        let (eyebrow, title, _) = format_invite_header(&stream);
        assert_eq!(title, "Test");
        assert!(eyebrow.unwrap().contains("Bob"));

        let app = InviteTarget::Guild {
            guild_id: Some(1),
            guild_name: "Test".into(),
            inviter_name: None,
            online: None,
            total: None,
            target_type: Some(TARGET_EMBEDDED_APPLICATION),
            target_user_name: None,
            target_app_name: Some("Chess".into()),
            is_nickname_changeable: false,
            is_guest: false,
            roles: Vec::new(),
        };
        let (eyebrow, title, sub) = format_invite_header(&app);
        assert_eq!(title, "Chess");
        assert!(eyebrow.unwrap().contains("play"));
        assert!(sub.unwrap().contains("Test"));

        let friend = InviteTarget::Friend {
            inviter_name: "Charlie".into(),
            is_register: false,
        };
        let (_, title, sub) = format_invite_header(&friend);
        assert!(title.contains("Charlie"));
        assert!(sub.unwrap().contains("Charlie"));
    }
}
