//! The profile editor's pending changes and the requests that save them. Follows the profile settings store
//! (module 836602), the body builders of module 435911 and the three writers: the account update (module
//! 631670), the guild member update (module 159001) and the profile update (module 207803).
//!
//! A pending field is `Option<Option<T>>`: `None` is "untouched" (undefined in the bundle), `Some(None)` is a
//! removal (null) and `Some(Some(v))` a new value. Pending values that equal the saved value are dropped by the
//! caller, as the store's setters do.

use dc_state::Id;
use dc_state::display_name_styles::DisplayNameStyles;
use reqwest::Method;
use serde_json::{Value, json};

/// A request the editor sends: the method, the route and the JSON body.
#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    pub method: Method,
    pub route: String,
    pub body: Value,
}

/// An image chosen for the avatar or the banner: the data URI the API takes, plus the description for an avatar.
#[derive(Clone, Debug, PartialEq)]
pub struct Picked {
    pub image_uri: String,
    pub description: Option<String>,
}

/// One profile's pending changes. `guild` is `None` for the main profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub guild: Option<Id>,
    pub global_name: Option<Option<String>>,
    pub nickname: Option<Option<String>>,
    pub pronouns: Option<String>,
    pub bio: Option<String>,
    pub avatar: Option<Option<Picked>>,
    pub banner: Option<Option<Picked>>,
    pub accent_color: Option<Option<u32>>,
    pub theme_colors: Option<Option<[u32; 2]>>,
    pub avatar_decoration: Option<Option<Id>>,
    pub nameplate: Option<Option<Id>>,
    pub profile_effect: Option<Option<Id>>,
    pub profile_frame: Option<Option<Id>>,
    pub display_name_styles: Option<Option<DisplayNameStyles>>,
}

/// The collectible sku ids the profile already carries, so a change of one kind keeps the other
/// (`collectibles_sku_ids` in module 435911 rebuilds the whole list).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Equipped {
    pub profile_effect: Option<Id>,
    pub profile_frame: Option<Id>,
}

