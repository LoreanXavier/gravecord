//! Parsers and routes of the application directory (`GET /application-directory-static/...`, module 503406).
//!
//! Shapes, as recorded from the live directory:
//! * `collections`: an array of `{id, type, position, title, application_directory_collection_items: [{type, position,
//!   application}]}`. Item type 1 is an application.
//! * `categories`: an array of `{id, name}` (Games 6, Entertainment 4, Moderation and Tools 8, Social 9, Utilities 10).
//! * an application: `{id, name, icon, description, flags, bot: {id, banner}, embedded_activity_config,
//!   categories: [{id, name}], directory_entry: {guild_count, carousel_items: [{type, url}], supported_locales,
//!   short_description, detailed_description}, terms_of_service_url, privacy_policy_url, install_params: {scopes,
//!   permissions}}`.
//! * `search`: `{results: [{type, data}], counts_by_category: {id: count}, result_count, num_pages, load_id}`; a result of
//!   type 1 carries an application in `data`.
use super::data::{encode_component, id_of, text_of};
use dc_state::Id;
use serde_json::Value;

pub const PAGE_SIZE: u32 = 48;
/// `PROMOTED`, application flag bit 29 (module 652215).
const PROMOTED: u64 = 1 << 29;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct App {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub description: String,
    pub category: Option<String>,
    pub promoted: bool,
    /// Has an embedded activity, so its cover is the `embedded_cover` asset.
    pub embedded: bool,
    pub banner: Option<(Id, String)>,
}

impl App {
    /// `getApplicationIconURL`: the icon as a PNG.
    pub fn icon_url(&self, size: u32) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size={size}",
                self.id
            )
        })
    }

    /// The bot banner (`getBannerURL` without animation).
    pub fn banner_url(&self) -> Option<String> {
        self.banner.as_ref().map(|(bot, hash)| {
            format!("https://cdn.discordapp.com/banners/{bot}/{hash}.png?size=600")
        })
    }
}

