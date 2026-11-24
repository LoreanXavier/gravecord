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
/// `image_H8y-o7` under `images-dark`: the permission search's empty picture.
pub(super) const NO_PERMISSIONS_ART: &str = "https://discord.com/assets/ab743e418ef21ecc.svg";
const COUNTS_PREFIX: &str = "guild-roles:counts:";
const SAVED_PREFIX: &str = "guild-roles:saved:";
const MEMBER_IDS_PREFIX: &str = "guild-roles:members:";
const STRIP_PREFIX: &str = "guild-roles:strip:";
/// Module 652215 `_tK`: the role colour swatches, the light row then the dark one.
const SWATCHES: [u32; 20] = [
    1752220, 3066993, 3447003, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 9807270, 6323595,
    1146986, 2067276, 2123412, 7419530, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 9936031, 5533306,
];
/// A role without a colour shows `rgb(153, 170, 181)` (`roleCircle_7RsUnm`); module 652215 `TGz`, the default.
const NO_COLOR: u32 = 0x99aab5;
const ADMINISTRATOR: u64 = 8;
/// Module 178758 `Oz`: the gradient presets (fire, golddust, emerald, diamond, blurple, cupid, rosegold, desert, moss,
/// platinum, plum, rose), the first six in the top row.
const GRADIENT_PRESETS: [[u32; 2]; 12] = [
    [0xaa3b3b, 0xff4848],
    [0xd4843d, 0xffde90],
    [3577974, 7471006],
    [5025232, 0xb2f9ff],
    [0x9e6bff, 0x9fc1ff],
    [0xff5dd6, 0xff9cbf],
    [9467253, 0xffcec6],
    [0x9b8666, 0xffe1b4],
    [7640178, 0xb3e1b9],
    [8886702, 0xf1f7ff],
    [8547266, 0xb7a7f1],
    [9854847, 0xffaadc],
];
/// Module 178758 `lO`: the Gradient style's own colours.
const GRADIENT_DEFAULT: [u32; 2] = [0x9e6bff, 0x9fc1ff];
/// Module 178758 `jl`: the Holographic style's colours (they cannot be picked).
const HOLOGRAPHIC: [u32; 3] = [0xa9c9ff, 0xffbbec, 0xffc3a0];
/// Module 144273: the preview message's avatar in the style cards.
const PREVIEW_AVATAR: &str = "https://cdn.discordapp.com/assets/content/2532e8cd0600ccfbc61ff81c21959bde90a6645d0e71644a92f6cde5bfc48439.png";
const ENHANCED_ROLE_COLORS: &str = "ENHANCED_ROLE_COLORS";

/// The role styles of Enhanced Role Styles (store 396816 `y`, the cards of module 512031 `wE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleStyle {
    Solid,
    Gradient,
    Holographic,
}

impl RoleStyle {
    const ALL: [RoleStyle; 3] = [
        RoleStyle::Solid,
        RoleStyle::Gradient,
        RoleStyle::Holographic,
    ];

    /// `8Qyahn`, `XpWmJz`, `QTKppe`.
    fn label(self) -> &'static str {
        dc_intl::t(match self {
            RoleStyle::Solid => "8Qyahn",
            RoleStyle::Gradient => "XpWmJz",
            RoleStyle::Holographic => "QTKppe",
        })
    }

    /// The card's preview colours (`wE`): the default colour, `lO`, `jl`.
    fn preview(self) -> dc_state::RoleColors {
        match self {
            RoleStyle::Solid => dc_state::RoleColors {
                primary: NO_COLOR,
                secondary: None,
                tertiary: None,
            },
            RoleStyle::Gradient => dc_state::RoleColors {
                primary: GRADIENT_DEFAULT[0],
                secondary: Some(GRADIENT_DEFAULT[1]),
                tertiary: None,
            },
            RoleStyle::Holographic => dc_state::RoleColors {
                primary: HOLOGRAPHIC[0],
                secondary: Some(HOLOGRAPHIC[1]),
                tertiary: Some(HOLOGRAPHIC[2]),
            },
        }
    }
}

/// What each style held last for a role, so switching back restores it (store 396816 `styleColors`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StyleColors {
    solid: u32,
    gradient: [u32; 2],
}

impl StyleColors {
    /// Store 396816 `z`: every style on its default (`TGz`, `lO`), the role's own style on the role's colours.
    fn seed(draft: &Draft, enhanced: bool) -> Self {
        let style = draft.style(enhanced);
        StyleColors {
            solid: if style == RoleStyle::Solid {
                draft.color
            } else {
                NO_COLOR
            },
            gradient: if style == RoleStyle::Gradient {
                draft.gradient_pair()
            } else {
                GRADIENT_DEFAULT
            },
        }
    }
}

/// `OK` of module 317097: how dark a colour is (1 minus its luma).
fn darkness(value: u32) -> f32 {
    let r = ((value >> 16) & 0xff) as f32;
    let g = ((value >> 8) & 0xff) as f32;
    let b = (value & 0xff) as f32;
    1. - (0.299 * r + 0.587 * g + 0.114 * b) / 255.
}

/// Module 508274 `L`/`sk` and module 48736 `q`: the Custom Color button shows the role's colour when it is neither
/// the default nor one of the swatches.
fn custom_color(value: u32) -> Option<u32> {
    (value != 0 && value != NO_COLOR && !SWATCHES.contains(&value)).then_some(value)
}

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
    /// `color`, which is also `colors.primary_color`.
    pub color: u32,
    /// `colors.secondary_color`: a gradient's second stop (and the holographic style's second colour).
    pub secondary: Option<u32>,
    /// `colors.tertiary_color`: only the holographic style has one.
    pub tertiary: Option<u32>,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: u64,
}

/// `hZ`'s `j`: the count as it is under 10,000, else compact ("12K", `Intl.NumberFormat` `compact`/`short`).
fn member_count_label(count: u64) -> String {
    if count < 10_000 {
        return count.to_string();
    }
    let (value, unit) = if count >= 1_000_000 {
        (count as f64 / 1_000_000., "M")
    } else {
        (count as f64 / 1_000., "K")
    };
    // Two significant digits, as `compact` rounds: one decimal under 10, none above.
    let shown = if value < 10. {
        let rounded = (value * 10.).round() / 10.;
        if rounded.fract() == 0. {
            format!("{rounded:.0}")
        } else {
            format!("{rounded:.1}")
        }
    } else {
        format!("{:.0}", value.round())
    };
    format!("{shown}{unit}")
}

