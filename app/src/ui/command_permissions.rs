//! Application Command Permissions editor (module 365200 `ep`, `ec`, `ed`, `ea`, `er`, `eu`).
//!
//! Renders Roles & Members overrides and Channel overrides for an application or slash command,
//! including Allow/Deny segmented switches, hierarchy/access lock tooltips, 100-override max
//! warning banner, default member permissions notice, self-lockout prevention confirmation dialog,
//! Command Permissions Migration callout, and Add Roles/Members / Add Channels target pickers.

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, IntoElement, SharedString, Subscription,
    div, prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Maximum permission overrides per command/application (`100` in module 365200).
const MAX_OVERRIDES: usize = 100;
/// Help Center article for application command permissions lockout (`MVz.APPLICATION_COMMAND_PERMISSIONS_LOCKOUT`).
const LOCKOUT_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/4644915651095";
/// Migration blog post link (`pW4Crz`).
const MIGRATION_BLOG_URL: &str = "https://discord.com/blog/welcome-to-the-new-era-of-discord-apps/";

/// Override target type (`x.RA` of module 392054 / 365200).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TargetType {
    Role = 1,
    User = 2,
    Channel = 3,
}

/// One permission overwrite row (`ed` of module 365200).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PermissionOverwrite {
    pub id: Id,
    pub kind: TargetType,
    pub name: String,
    pub permission: bool,
    pub can_write: bool,
    pub is_sentinel: bool,
}

/// Self-lockout warning reason (`eh` of module 365200).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum LockoutReason {
    RemoveSelf,
    RemoveRole(String),
    DenyRole(String),
}

impl LockoutReason {
    pub(crate) fn body(&self) -> String {
        match self {
            Self::RemoveSelf => dc_intl::t("/S6f5y").to_owned(),
            Self::RemoveRole(name) => dc_intl::t!("EBhv4R", name = name.as_str()),
            Self::DenyRole(name) => dc_intl::t!("59Dbw0", name = name.as_str()),
        }
    }
}

/// Computes the lock tooltip explaining why a row cannot be modified (`ed` of module 365200).
pub(crate) fn lock_tooltip(
    overwrite: &PermissionOverwrite,
    me: Option<Id>,
    has_access: bool,
    is_command: bool,
) -> Option<String> {
    if !has_access {
        return Some(if is_command {
            dc_intl::t("tybdas").to_owned()
        } else {
            dc_intl::t("z2hjk/").to_owned()
        });
    }
    if overwrite.can_write {
        return None;
    }
    match overwrite.kind {
        TargetType::User => {
            if me == Some(overwrite.id) {
                Some(dc_intl::t("1VF/0x").to_owned())
            } else {
                Some(dc_intl::t("P1GnEd").to_owned())
            }
        }
        TargetType::Role => Some(dc_intl::t("mcAijf").to_owned()),
        TargetType::Channel => None,
    }
}

/// Add-override picker mode (`U` / `B` in module 365200).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickerMode {
    RolesAndMembers,
    Channels,
}

pub(crate) struct CommandPermissionsModal {
    pub guild_id: Id,
    pub application_id: Id,
    pub bot_name: String,
    pub command_id: Option<Id>,
    pub command_name: Option<String>,
    pub has_default_member_permissions: bool,
    pub has_access: bool,
    pub show_migration_callout: bool,
    pub overwrites: BTreeMap<(TargetType, Id), PermissionOverwrite>,
    pub picker: Option<PickerMode>,
    pub picker_search: Entity<Editor>,
    _picker_events: Subscription,
    pub picker_query: String,
    pub lockout_alert: Option<LockoutReason>,
    focus: FocusHandle,
}

impl Root {
    pub(super) fn command_permissions_open(&self) -> bool {
        self.command_permissions.is_some()
    }

