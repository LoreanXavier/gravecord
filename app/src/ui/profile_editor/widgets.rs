//! The widgets column of the editor (`container_y4ZEM5`): the Board / Activity / Wishlist tab bar and the Board's
//! empty state (`aQ` of module 927071, `empty_jFcvcH`): the heading and the grid of widgets to add
//! (`widgetGrid_jFcvcH`, tiles of module 297520).
//!
//! The grid lists every addable widget (`a$`, module 116281's order: the featured games' widgets, then Favorite Game,
//! Games I Like, Games in Rotation, Want to Play), or, for an account in
//! `2026-09-profile-widget-empty-state-suggestions`, the `maxWidgetOptions` suggestions of `aw` and Browse All
//! Widgets. The featured games and every picture are the ones of the capture
//! (`design/dom/settings/profile-editor-darker-cozy.json`): `/widget-configs/featured` itself is not fetched.

use super::panel::plus_badge;
use super::save::{
    PersonalField, PersonalSection, UserWidget, WidgetData, WidgetGame, saveable_widgets,
};
use super::{Mode, Tab, profile_field_style, text};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::dm_profile;
use crate::ui::{Root, color, icon};
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, IntoElement, Subscription, div, img, prelude::*,
    px,
};
use serde_json::{Value, json};

/// `2026-09-profile-widget-empty-state-suggestions` (module 927071 `aX`): variation 1 offers 4, variation 2 offers 6.
const SUGGESTIONS_EXPERIMENT: &str = "2026-09-profile-widget-empty-state-suggestions";

/// How a featured game's add tile pictures it (`ADD_WIDGET_PREVIEW` of its widget config, module 297520 `U`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Art {
    /// `heroImage_Ls3EVK`: the picture across the tile's right 40%, at its own aspect ratio, faded.
    Hero(&'static str, f32),
    /// `image_VMx4zg`: a square picture inset 16px in the right 40%.
    Square(&'static str),
    /// `imageSkeleton_VMx4zg`: the square's empty skeleton.
    Skeleton,
}

/// A featured game with a widget (`sk.R` application widgets).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Featured {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    art: Art,
}

const CDN: &str = "https://cdn.discordapp.com";

/// The featured games of the capture, in its order.
const FEATURED: [Featured; 6] = [
    Featured {
        id: "1346069614634864772",
        name: "Marvel Rivals",
        icon: "/app-icons/1346069614634864772/2f7be4ce3fb3261f76e544c56709a6ad.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1346069614634864772/1509836387271053342.webp?size=4096",
            1.00314,
        ),
    },
    Featured {
        id: "672188101707169852",
        name: "Wolvesville",
        icon: "/app-icons/672188101707169852/b7a64ea9edb7cb22588dd6530bd66c35.png?size=16&keep_aspect_ratio=false",
        art: Art::Skeleton,
    },
    Featured {
        id: "1323482066758930452",
        name: "Wuthering Waves",
        icon: "/app-icons/1323482066758930452/0a424370271f81fa7399c0938966aadd.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1323482066758930452/1506737434887458867.webp?size=300",
            0.75,
        ),
    },
    Featured {
        id: "1463856624455254103",
        name: "Heartopia",
        icon: "/app-icons/1463856624455254103/a7bd13821ede07bf4df788e48609d59e.png?size=16&keep_aspect_ratio=false",
        art: Art::Square("/app-assets/1463856624455254103/1550358418597085287.webp?size=2048"),
    },
    Featured {
        id: "1496037472226906183",
        name: "Arknights Endfield",
        icon: "/app-icons/1496037472226906183/f7877b08efb704adc7f7992af7e3b799.png?size=16&keep_aspect_ratio=false",
        art: Art::Hero(
            "/app-assets/1496037472226906183/1519406898787520646.webp?size=300",
            0.666_667,
        ),
    },
    Featured {
        id: "1481180707966685245",
        name: "Neverness to Everness",
        icon: "/app-icons/1481180707966685245/8751bdb4c480ed61d49f9ff6d0d471f8.png?size=16&keep_aspect_ratio=false",
        art: Art::Square("/app-assets/1481180707966685245/1552590719875551272.webp?size=480"),
    },
];

/// The game covers of the placeholders (module 297520 `f`): League of Legends; Valorant; Peak, Battlefield 6, R.E.P.O.;
/// Marvel Rivals, World of Warcraft, Rust (the fourth of each grid is clipped).
const LEAGUE: &str = "https://cdn.discordapp.com/app-icons/1402418696126992445/fae8bbf6a5d14f494db342b5d53eea21.webp?size=128&keep_aspect_ratio=true";
const VALORANT: &str = "https://cdn.discordapp.com/app-icons/700136079562375258/9c55c3dbef3b1621d17ee32dc7765dcc.webp?size=128&keep_aspect_ratio=true";
const LIKED: [&str; 3] = [
    "https://images-ext-1.discordapp.net/external/GTMVuQIs0v5kSO3AeNN1D7YhN5b1E19OkHtBuS9HYMI/https/images.igdb.com/igdb/image/upload/t_cover_big/cocokj.jpg?keep_aspect_ratio=true&format=webp",
    "https://cdn.discordapp.com/app-icons/1402692356343599254/e15fd23de24370b3a882500315de4cf7.webp?size=128&keep_aspect_ratio=true",
    "https://images-ext-1.discordapp.net/external/PXGulRTdhLH3WyICOOLRsfQbfgrDovCiIVtYNyRs8D0/https/images.igdb.com/igdb/image/upload/t_cover_big/co9lxz.jpg?keep_aspect_ratio=true&format=webp",
];
const WANTED: [&str; 3] = [
    "https://cdn.discordapp.com/app-icons/1314395942253756416/ddfdd542275573a097aa9bcddfba7cf5.webp?size=128&keep_aspect_ratio=true",
    "https://cdn.discordapp.com/app-icons/356875762940379136/52a7ab86855bd1f28cb0871daecef1ee.webp?size=128&keep_aspect_ratio=true",
    "https://images-ext-1.discordapp.net/external/Yl44h7wUxiLNpstWKVClimZPFuvh6mY7i0SHN6PhASQ/https/images.igdb.com/igdb/image/upload/t_cover_big/coajjj.jpg?keep_aspect_ratio=true&format=webp",
];

#[derive(Clone, Copy)]
struct KnownGameMeta {
    id: &'static str,
    name: &'static str,
    icon: Option<&'static str>,
    cover: Option<&'static str>,
}

const KNOWN_GAMES: &[KnownGameMeta] = &[
    KnownGameMeta {
        id: "1402418696126992445",
        name: "League of Legends",
        icon: Some(LEAGUE),
        cover: Some(LEAGUE),
    },
    KnownGameMeta {
        id: "700136079562375258",
        name: "VALORANT",
        icon: Some(VALORANT),
        cover: Some(VALORANT),
    },
    KnownGameMeta {
        id: "1346069614634864772",
        name: "Marvel Rivals",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/1346069614634864772/2f7be4ce3fb3261f76e544c56709a6ad.png?size=64&keep_aspect_ratio=false",
        ),
        cover: Some(
            "https://cdn.discordapp.com/app-icons/1346069614634864772/2f7be4ce3fb3261f76e544c56709a6ad.png?size=128&keep_aspect_ratio=false",
        ),
    },
    KnownGameMeta {
        id: "1323482066758930452",
        name: "Wuthering Waves",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/1323482066758930452/0a424370271f81fa7399c0938966aadd.png?size=64&keep_aspect_ratio=false",
        ),
        cover: Some(
            "https://cdn.discordapp.com/app-assets/1323482066758930452/1506737434887458867.webp?size=300",
        ),
    },
    KnownGameMeta {
        id: "1402692356343599254",
        name: "PEAK",
        icon: Some(LIKED[1]),
        cover: Some(LIKED[1]),
    },
    KnownGameMeta {
        id: "356875762940379136",
        name: "World of Warcraft",
        icon: Some(WANTED[1]),
        cover: Some(WANTED[1]),
    },
    KnownGameMeta {
        id: "1314395942253756416",
        name: "R.E.P.O.",
        icon: Some(WANTED[0]),
        cover: Some(WANTED[0]),
    },
    KnownGameMeta {
        id: "672188101707169852",
        name: "Wolvesville",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/672188101707169852/b7a64ea9edb7cb22588dd6530bd66c35.png?size=64&keep_aspect_ratio=false",
        ),
        cover: None,
    },
    KnownGameMeta {
        id: "1463856624455254103",
        name: "Heartopia",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/1463856624455254103/a7bd13821ede07bf4df788e48609d59e.png?size=64&keep_aspect_ratio=false",
        ),
        cover: Some(
            "https://cdn.discordapp.com/app-assets/1463856624455254103/1550358418597085287.webp?size=480",
        ),
    },
    KnownGameMeta {
        id: "1496037472226906183",
        name: "Arknights Endfield",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/1496037472226906183/f7877b08efb704adc7f7992af7e3b799.png?size=64&keep_aspect_ratio=false",
        ),
        cover: Some(
            "https://cdn.discordapp.com/app-assets/1496037472226906183/1519406898787520646.webp?size=300",
        ),
    },
    KnownGameMeta {
        id: "1481180707966685245",
        name: "Neverness to Everness",
        icon: Some(
            "https://cdn.discordapp.com/app-icons/1481180707966685245/8751bdb4c480ed61d49f9ff6d0d471f8.png?size=64&keep_aspect_ratio=false",
        ),
        cover: Some(
            "https://cdn.discordapp.com/app-assets/1481180707966685245/1552590719875551272.webp?size=480",
        ),
    },
];

