//! ViewIcons (Vencord `src/plugins/viewIcons`, GPL-3.0-or-later): pictures opened full size in the picture viewer.
//! The user menu gains a group before its last one with View Avatar, View Server Avatar (a server avatar) and View
//! Avatar Decoration (an equipped decoration); the server menu View Icon and View Banner; a group DM's menu View
//! Icon. Every row carries ImageIcon. `openImage`: a picture asked for as animated stays `.webp`, a client asset
//! (`/assets/…`, here the default avatars) stays `.png`, anything else takes `format`; the viewer shows it at
//! `imgSize` and Open in Browser and Save use `size=4096`. Avatars open at 512 by 512, banners 1024 wide.
//!
//! GAP: the original also makes the avatars and banners of profiles and the DM header's icons clickable; those
//! clicks are not wired here yet. Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use crate::plugins::State;
use dc_state::Id;
use gpui::Context;
use serde_json::Value;

const ID: &str = "ViewIcons";
const CDN: &str = "https://cdn.discordapp.com";

/// `format` and `imgSize` while the plugin is on.
fn read(state: &State) -> Option<(String, String)> {
    if !state.is_on(ID) {
        return None;
    }
    let format = match state.value(ID, "format") {
        Value::String(format) if ["webp", "png", "jpg"].contains(&format.as_str()) => format,
        _ => "webp".to_owned(),
    };
    let size = match state.value(ID, "imgSize") {
        Value::String(size)
            if ["128", "256", "512", "1024", "2048", "4096"].contains(&size.as_str()) =>
        {
            size
        }
        _ => "1024".to_owned(),
    };
    Some((format, size))
}

fn settings() -> Option<(String, String)> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(read)
}

/// What a picture is: its address without extension or query, whether it was asked for animated, and whether it
/// is a client asset that stays PNG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Picture {
    pub(super) path: String,
    pub(super) animated: bool,
    pub(super) asset: bool,
    /// The rest of the query the address keeps (`passthrough=true` of a decoration).
    pub(super) keep: &'static str,
}

/// `openImage`: the address the viewer shows and the one Open in Browser and Save use.
pub(super) fn addresses(picture: &Picture, format: &str, size: &str) -> (String, String) {
    let extension = if picture.asset {
        "png"
    } else if picture.animated {
        "webp"
    } else {
        format
    };
    let animated = if picture.animated {
        "&animated=true"
    } else {
        ""
    };
    let at = |size: &str| {
        format!(
            "{}.{extension}?size={size}{animated}{}",
            picture.path, picture.keep
        )
    };
    (at(size), at("4096"))
}

fn hash_picture(path: String, hash: &str) -> Picture {
    Picture {
        path: format!("{path}/{hash}"),
        animated: hash.starts_with("a_"),
        asset: false,
        keep: "",
    }
}

/// A row of the plugin: its act and label.
const AVATAR: (Act, &str) = (Act::PluginViewAvatar, "View Avatar");
const SERVER_AVATAR: (Act, &str) = (Act::PluginViewServerAvatar, "View Server Avatar");
const DECORATION: (Act, &str) = (Act::PluginViewDecoration, "View Avatar Decoration");
const ICON: (Act, &str) = (Act::PluginViewIcon, "View Icon");
const BANNER: (Act, &str) = (Act::PluginViewBanner, "View Banner");

/// `children.splice(-1, 0, <MenuGroup>…)`: the rows as their own group before the menu's last group.
pub(super) fn insert_group(mut items: Vec<Entry>, rows: &[(Act, &str)]) -> Vec<Entry> {
    if rows.is_empty() {
        return items;
    }
    let mut group: Vec<Entry> = rows
        .iter()
        .map(|(act, label)| Entry::Row(Row::new(*act, *label).icon("settings_image")))
        .collect();
    match items
        .iter()
        .rposition(|entry| matches!(entry, Entry::Separator))
    {
        Some(last) => {
            group.push(Entry::Separator);
            items.splice(last + 1..last + 1, group);
        }
        None if items.is_empty() => items = group,
        None => {
            group.push(Entry::Separator);
            items.splice(0..0, group);
        }
    }
    items
}

impl Root {
    /// The menu with the plugin's group, for a user, a server or a group DM; the menu as it is otherwise.
    pub(in crate::ui) fn with_view_icons_rows(
        &self,
        items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if settings().is_none() {
            return items;
        }
        let rows = self.view_icons_rows(target);
        insert_group(items, &rows)
    }

    fn view_icons_rows(&self, target: Target) -> Vec<(Act, &'static str)> {
        match target {
            Target::User { user, guild } => {
                let Some(known) = self.view_icons_user(user) else {
                    return Vec::new();
                };
                let mut rows = vec![AVATAR];
                if guild
                    .and_then(|guild| self.stores.members.get(guild, user))
                    .is_some_and(|member| member.avatar.is_some())
                {
                    rows.push(SERVER_AVATAR);
                }
                if known.avatar_decoration.is_some() {
                    rows.push(DECORATION);
                }
                rows
            }
            Target::Guild(guild) => {
                let Some(known) = self.stores.guilds.iter().find(|known| known.id == guild) else {
                    return Vec::new();
                };
                let mut rows = Vec::new();
                if known.icon.is_some() {
                    rows.push(ICON);
                }
                if known.banner.is_some() {
                    rows.push(BANNER);
                }
                rows
            }
            // `gdm-context`: a group DM.
            Target::Dm(channel)
                if self
                    .stores
                    .private_channels
                    .iter()
                    .any(|dm| dm.id == channel && dm.kind == 3) =>
            {
                vec![ICON]
            }
            _ => Vec::new(),
        }
    }

