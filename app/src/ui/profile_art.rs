//! Profile banner precedence, popout size and CDN routes (527212/851767/207634/486020).
use super::Root;
use dc_state::Id;
use serde_json::Value;
use std::sync::Arc;

pub(super) fn banner_source(user: Id, guild: Option<Id>, body: &Value) -> Option<(String, String)> {
    let member = &body["guild_member_profile"];
    let member_guild = member["guild_id"]
        .as_str()
        .and_then(|id| id.parse::<Id>().ok());
    let scoped = guild
        .filter(|guild| Some(*guild) == member_guild)
        .zip(member["banner"].as_str());
    let url = if let Some((guild, hash)) = scoped {
        format!(
            "https://cdn.discordapp.com/guilds/{guild}/users/{user}/banners/{hash}.png?size=300"
        )
    } else {
        let hash = body["user_profile"]["banner"].as_str()?;
        format!("https://cdn.discordapp.com/banners/{user}/{hash}.png?size=300")
    };
    Some((format!("profile-banner:{url}"), url))
}

/// The banner file of a user whose banner is animated (`a_`), asked for the way module 486020 (`getUserBannerURL`)
/// does: `webp` at a power-of-two size with `animated=true`.
pub(super) fn live_banner_source(
    user: Id,
    guild: Option<Id>,
    body: &Value,
    side: u32,
) -> Option<(String, String)> {
    let member = &body["guild_member_profile"];
    let member_guild = member["guild_id"]
        .as_str()
        .and_then(|id| id.parse::<Id>().ok());
    let scoped = guild
        .filter(|guild| Some(*guild) == member_guild)
        .zip(member["banner"].as_str());
    let (base, hash) = if let Some((guild, hash)) = scoped {
        (
            format!("https://cdn.discordapp.com/guilds/{guild}/users/{user}/banners/{hash}"),
            hash,
        )
    } else {
        let hash = body["user_profile"]["banner"].as_str()?;
        (
            format!("https://cdn.discordapp.com/banners/{user}/{hash}"),
            hash,
        )
    };
    super::avatar_play::is_animated(hash).then(|| {
        (
            format!("banner-live:{base}:{side}"),
            super::avatar_play::animated_url(&base, side),
        )
    })
}

impl Root {
    /// The playing banner of a profile shown in a `width` x `height` box, or `None` when it is still. A profile card
    /// plays its banner whenever it is shown (module 891522 draws it as hovered).
    pub(super) fn live_banner(
        &self,
        user: Id,
        width: f32,
        height: f32,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.live_banner_in(user, self.guild, width, height, cx)
    }

    pub(super) fn live_banner_in(
        &self,
        user: Id,
        guild: Option<Id>,
        width: f32,
        height: f32,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use gpui::{IntoElement, ParentElement, Styled, div};
        let body = self.fetched.get(&format!("profile:{user}"))?;
        let scale = self.ui_scale.get();
        let side = super::avatar_play::file_side(width, scale);
        let (key, url) = live_banner_source(user, guild, body, side)?;
        if !super::avatar_play::should_animate(
            super::motion::window_active(),
            true,
            super::motion::reduced(),
            true,
            self.pref_on("animate_gifs", true),
        ) {
            return None;
        }
        let Some(image) = self.images.get(&key).cloned() else {
            self.want_image(key, url, cx);
            return None;
        };
        let pixels = (
            (width * scale).round() as u32,
            (height * scale).round() as u32,
        );
        Some(
            div()
                .absolute()
                .size_full()
                .child(self.gif_art_pixels(&key, image, pixels, 0., None, cx))
                .into_any_element(),
        )
    }

    pub(super) fn profile_banner_image(&self, user: Id) -> Option<Arc<gpui::Image>> {
        self.profile_banner_image_in(user, self.guild)
    }

    pub(super) fn profile_banner_image_in(
        &self,
        user: Id,
        guild: Option<Id>,
    ) -> Option<Arc<gpui::Image>> {
        let body = self.fetched.get(&format!("profile:{user}"))?;
        let (key, _) = banner_source(user, guild, body)?;
        self.images.get(&key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn banners_use_only_the_matching_guild_override_and_fall_back_to_global() {
        let mut body = json!({"user_profile":{"banner":"global"},
            "guild_member_profile":{"guild_id":"9", "banner":"server"}});
        assert_eq!(
            banner_source(3, Some(9), &body).unwrap().1,
            "https://cdn.discordapp.com/guilds/9/users/3/banners/server.png?size=300"
        );
        let global = "https://cdn.discordapp.com/banners/3/global.png?size=300";
        assert_eq!(banner_source(3, Some(10), &body).unwrap().1, global);
        assert_eq!(banner_source(3, None, &body).unwrap().1, global);
        body["guild_member_profile"]["banner"] = Value::Null;
        assert_eq!(banner_source(3, Some(9), &body).unwrap().1, global);
        body["user_profile"]["banner"] = Value::Null;
        assert!(banner_source(3, Some(9), &body).is_none());
    }
}