/// Module 761915 preset tags exposed on Favorite Game / Games in Rotation cards.
pub(in crate::ui) fn tag_choices() -> [(&'static str, &'static str); 5] {
    [
        ("casual", dc_intl::t!("YY79oH")),
        ("master", dc_intl::t!("AHqj6c")),
        ("obsessed", dc_intl::t!("isPJDu")),
        ("open_to_play", dc_intl::t!("q30PoH")),
        ("looking_for_group", dc_intl::t!("DWWAAQ")),
    ]
}

pub(in crate::ui) fn featured_app(id: &str) -> Option<(&'static str, String)> {
    FEATURED
        .iter()
        .find(|app| app.id == id)
        .map(|app| (app.name, format!("{CDN}{}", app.icon)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModalTab {
    Interests,
    GameStats,
}

pub(super) struct ModalState {
    pub tab: ModalTab,
    pub search: Entity<Editor>,
    _events: Vec<Subscription>,
}

pub(super) struct GamePickerState {
    pub widget_index: usize,
    pub search: Entity<Editor>,
    _events: Vec<Subscription>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ResolvedGame {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
}

/// The game list widgets' titles: Favorite Game, Games I Like, Games in Rotation, Want to Play.
const LIST_TITLES: [&str; 4] = [
    "Favorite Game",
    "Games I Like",
    "Games in Rotation",
    "Want to Play",
];

/// One addable widget (`WidgetType` of module 540185, the ones `useAddableWidgets` offers here).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Widget {
    Application(Featured),
    FavoriteGames,
    PlayedGames,
    CurrentGames,
    WantToPlayGames,
}

impl Widget {
    /// `Ok` of module 229231, the title under the tile's icon (a game's own name is not a message).
    fn title(self) -> &'static str {
        match self {
            Self::Application(app) => app.name,
            Self::FavoriteGames => LIST_TITLES[0],
            Self::PlayedGames => LIST_TITLES[1],
            Self::CurrentGames => LIST_TITLES[2],
            Self::WantToPlayGames => LIST_TITLES[3],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Application(app) => app.name,
            _ => dc_intl::tr(self.title()),
        }
    }

    fn into_data(self) -> WidgetData {
        match self {
            Self::Application(app) => WidgetData::Application {
                application_id: app.id.to_owned(),
            },
            Self::FavoriteGames => WidgetData::FavoriteGames { games: Vec::new() },
            Self::PlayedGames => WidgetData::PlayedGames { games: Vec::new() },
            Self::CurrentGames => WidgetData::CurrentGames { games: Vec::new() },
            Self::WantToPlayGames => WidgetData::WantToPlayGames { games: Vec::new() },
        }
    }

    fn is_on_board(self, board: &[UserWidget]) -> bool {
        board.iter().any(|existing| match (self, &existing.data) {
            (Self::Application(app), WidgetData::Application { application_id }) => {
                app.id == application_id
            }
            (Self::FavoriteGames, WidgetData::FavoriteGames { .. })
            | (Self::PlayedGames, WidgetData::PlayedGames { .. })
            | (Self::CurrentGames, WidgetData::CurrentGames { .. })
            | (Self::WantToPlayGames, WidgetData::WantToPlayGames { .. }) => true,
            _ => false,
        })
    }
}

fn default_personal_data() -> WidgetData {
    WidgetData::Personal {
        header: String::new(),
        sections: vec![
            PersonalSection::Cover {
                title: String::new(),
                subtitle: String::new(),
            },
            PersonalSection::Fields {
                fields: vec![PersonalField::default(), PersonalField::default()],
            },
        ],
    }
}

pub(super) fn widget_game_ids(widgets: &[UserWidget]) -> Vec<String> {
    let mut ids = Vec::new();
    for widget in widgets {
        if let Some(games) = widget.data.games() {
            for game in games {
                if !ids.contains(&game.game_id) {
                    ids.push(game.game_id.clone());
                }
            }
        }
    }
    ids
}

#[cfg(feature = "capture")]
pub(super) fn widget_data_from_script(kind: &str) -> Option<WidgetData> {
    if let Some(app_id) = kind.strip_prefix("app:") {
        return Some(WidgetData::Application {
            application_id: app_id.to_owned(),
        });
    }
    match kind {
        "favorite" | "favorite_games" => Some(WidgetData::FavoriteGames { games: Vec::new() }),
        "current" | "current_games" => Some(WidgetData::CurrentGames { games: Vec::new() }),
        "want" | "want_to_play_games" => Some(WidgetData::WantToPlayGames { games: Vec::new() }),
        "played" | "played_games" => Some(WidgetData::PlayedGames { games: Vec::new() }),
        "personal" => Some(default_personal_data()),
        "clips" | "clips_gallery" => Some(WidgetData::ClipsGallery { clips: Vec::new() }),
        _ => FEATURED
            .iter()
            .find(|app| app.id == kind)
            .map(|app| WidgetData::Application {
                application_id: app.id.to_owned(),
            }),
    }
}

fn max_games_for_data(data: &WidgetData) -> Option<usize> {
    match data {
        WidgetData::FavoriteGames { .. } => Some(1),
        WidgetData::CurrentGames { .. } => Some(5),
        WidgetData::WantToPlayGames { .. } | WidgetData::PlayedGames { .. } => Some(20),
        _ => None,
    }
}

pub(in crate::ui) fn widget_data_title(data: &WidgetData) -> gpui::SharedString {
    match data {
        WidgetData::FavoriteGames { .. } => dc_intl::t!("Rpf6Ak").into(),
        WidgetData::CurrentGames { .. } => dc_intl::t!("zs6NsE").into(),
        WidgetData::WantToPlayGames { .. } => dc_intl::t!("I509Dl").into(),
        WidgetData::PlayedGames { .. } => dc_intl::t!("QTq6Pf").into(),
        WidgetData::Personal { header, .. } => {
            if header.trim().is_empty() {
                dc_intl::t!("AVkYMx").into()
            } else {
                header.clone().into()
            }
        }
        WidgetData::ClipsGallery { .. } => dc_intl::t!("zY8Ghg").into(),
        WidgetData::Application { application_id } => FEATURED
            .iter()
            .find(|app| app.id == application_id)
            .map(|app| app.name.to_owned())
            .unwrap_or_else(|| application_id.clone())
            .into(),
    }
}

fn encode_query(query: &str) -> String {
    let mut out = String::new();
    for byte in query.as_bytes() {
        if byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_' || *byte == b'.' {
            out.push(*byte as char);
        } else if *byte == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn normalize_game_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

/// `a$`: every addable widget in `Zc`'s type order.
fn addable() -> Vec<Widget> {
    FEATURED
        .iter()
        .map(|app| Widget::Application(*app))
        .chain([
            Widget::FavoriteGames,
            Widget::PlayedGames,
            Widget::CurrentGames,
            Widget::WantToPlayGames,
        ])
        .collect()
}

/// `aw` without the connected or recently played games, the clips gallery and the personal widget (none of them is
/// offered here): Favorite Game, Games I Like, Marvel Rivals, Games in Rotation, Want to Play, Wuthering Waves, cut
/// to `max`.
fn suggested(max: usize) -> Vec<Widget> {
    let all = addable();
    let app = |id: &str| {
        all.iter()
            .find(|widget| matches!(widget, Widget::Application(app) if app.id == id))
            .copied()
    };
    [
        Some(Widget::FavoriteGames),
        Some(Widget::PlayedGames),
        app("1346069614634864772"),
        Some(Widget::CurrentGames),
        Some(Widget::WantToPlayGames),
        app("1323482066758930452"),
    ]
    .into_iter()
    .flatten()
    .take(max)
    .collect()
}

/// `maxWidgetOptions` of the account's variation.
fn suggestion_count(variant: Option<i64>) -> Option<usize> {
    match variant {
        Some(1) => Some(4),
        Some(2) => Some(6),
        _ => None,
    }
}

fn skeleton(width: f32, height: f32, radius: f32) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded(px(radius))
        .bg(color("--background-mod-subtle"))
}

/// `coverContainer_R0hw0B placeholder`: a 56px game cover at 20%.
fn cover(image: Option<std::sync::Arc<gpui::Image>>) -> Div {
    div()
        .flex_none()
        .w(px(56.))
        .h(px(74.66))
        .rounded(px(8.))
        .overflow_hidden()
        .opacity(0.2)
        .bg(color("--background-base-low"))
        .children(image.map(|image| {
            img(image)
                .w(px(56.))
                .h(px(74.66))
                .object_fit(gpui::ObjectFit::Cover)
        }))
}

impl Root {
    fn widget_image(&self, url: &str) -> Option<std::sync::Arc<gpui::Image>> {
        self.images.get(url).cloned()
    }

    pub(super) fn profile_editor_known_game_value(&self, id: &str) -> Option<Value> {
        let game = self.profile_editor_resolve_game(id);
        let mut media = serde_json::Map::new();
        if let Some(icon) = game.icon {
            media.insert("icon".into(), json!({ "type": "url", "value": icon }));
        }
        if let Some(cover) = game.cover {
            media.insert("cover".into(), json!({ "type": "url", "value": cover }));
        }
        Some(json!({
            "id": game.id,
            "name": game.name,
            "media": media,
        }))
    }

    pub(in crate::ui) fn profile_editor_resolve_game(&self, id: &str) -> ResolvedGame {
        if let Some(game) = self.games.get(id) {
            let fallback = KNOWN_GAMES.iter().find(|entry| entry.id == id);
            return ResolvedGame {
                id: id.to_owned(),
                name: if game.name.is_empty() {
                    fallback.map_or_else(|| id.to_owned(), |entry| entry.name.to_owned())
                } else {
                    game.name.clone()
                },
                icon: game
                    .icon
                    .clone()
                    .or_else(|| fallback.and_then(|entry| entry.icon.map(str::to_owned))),
                cover: game
                    .cover
                    .clone()
                    .or_else(|| fallback.and_then(|entry| entry.cover.map(str::to_owned))),
            };
        }
        if let Some(entry) = KNOWN_GAMES.iter().find(|entry| entry.id == id) {
            return ResolvedGame {
                id: id.to_owned(),
                name: entry.name.to_owned(),
                icon: entry.icon.map(str::to_owned),
                cover: entry.cover.map(str::to_owned),
            };
        }
        ResolvedGame {
            id: id.to_owned(),
            name: id.to_owned(),
            icon: None,
            cover: None,
        }
    }

    fn profile_editor_catalog_games(&self, query: &str) -> Vec<ResolvedGame> {
        let q = query.trim().to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut list = Vec::new();
        for meta in KNOWN_GAMES {
            if seen.insert(meta.id.to_owned()) {
                list.push(self.profile_editor_resolve_game(meta.id));
            }
        }
        for (id, game) in &self.games {
            if !game.adult && seen.insert(id.clone()) {
                list.push(ResolvedGame {
                    id: id.clone(),
                    name: game.name.clone(),
                    icon: game.icon.clone(),
                    cover: game.cover.clone(),
                });
            }
        }
        if !q.is_empty() {
            list.retain(|game| {
                game.name.to_lowercase().contains(&q) || game.id.to_lowercase().contains(&q)
            });
        }
        list
    }

    fn request_widget_art(&mut self, widgets: &[Widget]) {
        let mut urls: Vec<String> = Vec::new();
        for widget in widgets {
            match widget {
                Widget::Application(app) => {
                    urls.push(format!("{CDN}{}", app.icon));
                    if let Art::Hero(url, _) | Art::Square(url) = app.art {
                        urls.push(format!("{CDN}{url}"));
                    }
                }
                Widget::FavoriteGames => urls.push(LEAGUE.to_owned()),
                Widget::CurrentGames => urls.push(VALORANT.to_owned()),
                Widget::PlayedGames => urls.extend(LIKED.iter().map(|url| (*url).to_owned())),
                Widget::WantToPlayGames => urls.extend(WANTED.iter().map(|url| (*url).to_owned())),
            }
        }
        for url in urls {
            self.request_image(url.clone(), url);
        }
    }

    fn request_known_game_art(&mut self, games: &[ResolvedGame]) {
        for game in games {
            if let Some(url) = &game.cover {
                self.request_image(url.clone(), url.clone());
            }
            if let Some(url) = &game.icon {
                self.request_image(url.clone(), url.clone());
            }
        }
    }

    fn request_board_art(&mut self) {
        let Some(open) = self.profile_editor.as_ref() else {
            return;
        };
        let mut urls = Vec::new();
        let mut missing_ids = Vec::new();
        for widget in &open.board_widgets {
            if let WidgetData::Application { application_id } = &widget.data
                && let Some(app) = FEATURED.iter().find(|entry| entry.id == application_id)
            {
                urls.push(format!("{CDN}{}", app.icon));
                if let Art::Hero(path, _) | Art::Square(path) = app.art {
                    urls.push(format!("{CDN}{path}"));
                }
            }
            if let Some(games) = widget.data.games() {
                for entry in games {
                    if !self.games.contains_key(&entry.game_id) {
                        missing_ids.push(entry.game_id.clone());
                    }
                    let resolved = self.profile_editor_resolve_game(&entry.game_id);
                    if let Some(url) = resolved.cover {
                        urls.push(url);
                    }
                    if let Some(url) = resolved.icon {
                        urls.push(url);
                    }
                }
            }
        }
        for url in urls {
            self.request_image(url.clone(), url);
        }
        if !missing_ids.is_empty() {
            self.request_games(&missing_ids);
        }
    }

    fn search_widget_games(&mut self, query: &str) {
        let Some(normal) = normalize_game_query(query) else {
            return;
        };
        let key = format!(
            "{}search:{normal}",
            super::super::server_games::GAMES_PREFIX
        );
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!(
                    "/games/autocomplete?q={}&filter_group=default",
                    encode_query(&normal)
                ),
            });
        }
    }

    pub(super) fn profile_editor_sync_widgets_pending(&mut self) {
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let draft = saveable_widgets(&open.board_widgets);
        let saved = saveable_widgets(&open.saved.widgets);
        open.pending.widgets = (draft != saved).then_some(draft);
    }

    pub(super) fn profile_editor_prepend_widget(
        &mut self,
        data: WidgetData,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        let key = data.unique_key();
        let duplicate = open
            .board_widgets
            .iter()
            .any(|existing| existing.data.unique_key() == key);
        if !duplicate {
            open.board_widgets.insert(0, UserWidget { id: None, data });
            open.tab = Tab::Board;
        }
        open.widget_modal = None;
        self.profile_editor_sync_widgets_pending();
        cx.notify();
    }

    pub(super) fn profile_editor_open_widget_modal(&mut self, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let style = profile_field_style(
                14.,
                18.,
                400.,
                color("--text-default").into(),
                color("--input-placeholder-text-default").into(),
            );
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_placeholder(dc_intl::t!("fBRUk1").to_owned());
            editor
        });
        let sub = cx.subscribe(&search, |_, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                cx.notify();
            }
        });
        if let Some(open) = self.profile_editor.as_mut() {
            open.widget_modal = Some(ModalState {
                tab: ModalTab::Interests,
                search,
                _events: vec![sub],
            });
            open.game_picker = None;
            open.tag_select_open = false;
        }
        self.request_widget_art(&addable());
        cx.notify();
    }

    pub(super) fn profile_editor_open_game_picker(
        &mut self,
        widget_index: usize,
        cx: &mut Context<Self>,
    ) {
        let search = cx.new(|cx| {
            let style = profile_field_style(
                14.,
                18.,
                400.,
                color("--text-default").into(),
                color("--input-placeholder-text-default").into(),
            );
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_placeholder(dc_intl::t!("5h0QOP").to_owned());
            editor
        });
        let sub = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let query = editor.read(cx).text().to_owned();
                this.search_widget_games(&query);
                cx.notify();
            }
        });
        if let Some(open) = self.profile_editor.as_mut()
            && widget_index < open.board_widgets.len()
        {
            open.game_picker = Some(GamePickerState {
                widget_index,
                search,
                _events: vec![sub],
            });
        }
        let catalog = self.profile_editor_catalog_games("");
        self.request_known_game_art(&catalog);
        cx.notify();
    }

    pub(super) fn profile_editor_add_game_to_widget(
        &mut self,
        widget_index: usize,
        game_id: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.profile_editor.as_mut()
            && let Some(widget) = open.board_widgets.get_mut(widget_index)
            && let Some(limit) = max_games_for_data(&widget.data)
            && let Some(games) = widget.data.games_mut()
            && games.len() < limit
            && !games.iter().any(|existing| existing.game_id == game_id)
        {
            games.push(WidgetGame {
                game_id: game_id.clone(),
                comment: None,
                tags: Vec::new(),
            });
            open.game_picker = None;
        }
        self.request_games(&[game_id]);
        self.profile_editor_sync_widgets_pending();
        cx.notify();
    }

    fn profile_editor_remove_game_from_widget(
        &mut self,
        widget_index: usize,
        game_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.profile_editor.as_mut()
            && let Some(widget) = open.board_widgets.get_mut(widget_index)
            && let Some(games) = widget.data.games_mut()
        {
            games.retain(|existing| existing.game_id != game_id);
        }
        self.profile_editor_sync_widgets_pending();
        cx.notify();
    }

    fn profile_editor_toggle_game_tag(
        &mut self,
        widget_index: usize,
        game_id: &str,
        tag_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.profile_editor.as_mut()
            && let Some(widget) = open.board_widgets.get_mut(widget_index)
            && let Some(games) = widget.data.games_mut()
            && let Some(game) = games
                .iter_mut()
                .find(|existing| existing.game_id == game_id)
        {
            if let Some(pos) = game.tags.iter().position(|existing| existing == tag_id) {
                game.tags.remove(pos);
            } else if game.tags.len() < 3 {
                game.tags.push(tag_id.to_owned());
            }
        }
        self.profile_editor_sync_widgets_pending();
        cx.notify();
    }

    pub(super) fn profile_editor_remove_widget_now(
        &mut self,
        widget_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.profile_editor.as_mut()
            && widget_index < open.board_widgets.len()
        {
            open.board_widgets.remove(widget_index);
            open.widget_confirm_remove = None;
            open.game_picker = None;
            open.widget_comment_editors.clear();
        }
        self.profile_editor_sync_widgets_pending();
        cx.notify();
    }

    fn profile_editor_comment_editor(
        &mut self,
        key: String,
        initial: &str,
        placeholder: String,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        let style = profile_field_style(
            13.,
            18.,
            400.,
            palette.token("--text-default").into(),
            palette.token("--input-placeholder-text-default").into(),
        );
        if let Some(existing) = self
            .profile_editor
            .as_ref()
            .and_then(|open| open.widget_comment_editors.get(&key).cloned())
        {
            existing.update(cx, |editor, _| editor.set_style(style));
            return existing;
        }
        let editor = cx.new(|cx| {
            let mut ed = Editor::new(style, cx);
            ed.set_single_line(true);
            ed.set_placeholder(placeholder);
            ed.set_text(initial);
            ed
        });
        let key_clone = key.clone();
        let sub = cx.subscribe(&editor, move |this, ed, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let text = ed.read(cx).text().to_owned();
                if let Some(open) = this.profile_editor.as_mut() {
                    apply_comment_edit(&mut open.board_widgets, &key_clone, text);
                }
                this.profile_editor_sync_widgets_pending();
                cx.notify();
            }
        });
        if let Some(open) = self.profile_editor.as_mut() {
            open.widget_comment_editors.insert(key, editor.clone());
            open._events.push(sub);
        }
        editor
    }

    /// One add tile (`addButtonContainer_ECBSq8`, size small): the widget's placeholder picture behind the add (or
    /// link, for a game whose account is not linked) icon and the title.
    fn widget_tile(
        &self,
        id: gpui::SharedString,
        widget: Widget,
        width: f32,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let picture = match widget {
            Widget::Application(app) => {
                // `container_5TW5x6`: the skeleton widget on the left 60%, the art on the right 40%, 4px apart.
                let art_width = (width - 4.) * 0.4;
                let content_width = (width - 4.) * 0.6 - 32.;
                let stat = (content_width - 16.) / 3.;
                let art: Div = match app.art {
                    Art::Hero(url, ratio) => div()
                        .w(px(art_width))
                        .h(px(99.))
                        .overflow_hidden()
                        .children(self.widget_image(&format!("{CDN}{url}")).map(|image| {
                            img(image)
                                .w(px(art_width))
                                .h(px(art_width / ratio))
                                .opacity(0.2)
                        })),
                    Art::Square(url) => div().w(px(art_width)).p(px(16.)).child(
                        div()
                            .size(px(art_width - 32.))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(palette.token("--background-mod-subtle"))
                            .children(
                                self.widget_image(&format!("{CDN}{url}"))
                                    .map(|image| img(image).size(px(art_width - 32.)).opacity(0.2)),
                            ),
                    ),
                    Art::Skeleton => div().w(px(art_width)).p(px(16.)).child(skeleton(
                        art_width - 32.,
                        art_width - 32.,
                        8.,
                    )),
                };
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .gap(px(4.))
                    .child(
                        div()
                            .w(px(content_width + 32.))
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(skeleton(content_width / 2., 12., 8.))
                            .child(
                                div()
                                    .w(px(content_width))
                                    .h(px(1.))
                                    .bg(palette.token("--background-mod-subtle")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .w(px(content_width))
                                    .children((0..6).map(|_| skeleton(stat, 15., 12.))),
                            ),
                    )
                    .child(art)
            }
            // `placeholderDetailsCard_6BQISv`: one cover and two text bars.
            Widget::FavoriteGames | Widget::CurrentGames => {
                let url = if widget == Widget::FavoriteGames {
                    LEAGUE
                } else {
                    VALORANT
                };
                div()
                    .absolute()
                    .inset_0()
                    .p(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(cover(self.widget_image(url)))
                    .child(
                        div()
                            .w(px(120.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(skeleton(72., 12., 8.))
                            .child(skeleton(114., 12., 8.)),
                    )
            }
            // `placeholderCoverGrid_6BQISv`: a row of covers, clipped at 184px.
            Widget::PlayedGames | Widget::WantToPlayGames => {
                let urls = if widget == Widget::PlayedGames {
                    LIKED
                } else {
                    WANTED
                };
                div().absolute().inset_0().p(px(12.)).child(
                    div()
                        .w(px(184.))
                        .overflow_hidden()
                        .flex()
                        .gap(px(8.))
                        .children(urls.iter().map(|url| cover(self.widget_image(url)))),
                )
            }
        };
        let (badge, app_icon) = match widget {
            Widget::Application(app) => (
                icon("discover_link", 24., "--text-default").into_any_element(),
                Some(app.icon),
            ),
            _ => (plus_badge().into_any_element(), None),
        };
        let title = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(app_icon.map(|path| {
                div()
                    .size(px(16.))
                    .rounded(px(2.))
                    .overflow_hidden()
                    .children(
                        self.widget_image(&format!("{CDN}{path}"))
                            .map(|image| img(image).size(px(16.))),
                    )
            }))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(palette.token("--text-strong"))
                    .child(widget.label()),
            );
        let data = widget.into_data();
        div()
            .id(id)
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(if matches!(widget, Widget::Application(_)) {
                99.
            } else {
                98.66
            }))
            .rounded(px(8.))
            .overflow_hidden()
            .cursor_pointer()
            .bg(palette.tile_background(false))
            .hover(|style| style.bg(palette.tile_background(true)))
            .child(picture)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(badge)
                    .child(title),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_editor_prepend_widget(data.clone(), cx);
            }))
    }

    fn board_widget_card(
        &mut self,
        index: usize,
        widget: &UserWidget,
        board_width: f32,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let title = widget_data_title(&widget.data);
        let title_str = title.to_string();
        let subtitle: Option<gpui::SharedString> = match &widget.data {
            WidgetData::FavoriteGames { games } if games.is_empty() => {
                Some(dc_intl::t!("wiXdEa").into())
            }
            WidgetData::CurrentGames { games }
            | WidgetData::WantToPlayGames { games }
            | WidgetData::PlayedGames { games }
                if games.is_empty() =>
            {
                let max = max_games_for_data(&widget.data).unwrap_or(5);
                Some(dc_intl::t!("zR1+0/", numGames = max).into())
            }
            WidgetData::FavoriteGames { games }
            | WidgetData::CurrentGames { games }
            | WidgetData::WantToPlayGames { games }
            | WidgetData::PlayedGames { games } => {
                Some(dc_intl::t!("r/EUap", count = games.len()).into())
            }
            WidgetData::Personal { .. } if !self.profile_editor_premium() => {
                Some(dc_intl::t!("55tM3t").into())
            }
            WidgetData::ClipsGallery { clips } => {
                Some(dc_intl::t!("pb2Was", numClips = 6usize.saturating_sub(clips.len())).into())
            }
            WidgetData::Application { .. } => Some(dc_intl::t!("EKR8Ps").into()),
            _ => None,
        };
        let can_add_game = max_games_for_data(&widget.data)
            .is_some_and(|max| widget.data.games().is_some_and(|games| games.len() < max));
        let app_icon_el = match &widget.data {
            WidgetData::Application { application_id } => FEATURED
                .iter()
                .find(|app| app.id == application_id)
                .and_then(|app| self.widget_image(&format!("{CDN}{}", app.icon)))
                .map(|image| {
                    div()
                        .size(px(20.))
                        .rounded(px(4.))
                        .overflow_hidden()
                        .child(img(image).size(px(20.)))
                }),
            _ => None,
        };
        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .children(app_icon_el)
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(palette.token("--text-strong"))
                                    .child(title),
                            ),
                    )
                    .children(subtitle.map(|sub| {
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(palette.token("--text-muted"))
                            .child(sub)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(can_add_game.then(|| {
                        div()
                            .id(gpui::SharedString::from(format!(
                                "widget-browse-games-{index}"
                            )))
                            .h(px(28.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .bg(palette.token("--control-secondary-background-default"))
                            .hover(|style| {
                                style.bg(palette.token("--control-secondary-background-hover"))
                            })
                            .border_1()
                            .border_color(palette.token("--control-secondary-border-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .child(icon("plus_small", 14., "--control-secondary-text-default"))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(palette.token("--control-secondary-text-default"))
                                    .child(dc_intl::t!("uqw8wK")),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_editor_open_game_picker(index, cx);
                            }))
                    }))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("widget-remove-{index}")))
                            .size(px(28.))
                            .rounded(px(8.))
                            .bg(palette.token("--control-secondary-background-default"))
                            .hover(|style| {
                                style.bg(palette.token("--control-secondary-background-hover"))
                            })
                            .border_1()
                            .border_color(palette.token("--control-secondary-border-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(
                                icon("ob_trash", 14., "--control-secondary-text-default")
                                    .text_color(palette.token("--control-secondary-text-default")),
                            )
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                if event.modifiers().shift {
                                    this.profile_editor_remove_widget_now(index, cx);
                                } else if let Some(open) = this.profile_editor.as_mut() {
                                    open.widget_confirm_remove = Some(index);
                                    cx.notify();
                                }
                            })),
                    ),
            );
        let _ = title_str;

        let body: AnyElement = match &widget.data {
            WidgetData::FavoriteGames { games } => {
                if games.is_empty() {
                    self.empty_game_widget_body(index, true, 1, palette, cx)
                        .into_any_element()
                } else {
                    let entry = &games[0];
                    let resolved = self.profile_editor_resolve_game(&entry.game_id);
                    let cover_img = resolved
                        .cover
                        .as_deref()
                        .or(resolved.icon.as_deref())
                        .and_then(|url| self.widget_image(url));
                    let comment_ed = self.profile_editor_comment_editor(
                        format!("fav:{index}:{}", entry.game_id),
                        entry.comment.as_deref().unwrap_or(""),
                        dc_intl::t!("LYnrhc").to_owned(),
                        palette,
                        cx,
                    );
                    let gid_remove = entry.game_id.clone();
                    let gid_tag = entry.game_id.clone();
                    let active_tags = entry.tags.clone();
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(64.))
                                        .h(px(85.))
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .bg(palette.token("--background-mod-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .children(cover_img.map(|image| {
                                            img(image)
                                                .w(px(64.))
                                                .h(px(85.))
                                                .object_fit(gpui::ObjectFit::Cover)
                                        })),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(15.))
                                                        .line_height(px(20.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(palette.token("--text-strong"))
                                                        .child(resolved.name),
                                                )
                                                .child(
                                                    div()
                                                        .id(gpui::SharedString::from(format!(
                                                            "fav-remove-{index}-{}",
                                                            entry.game_id
                                                        )))
                                                        .size(px(22.))
                                                        .rounded(px(6.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .cursor_pointer()
                                                        .hover(|style| {
                                                            style.bg(palette.token(
                                                                "--background-mod-subtle",
                                                            ))
                                                        })
                                                        .child(icon(
                                                            "modal_close",
                                                            12.,
                                                            "--text-muted",
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.profile_editor_remove_game_from_widget(
                                                                    index,
                                                                    &gid_remove,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .rounded(px(8.))
                                                .px(px(8.))
                                                .py(px(6.))
                                                .bg(palette.token("--background-mod-subtle"))
                                                .border_1()
                                                .border_color(palette.token("--border-muted"))
                                                .child(comment_ed),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_wrap()
                                                .gap(px(6.))
                                                .children(tag_choices().into_iter().map(|(tag_id, label)| {
                                                    let selected = active_tags
                                                        .iter()
                                                        .any(|t| t == tag_id);
                                                    let tag_owned = tag_id.to_owned();
                                                    let gid = gid_tag.clone();
                                                    div()
                                                        .id(gpui::SharedString::from(format!(
                                                            "fav-tag-{index}-{tag_id}"
                                                        )))
                                                        .px(px(8.))
                                                        .h(px(22.))
                                                        .rounded(px(11.))
                                                        .flex()
                                                        .items_center()
                                                        .cursor_pointer()
                                                        .bg(if selected {
                                                            palette.token(
                                                                "--control-primary-background-default",
                                                            )
                                                        } else {
                                                            palette
                                                                .token("--background-mod-subtle")
                                                        })
                                                        .child(
                                                            div()
                                                                .text_size(px(11.))
                                                                .line_height(px(14.))
                                                                .font_weight(FontWeight(500.))
                                                                .text_color(if selected {
                                                                    palette.token(
                                                                        "--control-primary-text-default",
                                                                    )
                                                                } else {
                                                                    palette
                                                                        .token("--text-muted")
                                                                })
                                                                .child(label),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.profile_editor_toggle_game_tag(
                                                                    index, &gid, &tag_owned, cx,
                                                                );
                                                            },
                                                        ))
                                                })),
                                        ),
                                ),
                        )
                        .into_any_element()
                }
            }
            WidgetData::CurrentGames { games }
            | WidgetData::WantToPlayGames { games }
            | WidgetData::PlayedGames { games } => {
                let max = max_games_for_data(&widget.data).unwrap_or(5);
                if games.is_empty() {
                    self.empty_game_widget_body(index, false, max, palette, cx)
                        .into_any_element()
                } else {
                    let items: Vec<AnyElement> = games
                        .iter()
                        .map(|entry| {
                            let resolved = self.profile_editor_resolve_game(&entry.game_id);
                            let cover_img = resolved
                                .cover
                                .as_deref()
                                .or(resolved.icon.as_deref())
                                .and_then(|url| self.widget_image(url));
                            let gid = entry.game_id.clone();
                            div()
                                .w(px(72.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .relative()
                                        .w(px(72.))
                                        .h(px(96.))
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .bg(palette.token("--background-mod-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .children(cover_img.map(|image| {
                                            img(image)
                                                .w(px(72.))
                                                .h(px(96.))
                                                .object_fit(gpui::ObjectFit::Cover)
                                        }))
                                        .child(
                                            div()
                                                .id(gpui::SharedString::from(format!(
                                                    "wg-rm-{index}-{}",
                                                    entry.game_id
                                                )))
                                                .absolute()
                                                .top(px(4.))
                                                .right(px(4.))
                                                .size(px(20.))
                                                .rounded(px(10.))
                                                .bg(gpui::hsla(0., 0., 0., 0.65))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .child(icon("modal_close", 10., "--white"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.profile_editor_remove_game_from_widget(
                                                        index, &gid, cx,
                                                    );
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(72.))
                                        .text_size(px(12.))
                                        .line_height(px(15.))
                                        .text_color(palette.token("--text-default"))
                                        .truncate()
                                        .child(resolved.name),
                                )
                                .into_any_element()
                        })
                        .collect();
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(10.))
                        .children(items)
                        .into_any_element()
                }
            }
            WidgetData::Application { application_id } => {
                let app = FEATURED.iter().find(|entry| entry.id == application_id);
                let art = app.and_then(|entry| match entry.art {
                    Art::Hero(url, _) | Art::Square(url) => {
                        self.widget_image(&format!("{CDN}{url}"))
                    }
                    Art::Skeleton => None,
                });
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .p(px(10.))
                    .rounded(px(10.))
                    .bg(palette.token("--background-mod-subtle"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(palette.token("--text-default"))
                                    .child(app.map_or_else(
                                        || application_id.clone(),
                                        |a| a.name.to_owned(),
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(palette.token("--text-muted"))
                                    .child(dc_intl::t!("EKR8Ps")),
                            ),
                    )
                    .children(art.map(|image| {
                        div()
                            .size(px(56.))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .child(img(image).size(px(56.)).object_fit(gpui::ObjectFit::Cover))
                    }))
                    .into_any_element()
            }
            WidgetData::Personal { header, sections } => {
                let header_ed = self.profile_editor_comment_editor(
                    format!("personal:{index}:header"),
                    header,
                    dc_intl::t!("fjSaAm").to_owned(),
                    palette,
                    cx,
                );
                let mut rows: Vec<AnyElement> = vec![
                    div()
                        .rounded(px(8.))
                        .px(px(8.))
                        .py(px(6.))
                        .bg(palette.token("--background-mod-subtle"))
                        .border_1()
                        .border_color(palette.token("--border-muted"))
                        .child(header_ed)
                        .into_any_element(),
                ];
                for (sec_idx, sec) in sections.iter().enumerate() {
                    match sec {
                        PersonalSection::Cover { title, subtitle } => {
                            let t_ed = self.profile_editor_comment_editor(
                                format!("personal:{index}:sec:{sec_idx}:title"),
                                title,
                                dc_intl::t!("KqCDvK").to_owned(),
                                palette,
                                cx,
                            );
                            let s_ed = self.profile_editor_comment_editor(
                                format!("personal:{index}:sec:{sec_idx}:sub"),
                                subtitle,
                                dc_intl::t!("Hs14K3").to_owned(),
                                palette,
                                cx,
                            );
                            rows.push(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .px(px(8.))
                                            .py(px(5.))
                                            .bg(palette.token("--background-mod-subtle"))
                                            .child(t_ed),
                                    )
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .px(px(8.))
                                            .py(px(5.))
                                            .bg(palette.token("--background-mod-subtle"))
                                            .child(s_ed),
                                    )
                                    .into_any_element(),
                            );
                        }
                        PersonalSection::Fields { fields } => {
                            for (f_idx, field) in fields.iter().enumerate() {
                                let ft_ed = self.profile_editor_comment_editor(
                                    format!("personal:{index}:sec:{sec_idx}:f:{f_idx}:t"),
                                    &field.title,
                                    dc_intl::t!("KqCDvK").to_owned(),
                                    palette,
                                    cx,
                                );
                                let fd_ed = self.profile_editor_comment_editor(
                                    format!("personal:{index}:sec:{sec_idx}:f:{f_idx}:d"),
                                    &field.description,
                                    dc_intl::t!("Hs14K3").to_owned(),
                                    palette,
                                    cx,
                                );
                                rows.push(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .rounded(px(8.))
                                                .px(px(8.))
                                                .py(px(5.))
                                                .bg(palette.token("--background-mod-subtle"))
                                                .child(ft_ed),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .rounded(px(8.))
                                                .px(px(8.))
                                                .py(px(5.))
                                                .bg(palette.token("--background-mod-subtle"))
                                                .child(fd_ed),
                                        )
                                        .into_any_element(),
                                );
                            }
                        }
                    }
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows)
                    .into_any_element()
            }
            WidgetData::ClipsGallery { .. } => div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(palette.token("--background-mod-subtle"))
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(palette.token("--text-muted"))
                .child(dc_intl::t!("pb2Was", numClips = 6))
                .into_any_element(),
        };

        div()
            .id(gpui::SharedString::from(format!(
                "profile-editor-widget-card-{index}"
            )))
            .w(px(board_width))
            .rounded(px(16.))
            .p(px(16.))
            .bg(palette.token("--user-profile-overlay-background"))
            .border_1()
            .border_color(palette.token("--border-muted"))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(header)
            .child(body)
    }

    fn empty_game_widget_body(
        &mut self,
        widget_index: usize,
        single: bool,
        max_games: usize,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let quick = self
            .profile_editor_catalog_games("")
            .into_iter()
            .take(3)
            .collect::<Vec<_>>();
        self.request_known_game_art(&quick);
        let hint: gpui::SharedString = if single {
            dc_intl::t!("3FdPBT").into()
        } else {
            dc_intl::t!("W8K2GH", maxGames = max_games).into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(palette.token("--text-muted"))
                    .child(hint),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(quick.into_iter().map(|game| {
                        let gid = game.id.clone();
                        let cover_img = game
                            .cover
                            .as_deref()
                            .or(game.icon.as_deref())
                            .and_then(|url| self.widget_image(url));
                        div()
                            .id(gpui::SharedString::from(format!(
                                "quick-add-{widget_index}-{}",
                                game.id
                            )))
                            .h(px(36.))
                            .px(px(10.))
                            .rounded(px(8.))
                            .bg(palette.token("--background-mod-subtle"))
                            .hover(|style| style.bg(palette.token("--background-mod-normal")))
                            .border_1()
                            .border_color(palette.token("--border-muted"))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .children(cover_img.map(|image| {
                                div().size(px(22.)).rounded(px(4.)).overflow_hidden().child(
                                    img(image).size(px(22.)).object_fit(gpui::ObjectFit::Cover),
                                )
                            }))
                            .child(icon("plus_small", 14., "--text-strong"))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(palette.token("--text-strong"))
                                    .child(game.name),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_editor_add_game_to_widget(
                                    widget_index,
                                    gid.clone(),
                                    cx,
                                );
                            }))
                    })),
            )
    }

    /// The Board tab: `Your Widgets` when any widgets are on the board, plus suggested `+` add tiles & `Browse All Widgets`.
    fn profile_editor_board(
        &mut self,
        palette: &dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let geometry = open.geometry;
        let board_widgets = open.board_widgets.clone();
        #[cfg(feature = "capture")]
        let forced = open.suggestions;
        #[cfg(not(feature = "capture"))]
        let forced: Option<usize> = None;
        let count = forced
            .or_else(|| suggestion_count(self.stores.experiments.variant(SUGGESTIONS_EXPERIMENT)));
        let widgets = match count {
            Some(max) => suggested(max),
            None => addable(),
        };
        let remaining: Vec<Widget> = widgets
            .into_iter()
            .filter(|w| !w.is_on_board(&board_widgets))
            .collect();
        self.request_widget_art(&remaining);
        self.request_board_art();
        let tile_width = geometry.tile_width();
        let board_width = geometry.board_width();
        let rows: Vec<Div> = remaining
            .chunks(2)
            .enumerate()
            .map(|(row_idx, pair)| {
                div()
                    .flex()
                    .gap(px(16.))
                    .when(pair.len() == 1, |this| this.justify_center())
                    .children(pair.iter().enumerate().map(|(col_idx, widget)| {
                        self.widget_tile(
                            gpui::SharedString::from(format!(
                                "widget-add-tile-{row_idx}-{col_idx}"
                            )),
                            *widget,
                            tile_width,
                            palette,
                            cx,
                        )
                    }))
            })
            .collect();

        let browse_btn = div()
            .id("profile-editor-browse-widgets")
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .cursor_pointer()
            .bg(palette.token("--control-secondary-background-default"))
            .hover(|style| style.bg(palette.token("--control-secondary-background-hover")))
            .border_1()
            .border_color(palette.token("--control-secondary-border-default"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(palette.token("--control-secondary-text-default"))
                    .child(dc_intl::t!("/NKLK5")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.profile_editor_open_widget_modal(cx);
            }));

        if !board_widgets.is_empty() {
            let cards: Vec<gpui::Stateful<Div>> = board_widgets
                .iter()
                .enumerate()
                .map(|(index, widget)| {
                    self.board_widget_card(index, widget, board_width, palette, cx)
                })
                .collect();
            return div()
                .w(px(board_width))
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(palette.token("--text-strong"))
                                .child(dc_intl::t!("OYlggR")),
                        )
                        .child(browse_btn),
                )
                .children(cards)
                .when(!rows.is_empty(), |this| {
                    this.child(
                        div()
                            .w(px(board_width))
                            .pt(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .children(rows),
                    )
                });
        }

        // `tabPanelScroller_y4ZEM5` is the tab panel (the card's height less the 48px tab bar) less 20px above and
        // 16px below; `empty_jFcvcH` (`flex: 1`, centred) fills it.
        let fill = (geometry.card_height() - 48. - 36.).max(0.);
        div()
            .w(px(board_width))
            .min_h(px(fill))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.))
            .child(
                div()
                    .w(px(320.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(palette.token("--text-strong"))
                            .child(dc_intl::t!("oqalC+")),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(palette.token("--text-default"))
                            .child(dc_intl::tr(
                                "Choose from our library of Widgets to share more about yourself and your interests",
                            )),
                    ),
            )
            .child(
                div()
                    .w(px(board_width))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .children(rows),
            )
            .children(count.map(|_| browse_btn))
    }

    pub(super) fn profile_editor_widget_modal(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.profile_editor.as_ref()?.widget_modal.as_ref()?;
        let tab = modal.tab;
        let search = modal.search.clone();
        let query = search.read(cx).text().trim().to_lowercase();
        let board = self.profile_editor.as_ref()?.board_widgets.clone();
        let is_nitro = self.profile_editor_premium();

        let interests_widgets = [
            (
                Widget::FavoriteGames.into_data(),
                dc_intl::t!("Rpf6Ak"),
                None,
            ),
            (
                Widget::CurrentGames.into_data(),
                dc_intl::t!("zs6NsE"),
                None,
            ),
            (
                Widget::WantToPlayGames.into_data(),
                dc_intl::t!("I509Dl"),
                None,
            ),
            (Widget::PlayedGames.into_data(), dc_intl::t!("QTq6Pf"), None),
            (
                default_personal_data(),
                dc_intl::t!("AVkYMx"),
                Some(dc_intl::t!("xIJpoK")),
            ),
            (
                WidgetData::ClipsGallery { clips: Vec::new() },
                dc_intl::t!("zY8Ghg"),
                None,
            ),
        ];

        let cards: Vec<AnyElement> = match tab {
            ModalTab::Interests => {
                let mut items = Vec::new();
                for (idx, (data, title, desc)) in interests_widgets.into_iter().enumerate() {
                    let already = board.iter().any(|existing| {
                        std::mem::discriminant(&existing.data) == std::mem::discriminant(&data)
                    });
                    if already {
                        continue;
                    }
                    let is_personal = matches!(data, WidgetData::Personal { .. });
                    let data_clone = data.clone();
                    items.push(
                        div()
                            .id(gpui::SharedString::from(format!("modal-interest-{idx}")))
                            .w(px(236.))
                            .min_h(px(96.))
                            .p(px(14.))
                            .rounded(px(12.))
                            .bg(color("--background-surface-high"))
                            .hover(|style| style.bg(color("--background-mod-normal")))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .gap(px(8.))
                            .cursor_pointer()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(text(title, 15., 20., 600., "--text-strong"))
                                    .child(if is_personal && !is_nitro {
                                        icon("clips_nitro", 16., "--text-brand").into_any_element()
                                    } else {
                                        plus_badge().into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(desc.unwrap_or(dc_intl::t!("r6EJOu"))),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_editor_prepend_widget(data_clone.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                }
                items
            }
            ModalTab::GameStats => {
                let mut items = Vec::new();
                for (idx, app) in FEATURED.iter().enumerate() {
                    if !query.is_empty() && !app.name.to_lowercase().contains(&query) {
                        continue;
                    }
                    let widget = Widget::Application(*app);
                    if widget.is_on_board(&board) {
                        continue;
                    }
                    let app_icon = self.widget_image(&format!("{CDN}{}", app.icon));
                    let data = widget.into_data();
                    items.push(
                        div()
                            .id(gpui::SharedString::from(format!("modal-stat-{idx}")))
                            .w(px(236.))
                            .h(px(96.))
                            .p(px(14.))
                            .rounded(px(12.))
                            .bg(color("--background-surface-high"))
                            .hover(|style| style.bg(color("--background-mod-normal")))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .cursor_pointer()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .children(app_icon.map(|image| {
                                                div()
                                                    .size(px(20.))
                                                    .rounded(px(4.))
                                                    .overflow_hidden()
                                                    .child(img(image).size(px(20.)))
                                            }))
                                            .child(text(app.name, 15., 20., 600., "--text-strong")),
                                    )
                                    .child(icon("discover_link", 18., "--text-default")),
                            )
                            .child(text(dc_intl::t!("EKR8Ps"), 12., 16., 400., "--text-muted"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_editor_prepend_widget(data.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                }
                items
            }
        };

        let empty_notice = cards.is_empty().then(|| {
            let msg: gpui::SharedString = if tab == ModalTab::GameStats && !query.is_empty() {
                dc_intl::t!("+p0UgM").into()
            } else {
                let tab_name = match tab {
                    ModalTab::Interests => dc_intl::t!("NpchGq"),
                    ModalTab::GameStats => dc_intl::t!("EKR8Ps"),
                };
                dc_intl::t!("mcdIFp", tabName = tab_name).into()
            };
            div()
                .py(px(32.))
                .text_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(msg)
        });

        let sidebar = div()
            .w(px(176.))
            .flex_none()
            .p(px(12.))
            .border_r_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(
                [
                    (ModalTab::Interests, dc_intl::t!("NpchGq")),
                    (ModalTab::GameStats, dc_intl::t!("EKR8Ps")),
                ]
                .into_iter()
                .map(|(target, label)| {
                    let active = tab == target;
                    div()
                        .id(gpui::SharedString::from(format!(
                            "widget-modal-tab-{label}"
                        )))
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .bg(if active {
                            color("--background-mod-normal")
                        } else {
                            gpui::rgba(0)
                        })
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .child(text(
                            label,
                            14.,
                            18.,
                            if active { 600. } else { 500. },
                            if active {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            },
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(modal) = this
                                .profile_editor
                                .as_mut()
                                .and_then(|open| open.widget_modal.as_mut())
                            {
                                modal.tab = target;
                            }
                            cx.notify();
                        }))
                }),
            );

        let right_pane = div()
            .flex_1()
            .min_w(px(0.))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .when(tab == ModalTab::GameStats, |this| {
                this.child(
                    div()
                        .h(px(36.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .bg(color("--input-background-default"))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("search", 16., "--text-muted"))
                        .child(div().flex_1().child(search)),
                )
            })
            .child(
                div()
                    .id("widget-modal-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .children(empty_notice)
                    .when(!cards.is_empty(), |this| {
                        this.child(div().flex().flex_wrap().gap(px(12.)).children(cards))
                    }),
            );

        let dialog = div()
            .id("profile-editor-add-widget-modal")
            .occlude()
            .w(px(700.))
            .h(px(480.))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(56.))
                    .px(px(20.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(dc_intl::t!("grUgR+"), 18., 22., 600., "--text-strong"))
                    .child(
                        div()
                            .id("widget-modal-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-subtle")))
                            .child(icon("modal_close", 16., "--interactive-text-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.profile_editor.as_mut() {
                                    open.widget_modal = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(sidebar)
                    .child(right_pane),
            );

        Some(
            div()
                .id("profile-editor-widget-modal-backdrop")
                .absolute()
                .inset_0()
                .bg(color("--background-scrim"))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.widget_modal = None;
                    }
                    cx.notify();
                }))
                .child(dialog)
                .into_any_element(),
        )
    }

    pub(super) fn profile_editor_game_picker_popout(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (widget_index, search) = {
            let picker = self.profile_editor.as_ref()?.game_picker.as_ref()?;
            (picker.widget_index, picker.search.clone())
        };
        let query = search.read(cx).text().to_owned();
        let already: Vec<String> = self
            .profile_editor
            .as_ref()?
            .board_widgets
            .get(widget_index)
            .and_then(|w| w.data.games())
            .map(|list| list.iter().map(|g| g.game_id.clone()).collect())
            .unwrap_or_default();
        let games: Vec<ResolvedGame> = self
            .profile_editor_catalog_games(&query)
            .into_iter()
            .filter(|game| !already.contains(&game.id))
            .take(24)
            .collect();
        self.request_known_game_art(&games);

        let rows: Vec<AnyElement> = games
            .into_iter()
            .map(|game| {
                let gid = game.id.clone();
                let icon_img = game
                    .icon
                    .as_deref()
                    .or(game.cover.as_deref())
                    .and_then(|url| self.widget_image(url));
                div()
                    .id(gpui::SharedString::from(format!("game-pick-{}", game.id)))
                    .h(px(38.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(icon_img.map(|image| {
                                img(image).size(px(24.)).object_fit(gpui::ObjectFit::Cover)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .truncate()
                            .child(game.name),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_add_game_to_widget(widget_index, gid.clone(), cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let dialog = div()
            .id("profile-editor-game-picker")
            .occlude()
            .w(px(340.))
            .max_h(px(400.))
            .p(px(12.))
            .rounded(px(12.))
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(10.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(dc_intl::t!("PYyENc"), 15., 20., 600., "--text-strong"))
                    .child(
                        div()
                            .id("game-picker-close")
                            .size(px(24.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-subtle")))
                            .child(icon("modal_close", 14., "--text-muted"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.profile_editor.as_mut() {
                                    open.game_picker = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(34.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("search", 14., "--text-muted"))
                    .child(div().flex_1().child(search)),
            )
            .child(
                div()
                    .id("game-picker-list")
                    .flex_1()
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows)
                    .when(query.trim().len() > 1, |this| {
                        this.when_some(None::<()>, |el, _| el)
                    }),
            );

        Some(
            div()
                .id("profile-editor-game-picker-away")
                .absolute()
                .inset_0()
                .bg(color("--background-scrim"))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.game_picker = None;
                    }
                    cx.notify();
                }))
                .child(dialog)
                .into_any_element(),
        )
    }

    pub(super) fn profile_editor_remove_widget_modal(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let index = self.profile_editor.as_ref()?.widget_confirm_remove?;
        let dialog = div()
            .id("profile-editor-remove-widget-dialog")
            .occlude()
            .w(px(420.))
            .p(px(20.))
            .rounded(px(12.))
            .bg(color("--background-base-lower"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(14.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(text(dc_intl::t!("Mm07Yc"), 18., 22., 600., "--text-strong"))
            .child(text(dc_intl::t!("bQNGeT"), 13., 18., 400., "--text-muted"))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("widget-remove-cancel")
                            .h(px(34.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(text(
                                dc_intl::t!("ETE/oC"),
                                14.,
                                18.,
                                500.,
                                "--control-secondary-text-default",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.profile_editor.as_mut() {
                                    open.widget_confirm_remove = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("widget-remove-confirm")
                            .h(px(34.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .bg(color("--status-danger"))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(text(dc_intl::t!("Mm07Yc"), 14., 18., 500., "--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_editor_remove_widget_now(index, cx);
                            })),
                    ),
            );

        Some(
            div()
                .id("profile-editor-remove-widget-backdrop")
                .absolute()
                .inset_0()
                .bg(color("--background-scrim"))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.profile_editor.as_mut() {
                        open.widget_confirm_remove = None;
                    }
                    cx.notify();
                }))
                .child(dialog)
                .into_any_element(),
        )
    }

    pub(super) fn profile_editor_widgets(
        &mut self,
        palette: &dm_profile::Palette,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let (tab, nitro) = (open.tab, open.mode == Mode::Nitro);
        let geometry = open.geometry;
        let tabs: &[(Tab, &'static str)] = if nitro {
            &[
                (Tab::Activity, dc_intl::tr("Activity")),
                (Tab::Wishlist, dc_intl::tr("Wishlist")),
            ]
        } else {
            &[
                (Tab::Board, dc_intl::tr("Board")),
                (Tab::Activity, dc_intl::tr("Activity")),
                (Tab::Wishlist, dc_intl::tr("Wishlist")),
            ]
        };
        let bar = div()
            .flex()
            .border_b_1()
            .border_color(palette.token("--border-subtle"))
            .children(tabs.iter().enumerate().map(|(index, (kind, name))| {
                let selected = *kind == tab;
                let kind = *kind;
                div()
                    .id(*name)
                    .h(px(30.))
                    .when(index > 0, |this| this.ml(px(24.)))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(if selected {
                        palette.token("--text-strong")
                    } else {
                        gpui::rgba(0)
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(if selected { 600. } else { 500. }))
                            .text_color(palette.token(if selected {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            }))
                            .child(*name),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.tab = kind;
                        }
                        cx.notify();
                    }))
            }));
        let content: AnyElement = match tab {
            Tab::Board => self.profile_editor_board(palette, cx).into_any_element(),
            Tab::Activity => self.profile_editor_activity_tab().into_any_element(),
            Tab::Wishlist => self
                .profile_editor_wishlist_tab(window, cx)
                .into_any_element(),
        };
        let board = geometry.board_width();
        div()
            .parity("profile-editor-widgets")
            .w(px(geometry.widgets_width() + 8.))
            .h_full()
            .flex_none()
            .pl(px(36.))
            .pt(px(17.))
            .ml(px(-4.))
            .flex()
            .flex_col()
            .child(div().w(px(board)).child(bar))
            .child(
                div()
                    .id("profile-editor-widgets-scroll")
                    .w(px(board))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(px(19.))
                    .pb(px(16.))
                    .child(content),
            )
            .into_any_element()
    }
}

fn apply_comment_edit(widgets: &mut [UserWidget], key: &str, text: String) {
    let parts: Vec<&str> = key.split(':').collect();
    match parts.as_slice() {
        ["fav", idx_s, gid] => {
            if let Ok(idx) = idx_s.parse::<usize>()
                && let Some(widget) = widgets.get_mut(idx)
                && let Some(games) = widget.data.games_mut()
                && let Some(game) = games.iter_mut().find(|g| g.game_id == *gid)
            {
                let trimmed = text.trim().to_owned();
                game.comment = (!trimmed.is_empty()).then_some(trimmed);
            }
        }
        ["personal", idx_s, "header"] => {
            if let Ok(idx) = idx_s.parse::<usize>()
                && let Some(UserWidget {
                    data: WidgetData::Personal { header, .. },
                    ..
                }) = widgets.get_mut(idx)
            {
                *header = text;
            }
        }
        ["personal", idx_s, "sec", sec_s, which] => {
            if let (Ok(idx), Ok(sec_idx)) = (idx_s.parse::<usize>(), sec_s.parse::<usize>())
                && let Some(UserWidget {
                    data: WidgetData::Personal { sections, .. },
                    ..
                }) = widgets.get_mut(idx)
                && let Some(PersonalSection::Cover { title, subtitle }) = sections.get_mut(sec_idx)
            {
                match *which {
                    "title" => *title = text,
                    "sub" => *subtitle = text,
                    _ => {}
                }
            }
        }
        ["personal", idx_s, "sec", sec_s, "f", f_s, which] => {
            if let (Ok(idx), Ok(sec_idx), Ok(f_idx)) = (
                idx_s.parse::<usize>(),
                sec_s.parse::<usize>(),
                f_s.parse::<usize>(),
            ) && let Some(UserWidget {
                data: WidgetData::Personal { sections, .. },
                ..
            }) = widgets.get_mut(idx)
                && let Some(PersonalSection::Fields { fields }) = sections.get_mut(sec_idx)
                && let Some(field) = fields.get_mut(f_idx)
            {
                match *which {
                    "t" => field.title = text,
                    "d" => field.description = text,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture's grid: the six featured games, then the four game list widgets.
    #[test]
    fn every_addable_widget_in_the_captured_order() {
        let titles: Vec<&str> = addable().iter().map(|widget| widget.title()).collect();
        assert_eq!(
            titles,
            [
                "Marvel Rivals",
                "Wolvesville",
                "Wuthering Waves",
                "Heartopia",
                "Arknights Endfield",
                "Neverness to Everness",
                "Favorite Game",
                "Games I Like",
                "Games in Rotation",
                "Want to Play",
            ]
        );
    }

    /// `aw` with six options gives the owner's board; variation 1 keeps four.
    #[test]
    fn the_suggestions_follow_aw() {
        let titles =
            |max| -> Vec<&str> { suggested(max).iter().map(|widget| widget.title()).collect() };
        assert_eq!(
            titles(6),
            [
                "Favorite Game",
                "Games I Like",
                "Marvel Rivals",
                "Games in Rotation",
                "Want to Play",
                "Wuthering Waves",
            ]
        );
        assert_eq!(titles(4).len(), 4);
        assert_eq!(suggestion_count(Some(2)), Some(6));
        assert_eq!(suggestion_count(Some(1)), Some(4));
        assert_eq!(suggestion_count(Some(0)), None);
        assert_eq!(suggestion_count(None), None);
    }

    /// Every tile title is an en-US message.
    #[test]
    fn the_titles_are_messages() {
        for widget in addable() {
            if !matches!(widget, Widget::Application(_)) {
                assert!(dc_intl::keys_of(widget.title()).next().is_some());
            }
        }
    }
}
