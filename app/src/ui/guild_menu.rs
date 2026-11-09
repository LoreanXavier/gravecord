//! The server name's dropdown (`menu_M3qk4j fixed` "Server Actions", 220 wide under the header), measured in
//! `design/dom/menus/guild-header-darker-cozy.json`: icons on the left, 36px rows, separators after Server Boost,
//! App Directory and Privacy Settings, and Hide Muted Channels with its checkbox. Hide Muted Channels patches
//! `hide_muted_channels` through `PATCH /users/@me/guilds/settings`, as the bell menu's overrides do.

use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
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
    Notifications,
    Privacy,
    Profile,
    HideMuted,
    Leave,
}

/// Members who are not the owner can leave: the owner has to transfer or delete the server instead.
const LEAVE: &[(Entry, &str, &str)] = &[(Entry::Leave, "guild_menu_leave", "Leave Server")];

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

impl Root {
    pub(super) fn toggle_guild_menu(&mut self, cx: &mut Context<Self>) {
        self.guild_menu = !self.guild_menu;
        cx.notify();
    }

    pub(super) fn hide_muted(&self, guild: Id) -> bool {
        self.stores
            .guild_settings
            .guilds
            .get(&guild)
            .is_some_and(|settings| settings.hide_muted_channels)
    }

    fn run_guild_entry(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        if entry != Entry::HideMuted {
            self.guild_menu = false;
        }
        match entry {
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
            Entry::ActiveThreads => {
                if self.thread_browser.is_none() {
                    self.toggle_thread_browser(window, cx);
                }
            }
            Entry::Profile => self.open_settings_section("Account", cx),
            Entry::Privacy => self.open_settings_section("Data & Privacy", cx),
            Entry::Boost => self.open_settings_section("Server Boost", cx),
            Entry::CreateChannel => self.open_create_channel(false, window, cx),
            Entry::CreateCategory => self.open_create_channel(true, window, cx),
            Entry::Notifications => self.open_guild_notifications(cx),
            Entry::Invite => self.open_invite(cx),
            Entry::Settings => self.open_server_settings(guild, cx),
            // Their pages are being built: Create Event and App Directory.
            Entry::Leave => self.leave_guild = Some(guild),
            Entry::CreateEvent | Entry::AppDirectory => {}
        }
        cx.notify();
    }

    pub(super) fn guild_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.guild_menu {
            return None;
        }
        let guild = self.guild?;
        let hidden = self.hide_muted(guild);
        let mut items: Vec<AnyElement> = Vec::new();
        let owner = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .is_some_and(|known| {
                known.owner_id.is_some()
                    && known.owner_id == self.stores.current_user.as_ref().map(|me| me.id)
            });
        let mut groups: Vec<&[(Entry, &str, &str)]> = ENTRIES.to_vec();
        if !owner {
            groups.push(LEAVE);
        }
        for (group, entries) in groups.iter().enumerate() {
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
                            if entry == Entry::Leave {
                                "--text-feedback-critical"
                            } else {
                                "--interactive-text-default"
                            },
                        )))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(if entry == Entry::Leave {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--text-strong")
                                })
                                .child(*label),
                        )
                        .when(entry == Entry::HideMuted, |this| {
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
        Some(
            deferred(
                div().absolute().left(px(114.)).top(px(86.)).child(
                    div()
                        .parity_with_border("guild-menu", 1.)
                        .id("guild-menu")
                        .occlude()
                        .w(px(220.))
                        .p(px(8.))
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
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.guild_menu = false;
                            cx.notify();
                        }))
                        .children(items),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// The request behind Leave Server (`GUILD_LEAVE`, module 652215; `leaveGuild` sends `lurking`).
pub(super) fn leave_request(guild: Id) -> Request {
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
