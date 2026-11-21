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

/// A game inside a profile game widget (`289173.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WidgetGame {
    pub game_id: String,
    pub comment: Option<String>,
    pub tags: Vec<String>,
}

/// One section of a Personal widget (`210598.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonalSection {
    Cover { title: String, subtitle: String },
    Fields { fields: Vec<PersonalField> },
}

/// One row/card inside a Personal widget's `FIELDS` section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonalField {
    pub title: String,
    pub description: String,
}

/// The payload inside `widget.data` for `PUT /users/@me/widgets`.
#[derive(Clone, Debug, PartialEq)]
pub enum WidgetData {
    FavoriteGames {
        games: Vec<WidgetGame>,
    },
    CurrentGames {
        games: Vec<WidgetGame>,
    },
    WantToPlayGames {
        games: Vec<WidgetGame>,
    },
    PlayedGames {
        games: Vec<WidgetGame>,
    },
    Application {
        application_id: String,
    },
    Personal {
        header: String,
        sections: Vec<PersonalSection>,
    },
    ClipsGallery {
        clips: Vec<Value>,
    },
}

/// An editable widget entry on the user's Board (`widgets` array on profile + `PUT /users/@me/widgets`).
#[derive(Clone, Debug, PartialEq)]
pub struct UserWidget {
    pub id: Option<String>,
    pub data: WidgetData,
}

impl WidgetData {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::FavoriteGames { .. } => "favorite_games",
            Self::CurrentGames { .. } => "current_games",
            Self::WantToPlayGames { .. } => "want_to_play_games",
            Self::PlayedGames { .. } => "played_games",
            Self::Application { .. } => "application",
            Self::Personal { .. } => "personal",
            Self::ClipsGallery { .. } => "clips_gallery",
        }
    }

    /// Unique key within a user's Board (`getUniqueKey`, `289173.js` / `633075.js`).
    pub fn unique_key(&self) -> String {
        match self {
            Self::Application { application_id } => format!("application-{application_id}"),
            _ => self.kind().to_owned(),
        }
    }

    pub fn max_games(&self) -> usize {
        match self {
            Self::FavoriteGames { .. } => 1,
            Self::CurrentGames { .. } => 5,
            Self::WantToPlayGames { .. } | Self::PlayedGames { .. } => 20,
            _ => 0,
        }
    }

    pub fn supports_comment(&self) -> bool {
        matches!(self, Self::FavoriteGames { .. })
    }

    pub fn supports_tags(&self) -> bool {
        matches!(self, Self::FavoriteGames { .. } | Self::CurrentGames { .. })
    }

    pub fn games(&self) -> Option<&[WidgetGame]> {
        match self {
            Self::FavoriteGames { games }
            | Self::CurrentGames { games }
            | Self::WantToPlayGames { games }
            | Self::PlayedGames { games } => Some(games),
            _ => None,
        }
    }

    pub fn games_mut(&mut self) -> Option<&mut Vec<WidgetGame>> {
        match self {
            Self::FavoriteGames { games }
            | Self::CurrentGames { games }
            | Self::WantToPlayGames { games }
            | Self::PlayedGames { games } => Some(games),
            _ => None,
        }
    }

    pub fn is_discardable(&self) -> bool {
        match self {
            Self::FavoriteGames { games }
            | Self::CurrentGames { games }
            | Self::WantToPlayGames { games }
            | Self::PlayedGames { games } => games.is_empty(),
            Self::Application { .. } => false,
            Self::Personal { sections, .. } => sections.iter().all(|section| match section {
                PersonalSection::Cover { title, subtitle } => {
                    title.trim().is_empty() && subtitle.trim().is_empty()
                }
                PersonalSection::Fields { fields } => fields
                    .iter()
                    .all(|f| f.title.trim().is_empty() && f.description.trim().is_empty()),
            }),
            Self::ClipsGallery { clips } => clips.is_empty(),
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::FavoriteGames { games }
            | Self::CurrentGames { games }
            | Self::WantToPlayGames { games }
            | Self::PlayedGames { games } => !games.is_empty() && games.len() <= self.max_games(),
            Self::Application { application_id } => !application_id.is_empty(),
            Self::Personal { header, .. } => !header.trim().is_empty() && !self.is_discardable(),
            Self::ClipsGallery { clips } => !clips.is_empty(),
        }
    }

    pub fn to_json(&self) -> Value {
        let games_json = |games: &[WidgetGame]| -> Vec<Value> {
            games
                .iter()
                .map(|game| {
                    let mut entry = serde_json::Map::new();
                    entry.insert("game_id".into(), json!(game.game_id));
                    if self.supports_comment()
                        && let Some(comment) = game.comment.as_deref()
                        && !comment.is_empty()
                    {
                        entry.insert("comment".into(), json!(comment));
                    }
                    if self.supports_tags() && !game.tags.is_empty() {
                        entry.insert("tags".into(), json!(game.tags));
                    }
                    Value::Object(entry)
                })
                .collect()
        };
        match self {
            Self::FavoriteGames { games }
            | Self::CurrentGames { games }
            | Self::WantToPlayGames { games }
            | Self::PlayedGames { games } => json!({
                "type": self.kind(),
                "games": games_json(games),
            }),
            Self::Application { application_id } => json!({
                "type": "application",
                "application_id": application_id,
            }),
            Self::Personal { header, sections } => {
                let encoded_sections: Vec<Value> = sections
                    .iter()
                    .filter_map(|section| match section {
                        PersonalSection::Cover { title, subtitle }
                            if !title.trim().is_empty() || !subtitle.trim().is_empty() =>
                        {
                            Some(json!({
                                "type": "cover",
                                "title": title,
                                "subtitle": subtitle,
                            }))
                        }
                        PersonalSection::Fields { fields } => {
                            let non_empty: Vec<Value> = fields
                                .iter()
                                .filter(|f| {
                                    !f.title.trim().is_empty() || !f.description.trim().is_empty()
                                })
                                .map(|f| {
                                    json!({
                                        "title": f.title,
                                        "description": f.description,
                                    })
                                })
                                .collect();
                            (!non_empty.is_empty()).then(|| {
                                json!({
                                    "type": "fields",
                                    "fields": non_empty,
                                })
                            })
                        }
                        _ => None,
                    })
                    .collect();
                json!({
                    "type": "personal",
                    "header": header,
                    "sections": encoded_sections,
                })
            }
            Self::ClipsGallery { clips } => json!({
                "type": "clips_gallery",
                "clips": clips,
            }),
        }
    }
}

