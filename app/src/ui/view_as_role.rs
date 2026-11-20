//! View Server As Role / New Member / Server Shop: the role menu item (module 292024, shown when
//! PermissionStore `canImpersonateRole` holds, module 576705), the preview it starts (module 529942 `Tk`:
//! `IMPERSONATE_UPDATE` with `{type: ROLES | NEW_MEMBER | SERVER_SHOP, roles, returnToSection}`, then `m`,
//! which leaves a channel the role cannot view for the server's default one), the notice bar (`VIEWING_ROLES`
//! of the notice table, module 596339, drawn by module 25742 `q`), the Server Settings overlay bar (`Z` /
//! `settingsWrapper_INrZ8n`), and its searchable Select Roles / Select Tier popout (`V`, `container_V7nUkK`).
//!
//! The viewed roles live on the guild (`Guild::viewing_roles`, ImpersonateStore 164956), so every permission check
//! (`dc_state::permissions`) computes as a non-owner with @everyone and only those roles. Nothing is sent.

use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::editor::Editor;
use dc_intl::Arg;
use dc_state::{Channel, ChannelKind, Guild, Id, permissions};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, MouseDownEvent, SharedString, div,
    prelude::*, px,
};

/// The Select Roles / Select Tier popout's anchor (its button).
const SELECT_ANCHOR: &str = "view-as-role-select";
/// Maximum published tiers / products referenced by `eummvd` (`25742.js`).
const SERVER_SHOP_MAX_TIERS: u64 = 3;
const SERVER_SHOP_MAX_PRODUCTS: u64 = 50;

/// `ImpersonateType` (`164956.js` / `25742.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ImpersonateMode {
    #[default]
    Roles,
    NewMember,
    ServerShop,
}

/// `returnToSection` (`164956.js` / `25742.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ImpersonateReturnSection {
    #[default]
    Roles,
    Integrations,
    RoleSubscriptions,
    Onboarding,
}

impl ImpersonateReturnSection {
    /// Dynamic Back button label (`25742.js`):
    /// - `INTEGRATIONS` -> `k7LGdh` ("Back to Integrations Settings")
    /// - `ROLE_SUBSCRIPTIONS` -> `bRqiqa` ("Back to Server Subscriptions Settings")
    /// - `ONBOARDING` -> `qZpU3S` ("Back to Onboarding Settings")
    /// - `ROLES` (default) -> `MTIXhi` ("Back to Roles Settings")
    pub(crate) fn back_label(self) -> &'static str {
        dc_intl::t(match self {
            Self::Integrations => "k7LGdh",
            Self::RoleSubscriptions => "bRqiqa",
            Self::Onboarding => "qZpU3S",
            Self::Roles => "MTIXhi",
        })
    }

    /// Select button label (`25742.js`): `hZUCzd` ("Select Tier") for Server Subscriptions, else `/djIh7` ("Select Roles").
    pub(crate) fn select_label(self) -> &'static str {
        dc_intl::t(match self {
            Self::RoleSubscriptions => "hZUCzd",
            _ => "/djIh7",
        })
    }

    /// Target Server Settings page reopened when clicking Back (`25742.js` `k`).
    pub(super) fn settings_page(self) -> super::server_settings::Page {
        match self {
            Self::Integrations => super::server_settings::Page::Integrations,
            Self::Onboarding => super::server_settings::Page::Onboarding,
            Self::RoleSubscriptions => super::server_settings::Page::BoostPerks,
            Self::Roles => super::server_settings::Page::Roles,
        }
    }
}

/// The preview's local UI state; the roles themselves are on the guild.
#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    /// The Select Roles / Select Tier popout is open.
    pub(super) picker: bool,
    /// Current impersonation mode (`ROLES`, `NEW_MEMBER`, `SERVER_SHOP`).
    pub(super) mode: ImpersonateMode,
    /// Server Settings section to return to on Back (`returnToSection`).
    pub(super) return_section: ImpersonateReturnSection,
    /// Whether the user is currently viewing the synthetic `GUILD_ONBOARDING` channel (`25742.js`).
    pub(super) on_onboarding_channel: bool,
    /// Search filter editor inside the Select Roles popout (`Sojqsr` "Search Roles").
    pub(super) search: Option<Entity<Editor>>,
}

