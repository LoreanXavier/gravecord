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
}
