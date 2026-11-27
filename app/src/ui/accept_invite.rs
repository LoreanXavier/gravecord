//! Accept Invite modal & invite route (modules 654000 + 700156 + 972387 + 4274).
//!
//! Opened from `deep_link_route.rs` `Route::Invite(code)` and invite clicks:
//! - Resolves `GET /invites/{code}?with_counts=true&with_expiration=true&with_permissions=true`.
//! - Renders Loading (`ZTNur7`), Invalid/Expired (`u9zxnX` / `FWkU6P` / `wcqOoF`), or Valid card:
//!   - Guild, Group DM, Friend, Stream, and Embedded Activity header variants (`700156`: `3rE1P8`,
//!     `5u47va`, `x2L32Q`, `UW1Cav`, `ENSuN3`, `OsdY8B`, `4aF92R`, `Quj7HX`).
//!   - Collapsible Server Settings (`A5wHQ/`, `UKCSEd`, `me1lRk`, `09Q8yp`, `/2ed37`, `bN4m1G`).
//!   - 100-server limit state (`ZNCziL`) + inline Nitro upsell banner (`4/W4Pz`, `b6KOkI`).
//!   - Primary CTA: `9sWQNT` ("Accept as {user}"), `e/6Ogt` ("Accept Invite"), or `RscU7I` ("Play")
//!     + `eylRaY` ("Spectate") for activity invites; secondary `ndsK4Z` ("No Thanks").
//!   - Optional right-hand guild splash (`size: "xl"` split layout).
//! - Submits `POST /invites/{code}` and maps error codes (`4274`: 30001, 30019, 50095, 50096,
//!   40069, 20024, 20086, 400002, 700000).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, img, prelude::*, px, rgb, rgba};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(super) const RESOLVE_KEY_PREFIX: &str = "accept-invite:resolve:";
pub(super) const ACCEPT_KEY_PREFIX: &str = "accept-invite:accept:";

/// Resolved invite data (`GET /invites/{code}`).
#[derive(Clone, Debug)]
pub struct ResolvedInvite {
    pub code: String,
    #[allow(dead_code)]
    pub guild_id: Option<String>,
    pub guild_name: Option<String>,
    pub guild_splash: Option<String>,
    pub channel_name: Option<String>,
    pub is_group_dm: bool,
    pub inviter_name: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u64>,
    pub target_application_name: Option<String>,
    pub is_stream: bool,
    /// Module 897288 (`InviteRoles`): roles granted by the invite (`body["roles"]`).
    pub(super) roles: Vec<super::invite_embed::InviteRole>,
    /// Module 999527 (`InviteExpiringSoon`): ISO-8601 `expires_at` from `GET /invites/{code}?with_expiration=true`.
    pub expires_at: Option<String>,
}

/// Student Hubs guild ID excluded by module 999527 (`330936.TA`).
pub(crate) const STUDENT_HUBS_GUILD_ID: &str = "884924873015689226";

/// Module 999527 (`InviteExpiringSoon`): formats `"Invite expires in {n} day(s)/hour(s)/minute(s)"`
/// (`l7PkYW`, `Ifgxdy`, `5fXjSj`) when a guild invite expires within 7 days.
pub(crate) fn format_invite_expiring_soon(
    expires_at: Option<&str>,
    has_guild: bool,
    has_target_app: bool,
    guild_id: Option<&str>,
    now_ms: i64,
) -> Option<String> {
    if !has_guild || has_target_app || guild_id == Some(STUDENT_HUBS_GUILD_ID) {
        return None;
    }
    let iso = expires_at.filter(|s| !s.trim().is_empty())?;
    let end_ms = chrono::DateTime::parse_from_rfc3339(iso)
        .ok()?
        .timestamp_millis();
    let diff_ms = end_ms - now_ms;
    if diff_ms <= 0 || diff_ms > 604_800_000 {
        return None;
    }
    let total_min = diff_ms / 60_000;
    if total_min >= 1440 {
        let days = (total_min / 1440) as f64;
        Some(dc_intl::format("l7PkYW", &[("numDays", Arg::Num(days))]))
    } else if total_min >= 60 {
        let hours = (total_min / 60) as f64;
        Some(dc_intl::format("Ifgxdy", &[("numHours", Arg::Num(hours))]))
    } else {
        let minutes = total_min.max(1) as f64;
        Some(dc_intl::format(
            "5fXjSj",
            &[("numMinutes", Arg::Num(minutes))],
        ))
    }
}

