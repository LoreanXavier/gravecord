//! View Server As Role: the role menu item (module 292024, shown when PermissionStore `canImpersonateRole` holds,
//! module 576705), the preview it starts (module 529942 `Tk`: `IMPERSONATE_UPDATE` with `{type: ROLES, roles,
//! returnToSection: ROLES}`, then `m`, which leaves a channel the role cannot view for the server's default one), the
//! notice bar (`VIEWING_ROLES` of the notice table, module 596339, drawn by module 25742 `q`) and its Select Roles
//! popout (25742 `V`).
//!
//! The viewed roles live on the guild (`Guild::viewing_roles`, ImpersonateStore 164956), so every permission check
//! (`dc_state::permissions`) computes as a non-owner with @everyone and only those roles. Nothing is sent.

use super::{Root, color, icon};
use dc_state::{Channel, ChannelKind, Guild, Id, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, MouseDownEvent, div, prelude::*, px};

/// The Select Roles popout's anchor (its button).
const SELECT_ANCHOR: &str = "view-as-role-select";

/// The preview's local UI state; the roles themselves are on the guild.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct State {
    /// The Select Roles popout is open.
    pub(super) picker: bool,
}

/// Whether a sidebar row shows while the guild is viewed as roles (`GuildChannelStore` keeps the channels the
/// account can view). A category shows when one of its channels does, or, empty, when it can be viewed and managed
/// (the rule `followed_sidebar_row` already applies to an empty category).
pub(super) fn shows_row(guild: &Guild, me: permissions::Me, row: &Channel) -> bool {
    if guild.viewing_roles.is_none() {
        return true;
    }
    let viewable = |channel: &Channel| {
        permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
    };
    if row.kind != ChannelKind::Category {
        return viewable(row);
    }
    let mut children = guild
        .channels
        .iter()
        .filter(|child| child.kind != ChannelKind::Category && child.parent_id == Some(row.id))
        .peekable();
    if children.peek().is_none() {
        return permissions::can_in_channel(
            guild,
            row,
            me,
            permissions::VIEW_CHANNEL | permissions::MANAGE_CHANNELS,
        );
    }
    children.any(viewable)
}

/// 808728 `getDefaultChannel`: the first text or announcement channel of the list the account can view.
pub(super) fn landing_channel(guild: &Guild, me: permissions::Me) -> Option<Id> {
    guild
        .sidebar_rows()
        .into_iter()
        .find(|channel| {
            matches!(channel.kind, ChannelKind::Text | ChannelKind::Other(5))
                && permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
        })
        .map(|channel| channel.id)
}

/// 25742 `V`'s list: the roles below the account's real highest (or it), highest first; @everyone is added last,
/// always on and disabled.
pub(super) fn pickable_roles(guild: &Guild, me: permissions::Me) -> Vec<Id> {
    let highest = permissions::highest_role(guild, &guild.my_roles);
    let mut roles: Vec<Id> = guild
        .roles
        .keys()
        .copied()
        .filter(|role| *role != guild.id)
        .filter(|role| {
            highest == Some(*role)
                || permissions::role_is_higher(guild, me.id, &guild.my_roles, *role)
        })
        .collect();
    let position = |id: &Id| guild.role_info.get(id).map_or(0, |info| info.position);
    roles.sort_by(|a, b| position(b).cmp(&position(a)).then(a.cmp(b)));
    roles
}

/// `--shadow-border, --shadow-high` of `container_V7nUkK`, as the role popout paints them.
fn shadow_high() -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 1., 0.08),
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: gpui::point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