/// Notice text for `VIEWING_ROLES` (`25742.js` `q`).
pub(crate) fn notice_message(
    mode: ImpersonateMode,
    on_onboarding_channel: bool,
    role_count: usize,
) -> String {
    if mode == ImpersonateMode::NewMember {
        if on_onboarding_channel {
            // `PxbiAf`: "You are viewing the server as a new member."
            dc_intl::t("PxbiAf").to_owned()
        } else {
            // `0PHahI`: "You are viewing the server as a new member. {numRoles, plural, ...}."
            dc_intl::format("0PHahI", &[("numRoles", Arg::Num(role_count as f64))])
        }
    } else {
        // `vMlK8t`: "You are viewing this server as {numRoles, plural, =0 {@everyone} one {# role} other {# roles}}."
        dc_intl::format("vMlK8t", &[("numRoles", Arg::Num(role_count as f64))])
    }
}

/// Whether the trailing `Disable` button (`R9GHya`) is shown (`25742.js` `q`).
/// Hidden when `isFullServerPreview || backNavigationSection === ROLE_SUBSCRIPTIONS`.
pub(crate) fn shows_disable_button(
    mode: ImpersonateMode,
    return_section: ImpersonateReturnSection,
) -> bool {
    mode != ImpersonateMode::NewMember
        && return_section != ImpersonateReturnSection::RoleSubscriptions
}