    fn view_icons_user(&self, user: Id) -> Option<dc_state::User> {
        self.stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| self.stores.current_user.clone().filter(|me| me.id == user))
    }

    /// The picture a row opens.
    fn view_icons_picture(&self, act: Act, target: Target) -> Option<(Picture, (u32, u32))> {
        const AVATAR_BOX: (u32, u32) = (512, 512);
        match (act, target) {
            (Act::PluginViewAvatar, Target::User { user, .. }) => {
                let known = self.view_icons_user(user)?;
                Some((
                    match &known.avatar {
                        Some(hash) => hash_picture(format!("{CDN}/avatars/{user}"), hash),
                        // `getUserAvatarURL` without an avatar is a client asset.
                        None => Picture {
                            path: format!("{CDN}/embed/avatars/{}", known.default_avatar_index()),
                            animated: false,
                            asset: true,
                            keep: "",
                        },
                    },
                    AVATAR_BOX,
                ))
            }
            (Act::PluginViewServerAvatar, Target::User { user, guild }) => {
                let guild = guild?;
                let hash = self.stores.members.get(guild, user)?.avatar.clone()?;
                Some((
                    hash_picture(format!("{CDN}/guilds/{guild}/users/{user}/avatars"), &hash),
                    AVATAR_BOX,
                ))
            }
            (Act::PluginViewDecoration, Target::User { user, .. }) => {
                let asset = self
                    .view_icons_user(user)?
                    .avatar_decoration?
                    .asset
                    .clone()?;
                // `getAvatarDecorationURL({size: 1024, canAnimate: true})`: `passthrough=true` keeps the animation.
                Some((
                    Picture {
                        path: format!("{CDN}/avatar-decoration-presets/{asset}"),
                        animated: false,
                        asset: false,
                        keep: "&passthrough=true",
                    },
                    AVATAR_BOX,
                ))
            }
            (Act::PluginViewIcon, Target::Guild(guild)) => {
                let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
                Some((
                    hash_picture(format!("{CDN}/icons/{guild}"), known.icon.as_deref()?),
                    AVATAR_BOX,
                ))
            }
            (Act::PluginViewBanner, Target::Guild(guild)) => {
                let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
                // A server banner is 16:9.
                Some((
                    hash_picture(format!("{CDN}/banners/{guild}"), known.banner.as_deref()?),
                    (1024, 576),
                ))
            }
            (Act::PluginViewIcon, Target::Dm(channel)) => {
                let dm = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|dm| dm.id == channel)?;
                // `getChannelIconURL`: the group's icon, or its default group picture (a client asset).
                let url = super::super::unread_dms::group_icon(dm).1;
                let bare = url.split('?').next().unwrap_or(&url);
                let path = bare
                    .rsplit_once('.')
                    .map_or(bare, |(path, _)| path)
                    .to_owned();
                Some((
                    Picture {
                        asset: path.contains("/assets/"),
                        path,
                        animated: false,
                        keep: "",
                    },
                    AVATAR_BOX,
                ))
            }
            _ => None,
        }
    }

    /// A row ran: its picture in the viewer.
    pub(in crate::ui) fn view_icons_open(
        &mut self,
        act: Act,
        target: Target,
        cx: &mut Context<Self>,
    ) {
        let Some((format, size)) = settings() else {
            return;
        };
        let Some((picture, frame)) = self.view_icons_picture(act, target) else {
            return;
        };
        let (shown, original) = addresses(&picture, &format, &size);
        self.open_plain_picture(shown, original, frame, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn addresses_follow_open_image() {
        let still = hash_picture("https://cdn.discordapp.com/avatars/1".to_owned(), "abc");
        assert_eq!(
            addresses(&still, "png", "1024"),
            (
                "https://cdn.discordapp.com/avatars/1/abc.png?size=1024".to_owned(),
                "https://cdn.discordapp.com/avatars/1/abc.png?size=4096".to_owned()
            )
        );
        let animated = hash_picture("https://cdn.discordapp.com/avatars/1".to_owned(), "a_abc");
        assert_eq!(
            addresses(&animated, "jpg", "512").0,
            "https://cdn.discordapp.com/avatars/1/a_abc.webp?size=512&animated=true"
        );
        let asset = Picture {
            path: "https://cdn.discordapp.com/embed/avatars/3".to_owned(),
            animated: false,
            asset: true,
            keep: "",
        };
        assert_eq!(
            addresses(&asset, "webp", "1024").0,
            "https://cdn.discordapp.com/embed/avatars/3.png?size=1024"
        );
        let decoration = Picture {
            path: "https://cdn.discordapp.com/avatar-decoration-presets/x".to_owned(),
            animated: false,
            asset: false,
            keep: "&passthrough=true",
        };
        assert_eq!(
            addresses(&decoration, "webp", "1024").1,
            "https://cdn.discordapp.com/avatar-decoration-presets/x.webp?size=4096&passthrough=true"
        );
    }

    #[test]
    fn settings_default_to_webp_at_1024() {
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some(("webp".to_owned(), "1024".to_owned())));
        state.set_value(ID, "imgSize", &json!("4096"));
        state.set_value(ID, "format", &json!("png"));
        assert_eq!(read(&state), Some(("png".to_owned(), "4096".to_owned())));
    }

    #[test]
    fn the_group_goes_before_the_last_one() {
        let menu = vec![
            Entry::Row(Row::new(Act::MarkRead, "a")),
            Entry::Separator,
            Entry::Row(Row::new(Act::CopyId, "id")),
        ];
        let out = insert_group(menu, &[AVATAR]);
        let labels: Vec<String> = out
            .iter()
            .map(|entry| match entry {
                Entry::Row(row) => row.label.clone(),
                Entry::Separator => "-".to_owned(),
            })
            .collect();
        assert_eq!(labels, ["a", "-", "View Avatar", "-", "id"]);
    }
}