impl UserWidget {
    pub fn from_value(value: &Value) -> Option<Self> {
        let id = value["id"].as_str().map(str::to_owned);
        let data = &value["data"];
        let parse_games = || -> Vec<WidgetGame> {
            data["games"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| {
                    let game_id = g["game_id"].as_str()?.to_owned();
                    let comment = g["comment"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                    let tags = g["tags"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect();
                    Some(WidgetGame {
                        game_id,
                        comment,
                        tags,
                    })
                })
                .collect()
        };
        let parsed = match data["type"].as_str()? {
            "favorite_games" => WidgetData::FavoriteGames {
                games: parse_games(),
            },
            "current_games" => WidgetData::CurrentGames {
                games: parse_games(),
            },
            "want_to_play_games" => WidgetData::WantToPlayGames {
                games: parse_games(),
            },
            "played_games" => WidgetData::PlayedGames {
                games: parse_games(),
            },
            "application" => WidgetData::Application {
                application_id: data["application_id"].as_str()?.to_owned(),
            },
            "personal" => {
                let header = data["header"].as_str().unwrap_or_default().to_owned();
                let sections = data["sections"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| match s["type"].as_str()? {
                        "cover" => Some(PersonalSection::Cover {
                            title: s["title"].as_str().unwrap_or_default().to_owned(),
                            subtitle: s["subtitle"].as_str().unwrap_or_default().to_owned(),
                        }),
                        "fields" => Some(PersonalSection::Fields {
                            fields: s["fields"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|f| PersonalField {
                                    title: f["title"].as_str().unwrap_or_default().to_owned(),
                                    description: f["description"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_owned(),
                                })
                                .collect(),
                        }),
                        _ => None,
                    })
                    .collect();
                WidgetData::Personal { header, sections }
            }
            "clips_gallery" => WidgetData::ClipsGallery {
                clips: data["clips"].as_array().cloned().unwrap_or_default(),
            },
            _ => return None,
        };
        Some(Self { id, data: parsed })
    }

    pub fn to_json(&self) -> Value {
        let mut obj = serde_json::Map::new();
        if let Some(id) = &self.id {
            obj.insert("id".into(), json!(id));
        }
        obj.insert("data".into(), self.data.to_json());
        Value::Object(obj)
    }
}

pub fn parse_widgets(profile: &Value) -> Vec<UserWidget> {
    profile["widgets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(UserWidget::from_value)
        .collect()
}

/// Filter out discardable empty widgets matching module 61881 (`getSaveablePendingWidgets`).
pub fn saveable_widgets(widgets: &[UserWidget]) -> Vec<UserWidget> {
    widgets
        .iter()
        .filter(|w| !w.data.is_discardable())
        .cloned()
        .collect()
}

pub fn widgets_to_json(widgets: &[UserWidget]) -> Vec<Value> {
    widgets.iter().map(UserWidget::to_json).collect()
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
    /// Server Tag selection (`PUT /users/@me/clan`): `None` untouched, `Some(None)` cleared, `Some(Some(id))` set.
    pub primary_guild_id: Option<Option<Id>>,
    /// Saveable Board widgets (`PUT /users/@me/widgets`): `None` untouched, `Some(list)` changed.
    pub widgets: Option<Vec<UserWidget>>,
    /// Badge customization display order (`PATCH /users/@me/badges/settings`, module 842810 / 927071).
    pub badge_display_order: Option<Vec<u64>>,
    /// Badge customization hidden badge IDs (`PATCH /users/@me/badges/settings`, module 842810 / 927071).
    pub badge_hidden_badges: Option<Vec<u64>>,
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

    /// Whether the change needs Nitro: a name style, a profile theme, or a personal widget.
    pub fn needs_premium(&self) -> bool {
        matches!(self.display_name_styles, Some(Some(_)))
            || matches!(self.theme_colors, Some(Some(_)))
            || self.widgets.as_ref().is_some_and(|list| {
                list.iter()
                    .any(|w| matches!(w.data, WidgetData::Personal { .. }))
            })
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

    /// The Server Tag update (`PUT /users/@me/clan`, module 933725).
    pub fn clan_write(&self) -> Option<Write> {
        if self.guild.is_some() {
            return None;
        }
        let choice = self.primary_guild_id.as_ref()?;
        let body = match choice {
            Some(guild_id) => json!({
                "identity_guild_id": guild_id.to_string(),
                "identity_enabled": true,
            }),
            None => json!({
                "identity_guild_id": Value::Null,
                "identity_enabled": false,
            }),
        };
        Some(Write {
            method: Method::PUT,
            route: "/users/@me/clan".into(),
            body,
        })
    }

    /// The widgets update (`PUT /users/@me/widgets`, module 958805).
    pub fn widgets_write(&self) -> Option<Write> {
        if self.guild.is_some() {
            return None;
        }
        let widgets = self.widgets.as_ref()?;
        if !widgets.iter().all(|w| w.data.is_valid()) {
            return None;
        }
        Some(Write {
            method: Method::PUT,
            route: "/users/@me/widgets".into(),
            body: json!({ "widgets": widgets_to_json(widgets) }),
        })
    }

    /// `PATCH /users/@me/badges/settings` (`USER_BADGE_SETTINGS`, module 842810 / 927071).
    pub fn badge_settings_write(&self) -> Option<Write> {
        let mut body = serde_json::Map::new();
        if let Some(order) = self.badge_display_order.as_ref() {
            body.insert("display_order".to_owned(), json!(order));
        }
        if let Some(hidden) = self.badge_hidden_badges.as_ref() {
            body.insert("hidden_badges".to_owned(), json!(hidden));
        }
        if body.is_empty() {
            return None;
        }
        Some(Write {
            method: Method::PATCH,
            route: "/users/@me/badges/settings".to_owned(),
            body: Value::Object(body),
        })
    }

    /// Every request a Save sends, in the order the bundle sends them: account or member, profile, clan, widgets,
    /// badge settings.
    pub fn writes(&self, equipped: &Equipped) -> Vec<Write> {
        [
            self.account_write(),
            self.member_write(),
            self.profile_write(equipped),
            self.clan_write(),
            self.widgets_write(),
            self.badge_settings_write(),
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
    fn a_server_profile_sends_only_guild_routes_with_exact_bodies() {
        let mut p = Pending::new(Some(42));
        p.nickname = Some(Some("Nick".into()));
        p.avatar = Some(Some(Picked {
            image_uri: "data:image/png;base64,AQID".into(),
            description: Some("my face".into()),
        }));
        p.banner = Some(Some(Picked {
            image_uri: "data:image/gif;base64,AQID".into(),
            description: Some("Banner image".into()),
        }));
        p.bio = Some("about".into());
        p.pronouns = Some("he/him".into());
        let writes = p.writes(&Equipped::default());
        assert_eq!(writes.len(), 2, "no account write for a server profile");
        assert_eq!(writes[0].method, Method::PATCH);
        assert_eq!(writes[0].route, "/guilds/42/members/@me");
        assert_eq!(
            writes[0].body,
            json!({"nick": "Nick", "avatar": "data:image/png;base64,AQID", "avatar_description": "my face"})
        );
        assert_eq!(writes[1].method, Method::PATCH);
        assert_eq!(writes[1].route, "/guilds/42/profile/@me");
        assert_eq!(
            writes[1].body,
            json!({"banner": "data:image/gif;base64,AQID", "bio": "about", "pronouns": "he/him"})
        );
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

    #[test]
    fn clan_write_sets_and_clears_server_tag() {
        let mut p = Pending::new(None);
        p.primary_guild_id = Some(Some(555));
        let w = p.clan_write().unwrap();
        assert_eq!(w.method, Method::PUT);
        assert_eq!(w.route, "/users/@me/clan");
        assert_eq!(
            w.body,
            json!({"identity_guild_id": "555", "identity_enabled": true})
        );
        p.primary_guild_id = Some(None);
        let clear = p.clan_write().unwrap();
        assert_eq!(
            clear.body,
            json!({"identity_guild_id": null, "identity_enabled": false})
        );
        p.guild = Some(1);
        assert!(p.clan_write().is_none());
    }

    #[test]
    fn widgets_write_serializes_game_and_application_widgets() {
        let mut p = Pending::new(None);
        p.widgets = Some(vec![
            UserWidget {
                id: None,
                data: WidgetData::Application {
                    application_id: "1346069614634864772".into(),
                },
            },
            UserWidget {
                id: Some("w-1".into()),
                data: WidgetData::FavoriteGames {
                    games: vec![WidgetGame {
                        game_id: "1402418696126992445".into(),
                        comment: Some("main".into()),
                        tags: vec!["love_it".into()],
                    }],
                },
            },
        ]);
        let w = p.widgets_write().unwrap();
        assert_eq!(w.method, Method::PUT);
        assert_eq!(w.route, "/users/@me/widgets");
        assert_eq!(
            w.body,
            json!({
                "widgets": [
                    { "data": { "type": "application", "application_id": "1346069614634864772" } },
                    { "id": "w-1", "data": { "type": "favorite_games", "games": [ { "game_id": "1402418696126992445", "comment": "main", "tags": ["love_it"] } ] } }
                ]
            })
        );
    }
}