#[derive(Clone, Debug)]
pub(super) enum AcceptInvitePhase {
    Loading,
    Invalid { header: String, body: String },
    Ready(Box<ResolvedInvite>),
}

#[derive(Clone, Debug)]
pub(super) struct AcceptInviteModal {
    pub code: String,
    pub phase: AcceptInvitePhase,
    pub settings_open: bool,
    pub allow_dms: bool,
    pub show_activity: bool,
    pub at_server_limit: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// Maps Discord invite accept error codes (`4274.s`) to localized copy.
pub fn accept_error_message(code: Option<u64>, max_guilds: u64) -> String {
    match code {
        Some(30001) => {
            let limit = dc_intl::format("ttJ/hj", &[("quantity", Arg::Num(max_guilds as f64))]);
            format!("{} {}", dc_intl::t("iLyuDO"), limit)
        }
        Some(30019) => dc_intl::t("M6unNJ").to_owned(),
        Some(50095) => dc_intl::t("sRJGR1").to_owned(),
        Some(50096) => dc_intl::t("mY2R+F").to_owned(),
        Some(40069) => dc_intl::format(
            "RXSeLl",
            &[(
                "articleLink",
                Arg::Str(Cow::Borrowed("https://support.discord.com")),
            )],
        ),
        Some(20024) => dc_intl::t("vRw5lm").to_owned(),
        Some(20086) => dc_intl::t("8Ow7Xi").to_owned(),
        Some(400002) => format!("{} {}", dc_intl::t("kJwpBW"), dc_intl::t("ZUEGFn")),
        Some(700000) => format!("{} — {}", dc_intl::t("u/xsK9"), dc_intl::t("SxY4IW")),
        _ => dc_intl::t("dDZRdy").to_owned(),
    }
}

/// Parses `GET /invites/{code}` JSON into [`ResolvedInvite`].
pub fn parse_resolved_invite(code: &str, body: &Value) -> Option<ResolvedInvite> {
    if body.is_null() || body.get("code").is_some() && body.get("message").is_some() {
        return None;
    }
    let guild = body.get("guild").filter(|v| v.is_object());
    let channel = body.get("channel").filter(|v| v.is_object());
    let inviter = body.get("inviter").filter(|v| v.is_object());
    if guild.is_none() && channel.is_none() && inviter.is_none() {
        return None;
    }
    let guild_id = guild.and_then(|g| g["id"].as_str()).map(str::to_owned);
    let guild_splash = guild
        .and_then(|g| g["splash"].as_str())
        .zip(guild_id.as_deref())
        .map(|(hash, gid)| {
            format!("https://cdn.discordapp.com/splashes/{gid}/{hash}.jpg?size=1024")
        });
    Some(ResolvedInvite {
        code: body["code"].as_str().unwrap_or(code).to_owned(),
        guild_id,
        guild_name: guild.and_then(|g| g["name"].as_str()).map(str::to_owned),
        guild_splash,
        channel_name: channel.and_then(|c| c["name"].as_str()).map(str::to_owned),
        is_group_dm: channel.and_then(|c| c["type"].as_u64()) == Some(3),
        inviter_name: inviter
            .and_then(|u| u["global_name"].as_str().or_else(|| u["username"].as_str()))
            .map(str::to_owned),
        member_count: body["approximate_member_count"].as_u64(),
        online_count: body["approximate_presence_count"].as_u64(),
        target_application_name: body["target_application"]["name"]
            .as_str()
            .map(str::to_owned),
        is_stream: body["target_type"].as_u64() == Some(1),
        roles: super::invite_embed::invite_roles(body),
        expires_at: body["expires_at"].as_str().map(str::to_owned),
    })
}

/// Formats the top eyebrow heading from module `700156`.
pub fn invite_eyebrow(inv: &ResolvedInvite) -> String {
    let inviter = inv.inviter_name.as_deref();
    if inv.target_application_name.is_some() {
        return match inviter {
            Some(name) => dc_intl::format("UW1Cav", &[("username", Arg::Str(Cow::Borrowed(name)))]),
            None => dc_intl::t("ENSuN3").to_owned(),
        };
    }
    if inv.is_stream
        && let Some(name) = inviter
    {
        return dc_intl::format("x2L32Q", &[("username", Arg::Str(Cow::Borrowed(name)))]);
    }
    if inv.is_group_dm {
        return dc_intl::t("OsdY8B").to_owned();
    }
    if inv.guild_name.is_none()
        && let Some(name) = inviter
    {
        return dc_intl::format("4aF92R", &[("username", Arg::Str(Cow::Borrowed(name)))]);
    }
    match inviter {
        Some(name) => dc_intl::format("5u47va", &[("username", Arg::Str(Cow::Borrowed(name)))]),
        None => dc_intl::t("3rE1P8").to_owned(),
    }
}

impl Root {
    /// Opens the Accept Invite modal (module `654000`) and resolves `GET /invites/{code}`. `code` may be an invite
    /// key (`code?event=...`, module 21599): the modal shows and joins with its base code, the resolve sends its query.
    pub(super) fn open_accept_invite_modal(&mut self, code: &str, cx: &mut Context<Self>) {
        let key = code.trim().to_owned();
        let cleaned = dc_state::coded_links::split_invite_key(&key).base_code;
        let guild_cap = if self.stores.account.premium_type == Some(2) {
            200
        } else {
            100
        };
        let at_limit = self.stores.guilds.len() >= guild_cap;
        self.accept_invite_modal = Some(AcceptInviteModal {
            code: cleaned.clone(),
            phase: AcceptInvitePhase::Loading,
            settings_open: false,
            allow_dms: true,
            show_activity: true,
            at_server_limit: at_limit,
            busy: false,
            error: None,
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{RESOLVE_KEY_PREFIX}{cleaned}"),
            route: super::coded_link_cards::invite_resolve_route(&key),
        });
        cx.notify();
    }