pub fn parse_app(value: &Value) -> Option<App> {
    let id = id_of(&value["id"])?;
    let bot_banner = text_of(&value["bot"]["banner"]);
    let bot_id = id_of(&value["bot"]["id"]);
    Some(App {
        id,
        name: value["name"].as_str().unwrap_or_default().to_owned(),
        icon: text_of(&value["icon"]),
        description: value["directory_entry"]["short_description"]
            .as_str()
            .filter(|text| !text.is_empty())
            .or_else(|| value["description"].as_str())
            .unwrap_or_default()
            .to_owned(),
        category: value["categories"][0]["name"].as_str().map(str::to_owned),
        promoted: text_of(&value["flags_new"])
            .or_else(|| text_of(&value["flags"]))
            .and_then(|flags| flags.parse::<u64>().ok())
            .is_some_and(|flags| flags & PROMOTED != 0),
        embedded: !value["embedded_activity_config"].is_null()
            || value["embedded_surfaces"]
                .as_array()
                .is_some_and(|surfaces| !surfaces.is_empty()),
        banner: bot_id.zip(bot_banner),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub title: String,
    pub apps: Vec<App>,
}

/// The collections in `position` order, each with its applications in item `position` order.
pub fn parse_collections(body: &Value) -> Vec<Collection> {
    let mut rows: Vec<(i64, Collection)> = body
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let mut items: Vec<(i64, App)> = row["application_directory_collection_items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["type"].as_i64() == Some(1))
                .filter_map(|item| {
                    Some((
                        item["position"].as_i64().unwrap_or(0),
                        parse_app(&item["application"])?,
                    ))
                })
                .collect();
            items.sort_by_key(|(position, _)| *position);
            (
                row["position"].as_i64().unwrap_or(0),
                Collection {
                    title: row["title"].as_str().unwrap_or_default().to_owned(),
                    apps: items.into_iter().map(|(_, app)| app).collect(),
                },
            )
        })
        .filter(|(_, collection)| !collection.apps.is_empty())
        .collect();
    rows.sort_by_key(|(position, _)| *position);
    rows.into_iter().map(|(_, collection)| collection).collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppPage {
    pub apps: Vec<App>,
    pub total: u64,
    pub pages: u32,
    /// Result counts per category id.
    pub counts: Vec<(i64, u64)>,
}

pub fn parse_search(body: &Value) -> AppPage {
    let mut counts: Vec<(i64, u64)> = body["counts_by_category"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, count)| Some((id.parse().ok()?, count.as_u64()?)))
        .collect();
    counts.sort();
    AppPage {
        apps: body["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["type"].as_i64() == Some(1))
            .filter_map(|row| parse_app(&row["data"]))
            .collect(),
        total: body["result_count"].as_u64().unwrap_or(0),
        pages: body["num_pages"].as_u64().unwrap_or(0) as u32,
        counts,
    }
}

pub fn collections_route() -> String {
    "/application-directory-static/collections?surface=1&active_state=1&platform=4&locale=en-US&cache=true"
        .to_owned()
}

pub fn categories_route() -> String {
    "/application-directory-static/categories?locale=en-US".to_owned()
}

pub fn app_route(id: Id) -> String {
    format!("/application-directory-static/applications/{id}?locale=en-US")
}

pub fn similar_route(id: Id) -> String {
    format!("/application-directory-static/applications/{id}/similar?page=1&locale=en-US")
}

pub fn assets_route(id: Id) -> String {
    format!("/oauth2/applications/{id}/assets")
}

/// `GET /application-directory-static/search`: `category_id` only for a chosen category.
pub fn search_route(query: &str, category: Option<i64>, page: u32) -> String {
    let mut route = format!(
        "/application-directory-static/search?query={}&page={page}&page_size={PAGE_SIZE}",
        encode_component(query)
    );
    if let Some(category) = category {
        route.push_str(&format!("&category_id={category}"));
    }
    route.push_str("&locale=en-US&source=0");
    route
}

/// The `embedded_cover` asset of an application (`GET /oauth2/applications/{id}/assets`: `[{id, name, type}]`).
pub fn cover_url(application: Id, assets: &Value) -> Option<String> {
    let asset = assets
        .as_array()?
        .iter()
        .find(|asset| asset["name"] == "embedded_cover")?;
    let id = text_of(&asset["id"])?;
    Some(format!(
        "https://cdn.discordapp.com/app-assets/{application}/{id}.png?size=600"
    ))
}

/// Server counts as the detail page writes them: 361K, 1.6M.
pub fn short_count(count: u64) -> String {
    let scaled = |value: f64, suffix: &str| {
        let text = format!("{value:.1}");
        format!(
            "{}{suffix}",
            text.trim_end_matches('0').trim_end_matches('.')
        )
    };
    if count >= 1_000_000 {
        scaled(count as f64 / 1_000_000., "M")
    } else if count >= 10_000 {
        format!("{}K", count / 1_000)
    } else if count >= 1_000 {
        scaled(count as f64 / 1_000., "K")
    } else {
        count.to_string()
    }
}

/// Application flag bits of the gateway intents (module 652215 `gfo`: `1 << n`).
const GATEWAY_PRESENCE: u64 = 1 << 12;
const GATEWAY_PRESENCE_LIMITED: u64 = 1 << 13;
const GATEWAY_GUILD_MEMBERS: u64 = 1 << 14;
const GATEWAY_GUILD_MEMBERS_LIMITED: u64 = 1 << 15;
const GATEWAY_MESSAGE_CONTENT: u64 = 1 << 18;
const GATEWAY_MESSAGE_CONTENT_LIMITED: u64 = 1 << 19;

/// What the Data Access section lists (module 152249): the privileged intents the application holds, limited or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Intents {
    pub message_content: bool,
    pub presences: bool,
    pub members: bool,
}

impl Intents {
    pub fn any(self) -> bool {
        self.message_content || self.presences || self.members
    }
}

/// `(0, i.K)(e)`: \`flags_new\` before \`flags\`, a string or a number.
pub fn intents(body: &Value) -> Intents {
    let flags = [&body["flags_new"], &body["flags"]]
        .into_iter()
        .find_map(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0);
    Intents {
        message_content: flags & (GATEWAY_MESSAGE_CONTENT | GATEWAY_MESSAGE_CONTENT_LIMITED) != 0,
        presences: flags & (GATEWAY_PRESENCE | GATEWAY_PRESENCE_LIMITED) != 0,
        members: flags & (GATEWAY_GUILD_MEMBERS | GATEWAY_GUILD_MEMBERS_LIMITED) != 0,
    }
}

/// Popular Slash Commands (module 586193 `tb`): each command's localized name and description when the entry has them.
pub fn popular_commands(body: &Value) -> Vec<(String, String)> {
    body["directory_entry"]["popular_application_commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|command| {
            let pick = |localized: &str, plain: &str| {
                command[localized]
                    .as_str()
                    .or_else(|| command[plain].as_str())
                    .map(str::to_owned)
            };
            Some((
                pick("name_localized", "name")?,
                pick("description_localized", "description").unwrap_or_default(),
            ))
        })
        .collect()
}