/// Warning tooltip text shown beside Select Roles / Select Tier on `NewMember` or `ServerShop` previews (`25742.js`).
pub(crate) fn warning_tooltip_text(
    mode: ImpersonateMode,
    on_onboarding_channel: bool,
) -> Option<String> {
    match mode {
        ImpersonateMode::NewMember if !on_onboarding_channel => {
            Some(dc_intl::t("mW4DUE").to_owned())
        }
        ImpersonateMode::ServerShop => Some(dc_intl::format(
            "eummvd",
            &[
                ("maxTiers", Arg::Num(SERVER_SHOP_MAX_TIERS as f64)),
                ("maxProducts", Arg::Num(SERVER_SHOP_MAX_PRODUCTS as f64)),
            ],
        )),
        _ => None,
    }
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
/// always on and disabled. When `mode == ServerShop`, filters to roles carrying `tags.subscription_listing_id`.
/// When `query` is non-empty, filters case-insensitively by role name.
pub(super) fn pickable_roles_filtered(
    guild: &Guild,
    me: permissions::Me,
    mode: ImpersonateMode,
    query: &str,
) -> Vec<Id> {
    let highest = permissions::highest_role(guild, &guild.my_roles);
    let q = query.trim().to_lowercase();
    let mut roles: Vec<Id> = guild
        .roles
        .keys()
        .copied()
        .filter(|role| *role != guild.id)
        .filter(|role| {
            highest == Some(*role)
                || permissions::role_is_higher(guild, me.id, &guild.my_roles, *role)
        })
        .filter(|role| {
            mode != ImpersonateMode::ServerShop
                || guild
                    .role_info
                    .get(role)
                    .is_some_and(|info| info.has_subscription_listing)
        })
        .filter(|role| {
            q.is_empty()
                || guild
                    .roles
                    .get(role)
                    .is_some_and(|name| name.to_lowercase().contains(&q))
        })
        .collect();
    let position = |id: &Id| guild.role_info.get(id).map_or(0, |info| info.position);
    roles.sort_by(|a, b| position(b).cmp(&position(a)).then(a.cmp(b)));
    roles
}

#[cfg(test)]
pub(super) fn pickable_roles(guild: &Guild, me: permissions::Me) -> Vec<Id> {
    pickable_roles_filtered(guild, me, ImpersonateMode::Roles, "")
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
        self.start_impersonate(
            guild,
            vec![role],
            ImpersonateMode::Roles,
            ImpersonateReturnSection::Roles,
            false,
            cx,
        );
    }

    /// Full `IMPERSONATE_UPDATE` entry point supporting all modes and return sections (`25742.js`).
    pub(super) fn start_impersonate(
        &mut self,
        guild: Id,
        roles: Vec<Id>,
        mode: ImpersonateMode,
        return_section: ImpersonateReturnSection,
        on_onboarding_channel: bool,
        cx: &mut Context<Self>,
    ) {
        self.server_settings = None;
        self.view_as_role.mode = mode;
        self.view_as_role.return_section = return_section;
        self.view_as_role.on_onboarding_channel = on_onboarding_channel;
        self.view_as_role.picker = false;
        self.set_view_as_roles(guild, roles);
        cx.notify();
    }

    /// Ensures the `Sojqsr` ("Search Roles") editor exists and toggles the Select Roles popout.
    fn toggle_view_as_role_picker(&mut self, cx: &mut Context<Self>) {
        if self.view_as_role.picker {
            self.view_as_role.picker = false;
        } else {
            let ed = self.view_as_role.search.get_or_insert_with(|| {
                cx.new(|cx| {
                    let mut e = Editor::new(super::editor_style(), cx);
                    e.set_placeholder(dc_intl::t("Sojqsr").to_owned());
                    e
                })
            });
            ed.update(cx, |e, _| e.set_text(""));
            self.view_as_role.picker = true;
        }
        cx.notify();
    }

    /// `IMPERSONATE_UPDATE` then 529942 `m`: when the channel in view there cannot be viewed now, the server's
    /// default channel opens.
    pub(super) fn set_view_as_roles(&mut self, guild: Id, roles: Vec<Id>) {
        self.stores.impersonate(guild, roles);
        self.data_revision += 1;
        // `IMPERSONATE_UPDATE` runs PermissionVADStore's check (`vad_permission.rs`).
        self.sync_vad_permission();
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

    /// 25742 `k`: `IMPERSONATE_STOP`; Back also reopens Server Settings on the section the preview came from.
    pub(super) fn stop_view_as_role(
        &mut self,
        guild: Id,
        back_to_settings: bool,
        cx: &mut Context<Self>,
    ) {
        let return_section = self.view_as_role.return_section;
        self.view_as_role.picker = false;
        self.view_as_role.mode = ImpersonateMode::Roles;
        self.view_as_role.return_section = ImpersonateReturnSection::Roles;
        self.view_as_role.on_onboarding_channel = false;
        if self.stores.stop_impersonating(guild) {
            self.data_revision += 1;
            self.rebuild_rows();
            // `IMPERSONATE_STOP`, the same check.
            self.sync_vad_permission();
        }
        if back_to_settings {
            self.open_server_settings_at(guild, return_section.settings_page(), cx);
        }
        cx.notify();
    }

    /// The sidebar filter while viewing as roles.
    pub(super) fn view_as_role_shows(&self, guild: &Guild, row: &Channel) -> bool {
        guild.viewing_roles.is_none()
            || self.stores.me().is_some_and(|me| shows_row(guild, me, row))
    }

    /// Renders the `25742` `q` notice bar for `guild`. When `in_settings` is true, renders the fixed
    /// Server Settings overlay variant (`Z` / `settingsWrapper_INrZ8n`).
    fn render_impersonate_bar(
        &self,
        guild: Id,
        in_settings: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let count = self.stores.guild(guild)?.viewing_roles.as_ref()?.len();
        let mode = self.view_as_role.mode;
        let return_section = self.view_as_role.return_section;
        let on_onboarding = self.view_as_role.on_onboarding_channel;
        let text = notice_message(mode, on_onboarding, count);
        let show_select = !(mode == ImpersonateMode::NewMember && on_onboarding);
        let show_disable = shows_disable_button(mode, return_section);
        let warn_tip = warning_tooltip_text(mode, on_onboarding);
        let bar_id = if in_settings {
            "view-as-role-settings-notice"
        } else {
            "view-as-role-notice"
        };
        let mut bar = div()
            .id(bar_id)
            .flex_none()
            .w_full()
            .h(px(40.))
            .px(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .bg(color("--brand-600"))
            .text_size(px(14.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"));
        if in_settings {
            // `settingsWrapper_INrZ8n` (`25742.js` `Z`): fixed top overlay reserving Windows titlebar controls on the right.
            bar = bar.pr(px(104.));
        } else {
            bar = bar
                .relative()
                .border_t_1()
                .border_l_1()
                .border_color(color("--app-frame-border"))
                .rounded_tl(px(8.));
        }
        Some(
            bar
                // `leading_INrZ8n`: Back to {Integrations|Server Subscriptions|Onboarding|Roles} Settings (`k7LGdh`/`bRqiqa`/`qZpU3S`/`MTIXhi`).
                .child(
                    notice_button(if in_settings {
                        "view-as-role-settings-back"
                    } else {
                        "view-as-role-back"
                    })
                    .gap(px(8.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.stop_view_as_role(guild, true, cx);
                    }))
                    .child(icon("role_back", 16., "--white"))
                    .child(return_section.back_label()),
                )
                // `noticeContents_INrZ8n`: status text (semibold), optional Select Roles / Select Tier button, optional yellow warning icon.
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
                        .when(show_select, |this| {
                            this.child(
                                notice_button(if in_settings {
                                    "view-as-role-settings-select"
                                } else {
                                    "view-as-role-select"
                                })
                                .relative()
                                .child(self.anchor_probe(SELECT_ANCHOR))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_view_as_role_picker(cx);
                                }))
                                .child(return_section.select_label())
                                .child(
                                    div().ml(px(8.)).child(icon("chevron_down", 16., "--white")),
                                ),
                            )
                        })
                        .children(warn_tip.map(|tip_text| {
                            let badge = div()
                                .id(if in_settings {
                                    "view-as-role-settings-warn"
                                } else {
                                    "view-as-role-warn"
                                })
                                .ml(px(8.))
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("shop_circle_error", 16., "--status-warning"));
                            self.with_tip(
                                "view-as-role-warn-tip",
                                Tip::body(
                                    super::tooltip::Body::Text(SharedString::from(tip_text)),
                                    Side::Bottom,
                                ),
                                badge,
                                cx,
                            )
                        })),
                )
                // `trailing_INrZ8n`: Disable (`R9GHya`), hidden on New Member & Server Subscriptions previews.
                .when(show_disable, |this| {
                    this.child(
                        notice_button(if in_settings {
                            "view-as-role-settings-disable"
                        } else {
                            "view-as-role-disable"
                        })
                        .ml(px(0.))
                        .mr(px(16.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_view_as_role(guild, false, cx);
                        }))
                        .child(dc_intl::t("R9GHya")),
                    )
                })
                .when(!show_disable, |this| {
                    this.child(div().w(px(16.)).flex_none())
                })
                .into_any_element(),
        )
    }

    /// The `VIEWING_ROLES` notice (25742 `q`), for the server in view while it is viewed as roles.
    pub(super) fn view_as_role_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.guild?;
        self.render_impersonate_bar(guild, false, cx)
    }

    /// `Z` (`settingsWrapper_INrZ8n`, exported as `w` of module 25742): top overlay bar inside Server Settings
    /// while viewing the server as a role.
    pub(super) fn settings_impersonate_bar(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bar = self.render_impersonate_bar(guild, true, cx)?;
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .child(bar)
                .into_any_element(),
        )
    }

    /// 25742 `V`, under Select Roles / Select Tier: searchable role list (`Sojqsr` "Search Roles"),
    /// ticked when viewed; @everyone stays ticked and disabled. On `ServerShop`, lists subscription tier roles only.
    pub(super) fn view_as_role_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.view_as_role.picker {
            return None;
        }
        let guild_id = self.guild?;
        let guild = self.stores.guild(guild_id)?;
        let viewing = guild.viewing_roles.as_ref()?;
        let me = self.stores.me()?;
        let anchor = self.tip_anchors.borrow().get(SELECT_ANCHOR).copied()?;
        let is_owner = guild.owner_id == Some(me.id);
        let allowed = is_owner
            || permissions::true_guild_permissions(guild, me)
                & (permissions::MANAGE_GUILD | permissions::MANAGE_ROLES)
                == permissions::MANAGE_GUILD | permissions::MANAGE_ROLES;
        let query = self
            .view_as_role
            .search
            .as_ref()
            .map(|ed| ed.read(cx).text().to_owned())
            .unwrap_or_default();
        let mode = self.view_as_role.mode;
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
            for role in pickable_roles_filtered(guild, me, mode, &query) {
                list = list.child(row(role, viewing.contains(&role), false));
            }
            let show_everyone =
                query.trim().is_empty() || "@everyone".contains(&query.trim().to_lowercase());
            if show_everyone && mode != ImpersonateMode::ServerShop {
                list = list.child(row(guild_id, true, true));
            }
            div()
                .flex()
                .flex_col()
                .children(self.view_as_role.search.clone().map(|ed| {
                    // `Sojqsr`: "Search Roles" input box (margin 0 4px 8px).
                    div()
                        .mx(px(4.))
                        .mb(px(8.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .bg(color("--background-base-lowest"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_1().min_w_0().child(ed))
                        .child(icon("search", 14., "--icon-subtle"))
                }))
                .child(list)
                .into_any_element()
        } else {
            // `MNSTbY`, body-md/medium.
            div()
                .p(px(8.))
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

    /// Capture only: `view-as-role start <role id|everyone> [guild id]`,
    /// `view-as-role mode <roles|new-member|onboarding|server-shop> [roles|integrations|subscriptions|onboarding]`,
    /// `view-as-role picker`, `view-as-role stop`, `view-as-role state`.
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
                self.start_impersonate(
                    guild,
                    role.into_iter().collect(),
                    ImpersonateMode::Roles,
                    ImpersonateReturnSection::Roles,
                    false,
                    cx,
                );
                format!(
                    "view-as-role: viewing {guild} as {role:?} (menu would offer it: {allowed})"
                )
            }
            ["mode", mode_arg, rest @ ..] => {
                let Some(guild) = self.guild.or_else(|| self.stores.guilds.first().map(|g| g.id))
                else {
                    return "view-as-role: no guild".into();
                };
                if self.guild != Some(guild) {
                    self.select(guild, None);
                }
                let (mode, on_onboarding, default_ret) = match *mode_arg {
                    "new-member" => (
                        ImpersonateMode::NewMember,
                        false,
                        ImpersonateReturnSection::Onboarding,
                    ),
                    "onboarding" => (
                        ImpersonateMode::NewMember,
                        true,
                        ImpersonateReturnSection::Onboarding,
                    ),
                    "server-shop" => (
                        ImpersonateMode::ServerShop,
                        false,
                        ImpersonateReturnSection::RoleSubscriptions,
                    ),
                    _ => (
                        ImpersonateMode::Roles,
                        false,
                        ImpersonateReturnSection::Roles,
                    ),
                };
                let ret = match rest.first().copied() {
                    Some("integrations") => ImpersonateReturnSection::Integrations,
                    Some("subscriptions") => ImpersonateReturnSection::RoleSubscriptions,
                    Some("onboarding") => ImpersonateReturnSection::Onboarding,
                    Some("roles") => ImpersonateReturnSection::Roles,
                    _ => default_ret,
                };
                self.start_impersonate(guild, Vec::new(), mode, ret, on_onboarding, cx);
                format!(
                    "view-as-role: guild={guild} mode={mode:?} return={ret:?} onboarding={on_onboarding}"
                )
            }
            ["picker"] => {
                self.toggle_view_as_role_picker(cx);
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
                format!(
                    "view-as-role: {viewing:?} mode={:?} return={:?} channel {:?}",
                    self.view_as_role.mode, self.view_as_role.return_section, self.channel
                )
            }
            _ => "view-as-role: start <role|everyone> [guild] | mode <roles|new-member|onboarding|server-shop> [return] | picker | stop | state".into(),
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
                        {"id": "300", "name": "VIP Tier", "permissions": "0", "position": 2,
                         "tags": {"subscription_listing_id": "999"}}
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
        assert_eq!(rows(&stores), vec![1, 2, 10, 11, 20]);
        stores.impersonate(100, vec![]);
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
        stores.impersonate(100, vec![]);
        assert_eq!(
            pickable_roles(stores.guild(100).unwrap(), me),
            vec![300, 200]
        );
        // ServerShop mode filters to subscription tier roles only.
        assert_eq!(
            pickable_roles_filtered(
                stores.guild(100).unwrap(),
                me,
                ImpersonateMode::ServerShop,
                ""
            ),
            vec![300]
        );
        // Search query filters case-insensitively.
        assert_eq!(
            pickable_roles_filtered(
                stores.guild(100).unwrap(),
                me,
                ImpersonateMode::Roles,
                "mod"
            ),
            vec![200]
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

    #[test]
    fn notice_headers_and_buttons_follow_25742_modes() {
        assert_eq!(
            ImpersonateReturnSection::Integrations.back_label(),
            "Back to Integrations Settings"
        );
        assert_eq!(
            ImpersonateReturnSection::RoleSubscriptions.back_label(),
            "Back to Server Subscriptions Settings"
        );
        assert_eq!(
            ImpersonateReturnSection::Onboarding.back_label(),
            "Back to Onboarding Settings"
        );
        assert_eq!(
            ImpersonateReturnSection::Roles.back_label(),
            "Back to Roles Settings"
        );
        assert_eq!(
            ImpersonateReturnSection::RoleSubscriptions.select_label(),
            "Select Tier"
        );
        assert_eq!(
            ImpersonateReturnSection::Roles.select_label(),
            "Select Roles"
        );
        assert!(shows_disable_button(
            ImpersonateMode::Roles,
            ImpersonateReturnSection::Roles
        ));
        assert!(!shows_disable_button(
            ImpersonateMode::NewMember,
            ImpersonateReturnSection::Onboarding
        ));
        assert!(!shows_disable_button(
            ImpersonateMode::ServerShop,
            ImpersonateReturnSection::RoleSubscriptions
        ));
        assert_eq!(
            notice_message(ImpersonateMode::NewMember, true, 0),
            "You are viewing the server as a new member."
        );
        assert!(notice_message(ImpersonateMode::NewMember, false, 0).contains("You have no roles"));
        assert!(warning_tooltip_text(ImpersonateMode::NewMember, true).is_none());
        assert!(warning_tooltip_text(ImpersonateMode::NewMember, false).is_some());
        assert!(
            warning_tooltip_text(ImpersonateMode::ServerShop, false)
                .unwrap()
                .contains("3")
        );
    }
}
