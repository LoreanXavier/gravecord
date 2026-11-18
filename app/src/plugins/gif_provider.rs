//! GifProviderSwitcher's providers (Vencord `src/plugins/gifProviderSwitcher/tenor.ts` and `giphy.ts`,
//! GPL-3.0-or-later): the addresses of their search, trending, categories and share calls, and their answers turned
//! into the shapes of Discord's own GIF answers (`DiscordGif`, `TrendingCategoriesData`), so the picker reads them as
//! it reads Klipy's. The calls themselves go out from `net.rs`.

use serde_json::{Value, json};

/// The key the plugins' Tenor calls carry (taken by the original from the GBoard app).
const TENOR_KEY: &str = "3Z0688EVWYKH";
/// The key the plugin's Giphy calls carry (taken by the original from giphy.com).
const GIPHY_KEY: &str = "Gc7131jiJuvI7IdN0HZ1D7nh0ow5BU6g";
/// `MAX_PAGE_SIZE` of both.
pub(crate) const PAGE: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Provider {
    Tenor,
    Giphy,
}

impl Provider {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "tenor" => Some(Self::Tenor),
            "giphy" => Some(Self::Giphy),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tenor => "Tenor",
            Self::Giphy => "Giphy",
        }
    }
}

/// What the picker asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    /// The category tiles and the trending preview (`getCategories`).
    Categories,
    /// The trending results (`getTrending`).
    Trending,
    /// A search (`search`).
    Search(String),
    /// `registerShare` (Tenor; Giphy has none).
    Share { id: String, query: String },
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// The address of one page of `ask` (`pos` for Tenor's next page, `offset` for Giphy's).
pub(crate) fn address(
    provider: Provider,
    ask: &Ask,
    limit: u32,
    pos: &str,
    offset: u32,
) -> Option<String> {
    match provider {
        Provider::Tenor => {
            let base = format!("https://api.tenor.com/v1{{}}?key={TENOR_KEY}&locale=en_us");
            let (path, extra) = match ask {
                Ask::Categories => ("/categories", "&type=featured".to_owned()),
                Ask::Trending => ("/trending", format!("&limit={limit}")),
                Ask::Search(query) => ("/search", format!("&q={}&limit={limit}", encode(query))),
                Ask::Share { id, query } => (
                    "/registershare",
                    format!("&id={}&q={}", encode(id), encode(query)),
                ),
            };
            let pos = if pos.is_empty() {
                String::new()
            } else {
                format!("&pos={}", encode(pos))
            };
            Some(base.replace("{}", path) + &extra + &pos)
        }
        Provider::Giphy => {
            let base = format!("https://api.giphy.com/v1{{}}?api_key={GIPHY_KEY}&lang=en");
            let (path, extra) = match ask {
                Ask::Categories => ("/gifs/categories", String::new()),
                Ask::Trending => ("/gifs/trending", format!("&limit={limit}&offset={offset}")),
                Ask::Search(query) => (
                    "/gifs/search",
                    format!("&q={}&limit={limit}&offset={offset}", encode(query)),
                ),
                Ask::Share { .. } => return None,
            };
            Some(base.replace("{}", path) + &extra)
        }
    }
}

/// `toDiscordGif` of Tenor (the desktop's `webm` format reads `tinywebm`).
pub(crate) fn tenor_gif(item: &Value) -> Option<Value> {
    let media = &item["media"][0];
    let gif = &media["gif"];
    let small = &media["tinywebm"];
    let title = item["title"]
        .as_str()
        .filter(|title| !title.is_empty())
        .or_else(|| item["h1_title"].as_str())
        .unwrap_or_default();
    Some(json!({
        "id": item["id"].as_str()?,
        "title": title,
        "url": item["itemurl"].as_str()?,
        "gif_src": gif["url"].as_str()?,
        "src": small["url"].as_str()?,
        "width": small["dims"][0],
        "height": small["dims"][1],
        "preview": small["preview"],
    }))
}

