//! Server Settings > Roles once roles exist: the list (module 989433 `Eb`/`EE`, `rolesTable_EerRtM`, rows
//! `roleRow_YEKbw3`) and the role editor (`En`: the role column `sidebar_7NrAL2`, then Display, Permissions, Links and
//! Manage Members). Edits are drafts until Save, which sends module 636042 `JY`'s `updateRole` for every edited role
//! (`PATCH /guilds/{id}/roles/{role}` with name, permissions, color, hoist, mentionable). Permissions come from
//! module 558393 `generateGuildPermissionSpec` (`permission_spec::GUILD`). Manage Members adds with
//! `PATCH /guilds/{id}/roles/{role}/members` (`bulkAddMemberRoles`) and removes by `PATCH /guilds/{id}/members/{user}`
//! with the member's other roles. Measured in `design/dom/settings/guild-roles-list-darker-cozy.json` and
//! `guild-role-{display,permissions,members}-darker-cozy.json`.

use super::permission_spec::GUILD;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div,
    prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const ROLES_PREFIX: &str = "guild-roles:";
const COUNTS_PREFIX: &str = "guild-roles:counts:";
const SAVED_PREFIX: &str = "guild-roles:saved:";
const MEMBER_IDS_PREFIX: &str = "guild-roles:members:";
const STRIP_PREFIX: &str = "guild-roles:strip:";
/// Module 652215 `_tK`: the role colour swatches, the light row then the dark one.
const SWATCHES: [u32; 20] = [
    1752220, 3066993, 3447003, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 9807270, 6323595,
    1146986, 2067276, 2123412, 7419530, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 9936031, 5533306,
];
/// A role without a colour shows `rgb(153, 170, 181)` (`roleCircle_7RsUnm`).
const NO_COLOR: u32 = 0x99aab5;
const ADMINISTRATOR: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum RoleTab {
    #[default]
    Display,
    Permissions,
    Links,
    Members,
}

/// A role as the editor holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Draft {
    pub name: String,
    pub color: u32,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: u64,
}

impl Draft {
    fn from_role(role: &Value) -> Self {
        Draft {
            name: role["name"].as_str().unwrap_or_default().to_owned(),
            color: role["color"].as_u64().unwrap_or(0) as u32,
            hoist: role["hoist"].as_bool().unwrap_or(false),
            mentionable: role["mentionable"].as_bool().unwrap_or(false),
            permissions: role["permissions"]
                .as_str()
                .and_then(|bits| bits.parse().ok())
                .unwrap_or(0),
        }
    }

    /// `updateRole`'s body for this draft.
    fn body(&self, role: &Value) -> Value {
        serde_json::json!({
            "name": self.name,
            "permissions": self.permissions.to_string(),
            "color": self.color,
            "colors": {"primary_color": self.color, "secondary_color": role["colors"]["secondary_color"], "tertiary_color": role["colors"]["tertiary_color"]},
            "hoist": self.hoist,
            "mentionable": self.mentionable,
        })
    }
}

/// The Roles pages' fields and drafts.
pub(crate) struct RolesState {
    pub(super) tab: RoleTab,
    pub(super) drafts: HashMap<Id, Draft>,
    /// The Role name field, for the role it was made for.
    name: Option<(Id, Entity<Editor>, Subscription)>,
    pub(super) list_search: Entity<Editor>,
    pub(super) perm_search: Entity<Editor>,
    pub(super) member_search: Entity<Editor>,
    /// The "..." menu is open, for this role (module 396704).
    pub(super) menu: Option<Id>,
    /// Which button the menu hangs off: the editor's, or a row's of the Roles list.
    pub(super) menu_anchor: gpui::SharedString,
    /// "Delete Role" is asking about this role.
    pub(super) confirm_delete: Option<Id>,
    /// The role being duplicated, waiting for its copy to be created.
    duplicating: Option<Value>,
    /// "Add members": its search field and the members ticked so far.
    pub(super) adding: Option<(Entity<Editor>, Vec<Id>, Subscription)>,
    _events: Vec<Subscription>,
}

impl RolesState {
    #[cfg(feature = "capture")]
    pub(super) fn fields(&self) -> Vec<Entity<Editor>> {
        let mut list = vec![
            self.list_search.clone(),
            self.perm_search.clone(),
            self.member_search.clone(),
        ];
        list.extend(self.name.as_ref().map(|(_, field, _)| field.clone()));
        list.extend(self.adding.as_ref().map(|(field, _, _)| field.clone()));
        list
    }
}

fn rgb(value: u32) -> gpui::Rgba {
    gpui::rgb(value)
}

impl Root {
    /// Escape: the Delete Role confirmation, the Add members dialog and the "..." menu close one at a time (top
    /// first) before the page does.
    pub(super) fn close_roles_layer(&mut self) -> bool {
        let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        else {
            return false;
        };
        state.confirm_delete.take().is_some()
            || state.adding.take().is_some()
            || state.menu.take().is_some()
    }