    /// Handles `GET /invites/{code}` and `POST /invites/{code}` responses.
    pub(super) fn handle_accept_invite_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(code) = key.strip_prefix(RESOLVE_KEY_PREFIX) {
            if self.accept_invite_modal.is_none() {
                return;
            }
            let parsed = parse_resolved_invite(code, body);
            if let Some(inv) = parsed.as_ref()
                && let Some(splash) = inv.guild_splash.clone()
            {
                self.request_image(format!("invite-splash:{}", inv.code), splash);
            }
            if let Some(modal) = self.accept_invite_modal.as_mut() {
                modal.phase = match parsed {
                    Some(inv) => AcceptInvitePhase::Ready(Box::new(inv)),
                    None => AcceptInvitePhase::Invalid {
                        header: dc_intl::t("u9zxnX").to_owned(),
                        body: dc_intl::t("FWkU6P").to_owned(),
                    },
                };
            }
            cx.notify();
            return;
        }
        if key.starts_with(ACCEPT_KEY_PREFIX) {
            let Some(modal) = self.accept_invite_modal.as_mut() else {
                return;
            };
            modal.busy = false;
            let err_code = body["code"].as_u64();
            if err_code.is_some() || body.is_null() {
                let cap = if self.stores.account.premium_type == Some(2) {
                    200
                } else {
                    100
                };
                modal.error = Some(accept_error_message(err_code, cap));
            } else {
                let joined_guild = body["guild"]["id"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok());
                let joined_channel = body["channel"]["id"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok());
                self.accept_invite_modal = None;
                if let (Some(g), Some(c)) = (joined_guild, joined_channel) {
                    self.select(g, Some(c));
                }
            }
            cx.notify();
        }
    }

    /// Renders module `654000` + `700156` Accept Invite Modal.
    pub(super) fn accept_invite_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.accept_invite_modal.as_ref()?;
        let me_name = self
            .stores
            .current_user
            .as_ref()
            .map(|u| u.global_name.as_deref().unwrap_or(&u.username).to_owned())
            .unwrap_or_else(|| "User".to_owned());

