//! The reads behind the Discover page, their parsers and the small string rules. Routes and query keys follow the
//! bundle: `GUILD_DISCOVERY`, `GUILD_DISCOVERY_SEARCH` and `GUILD_DISCOVERY_CATEGORIES` in module 652215, the fetchers
//! of modules 808097 (featured and per-category lists) and 300290 (search), and for the application directory the
//! fetchers of module 503406. Every call here is a GET.

use dc_state::Id;
use serde_json::Value;

/// `FEATURED_DEFAULT_LIMIT` (module 808097).
pub const FEATURED_LIMIT: u32 = 30;
/// The search page size (`limit: 12`, module 300290; confirmed on the live client).
pub const SEARCH_LIMIT: u32 = 12;
/// The locale the client asks the directory in.
pub const LOCALE: &str = "en-US";

/// `encodeURIComponent`: everything but letters, digits and `-_.!~*'()` is percent-encoded as UTF-8.
pub fn encode_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub(super) fn id_of(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| value.as_u64())
}

pub(super) fn text_of(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

// ---- servers ----

/// A server in the directory (`MC`/`uk` in module 965660).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildCard {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub discovery_splash: Option<String>,
    pub features: Vec<String>,
    pub online: Option<u64>,
    pub members: Option<u64>,
}

impl GuildCard {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|known| known == feature)
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/icons/{}/{hash}.webp?size=96",
                self.id
            )
        })
    }

    pub fn splash_url(&self) -> Option<String> {
        self.discovery_splash.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/discovery-splashes/{}/{hash}.jpg?size=600",
                self.id
            )
        })
    }
}

/// The `guilds` array of `/discoverable-guilds` and of the search. Rows without an id are dropped.
pub fn parse_guilds(body: &Value) -> Vec<GuildCard> {
    body["guilds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(GuildCard {
                id: id_of(&row["id"])?,
                name: row["name"].as_str().unwrap_or_default().to_owned(),
                description: row["description"].as_str().unwrap_or_default().to_owned(),
                icon: text_of(&row["icon"]),
                discovery_splash: text_of(&row["discovery_splash"]),
                features: row["features"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|feature| feature.as_str().map(str::to_owned))
                    .collect(),
                online: row["approximate_presence_count"].as_u64(),
                members: row["approximate_member_count"].as_u64(),
            })
        })
        .collect()
}

/// A discovery category: an id and its name in the asked locale (`GET /discovery/categories`).
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub id: i64,
    pub name: String,
}

pub fn parse_categories(body: &Value) -> Vec<Category> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_i64()?;
            // The store skips id 0 (`getAllCategories`, module 997590).
            if id == 0 {
                return None;
            }
            let name = row["name"]
                .as_str()
                .or_else(|| row["name"]["default"].as_str())?
                .to_owned();
            Some(Category { id, name })
        })
        .collect()
}

pub fn featured_route() -> String {
    format!("/discoverable-guilds?offset=0&limit={FEATURED_LIMIT}")
}

pub fn category_route(category: i64) -> String {
    format!("/discoverable-guilds?categories={category}")
}

/// `PUT` route of `joinGuild` for a lurked directory server (module 778858): `lurker=true` and the gateway session
/// the preview belongs to. The other query fields are null for a card click and are left out.
pub fn lurk_route(guild: u64, session: Option<&str>) -> String {
    match session {
        Some(session) => format!("/guilds/{guild}/members/@me?lurker=true&session_id={session}"),
        None => format!("/guilds/{guild}/members/@me?lurker=true"),
    }
}

/// `GUILD_LEAVE` with `{lurking: true}` (module 854492): ends a preview without leaving a server the account is in.
pub fn stop_lurking_route(guild: u64) -> String {
    format!("/users/@me/guilds/{guild}")
}

pub fn categories_route() -> String {
    format!(
        "/discovery/categories?locale={}&primary_only=false",
        encode_component(LOCALE)
    )
}

/// `GET /discoverable-guilds/search` as module 300290's `F` builds it: `category_id` is left out for All (-1) and
/// `language_code` until a language is picked.
pub fn search_route(
    query: &str,
    category: i64,
    language: Option<&str>,
    offset: u32,
    with_counts: bool,
) -> String {
    let mut route = format!(
        "/discoverable-guilds/search?query={}",
        encode_component(query)
    );
    if category != -1 {
        route.push_str(&format!("&category_id={category}"));
    }
    route.push_str(&format!("&offset={offset}&limit={SEARCH_LIMIT}"));
    if let Some(language) = language {
        route.push_str(&format!("&language_code={}", encode_component(language)));
    }
    route.push_str(&format!("&with_counts={with_counts}"));
    route
}

/// One search response: the page of servers, the total for this category and language and, on the first page, the
/// per-category counts the Show filter lists (the first eight, as `slice(0, 8)` keeps them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchPage {
    pub guilds: Vec<GuildCard>,
    pub total: u64,
    pub counts: Vec<(i64, u64)>,
}

