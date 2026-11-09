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

impl Root {
    pub(super) fn profile_banner_image(&self, user: Id) -> Option<Arc<gpui::Image>> {
        let body = self.fetched.get(&format!("profile:{user}"))?;
        let (key, _) = banner_source(user, self.guild, body)?;
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