/// What `hZ` of module 989433 says over a managed role.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ManagedNotice {
    /// `FuXsWm`: the integration's name (Twitch and YouTube ones prefixed), linking to its app's page.
    Integration { name: String, app: String },
    /// `oF6FYT`: the Server Booster role.
    Booster,
    /// `ZQ37tH`: a role given by linked connections.
    Connections,
    /// `k5d7DJ`: any other managed role.
    Other,
}

/// `hZ`'s `m`: nothing for a role that is not managed; else the integration whose bot (`tags.bot_id`) or id
/// (`tags.integration_id`) it is, when it has an application and a name, else what the role's tags say.
pub(super) fn managed_notice(role: &Value, integrations: &Value) -> Option<ManagedNotice> {
    if role["managed"] != true {
        return None;
    }
    let tags = &role["tags"];
    let list = integrations
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let found = if let Some(bot) = tags["bot_id"].as_str() {
        list.iter()
            .find(|item| item["application"]["bot"]["id"].as_str() == Some(bot))
    } else if let Some(id) = tags["integration_id"].as_str() {
        list.iter().find(|item| item["id"].as_str() == Some(id))
    } else {
        None
    };
    if let Some(item) = found
        && let (Some(app), Some(name)) = (item["application"]["id"].as_str(), item["name"].as_str())
    {
        let name = match item["type"].as_str() {
            Some("twitch") => format!("Twitch - {name}"),
            Some("youtube") => format!("YouTube - {name}"),
            _ => name.to_owned(),
        };
        return Some(ManagedNotice::Integration {
            name,
            app: app.to_owned(),
        });
    }
    Some(if tags.get("premium_subscriber").is_some() {
        ManagedNotice::Booster
    } else if tags.get("guild_connections").is_some() {
        ManagedNotice::Connections
    } else {
        ManagedNotice::Other
    })
}

