//! Server Settings > Roles (module 989433 `E_`). With only @everyone it shows `Ea`: the illustration, "Organize your
//! members", Create Role, and the Default Permissions card (`Es`) that opens @everyone in the role editor. Create Role
//! is module 778858 `createRole`: `POST /guilds/{id}/roles` with "new role", colour 0 and no permissions, then the new
//! role is selected. Measured in `design/dom/settings/guild-roles-darker-cozy.json`.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

/// `Ei`: the empty state's illustration.
const ROLES_ART: &str = "https://cdn.discordapp.com/assets/content/b79820f6ba221c996b84565cae31b15b94dc23c5449e50f978369d2d82ac6355.svg";
pub(super) const ROLE_CREATED_PREFIX: &str = "guild-role-created:";

impl Root {
    pub(super) fn request_roles_page(&mut self) {
        self.request_image(ROLES_ART.to_owned(), ROLES_ART.to_owned());
    }

    /// `createRole` with no name or colour given.
    pub(super) fn create_role(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{ROLE_CREATED_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/roles"),
            body: Some(serde_json::json!({
                "name": "new role",
                "color": 0,
                "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null},
                "permissions": "0",
            })),
        });
    }

    /// The created role becomes the one being edited (`GUILD_SETTINGS_ROLE_SELECT`).
    pub(super) fn role_created(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if report["ok"] != true {
            return;
        }
        let role = report["body"]["id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok());
        let guild = self.server_settings.as_ref().map(|open| open.guild);
        if let (Some(guild), Some(role)) = (guild, role) {
            if let Some(list) = self
                .fetched
                .get_mut(&format!(
                    "{}{guild}",
                    super::server_role_editor::ROLES_PREFIX
                ))
                .and_then(|body| body.as_array_mut())
            {
                list.push(report["body"].clone());
            }
            let duplicate = self.finish_duplicate(guild, role);
            self.request_roles(guild, cx);
            if !duplicate {
                self.select_role(role, cx);
            }
        }
        cx.notify();
    }

    /// `E_`: the editor for the selected role, the list once roles exist, else `Ea`.
    pub(super) fn server_roles_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        if let Some(role) = self.server_settings.as_ref().and_then(|open| open.role) {
            return self.server_role_editor(guild, role, cx);
        }
        // `E_`: the list once a role other than @everyone (`Oy` of module 34457) is there, and from then on; the
        // same roles the list shows.
        let others = self
            .roles_of(guild)
            .iter()
            .filter(|role| role["id"].as_str() != Some(guild.to_string().as_str()))
            .count();
        let listed = self
            .server_settings
            .as_ref()
            .and_then(|open| open.roles.as_ref())
            .map(|state| &state.listed);
        if others > 0
            && let Some(listed) = listed
        {
            listed.set(true);
        }
        if role_list_shown(others, listed.is_some_and(|listed| listed.get())) {
            return self.server_roles_list(guild, cx);
        }
        self.server_roles_empty(guild, cx)
    }

    /// `Ea`: only @everyone exists.
    pub(super) fn server_roles_empty(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(ROLES_ART).cloned();
        div()
            .w(px(563.))
            .pb(px(80.))
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
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .mt(px(12.))
                            .mb(px(32.))
                            .w(px(288.))
                            .h(px(195.))
                            .children(art.map(|art| img(art).w(px(288.)).h(px(192.)))),
                    )
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Organize your members"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(
                                "Use roles to group your server members and assign permissions.",
                            ),
                    )
                    .child(
                        div()
                            .id("server-roles-create")
                            .mt(px(16.))
                            .h(px(40.))
                            .px(px(15.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Create Role")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.create_role(guild);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().my(px(32.)).h(px(1.)).bg(color("--border-subtle")))
            .child(self.default_permissions_card(guild, cx))
            .into_any_element()
    }

    /// `Es`: the @everyone card (`container_Ax3P1g`).
    pub(super) fn default_permissions_card(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("server-roles-everyone")
            .h(px(74.))
            .pl(px(16.))
            .pr(px(24.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .hover(|style| {
                style
                    .bg(color("--interactive-background-hover"))
                    .text_color(color("--interactive-text-hover"))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .text_color(color("--interactive-text-default"))
            .child(
                div()
                    .size(px(32.))
                    .flex_none()
                    .rounded_full()
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("roles_group", 20., "--interactive-text-default")),
            )
            .child(
                div()
                    .ml(px(16.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .child("Default Permissions"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child("@everyone \u{2022} applies to all server members"),
                    ),
            )
            .child(icon("roles_arrow", 24., "--interactive-text-default"))
            .on_click(cx.listener(move |this, _, _, cx| {
                // @everyone's id is the server's.
                this.select_role(guild, cx);
            }))
            .into_any_element()
    }
}

/// `E_` of module 989433: the list once a role besides @everyone exists, and from then on (`a` never turns back).
fn role_list_shown(others: usize, listed_before: bool) -> bool {
    others > 0 || listed_before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_stays_once_it_was_shown() {
        assert!(!role_list_shown(0, false));
        assert!(role_list_shown(1, false));
        assert!(role_list_shown(0, true));
    }
}