/// `install_params.permissions` as the checked rows of the Permissions section (module 586193 `lL`): the bits the
/// application asks for, in the official list order, with their labels (module 376092).
pub fn install_permissions(body: &Value) -> Vec<&'static str> {
    let bits = &body["install_params"]["permissions"];
    let Some(mask) = bits
        .as_u64()
        .or_else(|| bits.as_str().and_then(|text| text.parse().ok()))
    else {
        return Vec::new();
    };
    PERMISSION_LABELS
        .iter()
        .filter(|(bit, _)| mask & (1u64 << bit) != 0)
        .map(|(_, label)| *label)
        .collect()
}

/// \`(bit, label)\` in the order \`lP.Q\` lists them (module 376092).
const PERMISSION_LABELS: [(u32, &str); 53] = [
    (3, "Administrator"),
    (5, "Manage Server"),
    (28, "Manage Roles"),
    (4, "Manage Channels"),
    (1, "Kick Members"),
    (2, "Ban Members"),
    (0, "Create Invite"),
    (27, "Manage Nicknames"),
    (26, "Change Nickname"),
    (30, "Manage Expressions"),
    (43, "Create Expressions"),
    (29, "Manage Webhooks"),
    (7, "View Audit Log"),
    (10, "View Channels"),
    (33, "Manage Events"),
    (44, "Create Events"),
    (40, "Moderate Members"),
    (19, "View Server Insights"),
    (41, "View Server Subscription Insights"),
    (50, "Use External Apps"),
    (11, "Send Messages"),
    (38, "Send Messages in Threads"),
    (35, "Create Public Threads"),
    (36, "Create Private Threads"),
    (12, "Send TTS Messages"),
    (13, "Manage Messages"),
    (34, "Manage Threads"),
    (14, "Embed Links"),
    (15, "Attach Files"),
    (16, "Read Message History"),
    (17, "Mention @everyone, @here, and All Roles"),
    (6, "Add Reactions"),
    (18, "Use External Emoji"),
    (37, "Use External Stickers"),
    (31, "Use Application Commands"),
    (46, "Send Voice Message"),
    (49, "Create Polls"),
    (51, "Pin Messages"),
    (52, "Bypass Slowmode"),
    (53, "Manage Official Messages"),
    (20, "Connect"),
    (21, "Speak"),
    (22, "Mute Members"),
    (23, "Deafen Members"),
    (24, "Move Members"),
    (25, "Use Voice Activity"),
    (8, "Priority Speaker"),
    (32, "Request to Speak"),
    (9, "Video"),
    (39, "Use Activities"),
    (42, "Use Soundboard"),
    (45, "Use External Sounds"),
    (48, "Set Voice Channel Status"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collections_sort_by_position() {
        let body = json!([
            {"position": 2, "title": "B", "application_directory_collection_items": [
                {"type": 1, "position": 2, "application": {"id": "2", "name": "two"}},
                {"type": 1, "position": 1, "application": {"id": "1", "name": "one"}}]},
            {"position": 1, "title": "A", "application_directory_collection_items": [
                {"type": 1, "position": 1, "application": {"id": "3", "name": "three"}}]},
            {"position": 0, "title": "empty", "application_directory_collection_items": []}
        ]);
        let parsed = parse_collections(&body);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "A");
        assert_eq!(parsed[1].apps[0].name, "one");
    }

    #[test]
    fn search_route_matches_the_store_request() {
        assert_eq!(
            search_route("a b", Some(6), 2),
            "/application-directory-static/search?query=a%20b&page=2&page_size=48&category_id=6&locale=en-US&source=0"
        );
        assert!(!search_route("", None, 1).contains("category_id"));
    }

    #[test]
    fn app_flags_and_cover() {
        let app = parse_app(&json!({
            "id": "5", "name": "x", "flags_new": "536870912", "icon": "h",
            "embedded_activity_config": {}, "categories": [{"id": 6, "name": "Games"}],
            "bot": {"id": "5", "banner": "b"}
        }))
        .expect("an app");
        assert!(app.promoted && app.embedded);
        assert_eq!(app.category.as_deref(), Some("Games"));
        assert!(
            app.banner_url()
                .expect("banner")
                .contains("/banners/5/b.png")
        );
        let assets = json!([{"id": "77", "name": "embedded_cover", "type": 1}]);
        assert_eq!(
            cover_url(5, &assets).as_deref(),
            Some("https://cdn.discordapp.com/app-assets/5/77.png?size=600")
        );
    }

    #[test]
    fn counts_are_shortened() {
        assert_eq!(short_count(361_000), "361K");
        assert_eq!(short_count(1_600_000), "1.6M");
        assert_eq!(short_count(1_500), "1.5K");
        assert_eq!(short_count(12), "12");
    }

    #[test]
    fn search_page_reads_counts_and_pages() {
        let page = parse_search(&json!({
            "results": [{"type": 1, "data": {"id": "1", "name": "a"}}, {"type": 2, "data": {}}],
            "counts_by_category": {"6": 4, "8": 1}, "result_count": 5, "num_pages": 1
        }));
        assert_eq!(page.apps.len(), 1);
        assert_eq!(page.counts, vec![(6, 4), (8, 1)]);
        assert_eq!((page.total, page.pages), (5, 1));
    }

    #[test]
    fn data_access_follows_the_intent_flags() {
        let none = intents(&json!({"flags": 0}));
        assert!(!none.any());
        let all = intents(&json!({"flags_new": "262144", "flags": 0}));
        assert_eq!(
            (all.message_content, all.presences, all.members),
            (true, false, false)
        );
        let limited = intents(&json!({"flags": (1u64 << 13) | (1u64 << 15)}));
        assert_eq!(
            (limited.message_content, limited.presences, limited.members),
            (false, true, true)
        );
    }

    #[test]
    fn popular_commands_prefer_the_localized_text() {
        let body = json!({"directory_entry": {"popular_application_commands": [
            {"name": "wordle", "description": "Play", "description_localized": "Jouer"},
            {"name": "share", "name_localized": "partager", "description": "Share"}
        ]}});
        assert_eq!(
            popular_commands(&body),
            vec![
                ("wordle".to_owned(), "Jouer".to_owned()),
                ("partager".to_owned(), "Share".to_owned())
            ]
        );
        assert!(popular_commands(&json!({})).is_empty());
    }

    #[test]
    fn install_permissions_list_the_asked_bits_in_the_official_order() {
        let mask = (1u64 << 11) | (1u64 << 3) | (1u64 << 10);
        let body = json!({"install_params": {"permissions": mask.to_string()}});
        assert_eq!(
            install_permissions(&body),
            vec!["Administrator", "View Channels", "Send Messages"]
        );
        assert!(install_permissions(&json!({"install_params": {}})).is_empty());
    }
}