pub fn parse_search(body: &Value) -> SearchPage {
    SearchPage {
        guilds: parse_guilds(body),
        total: body["total_count"].as_u64().unwrap_or(0),
        counts: body["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .filter_map(|row| {
                Some((
                    id_of(&row["id"]).map(|id| id as i64)?,
                    row["count"].as_u64()?,
                ))
            })
            .collect(),
    }
}

/// `toLocaleString`: digits in groups of three.
pub fn group_digits(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The languages of `getLanguages()` (module 416678) in its order, without `en-GB`: the code and the name the select
/// shows. `en-US` reads "English" (`LQLMGP`).
pub const LANGUAGES: [(&str, &str); 35] = [
    ("en-US", "English"),
    ("zh-CN", "\u{4E2D}\u{6587}"),
    ("zh-TW", "\u{7E41}\u{9AD4}\u{4E2D}\u{6587}"),
    ("cs", "\u{10C}e\u{161}tina"),
    ("da", "Dansk"),
    ("nl", "Nederlands"),
    ("fr", "Fran\u{E7}ais"),
    ("de", "Deutsch"),
    (
        "el",
        "\u{395}\u{3BB}\u{3BB}\u{3B7}\u{3BD}\u{3B9}\u{3BA}\u{3AC}",
    ),
    ("hu", "Magyar"),
    ("it", "Italiano"),
    ("ja", "\u{65E5}\u{672C}\u{8A9E}"),
    ("ko", "\u{D55C}\u{AD6D}\u{C5B4}"),
    ("pl", "Polski"),
    ("pt-PT", "Portugu\u{EA}s"),
    ("pt-BR", "Portugu\u{EA}s do Brasil"),
    ("ru", "\u{420}\u{443}\u{441}\u{441}\u{43A}\u{438}\u{439}"),
    ("sk", "Sloven\u{10D}ina"),
    ("es-419", "Espa\u{F1}ol, LATAM"),
    ("es-ES", "Espa\u{F1}ol"),
    ("sv-SE", "Svenska"),
    ("tr", "T\u{FC}rk\u{E7}e"),
    (
        "bg",
        "\u{431}\u{44A}\u{43B}\u{433}\u{430}\u{440}\u{441}\u{43A}\u{438}",
    ),
    (
        "uk",
        "\u{423}\u{43A}\u{440}\u{430}\u{457}\u{43D}\u{441}\u{44C}\u{43A}\u{430}",
    ),
    ("fi", "Suomi"),
    ("no", "Norsk"),
    ("hr", "Hrvatski"),
    ("ro", "Rom\u{E2}n\u{103}"),
    ("lt", "Lietuvi\u{161}kai"),
    ("th", "\u{E44}\u{E17}\u{E22}"),
    ("vi", "Ti\u{1EBF}ng Vi\u{1EC7}t"),
    ("hi", "\u{939}\u{93F}\u{902}\u{926}\u{940}"),
    ("he", "\u{5E2}\u{5B4}\u{5D1}\u{5E8}\u{5B4}\u{5D9}\u{5EA}"),
    ("ar", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64A}\u{629}"),
    ("id", "Bahasa Indonesia"),
];

/// The language the select shows when none was picked (`Kv`): the client's locale if listed, else the first.
pub fn default_language() -> &'static str {
    LANGUAGES
        .iter()
        .find(|(code, _)| *code == LOCALE)
        .map_or(LANGUAGES[0].0, |(code, _)| code)
}

pub fn language_name(code: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or("English", |(_, name)| name)
}

/// The tabs of the Servers page in order (`jR` labels, `Ub` categories of module 965660): Home lists the featured
/// servers, Student Hubs lists the hub servers the user belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServersTab {
    Home,
    Gaming,
    Music,
    Entertainment,
    Tech,
    Education,
    Hubs,
}

impl ServersTab {
    pub const ALL: [ServersTab; 7] = [
        ServersTab::Home,
        ServersTab::Gaming,
        ServersTab::Music,
        ServersTab::Entertainment,
        ServersTab::Tech,
        ServersTab::Education,
        ServersTab::Hubs,
    ];

