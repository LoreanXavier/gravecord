//! PermissionsViewer (Vencord `src/plugins/permissionsViewer`, GPL-3.0-or-later): "View Permissions" in a member's
//! menu (with their roles, or in a group of its own before the last), a server channel's (after its mute rows), a
//! server's (its privacy group) and the server header's dropdown (with the SafetyIcon). It opens "<name> Permissions"
//! (size xl): on the left the roles (a member's roles and the owner, a channel's overwrites, or every role of the
//! server), on the right each permission they allow or deny with its title and an info icon whose tooltip is its
//! description (`getGuildPermissionSpecMap`). A right click on a role offers BetterRoleContext's rows and Copy Role
//! ID, on a member Copy User ID. A member's role list in their profile popout ends with a 24px shield button whose
//! popout (500 wide, below it) lists the permissions their roles grant, each a role pill whose tooltip names the role
//! that grants it ("Granted By"), with the sort order switch and Role Details (the dialog for the member).
//! Not part of the pinned client.
//!
//! GAP: the hidden `unsafeViewAsRole` row of the role menu (no switch shows it). The dialog lists the entries sorted
//! by their kind from the start, where the original's list takes that order once it draws again (its `useEffect`
//! sorts the array in place after the first frame).

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, px, rgb,
};
use std::cell::RefCell;

const ID: &str = "PermissionsViewer";

/// `PermissionsBits` in the order of its declaration (the bit of each).
const BITS: &[u32] = &[
    0, 1, 2, 3, 4, 5, 26, 27, 28, 29, 30, 43, 7, 10, 19, 41, 40, 39, 50, 11, 12, 13, 14, 15, 16,
    17, 18, 6, 31, 34, 35, 36, 37, 38, 46, 49, 51, 52, 53, 20, 21, 22, 23, 24, 25, 8, 9, 42, 45,
    48, 32, 33, 44,
];

/// The rows of `getGuildPermissionSpecMap` that `generateGuildPermissionSpec` (the server's own permission page, which
/// `permission_spec::GUILD` follows) leaves out: (bit, title, description).
const HIDDEN_SPECS: &[(u32, &str, &str)] = &[
    (
        19,
        "View Server Insights",
        "Allows members to view Server Insights, which shows data on community growth, engagement and more. This will \
         allow them to see certain data about channel activity, even for channels they cannot access.",
    ),
    (
        41,
        "View Server Subscription Insights",
        "Allows members to view Server Subscription Insights, which shows data on revenue, subscribers and free trials.",
    ),
    (
        53,
        "Manage Official Messages",
        "Allows members to mark or unmark messages as official.",
    ),
    (
        32,
        "Request to Speak",
        "Allow requests to speak in Stage channels. Stage moderators manually approve or deny each request.",
    ),
];

/// Every bit of `PermissionsBits` (what the owner's entry holds).
fn all_bits() -> u64 {
    BITS.iter().fold(0, |all, bit| all | 1 << bit)
}

/// `guildPermissionSpecMap[bit]`: the title and description of a permission.
fn spec(bit: u32) -> (&'static str, &'static str) {
    let flag = 1u64 << bit;
    super::super::permission_spec::GUILD
        .iter()
        .flat_map(|section| section.permissions.iter())
        .find(|permission| permission.flag == flag)
        .map(|permission| (permission.title, permission.description))
        .or_else(|| {
            HIDDEN_SPECS
                .iter()
                .find(|(hidden, _, _)| *hidden == bit)
                .map(|(_, title, description)| (*title, *description))
        })
        .unwrap_or(("", ""))
}

/// `PermissionOverwriteType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Role = 0,
    Member = 1,
    Owner = 2,
}

/// `RoleOrUserPermission`: a role (with its permissions), an overwrite (allow and deny) or the owner.
#[derive(Clone, Debug, PartialEq)]
struct Item {
    kind: Kind,
    id: Option<Id>,
    permissions: Option<u64>,
    allow: u64,
    deny: u64,
}

/// What a permission shows for an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    Allowed,
    Denied,
    Default,
}

fn shown(item: &Item, bit: u32) -> Shown {
    let flag = 1u64 << bit;
    if let Some(permissions) = item.permissions {
        return if permissions & flag == flag {
            Shown::Allowed
        } else {
            Shown::Denied
        };
    }
    if item.allow & flag == flag {
        Shown::Allowed
    } else if item.deny & flag == flag {
        Shown::Denied
    } else {
        Shown::Default
    }
}