    pub(super) fn request_roles(&mut self, guild: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ROLES_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/roles"),
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{COUNTS_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/roles/member-counts"),
        });
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        if open.roles.is_some() {
            return;
        }
        let field = |placeholder: &str, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_owned();
            cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_placeholder(placeholder);
                editor
            })
        };
        let list_search = field("Search Roles", cx);
        let perm_search = field("Search permissions", cx);
        let member_search = field("Search Members", cx);
        let mut events = Vec::new();
        for editor in [&list_search, &perm_search, &member_search] {
            events.push(cx.subscribe(editor, |this, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Cancel) {
                    this.server_settings = None;
                }
                cx.notify();
            }));
        }
        if let Some(open) = self.server_settings.as_mut() {
            open.roles = Some(RolesState {
                tab: RoleTab::Display,
                drafts: HashMap::new(),
                name: None,
                list_search,
                perm_search,
                member_search,
                menu: None,
                menu_anchor: "server-role-menu".into(),
                confirm_delete: None,
                duplicating: None,
                adding: None,
                _events: events,
            });
        }
    }

    pub(super) fn roles_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        // A member read to take a role away: everything else stays.
        if let Some(rest) = key.strip_prefix(STRIP_PREFIX) {
            let parts: Vec<&str> = rest.split(':').collect();
            if let [guild, role, user] = parts[..] {
                let others: Vec<Value> = self
                    .fetched
                    .get(key)
                    .and_then(|body| body["roles"].as_array().cloned())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|known| known.as_str() != Some(role))
                    .collect();
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{SAVED_PREFIX}{guild}")),
                    method: reqwest::Method::PATCH,
                    route: format!("/guilds/{guild}/members/{user}"),
                    body: Some(serde_json::json!({"roles": others})),
                });
            }
            return;
        }
        if let Some(guild) = key.strip_prefix(SAVED_PREFIX) {
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_roles(guild, cx);
                if let Some(role) = self.server_settings.as_ref().and_then(|open| open.role) {
                    self.request_role_members(guild, role);
                }
            }
            return;
        }
        cx.notify();
    }

    fn roles_of(&self, guild: Id) -> Vec<Value> {
        let mut roles: Vec<Value> = self
            .fetched
            .get(&format!("{ROLES_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        // Highest position first, as the list shows them.
        roles.sort_by_key(|role| std::cmp::Reverse(role["position"].as_i64().unwrap_or(0)));
        roles
    }

    fn role_value(&self, guild: Id, role: Id) -> Option<Value> {
        self.roles_of(guild)
            .into_iter()
            .find(|known| known["id"].as_str() == Some(role.to_string().as_str()))
    }

    /// The role as edited, or as saved.
    fn draft(&self, guild: Id, role: Id) -> Option<Draft> {
        if let Some(draft) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
            .and_then(|state| state.drafts.get(&role))
        {
            return Some(draft.clone());
        }
        self.role_value(guild, role)
            .map(|value| Draft::from_role(&value))
    }

    fn edit_draft(
        &mut self,
        guild: Id,
        role: Id,
        change: impl FnOnce(&mut Draft),
        cx: &mut Context<Self>,
    ) {
        let Some(mut draft) = self.draft(guild, role) else {
            return;
        };
        change(&mut draft);
        if let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        {
            state.drafts.insert(role, draft);
        }
        cx.notify();
    }

    /// Drafts that differ from the saved roles.
    pub(super) fn roles_dirty(&self) -> bool {
        let Some(open) = self.server_settings.as_ref() else {
            return false;
        };
        let Some(state) = open.roles.as_ref() else {
            return false;
        };
        state.drafts.iter().any(|(role, draft)| {
            self.role_value(open.guild, *role)
                .is_none_or(|saved| Draft::from_role(&saved) != *draft)
        })
    }

    pub(super) fn save_roles(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let guild = open.guild;
        let drafts: Vec<(Id, Draft)> = open
            .roles
            .as_ref()
            .map(|state| state.drafts.clone().into_iter().collect())
            .unwrap_or_default();
        for (role, draft) in drafts {
            let Some(saved) = self.role_value(guild, role) else {
                continue;
            };
            if Draft::from_role(&saved) == draft {
                continue;
            }
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/roles/{role}"),
                body: Some(draft.body(&saved)),
            });
            // The reply refetches; until then the list shows the draft as saved.
            if let Some(list) = self
                .fetched
                .get_mut(&format!("{ROLES_PREFIX}{guild}"))
                .and_then(|body| body.as_array_mut())
                && let Some(known) = list
                    .iter_mut()
                    .find(|known| known["id"].as_str() == Some(role.to_string().as_str()))
            {
                known["name"] = Value::String(draft.name.clone());
                known["color"] = Value::from(draft.color);
                known["hoist"] = Value::Bool(draft.hoist);
                known["mentionable"] = Value::Bool(draft.mentionable);
                known["permissions"] = Value::String(draft.permissions.to_string());
            }
        }
        if let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        {
            state.drafts.clear();
        }
        cx.notify();
    }

    pub(super) fn reset_roles(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        let guild = open.guild;
        let role = open.role;
        if let Some(state) = open.roles.as_mut() {
            state.drafts.clear();
        }
        if let Some(role) = role {
            let name = self
                .role_value(guild, role)
                .and_then(|value| value["name"].as_str().map(str::to_owned))
                .unwrap_or_default();
            if let Some((_, field, _)) = self
                .server_settings
                .as_ref()
                .and_then(|open| open.roles.as_ref())
                .and_then(|state| state.name.as_ref())
            {
                field.update(cx, |editor, _| editor.set_text(&name));
            }
        }
        cx.notify();
    }

    /// Opens a role in the editor, with a name field for it.
    pub(super) fn select_role(&mut self, role: Id, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let guild = open.guild;
        let name = self
            .draft(guild, role)
            .map(|draft| draft.name)
            .unwrap_or_default();
        let field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_text(&name);
            editor
        });
        let events = cx.subscribe(&field, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let text = editor.read(cx).text().to_owned();
                    this.edit_draft(guild, role, |draft| draft.name = text, cx);
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => this.server_settings = None,
            }
            cx.notify();
        });
        if let Some(open) = self.server_settings.as_mut() {
            open.role = Some(role);
            if let Some(state) = open.roles.as_mut() {
                state.name = Some((role, field, events));
                // @everyone opens on its permissions, as in the capture.
                state.tab = if role == guild {
                    RoleTab::Permissions
                } else {
                    RoleTab::Display
                };
            }
        }
        self.request_role_members(guild, role);
        cx.notify();
    }

    fn role_dot(value: u32, size: f32) -> gpui::Div {
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .bg(rgb(if value == 0 { NO_COLOR } else { value }))
    }

    fn secondary_square(id: SharedString, icon_name: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(icon_name, 20., "--control-secondary-text-default"))
    }

    fn role_search_field(field: Entity<Editor>, width: Option<f32>) -> gpui::Div {
        div()
            .when_some(width, |this, width| this.w(px(width)))
            .when(width.is_none(), |this| this.flex_1())
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .pl(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("picker_search", 16., "--input-text-default")),
            )
            .child(div().flex_1().min_w(px(0.)).pr(px(12.)).child(field))
    }

    /// `Eb`: heading, Default Permissions, search with Create Role, the help line, then the table.
    pub(super) fn server_roles_list(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
        else {
            return div().into_any_element();
        };
        let query = state.list_search.read(cx).text().trim().to_lowercase();
        let counts = self
            .fetched
            .get(&format!("{COUNTS_PREFIX}{guild}"))
            .cloned()
            .unwrap_or_default();
        let roles: Vec<Value> = self
            .roles_of(guild)
            .into_iter()
            .filter(|role| role["id"].as_str() != Some(guild.to_string().as_str()))
            .filter(|role| {
                role["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        let total = roles.len();
        let rows: Vec<AnyElement> = roles
            .iter()
            .map(|role| {
                let id: Id = role["id"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or(0);
                let draft = Draft::from_role(role);
                let count = counts[id.to_string()].as_u64().unwrap_or(0);
                div()
                    .id(SharedString::from(format!("server-role-row-{id}")))
                    .group(SharedString::from(format!("server-role-row-{id}")))
                    .ml(px(-32.))
                    .w(px(700.))
                    .h(px(61.))
                    .pt(px(1.))
                    .pr(px(8.))
                    .rounded(px(4.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(32.))
                            .flex_none()
                            .pl(px(8.))
                            .opacity(0.)
                            .group_hover(
                                SharedString::from(format!("server-role-row-{id}")),
                                |style| style.opacity(1.),
                            )
                            .child(icon("role_drag", 16., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .w(px(273.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                gpui::svg()
                                    .path("icons/role_shield.svg")
                                    .size(px(24.))
                                    .text_color(rgb(if draft.color == 0 {
                                        NO_COLOR
                                    } else {
                                        draft.color
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(draft.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .w(px(112.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .items_center()
                            .gap(px(4.))
                            .text_color(color("--interactive-text-default"))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .child(count.to_string()),
                            )
                            .child(icon("role_person", 20., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .flex_1()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                Self::secondary_square(
                                    SharedString::from(format!("server-role-edit-{id}")),
                                    "role_edit",
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.select_role(id, cx)),
                                ),
                            )
                            // The row's "..." opens the same menu as the editor's: Duplicate Role, Delete.
                            .child(
                                div()
                                    .relative()
                                    .child(self.anchor_probe(format!("server-role-more-{id}")))
                                    .child(
                                        Self::secondary_square(
                                            SharedString::from(format!("server-role-more-{id}")),
                                            "role_more",
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if let Some(state) = this
                                                    .server_settings
                                                    .as_mut()
                                                    .and_then(|open| open.roles.as_mut())
                                                {
                                                    state.menu_anchor =
                                                        format!("server-role-more-{id}").into();
                                                    state.menu = Some(id);
                                                }
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_role(id, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .pt(px(32.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Roles"),
            )
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Use roles to group your server members and assign permissions."),
            )
            .child(self.default_permissions_card(guild, cx))
            .child(
                div()
                    .mt(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(Self::role_search_field(state.list_search.clone(), Some(550.)))
                    .child(
                        div()
                            .id("server-roles-create-small")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Create Role")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.create_role(guild);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().mt(px(8.)).text_size(px(14.)).line_height(px(18.)).text_color(color("--text-default")).child(self.text_with_link(
                    "server-roles-help",
                    "Members use the color of the highest role they have on this list. Drag roles to reorder them. ",
                    "Need help with permissions?",
                    "",
                    cx,
                    |_, cx| cx.open_url(super::channel_settings_pages::PERMISSIONS_HELP),
                )),
            )
            .child(
                div()
                    .mt(px(32.))
                    .mb(px(8.))
                    .flex()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(div().w(px(273.)).flex_none().child(format!("Roles \u{2013} {total}")))
                    .child(div().ml(px(16.)).w(px(112.)).flex_none().flex().justify_end().child("Members")),
            )
            .children(rows)
            .into_any_element()
    }

    /// `En`: the role column, then the editor for the selected role.
    pub(super) fn server_role_editor(
        &self,
        guild: Id,
        role: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
        else {
            return div().into_any_element();
        };
        let tab = state.tab;
        let draft = self.draft(guild, role).unwrap_or(Draft {
            name: String::new(),
            color: 0,
            hoist: false,
            mentionable: false,
            permissions: 0,
        });
        let everyone = role == guild;
        let roles = self.roles_of(guild);
        let column_rows: Vec<AnyElement> = roles
            .iter()
            .map(|value| {
                let id: Id = value["id"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or(0);
                let shown = self
                    .draft(guild, id)
                    .unwrap_or_else(|| Draft::from_role(value));
                let selected = id == role;
                div()
                    .id(SharedString::from(format!("server-role-column-{id}")))
                    .flex_none()
                    .h(px(34.))
                    .mb(px(2.))
                    .ml(px(-6.))
                    .px(px(14.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected, |this| this.bg(color("--background-mod-strong")))
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--background-mod-subtle")))
                    })
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Self::role_dot(shown.color, 12.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(shown.name),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_role(id, cx)))
                    .into_any_element()
            })
            .collect();
        let column = div()
            .w(px(232.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(60.))
                    .pb(px(16.))
                    .px(px(24.))
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("server-role-back")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_color(color("--text-default"))
                            .child(icon("role_back", 24., "--text-default"))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .child("Back"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.server_settings.as_mut() {
                                    open.role = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("server-role-add")
                            .cursor_pointer()
                            .child(icon("role_add", 20., "--text-strong"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.create_role(guild);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .pl(px(40.))
                    .pr(px(24.))
                    .flex()
                    .flex_col()
                    .children(column_rows),
            );
        let tab_item = |label: String, which: RoleTab, cx: &mut Context<Self>| {
            let selected = tab == which;
            div()
                .id(SharedString::from(format!("server-role-tab-{which:?}")))
                .h(px(31.))
                .cursor_pointer()
                .border_b_2()
                .border_color(if selected {
                    color("--text-brand")
                } else {
                    gpui::transparent_black().into()
                })
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(if selected {
                    color("--text-brand")
                } else {
                    color("--interactive-text-default")
                })
                .when(!selected, |this| {
                    this.hover(|style| style.text_color(color("--interactive-text-hover")))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this
                        .server_settings
                        .as_mut()
                        .and_then(|open| open.roles.as_mut())
                    {
                        state.tab = which;
                    }
                    cx.notify();
                }))
        };
        let members = self.role_members(guild, role);
        let tabs = div()
            .mb(px(-2.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .gap(px(56.))
            .child(tab_item("Display".to_owned(), RoleTab::Display, cx))
            .child(tab_item("Permissions".to_owned(), RoleTab::Permissions, cx))
            .child(tab_item("Links".to_owned(), RoleTab::Links, cx))
            .child(tab_item(
                if everyone {
                    "Manage Members".to_owned()
                } else {
                    format!("Manage Members ({})", members.len())
                },
                RoleTab::Members,
                cx,
            ));
        let body = match tab {
            RoleTab::Display => self.role_display(guild, role, &draft, everyone, cx),
            RoleTab::Permissions => self.role_permissions(guild, role, &draft, cx),
            RoleTab::Links => self.role_links(cx),
            RoleTab::Members => self.role_members_tab(guild, role, &members, cx),
        };
        div()
            .size_full()
            .flex()
            .child(column)
            .child(
                div()
                    .id("server-role-content")
                    .w(px(508.))
                    .h_full()
                    .pl(px(24.))
                    .pr(px(42.))
                    .pb(px(60.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pt(px(60.))
                            .child(
                                div()
                                    .h(px(25.))
                                    .mb(px(24.))
                                    .flex()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(format!("Edit Role \u{2014} {}", draft.name)),
                                    )
                                    .when(!everyone, |this| {
                                        this.child(
                                            div()
                                                .id("server-role-menu")
                                                .relative()
                                                .child(self.anchor_probe("server-role-menu"))
                                                .cursor_pointer()
                                                .child(icon("role_menu", 24., "--text-strong"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(state) = this
                                                        .server_settings
                                                        .as_mut()
                                                        .and_then(|open| open.roles.as_mut())
                                                    {
                                                        state.menu_anchor =
                                                            "server-role-menu".into();
                                                        state.menu = if state.menu.is_some() {
                                                            None
                                                        } else {
                                                            Some(role)
                                                        };
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            )
                            .child(div().h(px(22.)))
                            .child(tabs),
                    )
                    .child(div().pt(px(15.)).child(body)),
            )
            .into_any_element()
    }

    /// Module 989433 `hK`: a copy named "{name} copy" (numbered until free), with the role's settings.
    fn duplicate_role(&mut self, guild: Id, role: Id) {
        let Some(source) = self.role_value(guild, role) else {
            return;
        };
        let names: Vec<String> = self
            .roles_of(guild)
            .iter()
            .filter_map(|known| known["name"].as_str().map(str::to_owned))
            .collect();
        let mut name = source["name"].as_str().unwrap_or_default().to_owned();
        loop {
            name = format!("{name} copy");
            if !names.contains(&name) {
                break;
            }
        }
        let color = source["color"].as_u64().unwrap_or(0);
        let colors = source["colors"].clone();
        let mut held = source;
        held["name"] = Value::String(name.clone());
        if let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        {
            state.duplicating = Some(held);
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{}{guild}", super::server_roles::ROLE_CREATED_PREFIX)),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/roles"),
            body: Some(serde_json::json!({"name": name, "color": color, "colors": colors, "permissions": "0"})),
        });
    }

    /// The copy exists: it takes the source's settings (`updateRole`). True when this creation was a duplicate.
    pub(super) fn finish_duplicate(&mut self, guild: Id, created: Id) -> bool {
        let Some(held) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
            .and_then(|state| state.duplicating.take())
        else {
            return false;
        };
        let body = Draft::from_role(&held).body(&held);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SAVED_PREFIX}{guild}")),
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/roles/{created}"),
            body: Some(body),
        });
        true
    }

    /// The editor's "..." (module 396704): Duplicate Role, then Delete under a separator.
    pub(super) fn role_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = open.roles.as_ref()?;
        let role = state.menu?;
        let guild = open.guild;
        let anchor = self.tip_anchors.borrow().get(&state.menu_anchor).copied()?;
        let item =
            |id: &'static str, label: &'static str, icon_name: &'static str, tint: &'static str| {
                div()
                    .id(id)
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(icon_name, 20., tint))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(if tint == "--text-feedback-critical" {
                                color(tint)
                            } else {
                                color("--text-default")
                            })
                            .child(label),
                    )
            };
        Some(
            gpui::deferred(
                div()
                    .id("server-role-menu-layer")
                    .occlude()
                    .absolute()
                    .left(anchor.left() + px(12.))
                    .top(anchor.top() + px(11.))
                    .w(px(182.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.roles.as_mut())
                        {
                            state.menu = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        item(
                            "server-role-duplicate",
                            "Duplicate Role",
                            "role_duplicate",
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(state) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.roles.as_mut())
                            {
                                state.menu = None;
                            }
                            this.duplicate_role(guild, role);
                            cx.notify();
                        })),
                    )
                    .child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        item(
                            "server-role-delete",
                            "Delete",
                            "role_delete",
                            "--text-feedback-critical",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(state) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.roles.as_mut())
                            {
                                state.menu = None;
                                state.confirm_delete = Some(role);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// "Delete Role": the name in bold, then `DELETE /guilds/{id}/roles/{role}` (module 778858 `deleteRole`).
    pub(super) fn role_delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let role = open.roles.as_ref()?.confirm_delete?;
        let guild = open.guild;
        let name = self.draft(guild, role)?.name;
        let close = |this: &mut Root| {
            if let Some(state) = this
                .server_settings
                .as_mut()
                .and_then(|open| open.roles.as_mut())
            {
                state.confirm_delete = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "server-role-delete-modal",
                title: "Delete Role".into(),
                subtitle: Some({
                    // `9+nrUS`: the name in bold inside the sentence.
                    let (before, after) = (
                        "Are you sure you want to delete the ",
                        " role? This action cannot be undone.",
                    );
                    let bold = before.len()..before.len() + name.len();
                    gpui::StyledText::new(format!("{before}{name}{after}"))
                        .with_highlights([(
                            bold,
                            gpui::HighlightStyle {
                                font_weight: Some(FontWeight(600.)),
                                ..Default::default()
                            },
                        )])
                        .into_any_element()
                }),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "server-role-delete-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "server-role-delete-confirm",
                        "Delete",
                        super::modal::Tone::Critical,
                        move |this, _, cx| {
                            let _ = this.requests.send(Request::Rest {
                                key: Some(format!("{SAVED_PREFIX}{guild}")),
                                method: reqwest::Method::DELETE,
                                route: format!("/guilds/{guild}/roles/{role}"),
                                body: None,
                            });
                            if let Some(list) = this
                                .fetched
                                .get_mut(&format!("{ROLES_PREFIX}{guild}"))
                                .and_then(|body| body.as_array_mut())
                            {
                                list.retain(|known| {
                                    known["id"].as_str() != Some(role.to_string().as_str())
                                });
                            }
                            if let Some(open) = this.server_settings.as_mut() {
                                open.role = None;
                                if let Some(state) = open.roles.as_mut() {
                                    state.drafts.remove(&role);
                                }
                            }
                            close(this);
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    fn field_title(text: &'static str, required: bool) -> gpui::Div {
        div()
            .mb(px(8.))
            .flex()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(text)
            .when(required, |this| {
                this.child(
                    div()
                        .font_weight(FontWeight(400.))
                        .text_color(color("--text-feedback-critical"))
                        .child("*"),
                )
            })
    }

    fn toggle_row(
        id: &'static str,
        title: &'static str,
        note: Option<&'static str>,
        on: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .py(px(24.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .gap(px(24.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .children(note.map(|note| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(note)
                    })),
            )
            .child(
                div()
                    .id(id)
                    .cursor_pointer()
                    .child(super::create_channel::switch(on))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        run(this, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Display (`xG`): Role name, Role color (default, then the 20 swatches), the two switches.
    fn role_display(
        &self,
        guild: Id,
        role: Id,
        draft: &Draft,
        everyone: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
            .and_then(|state| state.name.as_ref())
            .filter(|(id, _, _)| *id == role)
            .map(|(_, field, _)| field.clone());
        let swatch = |value: u32, size: f32, cx: &mut Context<Self>| {
            let selected = draft.color == value;
            div()
                .id(SharedString::from(format!("server-role-color-{value}")))
                .size(px(size))
                .rounded(px(4.))
                .bg(rgb(if value == 0 { NO_COLOR } else { value }))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |this| {
                    this.child(icon(
                        "select_check",
                        if size > 30. { 32. } else { 16. },
                        "--white",
                    ))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_draft(guild, role, |draft| draft.color = value, cx)
                }))
        };
        let mut swatch_rows = div().flex().flex_col().gap(px(10.));
        for row in SWATCHES.chunks(10) {
            swatch_rows = swatch_rows.child(
                div()
                    .flex()
                    .gap(px(10.))
                    .children(row.iter().map(|value| swatch(*value, 20., cx))),
            );
        }
        let hoist = draft.hoist;
        let mentionable = draft.mentionable;
        div()
            .pt(px(17.))
            .flex()
            .flex_col()
            .when(!everyone, |this| {
                this.child(Self::field_title("Role name", true))
                    .child(
                        div()
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .child(div().w_full().children(field)),
                    )
                    .child(div().my(px(24.)).h(px(1.)).bg(color("--border-subtle")))
                    .child(Self::field_title("Role color", true))
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("Members use the color of the highest role they have on the roles list."),
                    )
                    .child(div().flex().gap(px(16.)).child(swatch(0, 66., cx).w(px(66.)).h(px(50.))).child(swatch_rows))
                    .child(div().mt(px(24.)).h(px(1.)).bg(color("--border-subtle")))
                    .child(Self::toggle_row("server-role-hoist", "Display role members separately from online members", None, hoist, cx, move |this, cx| {
                        this.edit_draft(guild, role, |draft| draft.hoist = !draft.hoist, cx)
                    }))
                    .child(Self::toggle_row(
                        "server-role-mentionable",
                        "Allow anyone to @mention this role",
                        Some("Note: Members with the \"Mention @everyone, @here, and All Roles\" permission will always be able to ping this role."),
                        mentionable,
                        cx,
                        move |this, cx| this.edit_draft(guild, role, |draft| draft.mentionable = !draft.mentionable, cx),
                    ))
            })
            .into_any_element()
    }

    /// Permissions (`xQ`): the search, then each section's switches; Clear permissions empties them.
    fn role_permissions(
        &self,
        guild: Id,
        role: Id,
        draft: &Draft,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
        else {
            return div().into_any_element();
        };
        let query = state
            .perm_search
            .read(cx)
            .text()
            .trim_start()
            .to_lowercase();
        let mut list = div()
            .flex()
            .flex_col()
            .child(Self::role_search_field(state.perm_search.clone(), None));
        for (index, section) in GUILD.iter().enumerate() {
            let rows: Vec<_> = section
                .permissions
                .iter()
                .filter(|permission| {
                    permission.title.to_lowercase().contains(&query)
                        || permission.description.to_lowercase().contains(&query)
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            list = list.child(
                div()
                    .mt(px(32.))
                    .mb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(section.title),
                    )
                    .when(index == 0, |this| {
                        this.child(
                            div()
                                .id("server-role-clear-permissions")
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-link"))
                                .opacity(if draft.permissions == 0 { 0.5 } else { 1. })
                                .child("Clear permissions")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.edit_draft(guild, role, |draft| draft.permissions = 0, cx)
                                })),
                        )
                    }),
            );
            for permission in rows {
                let flag = permission.flag;
                let on = draft.permissions & flag == flag;
                list = list.child(
                    div()
                        .pb(px(24.))
                        .mb(px(24.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .gap(px(24.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(permission.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-subtle"))
                                        .child(permission.description),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("server-role-permission-{flag}")))
                                .cursor_pointer()
                                .child(super::create_channel::switch(on))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.edit_draft(
                                        guild,
                                        role,
                                        |draft| draft.permissions ^= flag,
                                        cx,
                                    );
                                })),
                        ),
                );
            }
        }
        let _ = ADMINISTRATOR;
        list.into_any_element()
    }

    /// Links (module 989433 `h1`): Create Linked Role over Add requirement. The requirement's connections grid
    /// (`CONNECTIONS_GRID_MODAL_SHOW`) is not built yet.
    fn role_links(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .pt(px(17.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Create Linked Role"),
            )
            .child(
                div().mb(px(16.)).text_size(px(14.)).line_height(px(18.)).text_color(color("--text-default")).child(self.text_with_link(
                    "server-role-links-help",
                    "Linking a connection or app requires members to connect their accounts before being added to the role. ",
                    "Learn more about Linked Roles",
                    ".",
                    cx,
                    // `MVz.CONNECTION_DETAILS_ADMIN`.
                    |_, cx| cx.open_url("https://support.discord.com/hc/en-us/articles/10388356626711"),
                )),
            )
            .child(
                div()
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-primary-border-default"))
                    .bg(color("--control-primary-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child("Add requirement"),
            )
            .into_any_element()
    }

    /// Opens "Add members" (`bulkAddMemberRoles`' dialog).
    fn open_add_members(&mut self, cx: &mut Context<Self>) -> Option<Entity<Editor>> {
        let field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Search members".to_owned());
            editor
        });
        let events = cx.subscribe(&field, |this, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Cancel)
                && let Some(state) = this
                    .server_settings
                    .as_mut()
                    .and_then(|open| open.roles.as_mut())
            {
                state.adding = None;
            }
            cx.notify();
        });
        let state = self.server_settings.as_mut()?.roles.as_mut()?;
        state.adding = Some((field.clone(), Vec::new(), events));
        cx.notify();
        Some(field)
    }

    /// The server's members this client knows who do not have the role, matching the search.
    fn add_member_candidates(&self, guild: Id, role: Id, query: &str) -> Vec<dc_state::User> {
        let having: Vec<Id> = self
            .role_members(guild, role)
            .iter()
            .map(|user| user.id)
            .collect();
        let mut users: Vec<dc_state::User> = self.stores.current_user.iter().cloned().collect();
        if let Some(list) = self.stores.member_lists.get(&guild) {
            for row in &list.rows {
                if let dc_state::member_list::Row::Member { user, .. } = row
                    && !users.iter().any(|known| known.id == user.id)
                {
                    users.push(user.clone());
                }
            }
        }
        let query = query.trim().to_lowercase();
        users
            .into_iter()
            .filter(|user| !having.contains(&user.id))
            .filter(|user| {
                user.display_name().to_lowercase().contains(&query)
                    || user.username.to_lowercase().contains(&query)
            })
            .collect()
    }

    /// "Add members" (`design/dom/modals/role-add-members-darker-cozy.json`): up to 30 ticked members, then
    /// `PATCH /guilds/{id}/roles/{role}/members` with their ids.
    pub(super) fn role_add_members_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = open.roles.as_ref()?;
        let (field, picked, _) = state.adding.as_ref()?;
        let guild = open.guild;
        let role = open.role?;
        let role_name = self.draft(guild, role)?.name;
        let query = field.read(cx).text().to_owned();
        let rows: Vec<AnyElement> =
            self.add_member_candidates(guild, role, &query)
                .into_iter()
                .map(|user| {
                    let id = user.id;
                    let ticked = picked.contains(&id);
                    let picture = self.images.get(&super::avatar_source(&user).0).cloned();
                    div()
                        .id(SharedString::from(format!("server-role-candidate-{id}")))
                        .flex_none()
                        .h(px(40.))
                        .pl(px(14.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .size(px(18.))
                                .flex_none()
                                .rounded(px(4.))
                                .border_1()
                                .when(ticked, |this| {
                                    this.border_color(gpui::rgba(0x97979f1f))
                                        .bg(color("--control-primary-background-default"))
                                })
                                .when(!ticked, |this| {
                                    this.border_color(gpui::rgba(0x97979fa3))
                                        .bg(gpui::rgba(0x00000014))
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(ticked, |this| {
                                    this.child(icon("checkbox_check", 16., "--white"))
                                }),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .size(px(24.))
                                .flex_none()
                                .rounded_full()
                                .children(picture.map(|picture| {
                                    gpui::img(picture).size(px(24.)).rounded_full()
                                })),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-strong"))
                                .child(user.display_name().to_owned()),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(user.username.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((_, picked, _)) = this
                                .server_settings
                                .as_mut()
                                .and_then(|open| open.roles.as_mut())
                                .and_then(|state| state.adding.as_mut())
                            {
                                if let Some(at) = picked.iter().position(|known| *known == id) {
                                    picked.remove(at);
                                } else if picked.len() < 30 {
                                    picked.push(id);
                                }
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect();
        let close = |this: &mut Root| {
            if let Some(state) = this
                .server_settings
                .as_mut()
                .and_then(|open| open.roles.as_mut())
            {
                state.adding = None;
            }
        };
        let tone = if picked.is_empty() {
            super::modal::Tone::PrimaryDisabled
        } else {
            super::modal::Tone::Primary
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "server-role-add-members",
                title: "Add members".into(),
                subtitle: Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .child("Select up to 30 members to add to role\u{a0}")
                        .child(div().font_weight(FontWeight(600.)).child(role_name))
                        .into_any_element(),
                ),
                body: Some(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(36.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .pl(px(10.))
                                .pr(px(8.))
                                .flex()
                                .items_center()
                                .child(div().w_full().child(field.clone())),
                        )
                        .child(
                            div()
                                .id("server-role-candidates")
                                .h(px(381.))
                                .pt(px(12.))
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex_none()
                                        .h(px(32.))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child("Members"),
                                )
                                .children(rows),
                        )
                        .into_any_element(),
                ),
                buttons: vec![
                    super::modal::button(
                        "server-role-add-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "server-role-add-confirm",
                        "Add",
                        tone,
                        move |this, _, cx| {
                            let picked: Vec<String> = this
                                .server_settings
                                .as_ref()
                                .and_then(|open| open.roles.as_ref())
                                .and_then(|state| state.adding.as_ref())
                                .map(|(_, picked, _)| {
                                    picked.iter().map(|id| id.to_string()).collect()
                                })
                                .unwrap_or_default();
                            let _ = this.requests.send(Request::Rest {
                                key: Some(format!("{SAVED_PREFIX}{guild}")),
                                method: reqwest::Method::PATCH,
                                route: format!("/guilds/{guild}/roles/{role}/members"),
                                body: Some(serde_json::json!({"member_ids": picked})),
                            });
                            close(this);
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// Members with this role (`GET /guilds/{id}/roles/{role}/member-ids`), as users this client knows.
    fn role_members(&self, guild: Id, role: Id) -> Vec<dc_state::User> {
        let ids: Vec<Id> = self
            .fetched
            .get(&format!("{MEMBER_IDS_PREFIX}{guild}:{role}"))
            .and_then(|body| body.as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().and_then(|id| id.parse().ok()))
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                self.stores.users.get(&id).cloned().or_else(|| {
                    self.stores
                        .current_user
                        .clone()
                        .filter(|user| user.id == id)
                })
            })
            .collect()
    }

    fn request_role_members(&mut self, guild: Id, role: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{MEMBER_IDS_PREFIX}{guild}:{role}"),
            route: format!("/guilds/{guild}/roles/{role}/member-ids"),
        });
    }

    /// Manage Members (`xz`): search, Add Members, the members with the role or the empty line.
    fn role_members_tab(
        &self,
        guild: Id,
        role: Id,
        members: &[dc_state::User],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
        else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(Self::role_search_field(state.member_search.clone(), None))
                    .child(
                        div()
                            .id("server-role-add-members")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Add Members")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(field) = this.open_add_members(cx) {
                                    let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
                                    window.focus(&handle, cx);
                                }
                            })),
                    ),
            )
            .when(members.is_empty(), |this| {
                this.child(
                    div()
                        .mt(px(32.))
                        .ml(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(icon("roles_group", 20., "--text-muted"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(self.text_with_link(
                                    "server-role-members-empty",
                                    "No members were found. ",
                                    "Add members to this role.",
                                    "",
                                    cx,
                                    move |this, cx| {
                                        this.open_add_members(cx);
                                    },
                                )),
                        ),
                )
            })
            .children(members.iter().enumerate().map(|(index, user)| {
                let id = user.id;
                let picture = self.images.get(&super::avatar_source(user).0).cloned();
                // `memberRow_mssHMP`: 24px under the search, a 24px avatar, name, tag, and the X on hover.
                div()
                    .when(index == 0, |this| this.mt(px(24.)))
                    .h(px(40.))
                    .pl(px(8.))
                    .rounded(px(4.))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .child(div().size(px(24.)).flex_none().rounded_full().children(
                        picture.map(|picture| gpui::img(picture).size(px(24.)).rounded_full()),
                    ))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .ml(px(4.))
                            .flex_1()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(user.username.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("server-role-remove-{id}")))
                            .mr(px(8.))
                            .cursor_pointer()
                            .child(icon(
                                "role_member_remove",
                                16.,
                                "--interactive-text-default",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // Module 727419: a member's roles are replaced whole, so read them first.
                                let _ = this.requests.send(Request::Fetch {
                                    key: format!("{STRIP_PREFIX}{guild}:{role}:{id}"),
                                    route: format!("/guilds/{guild}/members/{id}"),
                                });
                                cx.notify();
                            })),
                    )
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_read_and_write_like_update_role() {
        let role = serde_json::json!({"id": "1", "name": "new role", "color": 0, "hoist": false, "mentionable": true, "permissions": "1024", "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null}});
        let mut draft = Draft::from_role(&role);
        assert_eq!(draft.permissions, 1024);
        draft.color = SWATCHES[0];
        let body = draft.body(&role);
        assert_eq!(body["permissions"], "1024");
        assert_eq!(body["colors"]["primary_color"], 1752220);
        assert_eq!(body["mentionable"], true);
        assert_eq!(
            GUILD
                .iter()
                .map(|section| section.permissions.len())
                .sum::<usize>(),
            49
        );
    }
}