    /// The tab's label (`jR`).
    pub fn label(self) -> &'static str {
        match self {
            ServersTab::Home => "Home",
            ServersTab::Gaming => "Gaming",
            ServersTab::Music => "Music",
            ServersTab::Entertainment => "Entertainment",
            ServersTab::Tech => "Science & Tech",
            ServersTab::Education => "Education",
            ServersTab::Hubs => "Student Hubs",
        }
    }

    /// The hero's title (`kc`).
    pub fn hero_title(self) -> &'static str {
        match self {
            ServersTab::Home => "Find your community on Discord",
            ServersTab::Hubs => "Join the Student Hub for your school",
            other => other.label(),
        }
    }

    /// The hero's description (`Ot`).
    pub fn hero_description(self) -> &'static str {
        match self {
            ServersTab::Home => "From gaming, to music, to learning, there's a place for you.",
            ServersTab::Gaming => {
                "Get updates, connect with developers, and chat with other players."
            }
            ServersTab::Music => {
                "Get updates, connect with artists, and chat with other music fans."
            }
            ServersTab::Entertainment => "Connect with your favorite creators and their fans.",
            ServersTab::Tech => "Connect with developers, aficionados, and technology brands.",
            ServersTab::Education => "Connect with students, teachers, and learning centers.",
            ServersTab::Hubs => {
                "Meet classmates from your school, discover communities, and share your servers."
            }
        }
    }

    /// The section heading above the list (`UW`); the Student Hubs page has its own sections.
    pub fn section_title(self) -> Option<&'static str> {
        match self {
            ServersTab::Home => Some("Featured Servers"),
            ServersTab::Gaming => Some("Popular Gaming Servers"),
            ServersTab::Music => Some("Popular Music Servers"),
            ServersTab::Entertainment => Some("Popular Entertainment Servers"),
            ServersTab::Tech => Some("Popular Science & Tech Servers"),
            ServersTab::Education => Some("Popular Education Servers"),
            ServersTab::Hubs => None,
        }
    }

    /// The category's id (`Ub`): `Activity` 1, `Music` 2, `Television` 3, `Science` 5, `Education` 6.
    pub fn category(self) -> Option<i64> {
        match self {
            ServersTab::Home | ServersTab::Hubs => None,
            ServersTab::Gaming => Some(1),
            ServersTab::Music => Some(2),
            ServersTab::Entertainment => Some(3),
            ServersTab::Tech => Some(5),
            ServersTab::Education => Some(6),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_search_route_matches_the_live_request() {
        assert_eq!(
            search_route("minecraft", -1, None, 0, true),
            "/discoverable-guilds/search?query=minecraft&offset=0&limit=12&with_counts=true"
        );
    }

    #[test]
    fn category_and_language_join_the_search_only_when_set() {
        assert_eq!(
            search_route("a b&c", 1, Some("zh-CN"), 24, false),
            "/discoverable-guilds/search?query=a%20b%26c&category_id=1&offset=24&limit=12&language_code=zh-CN&with_counts=false"
        );
    }

    #[test]
    fn the_list_routes_follow_the_bundle() {
        assert_eq!(featured_route(), "/discoverable-guilds?offset=0&limit=30");
        assert_eq!(category_route(5), "/discoverable-guilds?categories=5");
        assert_eq!(
            categories_route(),
            "/discovery/categories?locale=en-US&primary_only=false"
        );
    }

    #[test]
    fn guilds_parse_with_counts_and_features() {
        let body = json!({"guilds": [
            {"id": "5", "name": "A", "description": "d", "icon": "h", "discovery_splash": null,
             "features": ["VERIFIED"], "approximate_presence_count": 3, "approximate_member_count": 10},
            {"name": "no id"}
        ]});
        let guilds = parse_guilds(&body);
        assert_eq!(guilds.len(), 1);
        assert!(guilds[0].has("VERIFIED"));
        assert_eq!(guilds[0].online, Some(3));
        assert_eq!(guilds[0].discovery_splash, None);
        assert_eq!(
            guilds[0].icon_url().as_deref(),
            Some("https://cdn.discordapp.com/icons/5/h.webp?size=96")
        );
    }

    #[test]
    fn search_counts_keep_the_first_eight() {
        let categories: Vec<Value> = (1..=10)
            .map(|id| json!({"id": id, "count": id * 10}))
            .collect();
        let page =
            parse_search(&json!({"guilds": [], "total_count": 7943, "categories": categories}));
        assert_eq!(page.total, 7943);
        assert_eq!(page.counts.len(), 8);
        assert_eq!(page.counts[0], (1, 10));
    }

    #[test]
    fn numbers_group_in_threes() {
        assert_eq!(group_digits(7), "7");
        assert_eq!(group_digits(7943), "7,943");
        assert_eq!(group_digits(4_237_959), "4,237,959");
    }

    #[test]
    fn the_language_list_starts_like_the_capture() {
        assert_eq!(LANGUAGES[0], ("en-US", "English"));
        assert_eq!(LANGUAGES[3].1, "\u{10C}e\u{161}tina");
        assert_eq!(default_language(), "en-US");
        assert_eq!(language_name("de"), "Deutsch");
    }

    #[test]
    fn categories_skip_id_zero() {
        let categories = parse_categories(&json!([
            {"id": 0, "name": "x"}, {"id": 1, "name": "Gaming"}, {"id": 2, "name": {"default": "Music"}}
        ]));
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[1].name, "Music");
    }
}