/// `GuildRoleStore.getSortedRoles`: the highest first, the older first at the same position.
fn sorted_roles(guild: &dc_state::Guild) -> Vec<Id> {
    let position = |role: &Id| guild.role_info.get(role).map_or(0, |info| info.position);
    let mut roles: Vec<Id> = guild.roles.keys().copied().collect();
    roles.sort_by(|a, b| position(b).cmp(&position(a)).then(a.cmp(b)));
    roles
}

fn role_item(guild: &dc_state::Guild, role: Id) -> Item {
    Item {
        kind: Kind::Role,
        id: Some(role),
        permissions: Some(guild.role_permissions.get(&role).copied().unwrap_or(0)),
        allow: 0,
        deny: 0,
    }
}

/// `getSortedRolesForMember`: @everyone (the server's id) and the member's roles, highest first.
fn member_roles(guild: &dc_state::Guild, roles: &[Id]) -> Vec<Id> {
    sorted_roles(guild)
        .into_iter()
        .filter(|role| *role == guild.id || roles.contains(role))
        .collect()
}

/// A member's entries: their roles, then the owner holding every permission.
fn member_items(guild: &dc_state::Guild, user: Id, roles: &[Id]) -> Vec<Item> {
    let mut items: Vec<Item> = member_roles(guild, roles)
        .into_iter()
        .map(|role| role_item(guild, role))
        .collect();
    if guild.owner_id == Some(user) {
        items.push(Item {
            kind: Kind::Owner,
            id: None,
            permissions: Some(all_bits()),
            allow: 0,
            deny: 0,
        });
    }
    items
}

/// `sortPermissionOverwrites`: role overwrites by their role's position, the highest first, anything else where it
/// stands (a stable insertion sort with the original's comparison).
fn channel_items(
    guild: &dc_state::Guild,
    overwrites: &[dc_state::permissions::Overwrite],
) -> Vec<Item> {
    let mut items: Vec<Item> = overwrites
        .iter()
        .map(|overwrite| Item {
            kind: if overwrite.kind == 1 {
                Kind::Member
            } else {
                Kind::Role
            },
            id: Some(overwrite.id),
            permissions: None,
            allow: overwrite.allow,
            deny: overwrite.deny,
        })
        .collect();
    let position = |item: &Item| {
        item.id
            .and_then(|id| guild.role_info.get(&id))
            .map_or(f64::NEG_INFINITY, |info| info.position as f64)
    };
    let before = |a: &Item, b: &Item| {
        a.kind == Kind::Role && b.kind == Kind::Role && position(b) - position(a) < 0.
    };
    for index in 1..items.len() {
        let mut at = index;
        while at > 0 && before(&items[at], &items[at - 1]) {
            items.swap(at, at - 1);
            at -= 1;
        }
    }
    items
}

/// The dialog's entries in kind order (`permissions.sort((a, b) => a.type - b.type)`, stable).
fn by_kind(mut items: Vec<Item>) -> Vec<Item> {
    items.sort_by_key(|item| item.kind);
    items
}

/// `sortUserRoles` with `permissionsSortOrder` (0: highest role first, 1: lowest).
fn sort_roles(guild: &dc_state::Guild, roles: &mut [Id], lowest: bool) {
    let position = |role: &Id| guild.role_info.get(role).map_or(0, |info| info.position);
    if lowest {
        roles.sort_by_key(position);
    } else {
        roles.sort_by_key(|role| std::cmp::Reverse(position(role)));
    }
}

/// One pill of the member's popout: the permission, and the role granting it (its colour and position).
#[derive(Clone, Debug, PartialEq)]
struct Granted {
    permission: &'static str,
    role: String,
    color: Option<u32>,
    position: f64,
}