        let card: AnyElement = match &modal.phase {
            AcceptInvitePhase::Loading => div()
                .id("accept-invite-loading")
                .w(px(440.))
                .h(px(210.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("ZTNur7")),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(color("--text-muted"))
                        .child(SharedString::from(format!("discord.gg/{}", modal.code))),
                )
                .into_any_element(),
            AcceptInvitePhase::Invalid { header, body } => div()
                .id("accept-invite-invalid")
                .w(px(420.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .size(px(52.))
                        .rounded(px(26.))
                        .bg(rgba(0xda373c22))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("shop_circle_error", 28., "--status-danger")),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("mDFGFj")),
                )
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(SharedString::from(header.clone())),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(SharedString::from(body.clone())),
                )
                .child(
                    div()
                        .id("accept-invite-invalid-ok")
                        .mt(px(8.))
                        .w_full()
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.accept_invite_modal = None;
                            cx.notify();
                        }))
                        .child(dc_intl::t("wcqOoF")),
                )
                .into_any_element(),
            AcceptInvitePhase::Ready(inv) => {
                let eyebrow = invite_eyebrow(inv);
                let title = inv
                    .guild_name
                    .clone()
                    .or_else(|| inv.channel_name.clone())
                    .or_else(|| inv.inviter_name.clone())
                    .unwrap_or_else(|| "Discord".to_owned());
                let is_friend_invite = inv.guild_name.is_none()
                    && inv.channel_name.is_none()
                    && inv.inviter_name.is_some();
                let is_activity = inv.target_application_name.is_some();
                let accept_prefix = dc_intl::format("9sWQNT", &[]);
                let has_splash = inv.guild_splash.is_some();
                let splash_img = self
                    .images
                    .get(&format!("invite-splash:{}", inv.code))
                    .cloned();
                let code_for_post = inv.code.clone();
                let is_busy = modal.busy;
                let at_limit = modal.at_server_limit;
                let settings_open = modal.settings_open;
                let allow_dms = modal.allow_dms;
                let show_activity = modal.show_activity;

                let left = div()
                    .flex_1()
                    .min_w_0()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    // Guild / Invite emblem
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(16.))
                            .bg(rgb(0x5865f2))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(SharedString::from(
                                title
                                    .chars()
                                    .next()
                                    .map(|c| c.to_uppercase().to_string())
                                    .unwrap_or_else(|| "D".into()),
                            )),
                    )
                    // Eyebrow (`700156`)
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(color("--text-muted"))
                            .child(SharedString::from(eyebrow)),
                    )
                    // Server / Target Title
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(SharedString::from(title.clone())),
                    )
                    // Activity target name subtitle if present
                    .when_some(inv.target_application_name.clone(), |col, app| {
                        col.child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(SharedString::from(format!(
                                    "{} {app}",
                                    dc_intl::t("3gg9fF")
                                ))),
                        )
                    })
                    // Friend invite warning (`Quj7HX`)
                    .when(is_friend_invite, |col| {
                        let friend_note = dc_intl::format(
                            "Quj7HX",
                            &[(
                                "username",
                                Arg::Str(Cow::Borrowed(inv.inviter_name.as_deref().unwrap_or(""))),
                            )],
                        );
                        col.child(
                            div()
                                .text_size(px(13.))
                                .text_color(color("--text-muted"))
                                .child(SharedString::from(friend_note)),
                        )
                    })
                    // Online + Total Member counts
                    .when(
                        inv.member_count.is_some() || inv.online_count.is_some(),
                        |col| {
                            col.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .when_some(inv.online_count, |row, online| {
                                        row.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .rounded(px(4.))
                                                        .bg(rgb(0x23a55a)),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-muted"))
                                                        .child(SharedString::from(format!(
                                                            "{online} Online"
                                                        ))),
                                                ),
                                        )
                                    })
                                    .when_some(inv.member_count, |row, members| {
                                        row.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .rounded(px(4.))
                                                        .bg(color("--text-muted")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-muted"))
                                                        .child(SharedString::from(format!(
                                                            "{members} Members"
                                                        ))),
                                                ),
                                        )
                                    }),
                            )
                        },
                    )
                    // Module 897288 (`InviteRoles` in Accept Invite modal `700156`): "Roles Granted" pills.
                    .children(
                        self.invite_roles_list(&inv.roles, cx)
                            .map(|list| div().w_full().child(list)),
                    )
                    // Collapsible Server Settings (`700156` `M`)
                    .when(inv.guild_name.is_some(), |col| {
                        col.child(
                            div()
                                .w_full()
                                .rounded(px(8.))
                                .bg(color("--background-base-lower"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .p(px(10.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .id("accept-invite-settings-toggle")
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.accept_invite_modal.as_mut() {
                                                m.settings_open = !m.settings_open;
                                                cx.notify();
                                            }
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .child(dc_intl::t("A5wHQ/")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("UKCSEd")),
                                                ),
                                        )
                                        .child(icon("chevron_down", 14., "--icon-subtle")),
                                )
                                .when(settings_open, |box_el| {
                                    box_el
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(4.))
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("me1lRk")),
                                                )
                                                .child(
                                                    div()
                                                        .h(px(32.))
                                                        .px(px(10.))
                                                        .rounded(px(6.))
                                                        .bg(color("--background-base-lowest"))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(12.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("09Q8yp")),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("accept-invite-dm-toggle")
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .text_size(px(12.))
                                                .text_color(color("--text-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.accept_invite_modal.as_mut()
                                                    {
                                                        m.allow_dms = !m.allow_dms;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(dc_intl::t("/2ed37"))
                                                .child(if allow_dms { "ON" } else { "OFF" }),
                                        )
                                        .child(
                                            div()
                                                .id("accept-invite-activity-toggle")
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .text_size(px(12.))
                                                .text_color(color("--text-default"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.accept_invite_modal.as_mut()
                                                    {
                                                        m.show_activity = !m.show_activity;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(dc_intl::t("bN4m1G"))
                                                .child(if show_activity { "ON" } else { "OFF" }),
                                        )
                                }),
                        )
                    })
                    // Inline 100-server Nitro upsell banner (`654000` `Z`: `4/W4Pz` + `b6KOkI`)
                    .when(at_limit, |col| {
                        col.child(
                            div()
                                .w_full()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(rgba(0x5865f224))
                                .border_1()
                                .border_color(rgb(0x5865f2))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("4/W4Pz")),
                                )
                                .child(
                                    div()
                                        .id("accept-invite-get-nitro")
                                        .flex_none()
                                        .h(px(30.))
                                        .px(px(12.))
                                        .rounded(px(6.))
                                        .bg(color("--control-primary-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.accept_invite_modal = None;
                                            this.open_nitro(cx);
                                        }))
                                        .child(dc_intl::t("b6KOkI")),
                                ),
                        )
                    })
                    // Error banner (`4274.s`)
                    .when_some(modal.error.clone(), |col, err| {
                        col.child(
                            div()
                                .w_full()
                                .p(px(8.))
                                .rounded(px(6.))
                                .bg(rgba(0xda373c22))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .text_size(px(12.))
                                .text_color(color("--text-feedback-critical"))
                                .child(SharedString::from(err)),
                        )
                    })
                    // Module 999527 (`InviteExpiringSoon`): live expiration countdown above action buttons
                    .when_some(
                        format_invite_expiring_soon(
                            inv.expires_at.as_deref(),
                            inv.guild_name.is_some(),
                            inv.target_application_name.is_some(),
                            inv.guild_id.as_deref(),
                            chrono::Utc::now().timestamp_millis(),
                        ),
                        |col, exp| {
                            col.child(
                                div()
                                    .w_full()
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(exp)),
                            )
                        },
                    )
                    // Action buttons
                    .child(
                        div()
                            .w_full()
                            .mt(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(if is_activity {
                                div()
                                    .w_full()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("accept-invite-spectate")
                                            .flex_1()
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.accept_invite_modal = None;
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("eylRaY")),
                                    )
                                    .child(
                                        div()
                                            .id("accept-invite-play")
                                            .flex_1()
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let _ = this.requests.send(Request::Rest {
                                                    key: Some(format!(
                                                        "{ACCEPT_KEY_PREFIX}{code_for_post}"
                                                    )),
                                                    method: reqwest::Method::POST,
                                                    route: format!("/invites/{code_for_post}"),
                                                    body: Some(json!({})),
                                                });
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("RscU7I")),
                                    )
                                    .into_any_element()
                            } else {
                                let label = if at_limit {
                                    dc_intl::t("ZNCziL").to_owned()
                                } else if is_friend_invite {
                                    dc_intl::t("e/6Ogt").to_owned()
                                } else {
                                    format!("{accept_prefix}{me_name}")
                                };
                                div()
                                    .id("accept-invite-primary")
                                    .w_full()
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(!at_limit && !is_busy, |s| s.cursor_pointer())
                                    .when(at_limit || is_busy, |s| s.opacity(0.5))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if at_limit || is_busy {
                                            return;
                                        }
                                        if let Some(m) = this.accept_invite_modal.as_mut() {
                                            m.busy = true;
                                            m.error = None;
                                        }
                                        let _ = this.requests.send(Request::Rest {
                                            key: Some(format!(
                                                "{ACCEPT_KEY_PREFIX}{code_for_post}"
                                            )),
                                            method: reqwest::Method::POST,
                                            route: format!("/invites/{code_for_post}"),
                                            body: Some(json!({})),
                                        });
                                        cx.notify();
                                    }))
                                    .child(SharedString::from(label))
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .id("accept-invite-no-thanks")
                                    .w_full()
                                    .h(px(32.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .hover(|s| s.text_color(color("--text-default")))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.accept_invite_modal = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("ndsK4Z")),
                            ),
                    );

                div()
                    .id("accept-invite-ready")
                    .w(px(if has_splash { 740. } else { 440. }))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .flex()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(left)
                    .when(has_splash, |row| {
                        row.child(
                            div()
                                .w(px(300.))
                                .bg(color("--background-base-lowest"))
                                .overflow_hidden()
                                .children(splash_img.map(|pic| {
                                    img(pic).size_full().object_fit(gpui::ObjectFit::Cover)
                                })),
                        )
                    })
                    .into_any_element()
            }
        };

        Some(
            gpui::deferred(
                div()
                    .id("accept-invite-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.accept_invite_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `accept-invite [loading|invalid|valid|limit|activity|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_accept_invite(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        let base_inv = ResolvedInvite {
            code: "gravecord".into(),
            guild_id: Some("100".into()),
            guild_name: Some("gravecord community".into()),
            guild_splash: None,
            channel_name: Some("general".into()),
            is_group_dm: false,
            inviter_name: Some("plü".into()),
            member_count: Some(1420),
            online_count: Some(318),
            target_application_name: None,
            is_stream: false,
            roles: vec![
                super::invite_embed::InviteRole {
                    id: 901,
                    name: "Early Tester".into(),
                    color: 0x5865f2,
                    position: 2,
                    colors: None,
                    icon: None,
                    unicode_emoji: Some("⚡".into()),
                },
                super::invite_embed::InviteRole {
                    id: 902,
                    name: "Community".into(),
                    color: 0x57f287,
                    position: 1,
                    colors: None,
                    icon: None,
                    unicode_emoji: None,
                },
            ],
            expires_at: Some((chrono::Utc::now() + chrono::Duration::hours(5)).to_rfc3339()),
        };
        match mode.trim() {
            "loading" => {
                self.accept_invite_modal = Some(AcceptInviteModal {
                    code: "gravecord".into(),
                    phase: AcceptInvitePhase::Loading,
                    settings_open: false,
                    allow_dms: true,
                    show_activity: true,
                    at_server_limit: false,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "accept-invite: loading".into()
            }
            "invalid" => {
                self.accept_invite_modal = Some(AcceptInviteModal {
                    code: "expired123".into(),
                    phase: AcceptInvitePhase::Invalid {
                        header: dc_intl::t("u9zxnX").to_owned(),
                        body: dc_intl::t("FWkU6P").to_owned(),
                    },
                    settings_open: false,
                    allow_dms: true,
                    show_activity: true,
                    at_server_limit: false,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "accept-invite: invalid".into()
            }
            "limit" => {
                self.accept_invite_modal = Some(AcceptInviteModal {
                    code: "gravecord".into(),
                    phase: AcceptInvitePhase::Ready(Box::new(base_inv)),
                    settings_open: false,
                    allow_dms: true,
                    show_activity: true,
                    at_server_limit: true,
                    busy: false,
                    error: Some(accept_error_message(Some(30001), 100)),
                });
                cx.notify();
                "accept-invite: limit".into()
            }
            "activity" => {
                let mut inv = base_inv;
                inv.target_application_name = Some("Watch Together".into());
                self.accept_invite_modal = Some(AcceptInviteModal {
                    code: "gravecord".into(),
                    phase: AcceptInvitePhase::Ready(Box::new(inv)),
                    settings_open: false,
                    allow_dms: true,
                    show_activity: true,
                    at_server_limit: false,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "accept-invite: activity".into()
            }
            "close" => {
                self.accept_invite_modal = None;
                cx.notify();
                "accept-invite: closed".into()
            }
            _ => {
                self.accept_invite_modal = Some(AcceptInviteModal {
                    code: "gravecord".into(),
                    phase: AcceptInvitePhase::Ready(Box::new(base_inv)),
                    settings_open: true,
                    allow_dms: true,
                    show_activity: true,
                    at_server_limit: false,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "accept-invite: valid".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_invite_accept_error_codes() {
        assert!(accept_error_message(Some(30001), 100).contains("100"));
        assert_eq!(accept_error_message(Some(30019), 100), dc_intl::t("M6unNJ"));
        assert_eq!(accept_error_message(Some(50096), 100), dc_intl::t("mY2R+F"));
        assert_eq!(accept_error_message(None, 100), dc_intl::t("dDZRdy"));
    }

    #[test]
    fn parses_resolved_invite_and_eyebrow() {
        let inv = parse_resolved_invite(
            "abc",
            &json!({
                "code": "abc",
                "guild": { "id": "10", "name": "Test Server" },
                "channel": { "id": "20", "name": "general", "type": 0 },
                "inviter": { "username": "alice" },
                "approximate_member_count": 50,
                "approximate_presence_count": 12
            }),
        )
        .expect("invite");
        assert!(invite_eyebrow(&inv).contains("alice"));
        assert_eq!(inv.member_count, Some(50));
    }

    #[test]
    fn invite_expiring_soon_999527_buckets_and_guards() {
        for key in ["l7PkYW", "Ifgxdy", "5fXjSj"] {
            assert!(dc_intl::has_key(key), "missing 999527 key {key}");
        }
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-23T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        // 3 days -> l7PkYW
        let d3 =
            format_invite_expiring_soon(Some("2026-11-26T00:00:00Z"), true, false, Some("10"), now)
                .unwrap();
        assert!(d3.contains('3'), "got {d3}");
        // 5 hours -> Ifgxdy
        let h5 =
            format_invite_expiring_soon(Some("2026-11-23T05:00:00Z"), true, false, Some("10"), now)
                .unwrap();
        assert!(h5.contains('5'), "got {h5}");
        // 12 minutes -> 5fXjSj
        let m12 =
            format_invite_expiring_soon(Some("2026-11-23T00:12:00Z"), true, false, Some("10"), now)
                .unwrap();
        assert!(m12.contains("12"), "got {m12}");
        // Student Hubs excluded
        assert!(
            format_invite_expiring_soon(
                Some("2026-11-23T05:00:00Z"),
                true,
                false,
                Some(STUDENT_HUBS_GUILD_ID),
                now,
            )
            .is_none()
        );
        // > 7 days excluded
        assert!(format_invite_expiring_soon(
            Some("2026-12-05T00:00:00Z"),
            true,
            false,
            Some("10"),
            now,
        )
        .is_none());
    }
}
