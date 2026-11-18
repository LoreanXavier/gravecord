//! BetterRoleContext (Vencord `src/plugins/betterRoleContext`, GPL-3.0-or-later): a right click on a role's group
//! header in the member list opens a role menu (`vc-better-role-context-member-list`): Edit Role (with Manage Roles:
//! Server Settings on Roles with the role selected), Copy Role Color (a coloured role: `#rrggbb`), View Role Icon (a
//! role with an uploaded icon: `role-icons/<role>/<icon>.<roleIconFileFormat>` at 128 by 128 in the picture viewer),
//! View Role Members (the role's member popout, beside the header) and Copy Role ID. The rows carry the original's
//! leading icons (PencilIcon, AppearanceIcon, ImageIcon, CopyIdIcon) and View Role Members its own trailing one.
//!
//! GAP: the original also adds Edit Role, Copy Role Color and View Role Icon to the developer `dev-context` menu of a
//! role chip in a profile; the profile's role chips here open no menu. Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row};
use crate::plugins::State;
use dc_state::Id;
use gpui::{Bounds, Context, Pixels};
use serde_json::Value;

const ID: &str = "BetterRoleContext";

/// `roleIconFileFormat` while the plugin is on (`png` by default).
fn format(state: &State) -> Option<String> {
    state
        .is_on(ID)
        .then(|| match state.value(ID, "roleIconFileFormat") {
            Value::String(format) if ["png", "webp", "jpg"].contains(&format.as_str()) => format,
            _ => "png".to_owned(),
        })
}

fn icon_format() -> Option<String> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(format)
}

/// What a role offers the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Offers {
    pub(in crate::ui) manage: bool,
    pub(in crate::ui) color: bool,
    pub(in crate::ui) icon: bool,
}

/// The rows in the original's order: `before` (Edit Role, Copy Role Color), `after` (View Role Icon, View Role
/// Members), then Copy Role ID.
pub(in crate::ui) fn items(offers: Offers) -> Vec<Entry> {
    let mut rows = Vec::new();
    if offers.manage {
        rows.push(Row::new(Act::PluginEditRole, "Edit Role").icon("menu_edit_message"));
    }
    if offers.color {
        rows.push(
            Row::new(Act::PluginCopyRoleColor, "Copy Role Color").icon("settings_appearance"),
        );
    }
    if offers.icon {
        rows.push(Row::new(Act::PluginViewRoleIcon, "View Role Icon").icon("settings_image"));
    }
    rows.push(
        Row::new(Act::PluginViewRoleMembers, "View Role Members").trailing("plugin_role_members"),
    );
    rows.push(Row::new(Act::CopyId, dc_intl::tr("Copy Role ID")).icon("menu_id"));
    rows.into_iter().map(Entry::Row).collect()
}

/// `#rrggbb` of `colorString`.
fn color_string(color: u32) -> String {
    format!("#{color:06x}")
}

impl Root {
    /// Whether a role header takes a right click (the plugin is on).
    pub(in crate::ui) fn better_role_menu_on(&self) -> bool {
        icon_format().is_some()
    }

    /// The role menu's rows, or none with the plugin off or the role unknown.
    pub(in crate::ui) fn better_role_items(&self, role: Id, guild: Id) -> Vec<Entry> {
        if icon_format().is_none() {
            return Vec::new();
        }
        let Some(known) = self.stores.guilds.iter().find(|known| known.id == guild) else {
            return Vec::new();
        };
        if !known.role_info.contains_key(&role) && role != guild {
            return Vec::new();
        }
        items(Offers {
            manage: self.my_permission(guild, None, dc_state::permissions::MANAGE_ROLES),
            color: known
                .role_info
                .get(&role)
                .is_some_and(|info| info.color != 0),
            icon: known
                .role_icons
                .get(&role)
                .is_some_and(|icon| icon.icon.is_some()),
        })
    }

    /// Edit Role: Server Settings on Roles, the role open in the editor.
    pub(in crate::ui) fn better_role_edit(&mut self, role: Id, guild: Id, cx: &mut Context<Self>) {
        self.open_server_settings_at(guild, super::super::server_settings::Page::Roles, cx);
        self.select_role(role, cx);
    }

    /// Copy Role Color.
    pub(in crate::ui) fn better_role_copy_color(
        &self,
        role: Id,
        guild: Id,
        cx: &mut Context<Self>,
    ) {
        let Some(color) = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .and_then(|known| known.role_info.get(&role))
            .map(|info| info.color)
            .filter(|color| *color != 0)
        else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(color_string(color)));
    }

    /// View Role Icon: the uploaded icon in the picture viewer.
    pub(in crate::ui) fn better_role_view_icon(
        &mut self,
        role: Id,
        guild: Id,
        cx: &mut Context<Self>,
    ) {
        let Some(format) = icon_format() else {
            return;
        };
        let Some(hash) = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .and_then(|known| known.role_icons.get(&role))
            .and_then(|icon| icon.icon.clone())
        else {
            return;
        };
        let url = format!("https://cdn.discordapp.com/role-icons/{role}/{hash}.{format}");
        self.open_plain_picture(url.clone(), url, (128, 128), cx);
    }

    /// View Role Members: the role's member popout beside `anchor` (the header).
    pub(in crate::ui) fn better_role_members(
        &mut self,
        role: Id,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_role_popout(Some(role), anchor, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Row(row) => row.label.clone(),
                Entry::Separator => "-".to_owned(),
                Entry::Heading(text) => format!("# {text}"),
            })
            .collect()
    }

    #[test]
    fn rows_follow_what_the_role_offers_in_the_original_order() {
        let all = items(Offers {
            manage: true,
            color: true,
            icon: true,
        });
        assert_eq!(
            labels(&all),
            [
                "Edit Role",
                "Copy Role Color",
                "View Role Icon",
                "View Role Members",
                "Copy Role ID"
            ]
        );
        let bare = items(Offers {
            manage: false,
            color: false,
            icon: false,
        });
        assert_eq!(labels(&bare), ["View Role Members", "Copy Role ID"]);
    }

    #[test]
    fn colors_copy_as_css_hex_and_the_format_defaults_to_png() {
        assert_eq!(color_string(0x00ff7f), "#00ff7f");
        assert_eq!(color_string(0xabc), "#000abc");
        let mut state = State::default();
        assert_eq!(format(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(format(&state).as_deref(), Some("png"));
        state.set_value(ID, "roleIconFileFormat", &json!("webp"));
        assert_eq!(format(&state).as_deref(), Some("webp"));
    }
}
