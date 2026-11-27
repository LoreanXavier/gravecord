//! The server name's dropdown (`menu_M3qk4j fixed` "Server Actions", 220 wide under the header), measured in
//! `design/dom/menus/guild-header-darker-cozy.json`: icons on the left, 36px rows, separators after Server Boost,
//! App Directory and Privacy Settings, and Hide Muted Channels with its checkbox. Hide Muted Channels patches
//! `hide_muted_channels` through `PATCH /users/@me/guilds/settings`, as the bell menu's overrides do.

use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use dc_state::permissions;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, prelude::*,
    px,
};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Boost,
    Invite,
    Settings,
    CreateChannel,
    CreateCategory,
    CreateEvent,
    ActiveThreads,
    AppDirectory,
    /// `opt-in` (module 36942): the checkbox that turns "only show the channels I joined" off.
    ShowAll,
    /// `linked-roles` (module 480900 / 488358): opens Role Connection Verification when the guild has linked roles.
    LinkedRoles,
    /// `friends-in-server` (module 151203): opens `GuildFriendsModal`.
    FriendsInServer,
    Notifications,
    Privacy,
    Profile,
    HideMuted,
    /// `server-lockdown` (module 664967): Security Actions, or Edit Security Actions during a pause (module 671576).
    SecurityActions,
    /// `report-raid` (module 808144): Report a Raid (module 935074).
    ReportRaid,
    Leave,
    /// Not in the pinned client: the plugins' rows at the end of the privacy group, in plugin order (NewGuildSettings'
    /// Apply NewGuildSettings, PermissionsViewer's View Permissions, ServerInfo's Server Info).
    PluginApplyNewGuildSettings,
    PluginViewPermissions,
    PluginServerInfo,
}

/// `p.yC` of module 488926, behind `canAccessGuildSettings`: holding any one of these opens Server Settings.
const SETTINGS_ACCESS: u64 = permissions::MANAGE_GUILD
    | permissions::MANAGE_ROLES
    | permissions::ADMINISTRATOR
    | permissions::BAN_MEMBERS
    | permissions::MANAGE_NICKNAMES
    | permissions::CREATE_GUILD_EXPRESSIONS
    | permissions::MANAGE_GUILD_EXPRESSIONS
    | permissions::MANAGE_WEBHOOKS
    | (1 << 7)  // VIEW_AUDIT_LOG
    | (1 << 19); // VIEW_GUILD_ANALYTICS
/// `CREATE_EVENTS` of module 652215 (module 770666 lets Administrator or this create an event).
const CREATE_EVENTS: u64 = 1 << 44;
/// `OPT_IN_ENABLED` of a server's notification settings flags (module 395504 `WW`).
const OPT_IN_ENABLED: u64 = 16384;

