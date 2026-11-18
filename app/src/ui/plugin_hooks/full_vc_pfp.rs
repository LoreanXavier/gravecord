//! FullVCPFP (Equicord `src/equicordplugins/fullVcPfp`, GPL-3.0-or-later): a call tile without video shows the person's
//! avatar filling the tile (`background: center / contain`, no rounding, the 80px circle hidden), at 1024px, the
//! server avatar first (`getUserAvatarUrl(user, guildId, isSpeaking, 1024)`). Not part of the pinned client.

use super::super::Root;
use dc_state::{Id, User};
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, img, px};

/// `getUserAvatarUrl`'s address at 1024px: the server avatar in `guild` when the member has one, else the account's.
pub(super) fn full_avatar_url(user: &User, guild_avatar: Option<(Id, &str)>) -> Option<String> {
    match guild_avatar {
        Some((guild, hash)) => Some(format!(
            "https://cdn.discordapp.com/guilds/{guild}/users/{}/avatars/{hash}.webp?size=1024",
            user.id
        )),
        None => user.avatar.as_deref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.webp?size=1024",
                user.id
            )
        }),
    }
}

impl Root {
    /// The full-tile avatar of `user` in `guild`'s call, or `None` with the plugin off (or no picture to ask for).
    pub(in crate::ui) fn full_vc_avatar(
        &self,
        user: &User,
        guild: Option<Id>,
        tile: (f32, f32),
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() || !crate::plugin_on!("FullVCPFP") {
            return None;
        }
        let member_hash = guild.and_then(|guild| {
            self.stores
                .members
                .get(guild, user.id)?
                .avatar
                .clone()
                .map(|hash| (guild, hash))
        });
        let url = full_avatar_url(
            user,
            member_hash
                .as_ref()
                .map(|(guild, hash)| (*guild, hash.as_str())),
        )?;
        let key = format!("avatar-full:{url}");
        let picture = self.images.get(&key).cloned();
        if picture.is_none() && !self.requested.contains(&key) {
            // Asked for on the next frame (`prepare_live_avatars`); the 80px avatar stands in until it arrives.
            self.avatar_wants.borrow_mut().push((key, url));
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                // `center / contain` of a square picture: the tile's shorter side.
                .children(picture.map(|picture| img(picture).size(px(tile.0.min(tile.1)))))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_avatar_comes_first_at_1024() {
        let user = User {
            id: 7,
            avatar: Some("abc".to_owned()),
            ..User::default()
        };
        assert_eq!(
            full_avatar_url(&user, None).as_deref(),
            Some("https://cdn.discordapp.com/avatars/7/abc.webp?size=1024")
        );
        assert_eq!(
            full_avatar_url(&user, Some((5, "def"))).as_deref(),
            Some("https://cdn.discordapp.com/guilds/5/users/7/avatars/def.webp?size=1024")
        );
        assert_eq!(full_avatar_url(&User::default(), None), None);
    }
}