impl Pending {
    pub fn new(guild: Option<Id>) -> Self {
        Self {
            guild,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::new(self.guild)
    }

    /// Whether the change needs Nitro: a name style or a profile theme.
    pub fn needs_premium(&self) -> bool {
        matches!(self.display_name_styles, Some(Some(_)))
            || matches!(self.theme_colors, Some(Some(_)))
    }

    /// The `PATCH /users/@me` body (module 631670 `_L` over module 435911 `o`), or `None` when nothing in it
    /// changed. Only the main profile writes it.
    pub fn account_write(&self) -> Option<Write> {
        if self.guild.is_some() {
            return None;
        }
        let mut body = serde_json::Map::new();
        if let Some(name) = &self.global_name {
            body.insert("global_name".into(), json!(name));
        }
        insert_avatar(&mut body, &self.avatar);
        if let Some(decoration) = &self.avatar_decoration {
            body.insert("avatar_decoration_sku_id".into(), sku(decoration));
        }
        if let Some(plate) = &self.nameplate {
            body.insert("nameplate_sku_id".into(), sku(plate));
        }
        insert_styles(&mut body, &self.display_name_styles);
        (!body.is_empty()).then(|| Write {
            method: Method::PATCH,
            route: "/users/@me".into(),
            body: Value::Object(body),
        })
    }

    /// The `PATCH /guilds/{id}/members/@me` body (module 159001 `GL` over module 435911 `p`).
    pub fn member_write(&self) -> Option<Write> {
        let guild = self.guild?;
        let mut body = serde_json::Map::new();
        // `pendingNickname ?? ""`: clearing a nickname sends an empty string.
        if let Some(nick) = &self.nickname {
            body.insert("nick".into(), json!(nick.clone().unwrap_or_default()));
        }
        insert_avatar(&mut body, &self.avatar);
        if let Some(decoration) = &self.avatar_decoration {
            body.insert("avatar_decoration_sku_id".into(), sku(decoration));
        }
        if let Some(plate) = &self.nameplate {
            let value = match plate {
                Some(id) => json!({"sku_id": id.to_string()}),
                None => Value::Null,
            };
            body.insert("collectibles".into(), json!({"nameplate": value}));
        }
        insert_styles(&mut body, &self.display_name_styles);
        (!body.is_empty()).then(|| Write {
            method: Method::PATCH,
            route: format!("/guilds/{guild}/members/@me"),
            body: Value::Object(body),
        })
    }

    /// The profile body (module 435911 `d`): `/users/@me/profile`, or `/guilds/{id}/profile/@me` for a server
    /// profile.
    pub fn profile_write(&self, equipped: &Equipped) -> Option<Write> {
        let mut body = serde_json::Map::new();
        match &self.banner {
            Some(None) => {
                body.insert("banner".into(), Value::Null);
            }
            Some(Some(picked)) => {
                body.insert("banner".into(), json!(picked.image_uri));
            }
            None => {}
        }
        if let Some(bio) = &self.bio {
            body.insert("bio".into(), json!(bio));
        }
        if let Some(pronouns) = &self.pronouns {
            body.insert("pronouns".into(), json!(pronouns));
        }
        if let Some(accent) = &self.accent_color {
            body.insert("accent_color".into(), json!(accent));
        }
        if let Some(theme) = &self.theme_colors {
            body.insert("theme_colors".into(), json!(theme));
        }
        if self.profile_effect.is_some() || self.profile_frame.is_some() {
            let effect = self.profile_effect.unwrap_or(equipped.profile_effect);
            let frame = self.profile_frame.unwrap_or(equipped.profile_frame);
            let ids: Vec<String> = [effect, frame]
                .into_iter()
                .flatten()
                .map(|id| id.to_string())
                .collect();
            body.insert("collectibles_sku_ids".into(), json!(ids));
        }
        (!body.is_empty()).then(|| Write {
            method: Method::PATCH,
            route: match self.guild {
                Some(guild) => format!("/guilds/{guild}/profile/@me"),
                None => "/users/@me/profile".into(),
            },
            body: Value::Object(body),
        })
    }

    /// Every request a Save sends, in the order the bundle sends them: the account or member write, then the
    /// profile write.
    pub fn writes(&self, equipped: &Equipped) -> Vec<Write> {
        [
            self.account_write(),
            self.member_write(),
            self.profile_write(equipped),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

fn sku(value: &Option<Id>) -> Value {
    match value {
        Some(id) => json!(id.to_string()),
        None => Value::Null,
    }
}

fn insert_avatar(body: &mut serde_json::Map<String, Value>, avatar: &Option<Option<Picked>>) {
    match avatar {
        Some(None) => {
            body.insert("avatar".into(), Value::Null);
        }
        Some(Some(picked)) => {
            body.insert("avatar".into(), json!(picked.image_uri));
            body.insert("avatar_description".into(), json!(picked.description));
        }
        None => {}
    }
}

/// Module 631670: a style writes its three ids, a removal writes three nulls.
fn insert_styles(
    body: &mut serde_json::Map<String, Value>,
    styles: &Option<Option<DisplayNameStyles>>,
) {
    match styles {
        Some(Some(style)) => {
            body.insert("display_name_font_id".into(), json!(style.font_id));
            body.insert("display_name_effect_id".into(), json!(style.effect_id));
            body.insert("display_name_colors".into(), json!(style.colors));
        }
        Some(None) => {
            body.insert("display_name_font_id".into(), Value::Null);
            body.insert("display_name_effect_id".into(), Value::Null);
            body.insert("display_name_colors".into(), Value::Null);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pending_writes_nothing() {
        assert!(Pending::new(None).writes(&Equipped::default()).is_empty());
        assert!(Pending::new(Some(3)).is_empty());
    }

    #[test]
    fn account_body_carries_only_changed_fields() {
        let mut p = Pending::new(None);
        p.global_name = Some(Some("Wren".into()));
        p.avatar_decoration = Some(None);
        p.nameplate = Some(Some(77));
        let w = p.account_write().unwrap();
        assert_eq!(w.method, Method::PATCH);
        assert_eq!(w.route, "/users/@me");
        assert_eq!(
            w.body,
            json!({"global_name": "Wren", "avatar_decoration_sku_id": null, "nameplate_sku_id": "77"})
        );
    }

    #[test]
    fn name_style_writes_three_fields() {
        let mut p = Pending::new(None);
        p.display_name_styles = Some(Some(DisplayNameStyles {
            font_id: 3,
            effect_id: 2,
            colors: vec![255, 65280],
        }));
        assert_eq!(
            p.account_write().unwrap().body,
            json!({"display_name_font_id": 3, "display_name_effect_id": 2, "display_name_colors": [255, 65280]})
        );
        p.display_name_styles = Some(None);
        assert_eq!(
            p.account_write().unwrap().body,
            json!({"display_name_font_id": null, "display_name_effect_id": null, "display_name_colors": null})
        );
    }

    #[test]
    fn avatar_upload_and_removal() {
        let mut p = Pending::new(None);
        p.avatar = Some(Some(Picked {
            image_uri: "data:image/png;base64,AAAA".into(),
            description: None,
        }));
        assert_eq!(
            p.account_write().unwrap().body,
            json!({"avatar": "data:image/png;base64,AAAA", "avatar_description": null})
        );
        p.avatar = Some(None);
        assert_eq!(p.account_write().unwrap().body, json!({"avatar": null}));
    }

    #[test]
    fn member_body_and_route() {
        let mut p = Pending::new(Some(42));
        p.nickname = Some(None);
        p.nameplate = Some(Some(9));
        p.avatar_decoration = Some(Some(5));
        let w = p.member_write().unwrap();
        assert_eq!(w.route, "/guilds/42/members/@me");
        assert_eq!(
            w.body,
            json!({"nick": "", "avatar_decoration_sku_id": "5", "collectibles": {"nameplate": {"sku_id": "9"}}})
        );
        p.nameplate = Some(None);
        assert_eq!(
            p.member_write().unwrap().body["collectibles"],
            json!({"nameplate": null})
        );
        assert!(p.account_write().is_none());
    }

    #[test]
    fn profile_body_main_and_guild() {
        let mut p = Pending::new(None);
        p.bio = Some("hello".into());
        p.pronouns = Some("they/them".into());
        p.accent_color = Some(Some(0x112233));
        p.theme_colors = Some(Some([1, 2]));
        p.banner = Some(None);
        let w = p.profile_write(&Equipped::default()).unwrap();
        assert_eq!(w.route, "/users/@me/profile");
        assert_eq!(
            w.body,
            json!({"banner": null, "bio": "hello", "pronouns": "they/them", "accent_color": 1122867, "theme_colors": [1, 2]})
        );
        p.guild = Some(7);
        assert_eq!(
            p.profile_write(&Equipped::default()).unwrap().route,
            "/guilds/7/profile/@me"
        );
    }

    #[test]
    fn collectibles_keep_the_other_kind() {
        let mut p = Pending::new(None);
        p.profile_effect = Some(Some(10));
        let equipped = Equipped {
            profile_effect: Some(1),
            profile_frame: Some(20),
        };
        assert_eq!(
            p.profile_write(&equipped).unwrap().body,
            json!({"collectibles_sku_ids": ["10", "20"]})
        );
        p.profile_effect = Some(None);
        p.profile_frame = Some(None);
        assert_eq!(
            p.profile_write(&equipped).unwrap().body,
            json!({"collectibles_sku_ids": []})
        );
    }

    #[test]
    fn writes_order_is_account_then_profile() {
        let mut p = Pending::new(None);
        p.global_name = Some(None);
        p.bio = Some(String::new());
        let writes = p.writes(&Equipped::default());
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].route, "/users/@me");
        assert_eq!(writes[1].route, "/users/@me/profile");
    }
}