/// `k$` of module 903093: a raid or DM spam was detected less than two hours ago.
fn incident_active(guild: &dc_state::Guild, now: chrono::DateTime<chrono::Utc>) -> bool {
    [&guild.raid_detected_at, &guild.dm_spam_detected_at]
        .into_iter()
        .flatten()
        .filter_map(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .any(|at| at.with_timezone(&chrono::Utc) + chrono::Duration::hours(2) > now)
}

/// The flags Show All Channels writes: the cached ones with the opt-in bit turned. With no settings loaded at all
/// the other bits are unknown, so nothing is written; a server missing from loaded settings holds the default 0.
fn toggled_opt_in(cached: Option<u64>, settings_loaded: bool) -> Option<u64> {
    match cached {
        Some(flags) => Some(flags ^ OPT_IN_ENABLED),
        None if settings_loaded => Some(OPT_IN_ENABLED),
        None => None,
    }
}

#[cfg(test)]
mod opt_in_tests {
    use super::*;

    #[test]
    fn show_all_keeps_the_other_flag_bits_and_never_guesses() {
        assert_eq!(toggled_opt_in(Some(5), true), Some(5 | OPT_IN_ENABLED));
        assert_eq!(toggled_opt_in(Some(5 | OPT_IN_ENABLED), true), Some(5));
        assert_eq!(toggled_opt_in(None, true), Some(OPT_IN_ENABLED));
        assert_eq!(toggled_opt_in(None, false), None);
    }

    /// `k$` of module 903093: an incident lasts two hours after a raid or DM spam was detected.
    #[test]
    fn an_incident_lasts_two_hours_after_it_was_detected() {
        let mut guild =
            dc_state::Guild::from_value(&json!({"id": "1", "name": "g", "channels": []})).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(!incident_active(&guild, now));
        guild.raid_detected_at = Some("2026-11-18T10:30:00+00:00".to_owned());
        assert!(incident_active(&guild, now));
        guild.raid_detected_at = Some("2026-11-18T09:59:00+00:00".to_owned());
        assert!(!incident_active(&guild, now));
        guild.dm_spam_detected_at = Some("2026-11-18T11:00:00+00:00".to_owned());
        assert!(incident_active(&guild, now));
    }
}

/// The last group: Security Actions (module 664967, the lock, in the danger colour), Report Raid (the shield, in the danger colour) for a Community server's moderators, then Leave
/// Server for members who are not the owner (the owner has to transfer or delete the server instead).
const LAST: &[(Entry, &str, &str)] = &[
    (Entry::SecurityActions, "access_lock", "Security Actions"),
    (Entry::ReportRaid, "guild_menu_privacy", "Report Raid"),
    (Entry::Leave, "guild_menu_leave", "Leave Server"),
];

/// Not in the pinned client: the plugins' rows of the privacy group, with the icons they pass as `leadingAccessory`.
const PLUGIN_ROWS: &[(Entry, &str, &str)] = &[
    (
        Entry::PluginApplyNewGuildSettings,
        "plugin_cog",
        "Apply NewGuildSettings",
    ),
    (
        Entry::PluginViewPermissions,
        "plugin_safety",
        "View Permissions",
    ),
    (Entry::PluginServerInfo, "circle_info", "Server Info"),
];

const ENTRIES: [&[(Entry, &str, &str)]; 4] = [
    &[(Entry::Boost, "guild_menu_premium_subscribe", "Server Boost")],
    &[
        (
            Entry::Invite,
            "guild_menu_invite_people",
            "Invite to Server",
        ),
        (Entry::Settings, "guild_menu_settings", "Server Settings"),
        (
            Entry::CreateChannel,
            "guild_menu_create_channel",
            "Create Channel",
        ),
        (
            Entry::CreateCategory,
            "guild_menu_create_category",
            "Create Category",
        ),
        (
            Entry::CreateEvent,
            "guild_menu_create_event",
            "Create Event",
        ),
        (
            Entry::ActiveThreads,
            "guild_menu_active_threads",
            "Active Threads",
        ),
        (
            Entry::AppDirectory,
            "guild_menu_application_directory",
            "App Directory",
        ),
    ],
    &[
        (Entry::ShowAll, "clips_eye", "Show All Channels"),
        (Entry::LinkedRoles, "oauth_entry_link", "Linked Roles"),
        (Entry::FriendsInServer, "members", "Friends in Server"),
        (
            Entry::Notifications,
            "guild_menu_notifications",
            "Notification Settings",
        ),
        (Entry::Privacy, "guild_menu_privacy", "Privacy Settings"),
    ],
    &[
        (
            Entry::Profile,
            "guild_menu_change_nickname",
            "Edit Per-server Profile",
        ),
        (
            Entry::HideMuted,
            "guild_menu_hide_muted_channels",
            "Hide Muted Channels",
        ),
    ],
];

/// The menu's top edge in the window (`design/dom/menus/guild-header-darker-cozy.json`).
const MENU_TOP: f32 = 86.;

impl Root {
    pub(super) fn toggle_guild_menu(&mut self, cx: &mut Context<Self>) {
        let opening = !self.guild_menu;
        if opening {
            self.close_competing_popouts(super::keyboard::Layer::GuildMenu);
        }
        self.guild_menu = opening;
        if opening {
            self.guild_menu_scroll
                .set_offset(gpui::point(px(0.), px(0.)));
        }
        cx.notify();
    }

    pub(super) fn hide_muted(&self, guild: Id) -> bool {
        self.stores
            .guild_settings
            .guilds
            .get(&guild)
            .is_some_and(|settings| settings.hide_muted_channels)
    }

    fn opt_in_flags(&self, guild: Id) -> u64 {
        self.stores
            .guild_settings
            .guilds
            .get(&guild)
            .map_or(0, |settings| settings.flags)
    }

    /// Whether the header menu lists the row for the current person (module 808144): Server Settings by
    /// `canAccessGuildSettings`, Create Channel and Create Category by Manage Channels, Create Event by Administrator
    /// or Create Events, Active Threads by Manage Threads and a thread being open, Show All Channels by a Community
    /// server. Create Event also opens for anyone with a voice channel they may host an event in (`A.p_`, not read).
    fn guild_entry_shown(&self, guild: Id, entry: Entry) -> bool {
        let Some(known) = self.stores.guild(guild) else {
            return true;
        };
        let owner = known.owner_id.is_some()
            && known.owner_id == self.stores.current_user.as_ref().map(|me| me.id);
        let held = self
            .stores
            .me()
            .map_or(0, |me| permissions::guild_permissions(known, me));
        let can = |bit: u64| owner || held & bit == bit;
        match entry {
            Entry::Settings => owner || held & SETTINGS_ACCESS != 0,
            Entry::CreateChannel | Entry::CreateCategory => can(permissions::MANAGE_CHANNELS),
            Entry::CreateEvent => owner || held & (permissions::ADMINISTRATOR | CREATE_EVENTS) != 0,
            Entry::ActiveThreads => can(permissions::MANAGE_THREADS) && !known.threads.is_empty(),
            Entry::Leave => !owner,
            Entry::SecurityActions => self.security_actions_item(guild).is_some(),
            // `ic` of module 974103: Ban, Kick, Timeout or Manage Server, and no incident in its two hours.
            Entry::ReportRaid => {
                known.features.iter().any(|feature| feature == "COMMUNITY")
                    && (owner
                        || held
                            & (permissions::BAN_MEMBERS
                                | permissions::KICK_MEMBERS
                                | permissions::MODERATE_MEMBERS
                                | permissions::MANAGE_GUILD
                                | permissions::ADMINISTRATOR)
                            != 0)
                    && !incident_active(known, chrono::Utc::now())
            }
            Entry::ShowAll => {
                known.features.iter().any(|feature| feature == "COMMUNITY")
                    || self
                        .stores
                        .current_user
                        .as_ref()
                        .is_some_and(|user| user.has_flag(dc_state::USER_FLAG_STAFF))
            }
            Entry::LinkedRoles => known.role_info.values().any(|role| role.guild_connection),
            // `useFriendsItem` (module 808144): only with `2026-01-friends-in-server` giving
            // `guildFriendsEnabled`.
            Entry::FriendsInServer => {
                self.stores.experiments.variant("2026-01-friends-in-server") == Some(1)
            }
            _ => true,
        }
    }

    /// `Lu` of module 234053: Show All Channels turns the opt-in flag of the server's notification settings.
    pub(super) fn toggle_show_all_channels(&mut self, guild: Id) {
        let known = self
            .stores
            .guild_settings
            .guilds
            .get(&guild)
            .map(|s| s.flags);
        let loaded = !self.stores.guild_settings.guilds.is_empty();
        let Some(flags) = toggled_opt_in(known, loaded) else {
            return;
        };
        self.stores
            .guild_settings
            .guilds
            .entry(guild)
            .or_insert_with(|| dc_state::guild_settings::GuildSettings::from_value(&json!({})))
            .flags = flags;
        let _ = self.requests.send(Request::GuildSettings {
            body: json!({"guilds": {guild.to_string(): {"flags": flags}}}),
        });
    }

    fn run_guild_entry(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        if !matches!(entry, Entry::HideMuted | Entry::ShowAll) {
            self.guild_menu = false;
        }
        match entry {
            // `Lu` of module 234053 turns the opt-in flag of the server's notification settings (`d.e4`). Its other
            // branch, a member who never finished onboarding, onboards them instead and is not built.
            Entry::ShowAll => self.toggle_show_all_channels(guild),
            Entry::LinkedRoles => {
                if let Some(known) = self.stores.guild(guild)
                    && let Some((&role_id, info)) = known
                        .role_info
                        .iter()
                        .find(|(_, info)| info.guild_connection)
                {
                    let role_name = known
                        .roles
                        .get(&role_id)
                        .cloned()
                        .unwrap_or_else(|| "Linked Role".to_owned());
                    let role_color = info.color;
                    self.open_role_connection_verify(guild, role_id, role_name, role_color, cx);
                }
            }
            Entry::HideMuted => {
                let hide = !self.hide_muted(guild);
                if let Some(settings) = self.stores.guild_settings.guilds.get_mut(&guild) {
                    settings.hide_muted_channels = hide;
                } else {
                    let mut settings =
                        dc_state::guild_settings::GuildSettings::from_value(&json!({}));
                    settings.hide_muted_channels = hide;
                    self.stores.guild_settings.guilds.insert(guild, settings);
                }
                let _ = self.requests.send(Request::GuildSettings {
                    body: json!({"guilds": {guild.to_string(): {"hide_muted_channels": hide}}}),
                });
            }
            // "Active Threads" opens the "All Active Threads" modal (module 400954).
            Entry::ActiveThreads => {
                self.thread_modal = true;
                cx.notify();
            }
            Entry::Profile => self.open_settings_section("Account", cx),
            Entry::Privacy => self.open_settings_section("Data & Privacy", cx),
            Entry::Boost => self.open_boost_page(cx),
            Entry::CreateChannel => self.open_create_channel(false, window, cx),
            Entry::CreateCategory => self.open_create_channel(true, window, cx),
            Entry::Notifications => self.open_guild_notifications(cx),
            Entry::Invite => self.open_invite(cx),
            Entry::Settings => self.open_server_settings(guild, cx),
            Entry::Leave => self.leave_guild = Some(guild),
            Entry::ReportRaid => self.open_report_raid(guild, cx),
            Entry::SecurityActions => self.open_security_actions(guild, cx),
            // `create-event` opens the Create Event flow of the current server, as the server context menu's row does.
            Entry::CreateEvent => {
                self.open_guild_events(cx);
                self.open_create_event(window, cx);
            }
            // `application-directory` (module 808144): `transitionToGlobalDiscovery({tab: APPS})`.
            Entry::AppDirectory => self.open_discover(super::discover::Place::Apps, cx),
            Entry::FriendsInServer => self.open_guild_friends_modal(guild, cx),
            // Not in the pinned client: the plugins' rows.
            Entry::PluginApplyNewGuildSettings => self.apply_new_guild_settings(guild),
            Entry::PluginViewPermissions => self.permissions_viewer_open_guild(guild, cx),
            Entry::PluginServerInfo => self.server_info_open(guild, cx),
        }
        cx.notify();
    }

    pub(super) fn guild_menu_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.guild_menu {
            return None;
        }
        let guild = self.guild?;
        let hidden = self.hide_muted(guild);
        let mut items: Vec<AnyElement> = Vec::new();
        let mut groups: Vec<&[(Entry, &str, &str)]> = ENTRIES.to_vec();
        groups.push(LAST);
        // Entries this person may not use are not listed, and a group left empty takes its rule with it.
        let mut shown: Vec<Vec<&(Entry, &str, &str)>> = groups
            .iter()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(entry, _, _)| self.guild_entry_shown(guild, *entry))
                    .collect()
            })
            .filter(|entries: &Vec<&(Entry, &str, &str)>| !entries.is_empty())
            .collect();
        // Not in the pinned client: the plugins' rows end the privacy group (`findGroupChildrenByChildId("privacy")`).
        if let Some(group) = shown
            .iter_mut()
            .find(|entries| entries.iter().any(|(entry, _, _)| *entry == Entry::Privacy))
        {
            group.extend(PLUGIN_ROWS.iter().filter(|(entry, _, _)| match entry {
                Entry::PluginApplyNewGuildSettings => self.new_guild_settings_on(),
                Entry::PluginViewPermissions => super::plugin_hooks::permissions_viewer(),
                Entry::PluginServerInfo => super::plugin_hooks::server_info_on(),
                _ => false,
            }));
        }
        let show_all = self.opt_in_flags(guild) & OPT_IN_ENABLED == 0;
        for (group, entries) in shown.iter().enumerate() {
            if group > 0 {
                items.push(
                    div()
                        .my(px(8.))
                        .h(px(1.))
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
            }
            for (entry, name, label) in entries.iter() {
                let entry = *entry;
                // Module 664967 with its icon: the open lock and Edit Security Actions while a pause runs.
                let lockdown = entry == Entry::SecurityActions
                    && self.security_actions_item(guild) == Some(true);
                let (name, label): (&'static str, &'static str) = if lockdown {
                    ("overlay_lock_unlocked", dc_intl::t("+tSVi3"))
                } else if entry == Entry::SecurityActions {
                    (*name, dc_intl::t("EPlEdu"))
                } else {
                    (*name, *label)
                };
                let danger = matches!(
                    entry,
                    Entry::Leave | Entry::ReportRaid | Entry::SecurityActions
                );
                items.push(
                    div()
                        .id(SharedString::from(format!("guild-menu-{entry:?}")))
                        .h(px(36.))
                        .p(px(8.))
                        .rounded(px(2.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .hover(|style| {
                            style
                                .bg(color("--interactive-background-hover"))
                                .rounded(px(4.))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_guild_entry(entry, window, cx)
                        }))
                        .child(div().mr(px(8.)).child(icon(
                            name,
                            20.,
                            if danger {
                                "--text-feedback-critical"
                            } else {
                                "--interactive-text-default"
                            },
                        )))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(if danger {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--text-strong")
                                })
                                // `label_M3qk4j` of the `fixed_M3qk4j` 220 menu: one line, cut with an ellipsis (a
                                // plugin's longer label no longer runs past the menu's edge).
                                .truncate()
                                .child(label),
                        )
                        // Not in the pinned client: NewGuildSettings gives its row the CogWheel after the label too.
                        .when(entry == Entry::PluginApplyNewGuildSettings, |this| {
                            this.child(div().ml(px(8.)).child(icon(
                                "plugin_cog",
                                18.,
                                "--interactive-text-default",
                            )))
                        })
                        .when(matches!(entry, Entry::HideMuted | Entry::ShowAll), |this| {
                            let hidden = if entry == Entry::ShowAll {
                                show_all
                            } else {
                                hidden
                            };
                            this.child(
                                div()
                                    .size(px(20.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if hidden {
                                        color("--checkbox-border-selected-default")
                                    } else {
                                        color("--checkbox-border-default")
                                    })
                                    .bg(if hidden {
                                        color("--checkbox-background-selected-default")
                                    } else {
                                        color("--checkbox-background-default")
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(hidden, |this| {
                                        this.child(icon("checkbox_check", 18., "--white"))
                                    }),
                            )
                        })
                        .into_any_element(),
                );
            }
        }
        // The menu's `max-height` is the layer's `--reference-position-layer-max-height` (module 623646): the room
        // below its top less 12px; past that its scroller scrolls.
        let max_height = (f32::from(window.viewport_size().height) - MENU_TOP - 12.).max(0.);
        Some(
            deferred(
                div().absolute().left(px(114.)).top(px(MENU_TOP)).child(
                    div()
                        .parity_with_border("guild-menu", 1.)
                        .id("guild-menu")
                        // The server header menu opens with `animation: SCALE` below its target (module 260269).
                        .popout("guild-menu", PopKind::Scale, PopSide::Bottom)
                        .occlude()
                        .relative()
                        .w(px(220.))
                        .max_h(px(max_height))
                        .flex()
                        .flex_col()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                if this.press_outside(super::keyboard::Layer::GuildMenu, cx)
                                    && event.button != gpui::MouseButton::Right
                                {
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .child(
                            div()
                                .id("guild-menu-scroller")
                                .track_scroll(&self.guild_menu_scroll)
                                .overflow_y_scroll()
                                // The border box holds the scroller: its 1px border on each side comes off.
                                .max_h(px((max_height - 2.).max(0.)))
                                .p(px(8.))
                                .flex()
                                .flex_col()
                                // Rows keep their height when the menu scrolls.
                                .children(
                                    items.into_iter().map(|row| div().flex_none().child(row)),
                                ),
                        )
                        .child(super::menu::menu_scrollbar(self.guild_menu_scroll.clone())),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// The request behind Leave Server (`GUILD_LEAVE`, module 652215; `leaveGuild` sends `lurking`).
pub(super) fn leave_request(guild: Id) -> Request {
    // Not in the pinned client: RelationshipNotifier keeps quiet about a server you leave.
    super::plugin_hooks::relationship_removed_by_you(None, Some(guild), None);
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/users/@me/guilds/{guild}"),
        body: Some(json!({"lurking": false})),
    }
}

impl Root {
    /// The confirm dialog (`ZEXC0r`): the name in bold, and the way back is Cancel.
    pub(super) fn leave_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.leave_guild?;
        let name = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .map(|known| known.name.clone())?;
        let subtitle = div()
            .child("Are you sure you want to leave ")
            .child(div().font_weight(FontWeight(600.)).child(name.clone()))
            .child("? You won't be able to rejoin this server unless you are re-invited.");
        let spec = super::modal::Spec {
            id: "leave-server",
            title: format!("Leave '{name}'").into(),
            subtitle: Some(subtitle.into_any_element()),
            body: None,
            buttons: vec![
                super::modal::button(
                    "leave-cancel",
                    "Cancel",
                    super::modal::Tone::Secondary,
                    |this, _, _| this.leave_guild = None,
                ),
                super::modal::button(
                    "leave-confirm",
                    "Leave Server",
                    super::modal::Tone::Critical,
                    move |this, _, _| {
                        let _ = this.requests.send(leave_request(guild));
                        this.leave_guild = None;
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.leave_guild = None),
        };
        Some(super::modal::render(spec, cx))
    }
}

#[cfg(test)]
mod leave_tests {
    use super::*;

    #[test]
    fn leaving_is_the_guild_leave_request() {
        match leave_request(42) {
            Request::Rest {
                method,
                route,
                body,
                ..
            } => {
                assert_eq!(method, reqwest::Method::DELETE);
                assert_eq!(route, "/users/@me/guilds/42");
                assert_eq!(body, Some(json!({"lurking": false})));
            }
            _ => panic!("not a REST request"),
        }
    }
}