/// `UserPermissionsComponent`'s list: the owner first when the member owns the server, then each permission with the
/// first role (in the chosen order) that holds it, sorted by that role's position, the highest first.
fn granted(guild: &dc_state::Guild, user: Id, roles: &[Id], lowest: bool) -> Vec<Granted> {
    let mut out = Vec::new();
    if guild.owner_id == Some(user) {
        out.push(Granted {
            permission: "Server Owner",
            role: "Owner".to_owned(),
            color: None,
            position: f64::INFINITY,
        });
    }
    let mut ordered = member_roles(guild, roles);
    sort_roles(guild, &mut ordered, lowest);
    for bit in BITS {
        let flag = 1u64 << bit;
        if let Some(role) = ordered
            .iter()
            .find(|role| guild.role_permissions.get(role).copied().unwrap_or(0) & flag == flag)
        {
            let info = guild.role_info.get(role).copied().unwrap_or_default();
            out.push(Granted {
                permission: spec(*bit).0,
                role: guild.roles.get(role).cloned().unwrap_or_default(),
                color: (info.color != 0).then_some(info.color),
                position: info.position as f64,
            });
        }
    }
    // `b.rolePosition - a.rolePosition`, a stable sort.
    out.sort_by(|a, b| {
        b.position
            .partial_cmp(&a.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The open dialog.
struct Dialog {
    guild: Id,
    header: String,
    items: Vec<Item>,
    selected: usize,
}

/// The entry whose menu is open (role or member) and the member popout's user.
#[derive(Clone, Copy)]
struct Popout {
    guild: Id,
    user: Id,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
    static POPOUT: RefCell<Option<Popout>> = const { RefCell::new(None) };
    /// The popout's pill under the pointer ("Granted By" shows over it) and where each pill is.
    static HOVERED: RefCell<Option<usize>> = const { RefCell::new(None) };
    static PILLS: RefCell<Vec<gpui::Bounds<gpui::Pixels>>> = const { RefCell::new(Vec::new()) };
}

/// Whether the plugin is on.
pub(in crate::ui) fn permissions_viewer() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("PermissionsViewer")
}

/// Whether the member popout is open (its layer, `keyboard/layers.rs`).
pub(in crate::ui) fn permissions_viewer_popout_open() -> bool {
    POPOUT.with(|popout| popout.borrow().is_some())
}

/// `permissionsSortOrder`: whether the lowest role comes first.
fn lowest_first() -> bool {
    crate::plugins::with(|state| state.value(ID, "permissionsSortOrder") == serde_json::json!(1))
}

impl Root {
    /// The rows of the menus: a member's ("roles" group), a server channel's (after its mute rows) and a server's (its
    /// privacy group, after NewGuildSettings' row and before ServerInfo's).
    pub(in crate::ui) fn with_permissions_viewer_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if !permissions_viewer() {
            return items;
        }
        let row = || Entry::Row(Row::new(Act::PluginViewPermissions, "View Permissions"));
        let after = |items: &[Entry], acts: &[Act]| {
            items
                .iter()
                .rposition(|entry| matches!(entry, Entry::Row(row) if acts.contains(&row.act)))
        };
        // The end of the group holding `at`.
        let group_end = |items: &[Entry], at: usize| {
            items[at..]
                .iter()
                .position(|entry| matches!(entry, Entry::Separator))
                .map_or(items.len(), |end| at + end)
        };
        match target {
            Target::User {
                user,
                guild: Some(guild),
            } => {
                if self.stores.members.get(guild, user).is_none() {
                    return items;
                }
                if let Some(at) = after(&items, &[Act::Roles]) {
                    let end = group_end(&items, at);
                    items.insert(end, row());
                } else {
                    // "roles" may be missing (a member without roles): a group of its own above the last.
                    let last = items
                        .iter()
                        .rposition(|entry| matches!(entry, Entry::Separator))
                        .unwrap_or(items.len());
                    items.splice(last..last, [Entry::Separator, row()]);
                }
            }
            // `channel-context` covers categories too; threads have `thread-context`, which the original leaves alone.
            Target::Channel(channel) | Target::Category(channel) => {
                if self.guild_of_channel(channel).is_none() {
                    return items;
                }
                if let Some(at) = after(&items, &[Act::Mute]) {
                    let end = group_end(&items, at);
                    items.insert(end, row());
                }
            }
            Target::Guild(_) => {
                let at = after(
                    &items,
                    &[Act::PluginApplyNewGuildSettings, Act::EditProfile],
                )
                .map_or(items.len(), |at| at + 1);
                items.insert(at, row());
            }
            _ => {}
        }
        items
    }

    /// View Permissions of a menu's target.
    pub(in crate::ui) fn permissions_viewer_open(
        &mut self,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        let opened = match target {
            Target::User {
                user,
                guild: Some(guild),
            } => self.permissions_for_member(guild, user),
            Target::Channel(channel) | Target::Category(channel) => {
                self.guild_of_channel(channel).and_then(|guild| {
                    let known = self.stores.guild(guild)?;
                    let channel = known.channels.iter().find(|known| known.id == channel)?;
                    Some((
                        guild,
                        channel.name.clone(),
                        channel_items(known, &channel.overwrites),
                    ))
                })
            }
            Target::Guild(guild) => self.stores.guild(guild).map(|known| {
                (
                    guild,
                    known.name.clone(),
                    sorted_roles(known)
                        .into_iter()
                        .map(|role| role_item(known, role))
                        .collect(),
                )
            }),
            _ => None,
        };
        if let Some((guild, header, items)) = opened {
            self.permissions_viewer_show(guild, header, items, cx);
        }
    }

    /// The server header dropdown's row: the server's roles.
    pub(in crate::ui) fn permissions_viewer_open_guild(
        &mut self,
        guild: Id,
        cx: &mut Context<Self>,
    ) {
        self.permissions_viewer_open(Target::Guild(guild), cx);
    }

    fn permissions_for_member(&self, guild: Id, user: Id) -> Option<(Id, String, Vec<Item>)> {
        let known = self.stores.guild(guild)?;
        let member = self.stores.members.get(guild, user)?;
        let header = member
            .nick
            .clone()
            .unwrap_or_else(|| member.user.username.clone());
        Some((guild, header, member_items(known, user, &member.roles)))
    }

    fn permissions_viewer_show(
        &mut self,
        guild: Id,
        header: String,
        items: Vec<Item>,
        cx: &mut Context<Self>,
    ) {
        // `GUILD_MEMBERS_REQUEST` for the overwrites' members the store lacks.
        let missing: Vec<Id> = items
            .iter()
            .filter(|item| item.kind == Kind::Member)
            .filter_map(|item| item.id)
            .filter(|user| self.stores.members.get(guild, *user).is_none())
            .collect();
        if !missing.is_empty() {
            let _ = self.requests.send(crate::net::Request::GuildMembers {
                guilds: vec![guild],
                query: String::new(),
                limit: 0,
                user_ids: missing,
            });
        }
        for item in &items {
            if item.kind == Kind::Member
                && let Some(user) = item.id.and_then(|id| self.stores.users.get(&id)).cloned()
            {
                let (key, url) = super::super::avatar_source(&user);
                self.request_image(key, url);
            }
        }
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                guild,
                header,
                items: by_kind(items),
                selected: 0,
            })
        });
        cx.notify();
    }

    fn permissions_viewer_close(&mut self, cx: &mut Context<Self>) {
        DIALOG.with(|dialog| dialog.borrow_mut().take());
        cx.notify();
    }

    /// The name an entry shows.
    fn permission_entry_label(&self, guild: &dc_state::Guild, item: &Item) -> Option<String> {
        match item.kind {
            Kind::Role => Some(
                item.id
                    .and_then(|id| guild.roles.get(&id).cloned())
                    .unwrap_or_else(|| "Unknown Role".to_owned()),
            ),
            // `getUniqueUsername`: the name, with its tag where it still has one.
            Kind::Member => Some(
                item.id
                    .and_then(|id| self.stores.users.get(&id))
                    .map(|user| {
                        if user.discriminator.is_empty() || user.discriminator == "0" {
                            user.username.clone()
                        } else {
                            format!("{}#{}", user.username, user.discriminator)
                        }
                    })
                    .unwrap_or_else(|| "Unknown User".to_owned()),
            ),
            Kind::Owner => None,
        }
    }

    /// `RolesAndUsersPermissionsComponent`.
    pub(in crate::ui) fn permissions_viewer_dialog(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::super::{color, icon, modal};
        let (guild_id, header, items, selected) = DIALOG.with(|dialog| {
            dialog.borrow().as_ref().map(|open| {
                (
                    open.guild,
                    open.header.clone(),
                    open.items.clone(),
                    open.selected,
                )
            })
        })?;
        let guild = self.stores.guild(guild_id)?;
        let body = match items.get(selected) {
            None => div()
                .w_full()
                .h_full()
                .pb(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .text_size(px(20.))
                .line_height(px(24.))
                .text_color(color("--text-default"))
                .child("No permissions to display!")
                .into_any_element(),
            Some(chosen) => {
                let tall = (f32::from(window.viewport_size().height) * 0.8).min(760.);
                let list: Vec<AnyElement> = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let label = self.permission_entry_label(guild, item);
                        let role = item.id.filter(|_| item.kind == Kind::Role);
                        let tint = role
                            .and_then(|role| guild.role_info.get(&role))
                            .filter(|info| info.color != 0)
                            .map_or_else(|| color("--primary-300"), |info| rgb(info.color));
                        let avatar = item
                            .id
                            .filter(|_| item.kind == Kind::Member)
                            .and_then(|id| self.stores.users.get(&id))
                            .and_then(|user| {
                                self.images
                                    .get(&super::super::avatar_source(user).0)
                                    .cloned()
                            });
                        let menu = (item.kind, item.id);
                        div()
                            .id(SharedString::from(format!(
                                "permissions-viewer-entry-{index}"
                            )))
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                DIALOG.with(|dialog| {
                                    if let Some(open) = dialog.borrow_mut().as_mut() {
                                        open.selected = index;
                                    }
                                });
                                cx.notify();
                            }))
                            .on_mouse_up(
                                gpui::MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                    cx.stop_propagation();
                                    match menu {
                                        (Kind::Role, Some(role)) => this.open_context_menu(
                                            Target::PluginPermissionRole {
                                                role,
                                                guild: guild_id,
                                            },
                                            event.position,
                                            cx,
                                        ),
                                        (Kind::Member, Some(user)) => this.open_context_menu(
                                            Target::PluginPermissionUser(user),
                                            event.position,
                                            cx,
                                        ),
                                        _ => {}
                                    }
                                }),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .p(px(8.))
                                    .rounded(px(5.))
                                    // Where View Role Members opens its popout from.
                                    .children(role.map(|role| {
                                        self.anchor_probe(format!("permissions-viewer-role-{role}"))
                                    }))
                                    .when(index == selected, |this| {
                                        this.bg(color("--background-mod-strong"))
                                    })
                                    .when(index != selected, |this| {
                                        this.hover(|style| {
                                            style.bg(color("--background-mod-subtle"))
                                        })
                                    })
                                    .when(matches!(item.kind, Kind::Role | Kind::Owner), |this| {
                                        this.child(
                                            div().size(px(12.)).flex_none().rounded_full().bg(tint),
                                        )
                                    })
                                    .children(
                                        role.and_then(|role| self.role_icon_art(guild, role, 20.)),
                                    )
                                    .children(avatar.map(|avatar| {
                                        img(avatar).size(px(20.)).flex_none().rounded_full()
                                    }))
                                    .child(match label {
                                        Some(label) => div()
                                            .min_w(px(0.))
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(label)
                                            .into_any_element(),
                                        None => div()
                                            .flex()
                                            .items_center()
                                            .gap(px(3.2))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child("@owner")
                                            .child(icon(
                                                "plugin_owner_crown",
                                                18.,
                                                "--text-default",
                                            ))
                                            .into_any_element(),
                                    }),
                            )
                            .into_any_element()
                    })
                    .collect();
                let shown_bits: Vec<(u32, Shown)> = BITS
                    .iter()
                    .map(|bit| (*bit, shown(chosen, *bit)))
                    .filter(|(_, shown)| *shown != Shown::Default)
                    .collect();
                let last = shown_bits.len().saturating_sub(1);
                let rows = shown_bits
                    .into_iter()
                    .enumerate()
                    .map(|(index, (bit, shown))| {
                        let (title, description) = spec(bit);
                        let info = div()
                            .id(SharedString::from(format!("permissions-viewer-info-{bit}")))
                            .ml_auto()
                            .flex_none()
                            .cursor_pointer()
                            .text_color(color("--interactive-muted"))
                            .hover(|style| style.text_color(color("--interactive-icon-active")))
                            .child(
                                gpui::svg()
                                    .path("icons/circle_info.svg")
                                    .size(px(24.))
                                    .text_color(color("--interactive-muted")),
                            );
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .pt(px(10.))
                            .pb(px(10.))
                            .pl(px(10.))
                            .pr(px(2.))
                            .when(index != last, |this| {
                                this.border_b(px(2.))
                                    .border_color(color("--background-mod-normal"))
                            })
                            .child(
                                div()
                                    .size(px(24.))
                                    .flex_none()
                                    .border_1()
                                    .border_color(color("--background-mod-strong"))
                                    .child(match shown {
                                        Shown::Allowed => icon(
                                            "plugin_permission_allowed",
                                            24.,
                                            "--status-positive",
                                        ),
                                        _ => {
                                            icon("plugin_permission_denied", 24., "--status-danger")
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(title),
                            )
                            .child(self.with_tip(
                                format!("permissions-viewer-info-tip-{bit}"),
                                super::super::tooltip::Tip::body(
                                    super::super::tooltip::Body::Text(description.into()),
                                    super::super::tooltip::Side::Top,
                                ),
                                info,
                                cx,
                            ))
                    });
                div()
                    .w_full()
                    .h(px(tall))
                    .min_h(px(0.))
                    .pb(px(24.))
                    .flex()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("permissions-viewer-list")
                            .flex_none()
                            .w(px(200.))
                            .max_h_full()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .pr(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(list),
                    )
                    .child(
                        div()
                            .w(px(2.))
                            .flex_none()
                            .bg(color("--background-mod-normal")),
                    )
                    .child(
                        div()
                            .id("permissions-viewer-permissions")
                            .flex_1()
                            .min_w(px(0.))
                            .max_h_full()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .pr(px(8.))
                            .flex()
                            .flex_col()
                            .children(rows),
                    )
                    .into_any_element()
            }
        };
        let spec = modal::Spec {
            id: "permissions-viewer-dialog",
            title: SharedString::from(format!("{header} Permissions")),
            subtitle: None,
            body: Some(body),
            buttons: Vec::new(),
            close: Box::new(|this: &mut Root, _, cx| this.permissions_viewer_close(cx)),
        };
        Some(modal::render_xl(spec, cx))
    }

    /// The role menu of the dialog's list: BetterRoleContext's `before` rows, Copy Role ID, then its `after` rows.
    pub(in crate::ui) fn permission_role_items(&self, role: Id, guild: Id) -> Vec<Entry> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if self.my_permission(guild, None, dc_state::permissions::MANAGE_ROLES) {
            rows.push(Row::new(Act::PluginEditRole, "Edit Role").icon("menu_edit_message"));
        }
        if known
            .role_info
            .get(&role)
            .is_some_and(|info| info.color != 0)
        {
            rows.push(
                Row::new(Act::PluginCopyRoleColor, "Copy Role Color").icon("settings_appearance"),
            );
        }
        rows.push(Row::new(Act::CopyId, dc_intl::tr("Copy Role ID")).icon("menu_id"));
        if known
            .role_icons
            .get(&role)
            .is_some_and(|icon| icon.icon.is_some())
        {
            rows.push(Row::new(Act::PluginViewRoleIcon, "View Role Icon").icon("settings_image"));
        }
        rows.push(
            Row::new(Act::PluginViewRoleMembers, "View Role Members")
                .trailing("plugin_role_members"),
        );
        rows.into_iter().map(Entry::Row).collect()
    }

    /// The member menu of the dialog's list: Copy User ID.
    pub(in crate::ui) fn permission_user_items(&self) -> Vec<Entry> {
        vec![Entry::Row(
            Row::new(Act::CopyId, dc_intl::tr("Copy User ID")).icon("menu_id"),
        )]
    }

    /// The shield button at the end of a member's role list in their profile popout.
    pub(in crate::ui) fn permissions_viewer_button(
        &self,
        guild: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !permissions_viewer() || self.stores.members.get(guild, user).is_none() {
            return None;
        }
        let palette = self.profile_palette(user, Some(guild), false);
        let border = palette.token("--user-profile-border");
        let hover = palette.token("--user-profile-background-hover");
        let tint = palette.token("--interactive-icon-default");
        let button = div()
            .id("permissions-viewer-button")
            .relative()
            .h(px(24.))
            .p(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(move |style| style.bg(hover))
            .child(
                gpui::svg()
                    .path("icons/plugin_safety.svg")
                    .size(px(16.))
                    .text_color(tint),
            )
            .child(self.anchor_probe("permissions-viewer-button"))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                POPOUT.with(|popout| {
                    let mut popout = popout.borrow_mut();
                    *popout = match *popout {
                        Some(open) if open.user == user && open.guild == guild => None,
                        _ => Some(Popout { guild, user }),
                    };
                });
                cx.notify();
            }));
        Some(
            self.with_tip(
                "permissions-viewer-button-tip",
                super::super::tooltip::Tip::text(
                    "View Permissions",
                    super::super::tooltip::Side::Top,
                ),
                button,
                cx,
            )
            .into_any_element(),
        )
    }

    /// Closes the member popout (the profile popout went away).
    pub(in crate::ui) fn permissions_viewer_popout_close(&mut self) {
        POPOUT.with(|popout| popout.borrow_mut().take());
        HOVERED.with(|hovered| hovered.borrow_mut().take());
    }

    /// `FakeRole`: a role pill (the dot in the role's colour, the text in `text-xs/medium`).
    fn permission_pill(&self, text: String, tint: gpui::Hsla) -> gpui::Div {
        use super::super::color;
        div()
            .min_h(px(24.))
            .max_w_full()
            .pl(px(2.))
            .pr(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(8.))
            .flex()
            .items_center()
            .child(
                div()
                    .mx(px(4.))
                    .size(px(12.))
                    .flex_none()
                    .rounded_full()
                    .bg(tint),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(text),
            )
    }

    /// The member popout: "Permissions", the sort switch and Role Details, then a pill per granted permission.
    pub(in crate::ui) fn permissions_viewer_popout(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::super::color;
        let open = POPOUT.with(|popout| *popout.borrow())?;
        // The popout belongs to the profile popout it was opened from.
        if !permissions_viewer()
            || self
                .profile
                .as_ref()
                .is_none_or(|profile| profile.user != open.user)
        {
            POPOUT.with(|popout| popout.borrow_mut().take());
            return None;
        }
        let anchor = self
            .tip_anchors
            .borrow()
            .get("permissions-viewer-button")
            .copied()?;
        let guild = self.stores.guild(open.guild)?;
        let member = self.stores.members.get(open.guild, open.user)?;
        let lowest = lowest_first();
        let pills = granted(guild, open.user, &member.roles, lowest);
        let (guild_id, user) = (open.guild, open.user);
        let header_name = member
            .nick
            .clone()
            .unwrap_or_else(|| member.user.username.clone());
        let width = 500.;
        let viewport = window.viewport_size();
        let left = (f32::from(anchor.center().x) - width / 2.)
            .clamp(8., (f32::from(viewport.width) - width - 8.).max(8.));
        let top = f32::from(anchor.bottom()) + 8.;
        let sort = div()
            .id("permissions-viewer-sort")
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path(if lowest {
                        "icons/plugin_sort_down.svg"
                    } else {
                        "icons/plugin_sort_up.svg"
                    })
                    .size(px(24.))
                    .text_color(color("--text-default")),
            )
            .on_click(cx.listener(move |_, _, _, cx| {
                crate::plugins::update(|state| {
                    state.set_value(
                        ID,
                        "permissionsSortOrder",
                        &serde_json::json!(if lowest { 0 } else { 1 }),
                    );
                });
                cx.notify();
            }));
        let details = div()
            .id("permissions-viewer-details")
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path("icons/plugin_more_dots.svg")
                    .size(px(24.))
                    .text_color(color("--text-default")),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.permissions_viewer_popout_close();
                if let Some((guild, _, items)) = this.permissions_for_member(guild_id, user) {
                    this.permissions_viewer_show(guild, header_name.clone(), items, cx);
                }
            }));
        let hovered = HOVERED.with(|hovered| *hovered.borrow());
        let count = pills.len();
        PILLS.with(|pills| pills.borrow_mut().resize(count, gpui::Bounds::default()));
        let granted_by = hovered.and_then(|index| {
            let pill = pills.get(index)?;
            let anchor = PILLS.with(|pills| pills.borrow().get(index).copied())?;
            let tint: gpui::Hsla = pill
                .color
                .map_or_else(|| color("--primary-300").into(), |value| rgb(value).into());
            // `GrantedByTooltip` in a tooltip at most 300 wide, its content in a row 4px apart.
            Some(super::super::tooltip::placed_content(
                anchor,
                div()
                    .max_w(px(300.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child("Granted By"),
                    )
                    .child(self.permission_pill(pill.role.clone(), tint))
                    .into_any_element(),
            ))
        });
        let pill_elements: Vec<AnyElement> = pills
            .into_iter()
            .enumerate()
            .map(|(index, pill)| {
                let tint: gpui::Hsla = pill
                    .color
                    .map_or_else(|| color("--primary-300").into(), |value| rgb(value).into());
                self.permission_pill(pill.permission.to_owned(), tint)
                    .id(SharedString::from(format!(
                        "permissions-viewer-pill-{index}"
                    )))
                    .on_hover(cx.listener(move |_, inside: &bool, _, cx| {
                        HOVERED.with(|hovered| {
                            let mut hovered = hovered.borrow_mut();
                            if *inside {
                                *hovered = Some(index);
                            } else if *hovered == Some(index) {
                                *hovered = None;
                            }
                        });
                        cx.notify();
                    }))
                    .child(
                        gpui::canvas(
                            move |bounds, _, _| {
                                PILLS.with(|pills| {
                                    if let Some(slot) = pills.borrow_mut().get_mut(index) {
                                        *slot = bounds;
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .into_any_element()
            })
            .collect();
        let card = div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .id("permissions-viewer-popout");
        // A press outside closes it while it is the top layer, as the role picker's does.
        let card = super::super::keyboard::outside(
            card,
            super::super::keyboard::Layer::PluginPermissions,
            cx,
        );
        Some(
            gpui::deferred(
                card.occlude()
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .mb(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child("Permissions"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        self.with_tip(
                                            "permissions-viewer-sort-tip",
                                            super::super::tooltip::Tip::body(
                                                super::super::tooltip::Body::Text(
                                                    if lowest {
                                                        "Sorting by Lowest Role"
                                                    } else {
                                                        "Sorting by Highest Role"
                                                    }
                                                    .into(),
                                                ),
                                                super::super::tooltip::Side::Top,
                                            ),
                                            sort,
                                            cx,
                                        ),
                                    )
                                    .child(self.with_tip(
                                        "permissions-viewer-details-tip",
                                        super::super::tooltip::Tip::text(
                                            "Role Details",
                                            super::super::tooltip::Side::Top,
                                        ),
                                        details,
                                        cx,
                                    )),
                            ),
                    )
                    .child(div().flex().flex_wrap().gap(px(4.)).children(pill_elements))
                    .children(granted_by),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guild() -> dc_state::Guild {
        dc_state::Guild::from_value(&json!({
            "id": "100", "name": "fixture", "owner_id": "200", "channels": [],
            "roles": [
                {"id": "100", "name": "@everyone", "position": 0, "permissions": "1024"},
                {"id": "101", "name": "low", "position": 1, "permissions": "2048", "color": 0},
                {"id": "102", "name": "high", "position": 5, "permissions": "3072", "color": 16711680}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn every_bit_has_a_title_and_the_order_is_the_originals() {
        assert_eq!(BITS.len(), 53);
        for bit in BITS {
            assert!(!spec(*bit).0.is_empty(), "bit {bit}");
        }
        assert_eq!(spec(19).0, "View Server Insights");
        assert_eq!(spec(10).0, "View Channels");
        assert_eq!(all_bits().count_ones(), 53);
    }

    #[test]
    fn entries_and_grants_follow_the_original() {
        let guild = guild();
        let items = member_items(&guild, 200, &[101, 102]);
        let kinds: Vec<(Kind, Option<Id>)> =
            items.iter().map(|item| (item.kind, item.id)).collect();
        assert_eq!(
            kinds,
            [
                (Kind::Role, Some(102)),
                (Kind::Role, Some(101)),
                (Kind::Role, Some(100)),
                (Kind::Owner, None)
            ]
        );
        assert_eq!(shown(&items[2], 10), Shown::Allowed);
        assert_eq!(shown(&items[2], 11), Shown::Denied);
        let overwrite = |id, kind, allow, deny| dc_state::permissions::Overwrite {
            id,
            kind,
            allow,
            deny,
        };
        let channel = channel_items(
            &guild,
            &[
                overwrite(101, 0, 1024, 0),
                overwrite(300, 1, 0, 2048),
                overwrite(102, 0, 0, 0),
            ],
        );
        assert_eq!(shown(&channel[0], 10), Shown::Allowed);
        assert_eq!(shown(&channel[1], 11), Shown::Denied);
        assert_eq!(shown(&channel[1], 10), Shown::Default);
        let sorted = by_kind(channel);
        assert_eq!(
            sorted.iter().map(|item| item.kind).collect::<Vec<_>>(),
            [Kind::Role, Kind::Role, Kind::Member]
        );
        let pills = granted(&guild, 200, &[101, 102], false);
        assert_eq!(pills[0].permission, "Server Owner");
        assert_eq!(pills[1].role, "high");
        let lowest = granted(&guild, 300, &[101, 102], true);
        assert_eq!(
            lowest
                .iter()
                .map(|pill| (pill.permission, pill.role.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Send Messages and Create Posts", "low"),
                ("View Channels", "@everyone")
            ]
        );
    }
}