/// `toDiscordGif` of Giphy (the desktop's `webm` format reads `mp4`).
pub(crate) fn giphy_gif(item: &Value) -> Option<Value> {
    let images = &item["images"];
    let original = images["original"]["url"].as_str()?;
    let fixed = &images["fixed_height"];
    let src = fixed["mp4"].as_str()?;
    let number = |value: &Value| value.as_str().and_then(|text| text.parse::<f64>().ok());
    Some(json!({
        "id": item["id"].as_str()?,
        "title": item["title"].as_str().unwrap_or_default(),
        "url": item["url"].as_str()?,
        "gif_src": original,
        "src": src,
        "width": number(&fixed["width"]),
        "height": number(&fixed["height"]),
        "preview": images["fixed_height_still"]["url"].as_str().unwrap_or(original),
    }))
}

/// `getCategories`: Discord's `{categories, gifs: [preview]}` shape, or `null` when there are none.
pub(crate) fn categories(provider: Provider, body: &Value) -> Value {
    let list: Vec<(String, String)> = match provider {
        Provider::Tenor => body["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tag| {
                Some((
                    tag["searchterm"].as_str()?.to_owned(),
                    tag["image"].as_str()?.to_owned(),
                ))
            })
            .collect(),
        Provider::Giphy => body["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|category| {
                Some((
                    category["name"].as_str()?.to_owned(),
                    category["gif"]["images"]["fixed_height"]["url"]
                        .as_str()?
                        .to_owned(),
                ))
            })
            .collect(),
    };
    let Some((_, first)) = list.first() else {
        return Value::Null;
    };
    json!({
        "categories": list.iter().map(|(name, src)| json!({"name": name, "src": src})).collect::<Vec<_>>(),
        "gifs": [{"id": "trending-preview", "url": "", "src": first}],
    })
}

/// One page's results and Tenor's next position.
pub(crate) fn page(provider: Provider, body: &Value) -> (Vec<Value>, Option<String>) {
    match provider {
        Provider::Tenor => (
            body["results"].as_array().cloned().unwrap_or_default(),
            body["next"].as_str().map(str::to_owned),
        ),
        Provider::Giphy => (body["data"].as_array().cloned().unwrap_or_default(), None),
    }
}

/// The results kept (by id, first seen first, at most `limit`) as Discord GIFs.
pub(crate) fn results(provider: Provider, items: &[Value], limit: usize) -> Value {
    let mut seen = std::collections::HashSet::new();
    let gifs: Vec<Value> = items
        .iter()
        .filter(|item| {
            item["id"]
                .as_str()
                .is_some_and(|id| seen.insert(id.to_owned()))
        })
        .take(limit)
        .filter_map(|item| match provider {
            Provider::Tenor => tenor_gif(item),
            Provider::Giphy => giphy_gif(item),
        })
        .collect();
    Value::Array(gifs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_take_discord_shapes() {
        let tenor = json!({"id": "1", "title": "", "h1_title": "Hi", "itemurl": "https://tenor.com/view/1",
            "media": [{"gif": {"url": "https://g/1.gif"}, "tinywebm": {"url": "https://g/1.webm", "dims": [220, 120], "preview": "https://g/1.png"}}]});
        let gif = tenor_gif(&tenor).unwrap();
        assert_eq!(gif["title"], "Hi");
        assert_eq!(gif["width"], 220);
        let giphy = json!({"id": "a", "title": "t", "url": "https://giphy.com/gifs/a",
            "images": {"original": {"url": "https://m/a.gif"}, "fixed_height": {"mp4": "https://m/a.mp4", "width": "200", "height": "100"}}});
        assert_eq!(giphy_gif(&giphy).unwrap()["height"], 100.0);
        let cats = categories(
            Provider::Tenor,
            &json!({"tags": [{"searchterm": "yes", "image": "https://c/y.gif"}]}),
        );
        assert_eq!(cats["categories"][0]["name"], "yes");
        assert_eq!(cats["gifs"][0]["src"], "https://c/y.gif");
        assert!(categories(Provider::Giphy, &json!({"data": []})).is_null());
        assert_eq!(
            address(Provider::Tenor, &Ask::Search("a b".into()), 50, "", 0).unwrap(),
            "https://api.tenor.com/v1/search?key=3Z0688EVWYKH&locale=en_us&q=a+b&limit=50"
        );
        assert!(
            address(
                Provider::Giphy,
                &Ask::Share {
                    id: "a".into(),
                    query: String::new()
                },
                0,
                "",
                0
            )
            .is_none()
        );
        assert_eq!(
            results(Provider::Tenor, &[tenor.clone(), tenor], 50)
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