    /// Opens the Application Command Permissions editor (`365200`).
    #[allow(dead_code)]
    pub(super) fn open_command_permissions(
        &mut self,
        guild_id: Id,
        application_id: Id,
        bot_name: String,
        command_id: Option<Id>,
        command_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let picker_search = cx.new(|cx| Editor::new(editor_style(), cx));
        let events = cx.subscribe(&picker_search, |this, ed, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited)
                && let Some(m) = this.command_permissions.as_mut()
            {
                m.picker_query = ed.read(cx).text().to_owned();
                cx.notify();
            }
        });
        let mut overwrites = BTreeMap::new();
        overwrites.insert(
            (TargetType::Role, guild_id),
            PermissionOverwrite {
                id: guild_id,
                kind: TargetType::Role,
                name: "@everyone".to_owned(),
                permission: true,
                can_write: true,
                is_sentinel: true,
            },
        );
        let all_channels_id = guild_id.saturating_sub(1);
        overwrites.insert(
            (TargetType::Channel, all_channels_id),
            PermissionOverwrite {
                id: all_channels_id,
                kind: TargetType::Channel,
                name: dc_intl::t("7YqSGx").to_owned(),
                permission: true,
                can_write: true,
                is_sentinel: true,
            },
        );
        self.command_permissions = Some(CommandPermissionsModal {
            guild_id,
            application_id,
            bot_name,
            command_id,
            command_name,
            has_default_member_permissions: command_id.is_some(),
            has_access: true,
            show_migration_callout: false,
            overwrites,
            picker: None,
            picker_search,
            _picker_events: events,
            picker_query: String::new(),
            lockout_alert: None,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    pub(super) fn close_command_permissions(&mut self, cx: &mut Context<Self>) {
        self.command_permissions = None;
        cx.notify();
    }

    pub(super) fn command_permissions_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(m) = self.command_permissions.as_mut() else {
            return false;
        };
        if key.key == "escape" {
            if m.lockout_alert.is_some() {
                m.lockout_alert = None;
                cx.notify();
            } else if m.picker.is_some() {
                m.picker = None;
                cx.notify();
            } else {
                self.close_command_permissions(cx);
            }
            return true;
        }
        false
    }

    fn save_command_permissions(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.command_permissions.as_ref() else {
            return;
        };
        let target_id = m.command_id.unwrap_or(m.application_id);
        let perms: Vec<Value> = m
            .overwrites
            .values()
            .map(|o| {
                json!({
                    "id": o.id.to_string(),
                    "type": o.kind as u8,
                    "permission": o.permission,
                })
            })
            .collect();
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!(
                "/applications/{}/guilds/{}/commands/{target_id}/permissions",
                m.application_id, m.guild_id
            ),
            body: Some(json!({ "permissions": perms })),
        });
        cx.notify();
    }

    /// Capture helper: `command-permissions [lockout|picker]` opens module 365200's editor.
    #[cfg(feature = "capture")]
    pub(crate) fn script_command_permissions(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        self.open_command_permissions(
            guild_id,
            880218394199220334,
            "Midjourney Bot".to_owned(),
            Some(991001),
            Some("/imagine".to_owned()),
            cx,
        );
        if let Some(m) = self.command_permissions.as_mut() {
            m.overwrites.insert(
                (TargetType::Role, 50001),
                PermissionOverwrite {
                    id: 50001,
                    kind: TargetType::Role,
                    name: "Moderators".to_owned(),
                    permission: true,
                    can_write: true,
                    is_sentinel: false,
                },
            );
            m.overwrites.insert(
                (TargetType::Channel, 60001),
                PermissionOverwrite {
                    id: 60001,
                    kind: TargetType::Channel,
                    name: "#general".to_owned(),
                    permission: false,
                    can_write: true,
                    is_sentinel: false,
                },
            );
            if arg.contains("lockout") {
                m.lockout_alert = Some(LockoutReason::DenyRole("@everyone".to_owned()));
            } else if arg.contains("picker") {
                m.picker = Some(PickerMode::RolesAndMembers);
            }
        }
        cx.notify();
        format!("command-permissions: opened ({arg})")
    }

    /// Renders the Application Command Permissions Editor modal (`365200`).
    pub(super) fn command_permissions_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.command_permissions.as_ref()?;
        let is_command = modal.command_id.is_some();
        let has_access = modal.has_access;
        let me = self.stores.current_user.as_ref().map(|u| u.id);
        let total_count = modal.overwrites.len();
        let over_limit = total_count.saturating_sub(MAX_OVERRIDES);
        let at_limit = total_count >= MAX_OVERRIDES;

        let role_user_rows: Vec<&PermissionOverwrite> = modal
            .overwrites
            .values()
            .filter(|o| matches!(o.kind, TargetType::Role | TargetType::User))
            .collect();
        let channel_rows: Vec<&PermissionOverwrite> = modal
            .overwrites
            .values()
            .filter(|o| o.kind == TargetType::Channel)
            .collect();

        let render_section = |title: &'static str,
                              btn_label: &'static str,
                              empty_text: &'static str,
                              rows: Vec<&PermissionOverwrite>,
                              picker_mode: PickerMode,
                              cx: &mut Context<Self>|
         -> AnyElement {
            let disabled_reason = if !has_access {
                Some(if is_command {
                    dc_intl::t("tybdas")
                } else {
                    dc_intl::t("z2hjk/")
                })
            } else if at_limit {
                Some(dc_intl::t("XTwtW/"))
            } else {
                None
            };
            let btn_id = match picker_mode {
                PickerMode::RolesAndMembers => "cmd-perms-add-roles",
                PickerMode::Channels => "cmd-perms-add-channels",
            };
            let add_btn = div()
                .id(btn_id)
                .h(px(30.))
                .px(px(12.))
                .rounded(px(6.))
                .bg(color("--control-secondary-background-default"))
                .border_1()
                .border_color(color("--control-secondary-border-default"))
                .when(disabled_reason.is_some(), |d| d.opacity(0.5))
                .when(disabled_reason.is_none(), |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.command_permissions.as_mut() {
                                m.picker = Some(picker_mode);
                                m.picker_query.clear();
                                let placeholder = match picker_mode {
                                    PickerMode::RolesAndMembers => dc_intl::t("wAfO5Y"),
                                    PickerMode::Channels => dc_intl::t("TLQo/i"),
                                };
                                m.picker_search.update(cx, |ed, _| {
                                    ed.set_text("");
                                    ed.set_placeholder(placeholder.to_owned());
                                });
                            }
                            cx.notify();
                        }))
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .font_weight(FontWeight(500.))
                .text_color(color("--control-secondary-text-default"))
                .child(btn_label);

            let add_btn_el: AnyElement = if let Some(tip) = disabled_reason {
                self.with_tip(
                    format!("{btn_id}-tip"),
                    Tip::body(Body::Text(tip.into()), Side::Top),
                    add_btn,
                    cx,
                )
                .into_any_element()
            } else {
                add_btn.into_any_element()
            };

            let mut list = div().flex().flex_col();
            if rows.is_empty() {
                list = list.child(
                    div()
                        .py(px(12.))
                        .text_size(px(13.))
                        .text_color(color("--text-muted"))
                        .child(empty_text),
                );
            } else {
                for row in rows {
                    let key = (row.kind, row.id);
                    let locked = !row.can_write || !has_access;
                    let tip = lock_tooltip(row, me, has_access, is_command);
                    let can_remove = has_access && (is_command || !row.is_sentinel);
                    let allowed = row.permission;
                    let row_name = row.name.clone();
                    let _remove_aria = dc_intl::t("mT0CQM");
                    let _allow_label = dc_intl::t("RzDfSk");
                    let _deny_label = dc_intl::t("6639O5");

                    let toggle = div()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lowest"))
                        .when(locked, |d| d.opacity(0.5))
                        .flex()
                        .overflow_hidden()
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "cmd-perm-deny-{}-{}",
                                    row.kind as u8, row.id
                                )))
                                .w(px(30.))
                                .h(px(26.))
                                .bg(if !allowed {
                                    color("--status-danger")
                                } else {
                                    rgba(0x00000000)
                                })
                                .when(!locked, |d| {
                                    d.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if let Some(m) = this.command_permissions.as_mut()
                                                && let Some(entry) = m.overwrites.get_mut(&key)
                                            {
                                                entry.permission = false;
                                            }
                                            this.save_command_permissions(cx);
                                        },
                                    ))
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    "automod_cross",
                                    12.,
                                    if !allowed { "--white" } else { "--icon-muted" },
                                )),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "cmd-perm-allow-{}-{}",
                                    row.kind as u8, row.id
                                )))
                                .w(px(30.))
                                .h(px(26.))
                                .bg(if allowed {
                                    color("--status-positive")
                                } else {
                                    rgba(0x00000000)
                                })
                                .when(!locked, |d| {
                                    d.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if let Some(m) = this.command_permissions.as_mut()
                                                && let Some(entry) = m.overwrites.get_mut(&key)
                                            {
                                                entry.permission = true;
                                            }
                                            this.save_command_permissions(cx);
                                        },
                                    ))
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    "checkmark_large",
                                    12.,
                                    if allowed { "--white" } else { "--icon-muted" },
                                )),
                        );

                    let toggle_el: AnyElement = if let Some(tip_text) = tip {
                        self.with_tip(
                            format!("cmd-perm-lock-{}-{}", row.kind as u8, row.id),
                            Tip::body(Body::Text(tip_text.into()), Side::Left),
                            toggle,
                            cx,
                        )
                        .into_any_element()
                    } else {
                        toggle.into_any_element()
                    };

                    // Module 739174 (`identifier_2zb7OH`): channel 20px icon + `7YqSGx` All Channels + `(category)`,
                    // role 23px tinted `role_shield`, user 24px avatar + streamer-mode-gated tag, + right-click `oJ1Muw`.
                    let row_id = row.id;
                    let row_kind = row.kind;
                    let is_sentinel = row.is_sentinel;
                    let hide_personal = self.streamer_hides_personal();
                    let (leading_icon, display_title, secondary_tag): (
                        AnyElement,
                        String,
                        Option<String>,
                    ) = match row_kind {
                        TargetType::Channel => {
                            let title = if is_sentinel {
                                dc_intl::t("7YqSGx").to_owned()
                            } else {
                                row_name.clone()
                            };
                            let cat_tag = if is_sentinel {
                                None
                            } else {
                                self.stores.guild(modal.guild_id).and_then(|g| {
                                    g.channels
                                        .iter()
                                        .find(|c| c.id == row_id)
                                        .and_then(|c| c.parent_id)
                                        .and_then(|pid| g.channels.iter().find(|c| c.id == pid))
                                        .map(|cat| format!("({})", cat.name))
                                })
                            };
                            (
                                div()
                                    .w(px(24.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("channel_type_text", 20., "--text-default"))
                                    .into_any_element(),
                                title,
                                cat_tag,
                            )
                        }
                        TargetType::Role => {
                            let role_color = self
                                .stores
                                .guild(modal.guild_id)
                                .and_then(|g| g.role_color(&[row_id]))
                                .unwrap_or(0);
                            let shield = if role_color != 0 {
                                gpui::svg()
                                    .path("icons/role_shield.svg")
                                    .size(px(23.))
                                    .text_color(gpui::rgb(role_color))
                            } else {
                                icon("role_shield", 23., "--text-default")
                            };
                            (
                                div()
                                    .w(px(24.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(shield)
                                    .into_any_element(),
                                row_name.clone(),
                                None,
                            )
                        }
                        TargetType::User => {
                            let known_user = self.stores.users.get(&row_id);
                            let av = known_user
                                .and_then(|u| self.images.get(&super::avatar_source(u).0).cloned());
                            let tag = if hide_personal {
                                None
                            } else {
                                known_user.map(|u| format!("@{}", u.username))
                            };
                            let av_el = div()
                                .size(px(24.))
                                .flex_none()
                                .rounded_full()
                                .overflow_hidden()
                                .bg(color("--background-surface-higher"))
                                .children(av.map(|im| gpui::img(im).size(px(24.)).rounded_full()))
                                .into_any_element();
                            (av_el, row_name.clone(), tag)
                        }
                    };

                    list = list.child(
                        div()
                            .id(SharedString::from(format!(
                                "cmd-perm-row-{}-{}",
                                row_kind as u8, row_id
                            )))
                            .py(px(10.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .when(!is_sentinel, |d| {
                                d.on_mouse_up(
                                    gpui::MouseButton::Right,
                                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                        this.open_context_menu(
                                            super::context_menu::Target::CommandPermId(row_id),
                                            event.position,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(leading_icon)
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(400.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(display_title),
                                    )
                                    .children(secondary_tag.map(|tag| {
                                        div()
                                            .flex_none()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(tag)
                                    }))
                                    .when(locked, |d| {
                                        d.child(div().ml(px(3.)).flex().child(icon(
                                            "access_lock",
                                            14.,
                                            "--text-muted",
                                        )))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .when(can_remove, |d| {
                                        d.child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "cmd-perm-del-{}-{}",
                                                    row.kind as u8, row.id
                                                )))
                                                .size(px(26.))
                                                .rounded(px(4.))
                                                .cursor_pointer()
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "channel_settings_trash",
                                                    15.,
                                                    "--icon-muted",
                                                ))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) =
                                                        this.command_permissions.as_mut()
                                                    {
                                                        m.overwrites.remove(&key);
                                                    }
                                                    this.save_command_permissions(cx);
                                                })),
                                        )
                                    })
                                    .child(toggle_el),
                            ),
                    );
                }
            }

            div()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .px(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .pb(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(add_btn_el),
                )
                .child(list)
                .into_any_element()
        };

        let roles_section = render_section(
            if is_command {
                dc_intl::t("1jLVGB")
            } else {
                dc_intl::t("vPWe++")
            },
            dc_intl::t("56jRn8"),
            dc_intl::t("C0rYfq"),
            role_user_rows,
            PickerMode::RolesAndMembers,
            cx,
        );
        let channels_section = render_section(
            if is_command {
                dc_intl::t("Ujbaqm")
            } else {
                dc_intl::t("OGiMXJ")
            },
            dc_intl::t("i1c+kV"),
            dc_intl::t("UBJhC/"),
            channel_rows,
            PickerMode::Channels,
            cx,
        );

        let migration_banner = modal.show_migration_callout.then(|| {
            let body = dc_intl::format(
                "pW4Crz",
                &[
                    ("botName", modal.bot_name.as_str().into()),
                    ("link", MIGRATION_BLOG_URL.into()),
                ],
            );
            div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(color("--brand-500"))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("ufFDiC")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(color("--white"))
                        .child(body),
                )
        });

        let picker_overlay = modal.picker.map(|mode| {
            let title = match mode {
                PickerMode::RolesAndMembers => dc_intl::t("56jRn8"),
                PickerMode::Channels => dc_intl::t("i1c+kV"),
            };
            let q = modal
                .picker_query
                .trim()
                .trim_start_matches(['@', '#'])
                .to_lowercase();
            let mut candidates: Vec<(TargetType, Id, String)> = Vec::new();
            if let Some(guild) = self.stores.guild(modal.guild_id) {
                match mode {
                    PickerMode::RolesAndMembers => {
                        for (&rid, name) in &guild.roles {
                            if !modal.overwrites.contains_key(&(TargetType::Role, rid))
                                && (q.is_empty() || name.to_lowercase().contains(&q))
                            {
                                candidates.push((TargetType::Role, rid, name.clone()));
                            }
                        }
                        for (uid, user) in &self.stores.users {
                            if !user.bot
                                && !modal.overwrites.contains_key(&(TargetType::User, *uid))
                                && (q.is_empty()
                                    || user.username.to_lowercase().contains(&q)
                                    || user.display_name().to_lowercase().contains(&q))
                            {
                                candidates.push((
                                    TargetType::User,
                                    *uid,
                                    user.display_name().to_owned(),
                                ));
                            }
                        }
                    }
                    PickerMode::Channels => {
                        for ch in &guild.channels {
                            if !modal.overwrites.contains_key(&(TargetType::Channel, ch.id))
                                && (q.is_empty() || ch.name.to_lowercase().contains(&q))
                            {
                                candidates.push((
                                    TargetType::Channel,
                                    ch.id,
                                    format!("#{}", ch.name),
                                ));
                            }
                        }
                    }
                }
            }
            candidates.truncate(20);
            let mut items = div().flex().flex_col().gap(px(4.));
            for (kind, cid, cname) in candidates {
                let add_name = cname.clone();
                items = items.child(
                    div()
                        .id(SharedString::from(format!("cmd-pick-{}-{cid}", kind as u8)))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .text_size(px(14.))
                        .text_color(color("--text-strong"))
                        .child(cname)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.command_permissions.as_mut() {
                                m.overwrites.insert(
                                    (kind, cid),
                                    PermissionOverwrite {
                                        id: cid,
                                        kind,
                                        name: add_name.clone(),
                                        permission: true,
                                        can_write: true,
                                        is_sentinel: false,
                                    },
                                );
                                m.picker = None;
                            }
                            this.save_command_permissions(cx);
                        })),
                );
            }
            div()
                .id("cmd-perms-picker-backdrop")
                .absolute()
                .inset_0()
                .bg(rgba(0x00000099))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(m) = this.command_permissions.as_mut() {
                        m.picker = None;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .id("cmd-perms-picker-modal")
                        .w(px(420.))
                        .max_h(px(420.))
                        .p(px(16.))
                        .rounded(px(12.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_xl()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .h(px(36.))
                                .px(px(10.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .flex()
                                .items_center()
                                .child(modal.picker_search.clone()),
                        )
                        .child(
                            div()
                                .id("cmd-perms-picker-list")
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .child(items),
                        ),
                )
        });

        let lockout_overlay = modal.lockout_alert.as_ref().map(|reason| {
            div()
                .id("cmd-perms-lockout-backdrop")
                .absolute()
                .inset_0()
                .bg(rgba(0x00000099))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("cmd-perms-lockout-modal")
                        .w(px(420.))
                        .p(px(20.))
                        .rounded(px(12.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_xl()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("vElC9b")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(reason.body()),
                        )
                        .child(
                            div()
                                .mt(px(6.))
                                .flex()
                                .justify_end()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .id("cmd-perms-lockout-help")
                                        .h(px(34.))
                                        .px(px(14.))
                                        .rounded(px(6.))
                                        .bg(color("--control-secondary-background-default"))
                                        .border_1()
                                        .border_color(color("--control-secondary-border-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(13.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(dc_intl::t("psXQHP"))
                                        .on_click(|_, _, cx| cx.open_url(LOCKOUT_HELP_URL)),
                                )
                                .child(
                                    div()
                                        .id("cmd-perms-lockout-ok")
                                        .h(px(34.))
                                        .px(px(16.))
                                        .rounded(px(6.))
                                        .bg(color("--brand-500"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("cpT0Cq"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.command_permissions.as_mut() {
                                                m.lockout_alert = None;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
        });

        let heading = match &modal.command_name {
            Some(cmd) => format!("{} — {cmd}", modal.bot_name),
            None => modal.bot_name.clone(),
        };

        Some(
            div()
                .id("cmd-perms-backdrop")
                .track_focus(&modal.focus)
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.close_command_permissions(cx)))
                .child(
                    div()
                        .id("cmd-perms-modal")
                        .relative()
                        .w(px(600.))
                        .max_h(px(650.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_xl()
                        .flex()
                        .flex_col()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(20.))
                                .py(px(16.))
                                .border_b_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(heading),
                                )
                                .child(
                                    div()
                                        .id("cmd-perms-close")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "modal_close",
                                            18.,
                                            "--interactive-text-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_command_permissions(cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id("cmd-perms-body")
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .p(px(16.))
                                .flex()
                                .flex_col()
                                .gap(px(14.))
                                .when(over_limit > 0, |d| {
                                    d.child(
                                        div()
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .bg(color("--status-warning"))
                                            .text_size(px(13.))
                                            .text_color(gpui::rgb(0x111214))
                                            .child(dc_intl::format(
                                                "B/dFBH",
                                                &[("removeCount", (over_limit as i64).into())],
                                            )),
                                    )
                                })
                                .when(modal.has_default_member_permissions, |d| {
                                    d.child(
                                        div()
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .bg(color("--background-base-lower"))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(div().flex_none().child(icon(
                                                "circle_info",
                                                16.,
                                                "--icon-muted",
                                            )))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .text_size(px(13.))
                                                    .line_height(px(17.))
                                                    .text_color(color("--text-default"))
                                                    .child(dc_intl::t("2889Gq")),
                                            )
                                            .child(
                                                div()
                                                    .id("cmd-perms-view-default")
                                                    .flex_none()
                                                    .h(px(28.))
                                                    .px(px(10.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .border_1()
                                                    .border_color(color(
                                                        "--control-secondary-border-default",
                                                    ))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .child(dc_intl::t("HO/oXl")),
                                            ),
                                    )
                                })
                                .children(migration_banner)
                                .child(roles_section)
                                .child(channels_section),
                        )
                        .children(picker_overlay)
                        .children(lockout_overlay),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_365200_command_permissions_strings_resolve() {
        for key in [
            "RzDfSk", "6639O5", "1VF/0x", "P1GnEd", "mcAijf", "tybdas", "z2hjk/", "ufFDiC",
            "pW4Crz", "mT0CQM", "/S6f5y", "EBhv4R", "59Dbw0", "vElC9b", "psXQHP", "i1c+kV",
            "TLQo/i", "56jRn8", "wAfO5Y", "C0rYfq", "vPWe++", "1jLVGB", "UBJhC/", "OGiMXJ",
            "Ujbaqm", "B/dFBH", "2889Gq", "HO/oXl", "XTwtW/", "7YqSGx",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in command_permissions 365200"
            );
        }
    }

    #[test]
    fn lock_tooltips_and_lockout_copy_match_official_rules() {
        let self_user = PermissionOverwrite {
            id: 42,
            kind: TargetType::User,
            name: "Me".into(),
            permission: true,
            can_write: false,
            is_sentinel: false,
        };
        assert_eq!(
            lock_tooltip(&self_user, Some(42), true, true).as_deref(),
            Some("Cannot modify permission to deny yourself access!")
        );
        assert_eq!(
            lock_tooltip(&self_user, Some(99), true, true).as_deref(),
            Some("Cannot modify permission to explicitly deny a user with a higher role than you!")
        );
        assert_eq!(
            lock_tooltip(&self_user, Some(99), false, false).as_deref(),
            Some("Cannot modify configurations due to not having access to this application")
        );
        assert!(LockoutReason::RemoveSelf.body().contains("yourself"));
        assert!(
            LockoutReason::RemoveRole("Mods".into())
                .body()
                .contains("Mods")
        );
        assert!(
            LockoutReason::DenyRole("@everyone".into())
                .body()
                .contains("@everyone")
        );
    }
}