/// `Y` of module 25742: an outlined white button (`button_INrZ8n`, body-xs/semibold, 4px 8px, 16px from what is
/// before it, `--brand-530` on hover).
fn notice_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .ml(px(16.))
        .px(px(8.))
        .py(px(4.))
        .flex()
        .items_center()
        .border_1()
        .border_color(color("--white"))
        .rounded(px(8.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(color("--white"))
        .cursor_pointer()
        .hover(|style| style.bg(color("--brand-530")))
}

impl Root {
    /// Whether the role menu offers View Server As Role for this role (576705 `canImpersonateRole`).
    pub(super) fn can_view_as_role(&self, guild: Id, role: Id) -> bool {
        let (Some(me), Some(known)) = (self.stores.me(), self.stores.guild(guild)) else {
            return false;
        };
        permissions::can_impersonate_role(known, me, role)
    }

    /// The menu item's action (292024): Server Settings close, then `Tk` views the server as that role.
    pub(super) fn start_view_as_role(&mut self, guild: Id, role: Id, cx: &mut Context<Self>) {
        self.server_settings = None;
        self.set_view_as_roles(guild, vec![role]);
        cx.notify();
    }

    /// `IMPERSONATE_UPDATE` then 529942 `m`: when the channel in view there cannot be viewed now, the server's
    /// default channel opens.
    pub(super) fn set_view_as_roles(&mut self, guild: Id, roles: Vec<Id>) {
        self.stores.impersonate(guild, roles);
        self.data_revision += 1;
        if self.guild != Some(guild) {
            return;
        }
        let Some(me) = self.stores.me() else {
            return;
        };
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        // GAP: a thread in view is not checked (its parent's permissions decide in the official).
        let hidden = self.channel.is_some_and(|channel| {
            known.channels.iter().any(|found| {
                found.id == channel
                    && !permissions::can_in_channel(known, found, me, permissions::VIEW_CHANNEL)
            })
        });
        if hidden && let Some(landing) = landing_channel(known, me) {
            self.select(guild, Some(landing));
        } else {
            self.rebuild_rows();
        }
    }

    /// 25742 `k`: `IMPERSONATE_STOP`; Back also reopens Server Settings on the section the preview came from (Roles).
    pub(super) fn stop_view_as_role(
        &mut self,
        guild: Id,
        back_to_settings: bool,
        cx: &mut Context<Self>,
    ) {
        self.view_as_role.picker = false;
        if self.stores.stop_impersonating(guild) {
            self.data_revision += 1;
            self.rebuild_rows();
        }
        if back_to_settings {
            self.open_server_settings_at(guild, super::server_settings::Page::Roles, cx);
        }
        cx.notify();
    }

    /// The sidebar filter while viewing as roles.
    pub(super) fn view_as_role_shows(&self, guild: &Guild, row: &Channel) -> bool {
        guild.viewing_roles.is_none()
            || self.stores.me().is_some_and(|me| shows_row(guild, me, row))
    }

    /// The `VIEWING_ROLES` notice (25742 `q`), for the server in view while it is viewed as roles.
    pub(super) fn view_as_role_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.guild?;
        let count = self.stores.guild(guild)?.viewing_roles.as_ref()?.len();
        // `vMlK8t`: "You are viewing this server as {numRoles, plural, =0 {@everyone} one {# role} other {# roles}}."
        let text = dc_intl::format("vMlK8t", &[("numRoles", count.into())]);
        Some(
            // `notice_Q02kKo colorBrand_Q02kKo notice_INrZ8n`: 40px, 0 8px, the leading, centred and trailing parts
            // spread apart, white on `--brand-600`.
            // GAP: `--brand-600` (`notice_INrZ8n`) and `colorBrand`'s `--brand-500` tie in specificity; no DOM
            // capture says which sheet wins, `--brand-600` is taken from the later chunk (811198).
            div()
                .id("view-as-role-notice")
                .relative()
                .flex_none()
                .w_full()
                .h(px(40.))
                .px(px(8.))
                .flex()
                .items_center()
                .justify_between()
                .bg(color("--brand-600"))
                .border_t_1()
                .border_l_1()
                .border_color(color("--app-frame-border"))
                .rounded_tl(px(8.))
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                // `leading_INrZ8n`: Back to Roles Settings (`MTIXhi`) behind the left arrow (`backArrow`, 8px gap).
                .child(
                    notice_button("view-as-role-back")
                        .gap(px(8.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_view_as_role(guild, true, cx);
                        }))
                        .child(icon("role_back", 16., "--white"))
                        .child(dc_intl::t("MTIXhi")),
                )
                // `noticeContents_INrZ8n`: the text (semibold) and the Select Roles button, centred.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .line_height(px(36.))
                                .font_weight(FontWeight(600.))
                                .child(text),
                        )
                        .child(
                            notice_button("view-as-role-select")
                                .relative()
                                .child(self.anchor_probe(SELECT_ANCHOR))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.view_as_role.picker = !this.view_as_role.picker;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("/djIh7"))
                                // `selectCaret_INrZ8n`: the small chevron, 8px after.
                                .child(div().ml(px(8.)).child(icon(
                                    "chevron_down",
                                    16.,
                                    "--white",
                                ))),
                        ),
                )
                // `trailing_INrZ8n`: Disable (`R9GHya`), 16px from the right edge.
                .child(
                    notice_button("view-as-role-disable")
                        .ml(px(0.))
                        .mr(px(16.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_view_as_role(guild, false, cx);
                        }))
                        .child(dc_intl::t("R9GHya")),
                )
                .into_any_element(),
        )
    }

    /// 25742 `V`, under Select Roles: the roles one may view as, ticked when viewed; @everyone stays ticked.
    /// GAP: the combobox's Search Roles field (`Sojqsr`) is not drawn; the rows follow `role_V7nUkK` and
    /// `iconContainer_V7nUkK` of the stylesheet (the combobox item module 44482 is not ported).
    pub(super) fn view_as_role_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.view_as_role.picker {
            return None;
        }
        let guild_id = self.guild?;
        let guild = self.stores.guild(guild_id)?;
        let viewing = guild.viewing_roles.as_ref()?;
        let me = self.stores.me()?;
        let anchor = self.tip_anchors.borrow().get(SELECT_ANCHOR).copied()?;
        let allowed = permissions::true_guild_permissions(guild, me)
            & (permissions::MANAGE_GUILD | permissions::MANAGE_ROLES)
            == permissions::MANAGE_GUILD | permissions::MANAGE_ROLES;
        let row = |role: Id, checked: bool, disabled: bool| {
            let name = guild.roles.get(&role).cloned().unwrap_or_default();
            let tint = guild
                .role_info
                .get(&role)
                .map(|info| info.color)
                .filter(|value| *value != 0)
                .map_or_else(|| color("--text-default"), gpui::rgb);
            div()
                .id(("view-as-role-row", role))
                .flex()
                .items_center()
                .justify_between()
                .p(px(8.))
                .rounded(px(4.))
                .when(disabled, |this| this.opacity(0.5))
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--background-base-lower")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_view_as_role(guild_id, role);
                            cx.notify();
                        }))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w_0()
                        // `z`: the role's colour as a 12px dot.
                        .child(div().flex_none().size(px(12.)).rounded_full().bg(tint))
                        .child(
                            div()
                                .mr(px(8.))
                                .truncate()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(name),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .border_2()
                        .when(checked, |this| {
                            this.border_color(color("--brand-500"))
                                .bg(color("--brand-500"))
                                .child(icon("checkbox_check", 12., "--white"))
                        })
                        .when(!checked, |this| {
                            this.border_color(color("--interactive-text-default"))
                        }),
                )
        };
        let body: AnyElement = if allowed {
            let mut list = div()
                .id("view-as-role-roles")
                .max_h(px(300.))
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for role in pickable_roles(guild, me) {
                list = list.child(row(role, viewing.contains(&role), false));
            }
            list.child(row(guild_id, true, true)).into_any_element()
        } else {
            // `MNSTbY`, body-md/medium.
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(dc_intl::t("MNSTbY"))
                .into_any_element()
        };
        Some(
            gpui::deferred(
                // `container_V7nUkK`: 4px below the button, at most 250px wide, 8px 8px 4px.
                // GAP: the popout's horizontal alignment (265872 `position: bottom`) is taken as the button's left.
                div()
                    .id("view-as-role-picker")
                    .occlude()
                    .absolute()
                    .left(anchor.left())
                    .top(anchor.bottom() + px(4.))
                    .w(px(250.))
                    .pt(px(8.))
                    .px(px(8.))
                    .pb(px(4.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--background-surface-high"))
                    .shadow(shadow_high())
                    .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        let on_button = this
                            .tip_anchors
                            .borrow()
                            .get(SELECT_ANCHOR)
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !on_button {
                            this.view_as_role.picker = false;
                            cx.notify();
                        }
                    }))
                    .child(body),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// A row of the popout ticked or unticked (25742's effect: `IA(guild, {type, roles})`).
    fn toggle_view_as_role(&mut self, guild: Id, role: Id) {
        let Some(mut roles) = self
            .stores
            .guild(guild)
            .and_then(|known| known.viewing_roles.clone())
        else {
            return;
        };
        if let Some(index) = roles.iter().position(|known| *known == role) {
            roles.remove(index);
        } else {
            roles.push(role);
        }
        self.set_view_as_roles(guild, roles);
    }

    /// Capture only: `view-as-role start <role id|everyone> [guild id]` views the server in view (or that one) as
    /// the role, `view-as-role picker` toggles Select Roles, `view-as-role stop` disables, `view-as-role state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_view_as_role(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        let guild = |root: &Root, given: Option<&&str>| {
            given
                .and_then(|text| text.parse::<Id>().ok())
                .or(root.guild)
        };
        match args {
            ["start", role, rest @ ..] => {
                let Some(guild) = guild(self, rest.first()) else {
                    return "view-as-role: no guild".into();
                };
                let role = if *role == "everyone" {
                    None
                } else {
                    match role.parse::<Id>() {
                        Ok(role) => Some(role),
                        Err(_) => return format!("view-as-role: bad role {role}"),
                    }
                };
                let allowed = role.is_none_or(|role| self.can_view_as_role(guild, role));
                self.server_settings = None;
                self.set_view_as_roles(guild, role.into_iter().collect());
                cx.notify();
                format!(
                    "view-as-role: viewing {guild} as {role:?} (menu would offer it: {allowed})"
                )
            }
            ["picker"] => {
                self.view_as_role.picker = !self.view_as_role.picker;
                cx.notify();
                format!("view-as-role: picker {}", self.view_as_role.picker)
            }
            ["stop"] => {
                let Some(guild) = self.guild else {
                    return "view-as-role: no guild".into();
                };
                self.stop_view_as_role(guild, false, cx);
                "view-as-role: stopped".into()
            }
            ["state"] => {
                let viewing = self
                    .guild
                    .and_then(|guild| self.stores.guild(guild))
                    .and_then(|known| known.viewing_roles.clone());
                format!("view-as-role: {viewing:?} channel {:?}", self.channel)
            }
            _ => "view-as-role: start <role|everyone> [guild] | picker | stop | state".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stores() -> dc_state::Stores {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "5", "username": "me"},
                "guilds": [{
                    "id": "100",
                    "properties": {"name": "g", "owner_id": "5"},
                    "roles": [
                        {"id": "100", "name": "@everyone", "permissions": "1024", "position": 0},
                        {"id": "200", "name": "mods", "permissions": "0", "position": 1},
                        {"id": "300", "name": "admins", "permissions": "0", "position": 2}
                    ],
                    "channels": [
                        {"id": "1", "type": 0, "name": "staff", "position": 0, "permission_overwrites": [
                            {"id": "100", "type": 0, "allow": "0", "deny": "1024"},
                            {"id": "200", "type": 0, "allow": "1024", "deny": "0"}
                        ]},
                        {"id": "2", "type": 0, "name": "general", "position": 1, "permission_overwrites": []},
                        {"id": "10", "type": 4, "name": "Staff", "position": 2, "permission_overwrites": []},
                        {"id": "11", "type": 0, "name": "staff-only", "position": 0, "parent_id": "10",
                            "permission_overwrites": [{"id": "100", "type": 0, "allow": "0", "deny": "1024"}]},
                        {"id": "20", "type": 4, "name": "Empty", "position": 3, "permission_overwrites": []}
                    ]
                }],
                "merged_members": [[{"user_id": "5", "roles": []}]]
            }),
        );
        stores
    }

    #[test]
    fn the_owner_viewing_as_a_role_sees_only_what_it_can_view() {
        let mut stores = stores();
        let me = stores.me().unwrap();
        let rows = |stores: &dc_state::Stores| -> Vec<Id> {
            let guild = stores.guild(100).unwrap();
            guild
                .sidebar_rows()
                .into_iter()
                .filter(|row| shows_row(guild, me, row))
                .map(|row| row.id)
                .collect()
        };
        // The owner sees everything before the preview.
        assert_eq!(rows(&stores), vec![1, 2, 10, 11, 20]);
        stores.impersonate(100, vec![]);
        // As @everyone: the staff channels go, the category whose only channel is hidden goes with them, and the
        // empty category needs Manage Channels.
        assert_eq!(rows(&stores), vec![2]);
        assert_eq!(landing_channel(stores.guild(100).unwrap(), me), Some(2));
        stores.impersonate(100, vec![200]);
        assert_eq!(rows(&stores), vec![1, 2]);
        assert_eq!(landing_channel(stores.guild(100).unwrap(), me), Some(1));
        stores.stop_impersonating(100);
        assert_eq!(rows(&stores), vec![1, 2, 10, 11, 20]);
    }

    #[test]
    fn the_picker_lists_roles_below_the_real_highest_highest_first() {
        let mut stores = stores();
        let me = stores.me().unwrap();
        // The owner may pick every role, even while viewed as @everyone.
        stores.impersonate(100, vec![]);
        assert_eq!(
            pickable_roles(stores.guild(100).unwrap(), me),
            vec![300, 200]
        );
        let not_owner = permissions::Me {
            id: 6,
            mfa_enabled: false,
        };
        let mut guild = stores.guild(100).unwrap().clone();
        guild.owner_id = Some(9);
        guild.my_roles = vec![200];
        assert_eq!(pickable_roles(&guild, not_owner), vec![200]);
    }
}