/// `AT` of module 7864 for the list: highest position first, then the lower id.
fn sort_roles(roles: &mut [Value]) {
    let id = |role: &Value| {
        role["id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
            .unwrap_or(0)
    };
    roles.sort_by(|a, b| {
        let position = |role: &Value| role["position"].as_i64().unwrap_or(0);
        position(b).cmp(&position(a)).then(id(a).cmp(&id(b)))
    });
}

/// The role store's roles as the API spells a role: the fields the list and the editor read.
fn store_roles(known: &dc_state::Guild) -> Vec<Value> {
    known
        .roles
        .iter()
        .map(|(id, name)| {
            let info = known.role_info.get(id).copied().unwrap_or_default();
            let permissions = known.role_permissions.get(id).copied().unwrap_or(0);
            serde_json::json!({
                "id": id.to_string(),
                "name": name,
                "position": info.position,
                "color": info.color,
                "colors": {
                    "primary_color": info.color,
                    "secondary_color": info.secondary_color,
                    "tertiary_color": info.tertiary_color,
                },
                "hoist": info.hoist,
                "mentionable": info.mentionable,
                "managed": info.managed,
                "permissions": permissions.to_string(),
            })
        })
        .collect()
}

impl Draft {
    fn from_role(role: &Value) -> Self {
        let colors = &role["colors"];
        let part = |key: &str| colors[key].as_u64().map(|value| value as u32);
        Draft {
            name: role["name"].as_str().unwrap_or_default().to_owned(),
            color: role["color"].as_u64().unwrap_or(0) as u32,
            secondary: part("secondary_color"),
            tertiary: part("tertiary_color"),
            hoist: role["hoist"].as_bool().unwrap_or(false),
            mentionable: role["mentionable"].as_bool().unwrap_or(false),
            permissions: role["permissions"]
                .as_str()
                .and_then(|bits| bits.parse().ok())
                .unwrap_or(0),
        }
    }

    /// Store 396816 `z`: a third colour is Holographic, a second one Gradient, but only in a server with
    /// `ENHANCED_ROLE_COLORS` (module 676608); otherwise the editor shows Solid (`xG`).
    fn style(&self, enhanced: bool) -> RoleStyle {
        if !enhanced {
            RoleStyle::Solid
        } else if self.tertiary.is_some() {
            RoleStyle::Holographic
        } else if self.secondary.is_some() {
            RoleStyle::Gradient
        } else {
            RoleStyle::Solid
        }
    }

    /// The gradient strip's two stops (`xT`: `[color, colors.secondary_color ?? TGz]`).
    fn gradient_pair(&self) -> [u32; 2] {
        [self.color, self.secondary.unwrap_or(NO_COLOR)]
    }

    /// A solid colour (module 636042 `ZH`, `GUILD_SETTINGS_ROLES_UPDATE_COLOR`): the default is saved as 0, the
    /// other colours go; picking the colour the role already has changes nothing.
    fn set_solid(&mut self, value: u32) {
        let is_default = |colour: u32| colour == 0 || colour == NO_COLOR;
        if self.color == value || (is_default(value) && is_default(self.color)) {
            return;
        }
        self.color = if value == NO_COLOR { 0 } else { value };
        self.secondary = None;
        self.tertiary = None;
    }

    /// Two gradient stops (module 636042 `uc` with `{e[0], e[1], null}`).
    fn set_gradient(&mut self, pair: [u32; 2]) {
        self.color = pair[0];
        self.secondary = Some(pair[1]);
        self.tertiary = None;
    }

    /// Another style (module 636042 `C5`, `GUILD_SETTINGS_ROLES_ROLE_STYLE_UPDATE`): the style left keeps what it
    /// had, the style taken gets what it held last.
    fn switch_style(&mut self, memory: &mut StyleColors, enhanced: bool, to: RoleStyle) {
        match self.style(enhanced) {
            RoleStyle::Solid => memory.solid = self.color,
            RoleStyle::Gradient => memory.gradient = self.gradient_pair(),
            RoleStyle::Holographic => {}
        }
        match to {
            RoleStyle::Solid => {
                self.color = memory.solid;
                self.secondary = None;
                self.tertiary = None;
            }
            RoleStyle::Gradient => self.set_gradient(memory.gradient),
            RoleStyle::Holographic => {
                self.color = HOLOGRAPHIC[0];
                self.secondary = Some(HOLOGRAPHIC[1]);
                self.tertiary = Some(HOLOGRAPHIC[2]);
            }
        }
    }

    /// `updateRole`'s body for this draft (module 636042 `JY`): `color` and `colors` as the store holds them.
    fn body(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "permissions": self.permissions.to_string(),
            "color": self.color,
            "colors": self.colors_value(),
            "hoist": self.hoist,
            "mentionable": self.mentionable,
        })
    }

    fn colors_value(&self) -> Value {
        serde_json::json!({
            "primary_color": self.color,
            "secondary_color": self.secondary,
            "tertiary_color": self.tertiary,
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
    /// `a` of `E_` (module 989433): the list has been shown, and stays once the last role is gone.
    pub(super) listed: std::cell::Cell<bool>,
    /// "Add members": its search field and the members ticked so far.
    pub(super) adding: Option<(Entity<Editor>, Vec<Id>, Subscription)>,
    /// What each role's styles held last (store 396816 `styleColors`), seeded when a style first changes.
    styles: HashMap<Id, StyleColors>,
    /// The colour popout of the Custom Color button or a gradient stop (module 48736 `k`, 508274 `VN`).
    pub(super) picking: Option<super::profile_editor::colour::State>,
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
            || state.picking.take().is_some()
            || state.menu.take().is_some()
    }

    /// The server has Enhanced Role Styles (module 676608 `jV`).
    fn enhanced_role_colors(&self, guild: Id) -> bool {
        self.stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .is_some_and(|known| {
                known
                    .features
                    .iter()
                    .any(|feature| feature == ENHANCED_ROLE_COLORS)
            })
    }

    /// A Role Style card was pressed (module 989433 `x_`, then 636042 `C5`).
    fn pick_role_style(&mut self, guild: Id, role: Id, to: RoleStyle, cx: &mut Context<Self>) {
        let enhanced = self.enhanced_role_colors(guild);
        let Some(mut draft) = self.draft(guild, role) else {
            return;
        };
        let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        else {
            return;
        };
        let memory = state
            .styles
            .entry(role)
            .or_insert_with(|| StyleColors::seed(&draft, enhanced));
        draft.switch_style(memory, enhanced, to);
        state.picking = None;
        state.drafts.insert(role, draft);
        cx.notify();
    }

    /// Opens (or closes, when it is open) the colour popout of the Custom Color button or a gradient stop.
    fn toggle_role_colour(
        &mut self,
        target: super::profile_editor::colour::Target,
        initial: u32,
        cx: &mut Context<Self>,
    ) {
        let open_now = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
            .and_then(|state| state.picking.as_ref())
            .is_some_and(|picking| picking.target == target);
        let picking =
            (!open_now).then(|| super::profile_editor::colour::State::new(target, initial, cx));
        if let Some(state) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.roles.as_mut())
        {
            state.picking = picking;
        }
        cx.notify();
    }

    /// The colour popout changed a role colour: the Custom Color is a solid pick (`X`), a stop changes one colour
    /// of the pair (`Q`, then `uc`).
    pub(super) fn role_colour_picked(
        &mut self,
        target: super::profile_editor::colour::Target,
        rgb: u32,
        cx: &mut Context<Self>,
    ) {
        use super::profile_editor::colour::Target;
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let guild = open.guild;
        let Some(role) = open.role else {
            return;
        };
        match target {
            Target::RoleCustom => self.edit_draft(guild, role, |draft| draft.set_solid(rgb), cx),
            Target::RoleStop(index) => self.edit_draft(
                guild,
                role,
                |draft| {
                    let mut pair = draft.gradient_pair();
                    if let Some(stop) = pair.get_mut(index) {
                        *stop = rgb;
                    }
                    draft.set_gradient(pair);
                },
                cx,
            ),
            Target::Banner | Target::Style(_) => {}
        }
    }

    /// The open role colour popout, to the right of its button (`customPickerPosition: "right"`), and the click-away
    /// layer that closes it.
    pub(super) fn role_colour_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use super::profile_editor::colour::Target;
        let open = self.server_settings.as_ref()?;
        open.role?;
        let state = open.roles.as_ref()?;
        let picking = state.picking.as_ref()?;
        let key: SharedString = match picking.target {
            Target::RoleCustom => "server-role-colour-custom".into(),
            Target::RoleStop(index) => format!("server-role-colour-stop-{index}").into(),
            Target::Banner | Target::Style(_) => return None,
        };
        let anchor = self.tip_anchors.borrow().get(&key).copied()?;
        // GAP: the popout's 8px offset and its clamp to the window are not measured (no capture of it open).
        let left = (f32::from(anchor.right()) + 8.)
            .min(self.viewport.0 - 264.)
            .max(8.);
        let top = f32::from(anchor.top()).min(self.viewport.1 - 300.).max(8.);
        let popout = self.profile_editor_colour_popout(left, top, cx)?;
        Some(
            gpui::deferred(
                div()
                    .id("server-role-colour-away")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this
                            .server_settings
                            .as_mut()
                            .and_then(|open| open.roles.as_mut())
                        {
                            state.picking = None;
                        }
                        cx.notify();
                    }))
                    .child(popout),
            )
            .with_priority(2)
            .into_any_element(),
        )
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
                listed: std::cell::Cell::new(false),
                adding: None,
                styles: HashMap::new(),
                picking: None,
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

    /// The server's roles, highest first. Store 396816 lists `getSortedRoles` of the role store (module 317525),
    /// which READY and the role events fill; the fetched list, when there is one, carries the edits saved since.
    pub(super) fn roles_of(&self, guild: Id) -> Vec<Value> {
        let fetched = self
            .fetched
            .get(&format!("{ROLES_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned());
        let mut roles = fetched.unwrap_or_else(|| {
            self.stores
                .guild(guild)
                .map(store_roles)
                .unwrap_or_default()
        });
        sort_roles(&mut roles);
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
                body: Some(draft.body()),
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
                known["colors"] = draft.colors_value();
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
            state.styles.clear();
            state.picking = None;
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
            state.styles.clear();
            state.picking = None;
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
                state.picking = None;
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
        let last = roles.len().saturating_sub(1);
        let row_ids: Vec<Id> = roles
            .iter()
            .map(|role| {
                role["id"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or(0)
            })
            .collect();
        let rows: Vec<AnyElement> = roles
            .iter()
            .enumerate()
            .map(|(index, role)| {
                let id: Id = row_ids[index];
                let draft = Draft::from_role(role);
                let count = counts[id.to_string()].as_u64().unwrap_or(0);
                let group = SharedString::from(format!("server-role-row-{id}"));
                // `roleRow_YEKbw3:before` / `:last-child:after`: a 1px line 32px and 8px in, at the row's top (and
                // under the last row), gone while this row or the one above it is hovered.
                let line = |top: bool| {
                    let above = (top && index > 0).then(|| {
                        SharedString::from(format!("server-role-row-{}", row_ids[index - 1]))
                    });
                    div()
                        .absolute()
                        .left(px(32.))
                        .right(px(8.))
                        .h(px(1.))
                        .when(top, |line| line.top_0())
                        .when(!top, |line| line.bottom(px(-1.)))
                        .when_some(above, |line, above| {
                            line.group_hover(above, |style| style.opacity(0.))
                        })
                        .child(
                            div()
                                .size_full()
                                .bg(color("--border-subtle"))
                                .group_hover(group.clone(), |style| style.opacity(0.)),
                        )
                };
                div()
                    .id(SharedString::from(format!("server-role-row-{id}")))
                    .group(group.clone())
                    .relative()
                    .ml(px(-32.))
                    .w(px(700.))
                    .h(px(61.))
                    .pt(px(1.))
                    .pr(px(8.))
                    .rounded(px(4.))
                    .child(line(true))
                    .when(index == last, |row| row.child(line(false)))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
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
            secondary: None,
            tertiary: None,
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
                    // `bHnZWW` with the role's member count (`getRoleMemberCount`), short from 10,000 up.
                    dc_intl::format(
                        "bHnZWW",
                        &[(
                            "numMembers",
                            member_count_label(
                                self.settings_role_member_count(guild, role)
                                    .unwrap_or(members.len() as u64),
                            )
                            .into(),
                        )],
                    )
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
                                    .when(
                                        !everyone || self.can_view_as_role(guild, role),
                                        |this| {
                                            this.child(
                                                div()
                                                    .id("server-role-menu")
                                                    .relative()
                                                    .child(self.anchor_probe("server-role-menu"))
                                                    .cursor_pointer()
                                                    .child(icon("role_menu", 24., "--text-strong"))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(state) = this
                                                                .server_settings
                                                                .as_mut()
                                                                .and_then(|open| {
                                                                    open.roles.as_mut()
                                                                })
                                                            {
                                                                state.menu_anchor =
                                                                    "server-role-menu".into();
                                                                state.menu = if state.menu.is_some()
                                                                {
                                                                    None
                                                                } else {
                                                                    Some(role)
                                                                };
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                        },
                                    ),
                            )
                            .children(self.managed_role_notice(guild, role, cx))
                            .child(div().h(px(22.)))
                            .child(tabs),
                    )
                    .child(div().pt(px(15.)).child(body)),
            )
            .into_any_element()
    }

    /// `hZ`'s warning notice (`helpMessageContainer_9pNIZg` holding module 683071's `warning` notice: 12px in, the
    /// 16px icon 8px before `body-sm/normal` in `text-strong`) over a managed role.
    fn managed_role_notice(
        &self,
        guild: Id,
        role: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut value = self.role_value(guild, role)?;
        if value.get("managed").is_none() {
            let managed = self
                .stores
                .guild(guild)
                .and_then(|known| known.role_info.get(&role))
                .is_some_and(|info| info.managed);
            value["managed"] = Value::Bool(managed);
        }
        let integrations = self
            .fetched
            .get(&format!(
                "{}{guild}",
                super::integration_apps::INTEGRATIONS_PREFIX
            ))
            .cloned()
            .unwrap_or_default();
        let notice = managed_notice(&value, &integrations)?;
        let text: AnyElement = match notice {
            ManagedNotice::Integration { name, app } => {
                let runs = dc_intl::format_runs("FuXsWm", &[("name", name.as_str().into())]);
                let mut before = String::new();
                let mut link = String::new();
                let mut after = String::new();
                for (text, tag) in runs {
                    if tag.is_some() {
                        link.push_str(&text);
                    } else if link.is_empty() {
                        before.push_str(&text);
                    } else {
                        after.push_str(&text);
                    }
                }
                self.text_with_link(
                    "server-role-integration",
                    &before,
                    &link,
                    &after,
                    cx,
                    move |this, _| {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.page = super::server_settings::Page::Integrations;
                            open.role = None;
                            open.app_open = Some(app.clone());
                        }
                    },
                )
                .into_any_element()
            }
            ManagedNotice::Booster => dc_intl::t("oF6FYT").into_any_element(),
            ManagedNotice::Connections => dc_intl::t("ZQ37tH").into_any_element(),
            ManagedNotice::Other => dc_intl::t("k5d7DJ").into_any_element(),
        };
        Some(
            div()
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--inlinenotice-border-warning"))
                .bg(color("--background-feedback-mod-warning"))
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .child(icon("warning_triangle", 16., "--icon-feedback-warning")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-strong"))
                        .child(text),
                )
                .into_any_element(),
        )
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
        let body = Draft::from_role(&held).body();
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
        let everyone = role == guild;
        let can_view = self.can_view_as_role(guild, role);
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
                            .whitespace_nowrap()
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
                    .min_w(px(210.))
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
                    .when(!everyone, |this| {
                        this.child(
                            item(
                                "server-role-duplicate",
                                "Duplicate Role",
                                "role_duplicate",
                                "--interactive-text-default",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(state) = this
                                        .server_settings
                                        .as_mut()
                                        .and_then(|open| open.roles.as_mut())
                                    {
                                        state.menu = None;
                                    }
                                    this.duplicate_role(guild, role);
                                    cx.notify();
                                },
                            )),
                        )
                    })
                    // Module 292024, behind `canImpersonateRole` (`ui/view_as_role.rs`).
                    .when(can_view, |this| {
                        this.when(!everyone, |this| {
                            this.child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                        })
                        .child(
                            item(
                                "server-role-view-as",
                                dc_intl::t("arFPfJ"),
                                "discover_arrow_right",
                                "--interactive-text-default",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(state) = this
                                        .server_settings
                                        .as_mut()
                                        .and_then(|open| open.roles.as_mut())
                                    {
                                        state.menu = None;
                                    }
                                    this.start_view_as_role(guild, role, cx);
                                },
                            )),
                        )
                    })
                    .when(!everyone, |this| {
                        this.child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                            .child(
                                item(
                                    "server-role-delete",
                                    "Delete",
                                    "role_delete",
                                    "--text-feedback-critical",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(state) = this
                                            .server_settings
                                            .as_mut()
                                            .and_then(|open| open.roles.as_mut())
                                        {
                                            state.menu = None;
                                            state.confirm_delete = Some(role);
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                    }),
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

    /// The checkmark of a selected swatch (module 508274 `$`): white on a colour darker than 0.2, else black.
    fn swatch_check(value: u32, size: f32) -> gpui::Svg {
        let check = icon("select_check", size, "--white");
        if darkness(value) > 0.2 {
            check
        } else {
            check.text_color(rgb(0x000000))
        }
    }

    /// Role Style (module 989433 `x_`): the Solid, Gradient and Holographic cards, each a preview message in its
    /// style; without `ENHANCED_ROLE_COLORS` Gradient and Holographic are disabled and the boosting upsell shows.
    /// Measured in `design/dom/settings/guild-role-display-darker-cozy.json` (`roleStyleContainer_cXmH6r`).
    fn role_style_picker(
        &self,
        guild: Id,
        role: Id,
        selected: RoleStyle,
        enhanced: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.want_image(PREVIEW_AVATAR.to_owned(), PREVIEW_AVATAR.to_owned(), cx);
        let avatar = self.images.get(PREVIEW_AVATAR).cloned();
        let role_emoji = self
            .stores
            .guild(guild)
            .and_then(|g| g.role_icons.get(&role))
            .and_then(|r| r.unicode_emoji.clone())
            .filter(|e| !e.is_empty());
        let cards = RoleStyle::ALL.map(|style| {
            let disabled = !enhanced && style != RoleStyle::Solid;
            let chosen = style == selected;
            let preview = style.preview();
            let name: AnyElement = if style == RoleStyle::Solid {
                div()
                    .text_color(rgb(preview.primary))
                    .child(dc_intl::t("cqpybK"))
                    .into_any_element()
            } else {
                super::role_gradient::name(dc_intl::t("cqpybK"), preview).into_any_element()
            };
            let badge_emoji = role_emoji.clone();
            div()
                .id(SharedString::from(format!("server-role-style-{style:?}")))
                .flex_none()
                .w(px(142.))
                .h(px(94.))
                .rounded(px(8.))
                .border_2()
                .border_color(if chosen {
                    color("--brand-500")
                } else {
                    gpui::transparent_black().into()
                })
                .bg(if chosen {
                    color("--background-mod-subtle")
                } else {
                    color("--background-base-low")
                })
                .overflow_hidden()
                .flex()
                .flex_col()
                .when(disabled, |this| this.opacity(0.3))
                .when(!disabled, |this| this.cursor_pointer())
                .child(
                    // `roleStyleMessageContainer_cXmH6r`: the preview message, clipped, fading out on the right.
                    div()
                        .relative()
                        .flex_none()
                        .w(px(142.))
                        .h(px(62.))
                        .bg(color("--background-base-lowest"))
                        .overflow_hidden()
                        .child(
                            div()
                                .absolute()
                                .left(px(12.))
                                .top(px(12.))
                                .size(px(40.))
                                .rounded_full()
                                .overflow_hidden()
                                .children(avatar.clone().map(|picture| {
                                    gpui::img(picture).size(px(40.)).rounded_full()
                                })),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(60.5))
                                .top(px(10.))
                                .w(px(220.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .whitespace_nowrap()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(22.))
                                        .font_weight(FontWeight(500.))
                                        .child(name),
                                )
                                .when_some(badge_emoji, |row, emoji| {
                                    row.child(div().text_size(px(14.)).child(emoji))
                                })
                                .child(
                                    div()
                                        .ml(px(2.))
                                        .text_size(px(12.))
                                        .line_height(px(22.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-muted"))
                                        .child("12:00 PM"),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(62.5))
                                .top(px(32.))
                                .w(px(200.))
                                .text_size(px(16.))
                                .line_height(px(22.))
                                .whitespace_nowrap()
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("Mi9Kbe")),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(0.))
                                .w(px(40.))
                                .bg(gpui::linear_gradient(
                                    90.,
                                    gpui::linear_color_stop(gpui::transparent_black(), 0.),
                                    gpui::linear_color_stop(color("--background-base-lowest"), 1.),
                                )),
                        ),
                )
                .child(
                    // `roleStyleLabel_cXmH6r`.
                    div()
                        .flex_none()
                        .h(px(28.))
                        .pt(px(6.))
                        .bg(if chosen {
                            color("--background-mod-subtle")
                        } else {
                            color("--background-surface-higher")
                        })
                        .text_color(if chosen {
                            color("--interactive-text-active")
                        } else {
                            color("--text-subtle")
                        })
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .flex()
                        .justify_center()
                        .child(style.label()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !disabled {
                        this.pick_role_style(guild, role, style, cx);
                    }
                }))
        });
        let upsell = (!enhanced).then(|| {
            // `VpEDJc` puts the gradient word `+/IHLl` (module 989433 `xv`, the holographic colours) in its sentence.
            let sentence = dc_intl::format("VpEDJc", &[("magical", "\u{1}".into())]);
            let (before, after) = sentence
                .split_once('\u{1}')
                .unwrap_or((sentence.as_str(), ""));
            let holo = RoleStyle::Holographic.preview();
            // GAP: the expressive button's Rive background; its press does not open the server's boosts page
            // (`/channels/{id}/guild-boosts`, not built), it closes the settings (`LAYER_POP`).
            div()
                .mt(px(4.))
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // GAP: `button-outline-primary-text` is white in the dark capture; no light-theme measure.
                        .text_color(color("--white"))
                        .child(
                            div()
                                .flex()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .child(before.to_owned())
                                .child(
                                    super::role_gradient::name(dc_intl::t("+/IHLl"), holo)
                                        .animate(true),
                                )
                                .child(after.to_owned()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .child(dc_intl::t("FJZeZF")),
                        ),
                )
                .child(
                    div()
                        .id("server-role-style-upsell")
                        .flex_none()
                        .h(px(40.))
                        .px(px(15.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-expressive-border-default"))
                        .bg(color("--control-expressive-background-default"))
                        .text_color(color("--control-expressive-text-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            icon("boost_gem", 20., "--white")
                                .text_color(color("--control-expressive-text-default")),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .child(dc_intl::t("+7XY31")),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.server_settings = None;
                            this.select(guild, None);
                            this.open_boost_page(cx);
                        })),
                )
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Self::field_title(dc_intl::t("9wVJRB"), false))
                    .child(
                        div()
                            .mb(px(8.))
                            .px(px(6.))
                            .h(px(16.))
                            .rounded(px(8.))
                            .bg(color("--brand-500"))
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("y2b7CA").to_uppercase()),
                    ),
            )
            .child(div().pb(px(8.)).flex().gap(px(8.)).children(cards))
            .children(upsell)
            .into_any_element()
    }

    /// Module 48736's gradient strip (`U`): the two stops on a 90deg gradient bar, each a ringed circle with the
    /// eye-dropper that opens the colour popout; then module 508274 `sk`'s twelve presets in two rows (180deg).
    /// GAP: no DOM capture of a server with `ENHANCED_ROLE_COLORS`; the sizes are the stylesheet's
    /// (`gradientButtonContainer_6C7JFt`, `circleGradientPicker_6C7JFt`, `gradientPreset_BorJ2C`).
    fn role_gradient_picker(
        &self,
        draft: &Draft,
        guild: Id,
        role: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::profile_editor::colour::Target;
        use super::tooltip::{Side, Tip};
        let pair = draft.gradient_pair();
        let stops = pair.iter().enumerate().map(|(index, value)| {
            let value = *value;
            // `n`: a colour darker than 0.1 has the white ring and a white dropper, a near-white one black.
            let dark = darkness(value) > 0.1;
            let ring = if dark { rgb(0xffffff) } else { rgb(0x000000) };
            let label = gradient_stop_label(index, pair.len());
            let circle = div()
                .id(SharedString::from(format!(
                    "server-role-gradient-stop-{index}"
                )))
                .size(px(34.))
                .rounded_full()
                .bg(ring)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(32.))
                        .rounded_full()
                        .bg(rgb(value))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.05),
                                offset: gpui::point(px(0.), px(0.)),
                                blur_radius: px(2.),
                                spread_radius: px(0.),
                                inset: false,
                            },
                            gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.15),
                                offset: gpui::point(px(0.), px(0.)),
                                blur_radius: px(13.),
                                spread_radius: px(0.),
                                inset: false,
                            },
                        ])
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui::svg()
                                .path("icons/profile_editor_eyedropper.svg")
                                .size(px(14.))
                                .text_color(ring),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_role_colour(Target::RoleStop(index), value, cx);
                }));
            self.with_tip(
                format!("server-role-colour-stop-{index}"),
                Tip::body(super::tooltip::Body::Text(label.into()), Side::Bottom),
                circle,
                cx,
            )
        });
        let strip = div()
            .relative()
            .flex_none()
            .w(px(154.))
            .h(px(50.))
            .mr(px(10.))
            .child(div().size_full().rounded(px(4.)).bg(gpui::linear_gradient(
                90.,
                gpui::linear_color_stop(rgb(pair[0]), 0.),
                gpui::linear_color_stop(rgb(pair[1]), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .children(stops.collect::<Vec<_>>()),
            );
        let preset = |colors: [u32; 2], cx: &mut Context<Self>| {
            let chosen = colors == pair;
            div()
                .id(SharedString::from(format!(
                    "server-role-gradient-{:06x}-{:06x}",
                    colors[0], colors[1]
                )))
                .flex_none()
                .w(px(40.))
                .h(px(20.))
                .mr(px(8.))
                .rounded(px(8.))
                .bg(gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(rgb(colors[0]), 0.),
                    gpui::linear_color_stop(rgb(colors[1]), 1.),
                ))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .when(chosen, |this| {
                    this.child(Self::swatch_check(colors[0], 16.))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_draft(guild, role, |draft| draft.set_gradient(colors), cx)
                }))
        };
        let mut rows = div().flex().flex_col().gap(px(10.));
        for row in GRADIENT_PRESETS.chunks(6) {
            rows = rows.child(
                div()
                    .h(px(20.))
                    .flex()
                    .children(row.iter().map(|colors| preset(*colors, cx))),
            );
        }
        div().flex().child(strip).child(rows).into_any_element()
    }

    /// Module 508274 `sk` without a gradient: Default, Custom Color (`nJ`, its popout to the right), then the 20
    /// swatches in two rows.
    fn role_solid_picker(
        &self,
        draft: &Draft,
        guild: Id,
        role: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::profile_editor::colour::Target;
        use super::tooltip::{Side, Tip};
        let swatch = |value: u32, size: f32, cx: &mut Context<Self>| {
            // The default (`A`) is also chosen when the role holds the default colour itself.
            let selected = draft.color == value || (value == 0 && draft.color == NO_COLOR);
            let shown = if value == 0 { NO_COLOR } else { value };
            div()
                .id(SharedString::from(format!("server-role-color-{value}")))
                .size(px(size))
                .rounded(px(8.))
                .bg(rgb(shown))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |this| {
                    // `38%` of the 66px Default button, 16px on a swatch.
                    this.child(Self::swatch_check(
                        shown,
                        if size > 30. { 25. } else { 16. },
                    ))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_draft(guild, role, |draft| draft.set_solid(value), cx)
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
        let custom = custom_color(draft.color);
        let initial = draft.color;
        // `L`: chosen while the role's colour is the custom one; the pencil is white until then.
        let custom_button = div()
            .id("server-role-color-custom")
            .relative()
            .w(px(66.))
            .h(px(50.))
            .rounded(px(8.))
            .border_1()
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .map(|this| match custom {
                Some(value) => this.border_color(gpui::transparent_black()).bg(rgb(value)),
                // GAP: `.theme-dark`'s border; the light theme's `hsl(var(--primary-500-hsl)/.1)` is not used.
                None => this.border_color(color("--opacity-white-8")),
            })
            .child(
                div().absolute().top(px(4.)).right(px(4.)).child(
                    icon("clips_pencil", 14., "--white")
                        .when(custom.is_some_and(|value| darkness(value) <= 0.2), |this| {
                            this.text_color(rgb(0x000000))
                        }),
                ),
            )
            .children(custom.map(|value| Self::swatch_check(value, 25.)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_role_colour(Target::RoleCustom, initial, cx);
            }));
        div()
            .flex()
            .gap(px(10.))
            .child(self.with_tip(
                "server-role-colour-default",
                Tip::text(dc_intl::t("bBvAEH"), Side::Bottom),
                swatch(0, 66., cx).w(px(66.)).h(px(50.)),
                cx,
            ))
            .child(self.with_tip(
                "server-role-colour-custom",
                Tip::text(dc_intl::t("FHBa/1"), Side::Bottom),
                custom_button,
                cx,
            ))
            .child(swatch_rows)
            .into_any_element()
    }

    /// Display (`xG`): Role name, Role Style, Role color (solid swatches or the gradient strip; none for
    /// Holographic, which shows its note), the two switches.
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
        let enhanced = self.enhanced_role_colors(guild);
        let style = draft.style(enhanced);
        // GAP: a locked or managed role (`g` of `xG`) does not disable the cards, swatches and stops.
        let (style_picker, colour_picker) = if everyone {
            (None, None)
        } else {
            (
                Some(self.role_style_picker(guild, role, style, enhanced, cx)),
                match style {
                    RoleStyle::Solid => Some(self.role_solid_picker(draft, guild, role, cx)),
                    RoleStyle::Gradient => Some(self.role_gradient_picker(draft, guild, role, cx)),
                    RoleStyle::Holographic => None,
                },
            )
        };
        let holographic_note = (style == RoleStyle::Holographic).then(|| {
            // Module 546385 `A` (look "info", `root_vb4RFD`/`info_vb4RFD`) with `tBhCyr`.
            div()
                .mt(px(8.))
                .p(px(8.))
                .rounded(px(4.))
                .border_1()
                .border_color(color("--icon-feedback-info"))
                .bg(color("--background-feedback-mod-info"))
                .flex()
                .items_center()
                .child(
                    div()
                        .mr(px(10.))
                        .child(icon("circle_info", 24., "--icon-feedback-info")),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("tBhCyr")),
                )
        });
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
                    .children(style_picker)
                    .when_some(colour_picker, |this, picker| {
                        // `colorPicker_cXmH6r`: 24px under the styles.
                        this.child(
                            div()
                                .mt(px(24.))
                                .flex()
                                .flex_col()
                                .child(Self::field_title("Role color", true))
                                .child(
                                    div()
                                        .mt(px(-4.))
                                        .mb(px(8.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-subtle"))
                                        .child(dc_intl::t("9UCxlf")),
                                )
                                .child(picker),
                        )
                    })
                    .children(holographic_note)
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
        // `xZ` with no section left: the 85px picture 16px over "No permissions found" (`noResultsContainer_kyu-6S`).
        let any = GUILD.iter().any(|section| {
            section.permissions.iter().any(|permission| {
                permission.title.to_lowercase().contains(&query)
                    || permission.description.to_lowercase().contains(&query)
            })
        });
        if !any {
            let art = self.images.get(NO_PERMISSIONS_ART).cloned();
            list = list.child(
                div()
                    .mt(px(32.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .size(px(85.))
                            .mb(px(16.))
                            .children(art.map(|art| gpui::img(art).size(px(85.)))),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("DEBGqA")),
                    ),
            );
        }
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

    /// `getRoleMemberCount` of the role, from `GET /guilds/{id}/roles/member-counts`.
    fn settings_role_member_count(&self, guild: Id, role: Id) -> Option<u64> {
        self.fetched
            .get(&format!("{COUNTS_PREFIX}{guild}"))
            .and_then(|counts| counts[role.to_string()].as_u64())
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
            // `xz`'s `x`: the role has more members than are loaded here (`searchWarning_mssHMP`, an `info` notice
            // 16px under the search).
            .when(
                self.settings_role_member_count(guild, role)
                    .is_some_and(|count| count > members.len() as u64),
                |this| {
                    this.child(
                        div()
                            .mt(px(16.))
                            .p(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--inlinenotice-border-info"))
                            .bg(color("--background-feedback-mod-info"))
                            .flex()
                            .gap(px(8.))
                            .child(div().h(px(18.)).flex().items_center().child(
                                super::settings::mask_icon(
                                    "settings_notice_info",
                                    16.,
                                    "--icon-feedback-info",
                                ),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("RQxHZ8")),
                            ),
                    )
                },
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

/// Module 48736 `f`: stop tooltip label (`apbXbH` / `JaVq1b` for 2 stops, `XdByBm` for >2 stops).
pub(super) fn gradient_stop_label(index: usize, count: usize) -> String {
    if count <= 2 {
        dc_intl::t(if index == 0 { "apbXbH" } else { "JaVq1b" }).to_owned()
    } else {
        dc_intl::format("XdByBm", &[("number", (index + 1).to_string().into())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_counts_go_short_from_ten_thousand() {
        assert_eq!(member_count_label(9_999), "9999");
        assert_eq!(member_count_label(12_345), "12K");
        assert_eq!(member_count_label(15_600), "16K");
        assert_eq!(member_count_label(1_250_000), "1.3M");
    }

    /// `hZ`: a bot's role names its integration, other managed roles say what manages them.
    #[test]
    fn managed_roles_say_what_manages_them() {
        let integrations = serde_json::json!([
            {"id": "5", "type": "discord", "name": "Helper", "application": {"id": "9", "bot": {"id": "7"}}},
            {"id": "6", "type": "twitch", "name": "chan", "application": {"id": "8"}},
        ]);
        let role = |tags: serde_json::Value| serde_json::json!({"managed": true, "tags": tags});
        assert_eq!(
            managed_notice(&role(serde_json::json!({"bot_id": "7"})), &integrations),
            Some(ManagedNotice::Integration {
                name: "Helper".into(),
                app: "9".into()
            })
        );
        assert_eq!(
            managed_notice(
                &role(serde_json::json!({"integration_id": "6"})),
                &integrations
            ),
            Some(ManagedNotice::Integration {
                name: "Twitch - chan".into(),
                app: "8".into()
            })
        );
        assert_eq!(
            managed_notice(
                &role(serde_json::json!({"premium_subscriber": null})),
                &integrations
            ),
            Some(ManagedNotice::Booster)
        );
        assert_eq!(
            managed_notice(&role(serde_json::json!({"bot_id": "1"})), &integrations),
            Some(ManagedNotice::Other)
        );
        assert_eq!(
            managed_notice(&serde_json::json!({"managed": false}), &integrations),
            None
        );
    }

    /// Without a fetched list the role store's roles stand in, read back as `Draft` reads a role, and sort as `AT`.
    #[test]
    fn store_roles_read_as_the_api_spells_them() {
        let mut guild = dc_state::Guild::default();
        guild.roles.insert(3, "Mods".into());
        guild.roles.insert(2, "Plain".into());
        guild.roles.insert(4, "Top".into());
        guild.role_permissions.insert(3, 1 << 10);
        guild.role_info.insert(
            3,
            dc_state::RoleInfo {
                position: 1,
                color: 0x3498db,
                hoist: true,
                mentionable: true,
                secondary_color: Some(0x123456),
                ..Default::default()
            },
        );
        guild.role_info.insert(
            2,
            dc_state::RoleInfo {
                position: 1,
                ..Default::default()
            },
        );
        guild.role_info.insert(
            4,
            dc_state::RoleInfo {
                position: 2,
                ..Default::default()
            },
        );
        let mut roles = store_roles(&guild);
        sort_roles(&mut roles);
        let ids: Vec<&str> = roles
            .iter()
            .filter_map(|role| role["id"].as_str())
            .collect();
        assert_eq!(ids, ["4", "2", "3"]);
        let mods = Draft::from_role(&roles[2]);
        assert_eq!(mods.name, "Mods");
        assert_eq!(mods.color, 0x3498db);
        assert_eq!(mods.secondary, Some(0x123456));
        assert!(mods.hoist && mods.mentionable);
        assert_eq!(mods.permissions, 1 << 10);
    }

    #[test]
    fn drafts_read_and_write_like_update_role() {
        let role = serde_json::json!({"id": "1", "name": "new role", "color": 0, "hoist": false, "mentionable": true, "permissions": "1024", "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null}});
        let mut draft = Draft::from_role(&role);
        assert_eq!(draft.permissions, 1024);
        draft.color = SWATCHES[0];
        let body = draft.body();
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

    fn role_with(color: u32, secondary: Option<u32>, tertiary: Option<u32>) -> Value {
        serde_json::json!({"id": "2", "name": "r", "color": color, "permissions": "0",
            "colors": {"primary_color": color, "secondary_color": secondary, "tertiary_color": tertiary}})
    }

    #[test]
    fn role_style_follows_the_colours_and_the_feature() {
        let gradient = Draft::from_role(&role_with(0xaa3b3b, Some(0xff4848), None));
        assert_eq!(gradient.style(true), RoleStyle::Gradient);
        assert_eq!(gradient.style(false), RoleStyle::Solid);
        assert_eq!(gradient.gradient_pair(), [0xaa3b3b, 0xff4848]);
        let holo = Draft::from_role(&role_with(
            HOLOGRAPHIC[0],
            Some(HOLOGRAPHIC[1]),
            Some(HOLOGRAPHIC[2]),
        ));
        assert_eq!(holo.style(true), RoleStyle::Holographic);
        let solid = Draft::from_role(&role_with(0x1abc9c, None, None));
        assert_eq!(solid.style(true), RoleStyle::Solid);
        // A role without `colors` is solid.
        let bare = Draft::from_role(&serde_json::json!({"color": 5}));
        assert_eq!((bare.secondary, bare.tertiary), (None, None));
    }

    #[test]
    fn gradient_body_is_uc_and_a_solid_pick_drops_the_stops() {
        let role = role_with(0x1abc9c, None, None);
        let mut draft = Draft::from_role(&role);
        draft.set_gradient(GRADIENT_PRESETS[0]);
        let body = draft.body();
        assert_eq!(body["color"], 0xaa3b3b);
        assert_eq!(
            body["colors"],
            serde_json::json!({"primary_color": 0xaa3b3b, "secondary_color": 0xff4848, "tertiary_color": null})
        );
        draft.set_solid(0x3498db);
        let body = draft.body();
        assert_eq!(
            body["colors"],
            serde_json::json!({"primary_color": 0x3498db, "secondary_color": null, "tertiary_color": null})
        );
        // The Default button saves 0.
        draft.set_solid(NO_COLOR);
        assert_eq!(draft.color, 0);
        assert_eq!(draft.body()["colors"]["primary_color"], 0);
        // Picking the colour the role has changes nothing (`ZH`), so a kept second colour stays.
        let mut kept = Draft::from_role(&role_with(0x123456, Some(0x654321), None));
        kept.set_solid(0x123456);
        assert_eq!(kept.secondary, Some(0x654321));
    }

    #[test]
    fn switching_styles_restores_what_each_held() {
        let saved = Draft::from_role(&role_with(0x1abc9c, None, None));
        let mut memory = StyleColors::seed(&saved, true);
        assert_eq!(
            memory,
            StyleColors {
                solid: 0x1abc9c,
                gradient: GRADIENT_DEFAULT
            }
        );
        let mut draft = saved.clone();
        draft.switch_style(&mut memory, true, RoleStyle::Gradient);
        assert_eq!(draft.gradient_pair(), GRADIENT_DEFAULT);
        assert_eq!(draft.style(true), RoleStyle::Gradient);
        // A stop edit (`Q`), then Holographic, then back: the gradient is the edited one.
        draft.set_gradient([0x111111, GRADIENT_DEFAULT[1]]);
        draft.switch_style(&mut memory, true, RoleStyle::Holographic);
        assert_eq!(
            (draft.color, draft.secondary, draft.tertiary),
            (HOLOGRAPHIC[0], Some(HOLOGRAPHIC[1]), Some(HOLOGRAPHIC[2]))
        );
        draft.switch_style(&mut memory, true, RoleStyle::Gradient);
        assert_eq!(draft.gradient_pair(), [0x111111, GRADIENT_DEFAULT[1]]);
        draft.switch_style(&mut memory, true, RoleStyle::Solid);
        assert_eq!(draft, saved);
        // A gradient role starts Solid on the default colour (`z`).
        let gradient = Draft::from_role(&role_with(0xaa3b3b, Some(0xff4848), None));
        let mut memory = StyleColors::seed(&gradient, true);
        let mut draft = gradient.clone();
        draft.switch_style(&mut memory, true, RoleStyle::Solid);
        assert_eq!((draft.color, draft.secondary), (NO_COLOR, None));
        // The Default button (value 0) on the default colour changes nothing (`ZH`).
        draft.set_solid(0);
        assert_eq!(draft.color, NO_COLOR);
        draft.switch_style(&mut memory, true, RoleStyle::Gradient);
        assert_eq!(draft, gradient);
    }

    #[test]
    fn swatch_marks_follow_the_bundle() {
        assert!(darkness(0x000000) > 0.99);
        assert!(darkness(0xffffff) < 0.01);
        // `platinum`'s light stop takes the black check, `fire`'s dark one the white.
        assert!(darkness(0xf1f7ff) <= 0.2);
        assert!(darkness(0xaa3b3b) > 0.2);
        assert_eq!(custom_color(0), None);
        assert_eq!(custom_color(NO_COLOR), None);
        assert_eq!(custom_color(SWATCHES[3]), None);
        assert_eq!(custom_color(0x123456), Some(0x123456));
        assert_eq!(gradient_stop_label(0, 2), "Custom Gradient Start Color");
        assert_eq!(gradient_stop_label(1, 2), "Custom Gradient End Color");
        assert_eq!(gradient_stop_label(2, 3), "Custom Gradient Color 3");
    }
}
