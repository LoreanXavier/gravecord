//! The expression picker's GIFs tab, measured in `design/dom/pickers/gif-darker-cozy.json` (categories) and
//! `gif-trending-darker-cozy.json` (a result list). Data from module 497685: `GET /gifs/trending` gives the category
//! tiles, `GET /gifs/trending-gifs` and `GET /gifs/search?q=` the results (Klipy, `media_format=webm`). Picking a GIF
//! posts `POST /gifs/select {id, q}` and sends its page URL as the message, as recorded in `TRAFFIC/gif-send.json`.
//! gpui cannot play webm, so tiles show the provider's still/animated `gif_src` image.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::{Value, json};

const TRENDING_KEY: &str = "gifs:trending";
const TRENDING_GIFS_KEY: &str = "gifs:trending-gifs";
const SEARCH_PREFIX: &str = "gifs:search:";
/// The Favorites tile's picture when nothing is favourited yet (the capture's `categoryFadeBlurple` tile).
const FAVORITES_PICTURE: &str = "https://media.giphy.com/media/1TOSaJsWtnhe0/giphy.gif";
const TILE_WIDTH: f32 = 230.;
const GAP: f32 = 12.;
/// `GIF_PICKER_QUERY` asks for 50 results.
const SEARCH_LIMIT: u32 = 50;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GifView {
    Categories,
    Favorites,
    Trending,
    Search(String),
}

pub(crate) struct Gifs {
    pub view: GifView,
    pub search: Entity<Editor>,
    _events: Subscription,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Gif {
    pub id: String,
    pub url: String,
    pub picture: String,
    pub width: f32,
    pub height: f32,
}

/// One result of `/gifs/search` or `/gifs/trending-gifs`.
pub(crate) fn gifs_of(body: &Value) -> Vec<Gif> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some(Gif {
                id: entry["id"].as_str()?.to_owned(),
                url: entry["url"].as_str()?.to_owned(),
                picture: entry["gif_src"]
                    .as_str()
                    .or_else(|| entry["src"].as_str())?
                    .to_owned(),
                width: entry["width"].as_f64().unwrap_or(1.) as f32,
                height: entry["height"].as_f64().unwrap_or(1.) as f32,
            })
        })
        .collect()
}

/// A query string value, percent-encoded byte by byte outside the unreserved set.
pub(super) fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_owned(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Two 230px columns: each GIF goes under the shorter one, scaled to the column width, 12px apart.
pub(crate) fn masonry(gifs: &[Gif]) -> Vec<(usize, f32, f32)> {
    let mut columns = [0f32; 2];
    gifs.iter()
        .map(|gif| {
            let column = usize::from(columns[1] < columns[0]);
            let height = TILE_WIDTH * gif.height / gif.width.max(1.);
            let top = columns[column];
            columns[column] += height + GAP;
            (column, top, height)
        })
        .collect()
}

impl Root {
    pub(super) fn ensure_gifs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gifs.is_none() {
            let search = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder("Search Klipy".to_owned());
                editor
            });
            let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Edited => {
                        let query = editor.read(cx).text().trim().to_owned();
                        this.show_gif_view(
                            if query.is_empty() {
                                GifView::Categories
                            } else {
                                GifView::Search(query)
                            },
                            cx,
                        );
                    }
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text));
                    }
                    EditorEvent::Cancel => this.picker = None,
                }
                cx.notify();
            });
            self.gifs = Some(Gifs {
                view: GifView::Categories,
                search,
                _events: events,
            });
        }
        if !self.fetched.contains_key(TRENDING_KEY) {
            let _ = self.requests.send(Request::Fetch {
                key: TRENDING_KEY.to_owned(),
                route: "/gifs/trending?locale=en-US&media_format=webm".to_owned(),
            });
        }
        self.request_image("gif:favorites".to_owned(), FAVORITES_PICTURE.to_owned());
        if let Some(gifs) = self.gifs.as_ref() {
            window.focus(&gpui::Focusable::focus_handle(gifs.search.read(cx), cx), cx);
        }
    }

    /// The favourite GIFs: this session's list once it changed, else the account's settings as fetched.
    pub(super) fn favorite_gif_list(&self) -> Vec<dc_proto::frecency::FavoriteGif> {
        if let Some(list) = self.favorite_gifs.as_ref() {
            return list.clone();
        }
        self.fetched
            .get(super::actions::FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(dc_proto::frecency::favorite_gifs)
            .unwrap_or_default()
    }

    fn favorites_as_gifs(&self) -> Vec<Gif> {
        self.favorite_gif_list()
            .into_iter()
            .rev()
            .map(|favorite| Gif {
                id: favorite.url.clone(),
                url: favorite.url,
                picture: favorite.src,
                width: favorite.width.max(1) as f32,
                height: favorite.height.max(1) as f32,
            })
            .collect()
    }

    /// `toggle` in module 497685: the GIF joins the favourites or leaves them, and the whole message goes to the
    /// account's frecency settings (the server replaces `favorite_gifs` as one).
    fn toggle_favorite_gif(&mut self, gif: &Gif, cx: &mut Context<Self>) {
        let video = gif.picture.contains(".mp4") || gif.picture.contains(".webm");
        let favorite = dc_proto::frecency::FavoriteGif {
            url: gif.url.clone(),
            format: if video { 2 } else { 1 },
            src: gif.picture.clone(),
            width: gif.width.round() as u32,
            height: gif.height.round() as u32,
            order: 0,
        };
        let list = dc_proto::frecency::toggle_favorite_gif(self.favorite_gif_list(), favorite);
        if std::env::var_os("GRAVECORD_NO_SETTINGS_WRITE").is_none() {
            let patch = dc_proto::frecency::favorite_gifs_patch(&list, list.len() > 2);
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/2".to_owned(),
                body: Some(json!({"settings": patch})),
            });
        }
        self.favorite_gifs = Some(list);
        cx.notify();
    }

    /// Pictures for a GIF answer that just arrived: category tiles and the trending preview, or result tiles.
    pub(super) fn request_gif_images(&mut self, key: &str) {
        let Some(body) = self.fetched.get(key).cloned() else {
            return;
        };
        let mut wanted: Vec<(String, String)> = Vec::new();
        if key == TRENDING_KEY {
            for entry in body["categories"].as_array().into_iter().flatten() {
                if let (Some(name), Some(src)) = (entry["name"].as_str(), entry["src"].as_str()) {
                    wanted.push((format!("gif-category:{name}"), src.to_owned()));
                }
            }
            for gif in gifs_of(&json!([body["gifs"][0].clone()])) {
                wanted.push((format!("gif:{}", gif.id), gif.picture));
            }
        } else if key == TRENDING_GIFS_KEY || key.starts_with(SEARCH_PREFIX) {
            for gif in gifs_of(&body) {
                wanted.push((format!("gif:{}", gif.id), gif.picture));
            }
        }
        for (image, url) in wanted {
            self.request_image(image, url);
        }
    }

    /// What the GIF answers and pictures are, for the capture log: the media each entry names, and for every picture
    /// held its format, size and whether it carries animation.
    #[cfg(feature = "capture")]
    pub(crate) fn gif_report(&self) -> String {
        let mut out = String::new();
        let ext = |url: &str| {
            url.split(['?', '#'])
                .next()
                .and_then(|path| path.rsplit('.').next())
                .unwrap_or("?")
                .to_owned()
        };
        if let Some(body) = self.fetched.get(TRENDING_KEY) {
            let first = &body["categories"][0];
            out += &format!(
                "category src={} preview src={} gif_src={}; ",
                ext(first["src"].as_str().unwrap_or("")),
                ext(body["gifs"][0]["src"].as_str().unwrap_or("")),
                ext(body["gifs"][0]["gif_src"].as_str().unwrap_or("")),
            );
        }
        for key in [TRENDING_GIFS_KEY] {
            if let Some(body) = self.fetched.get(key) {
                let first = &body[0];
                out += &format!(
                    "results src={} gif_src={}; ",
                    ext(first["src"].as_str().unwrap_or("")),
                    ext(first["gif_src"].as_str().unwrap_or("")),
                );
            }
        }
        let mut pictures: Vec<_> = self
            .images
            .iter()
            .filter(|(key, _)| key.starts_with("gif:") || key.starts_with("gif-category:"))
            .collect();
        pictures.sort_by_key(|(key, _)| key.as_str());
        for (key, image) in pictures.into_iter().take(8) {
            let bytes = image.bytes();
            let animated = bytes.windows(4).take(400).any(|w| w == b"ANIM")
                || (bytes.starts_with(b"GIF8") && bytes.windows(11).any(|w| w == b"NETSCAPE2.0"));
            out += &format!(
                "{key}: {:?} {} bytes animated={animated}; ",
                image.format(),
                bytes.len()
            );
        }
        out
    }

    /// Writes the GIF pictures held to `dir`, for looking at them outside the app.
    #[cfg(feature = "capture")]
    pub(crate) fn gif_dump(&self, dir: &std::path::Path) -> usize {
        let mut count = 0;
        for (key, image) in &self.images {
            if key.starts_with("gif:") || key.starts_with("gif-category:") {
                let name = key.replace([':', '/'], "_");
                if std::fs::write(dir.join(format!("{name}.webp")), image.bytes()).is_ok() {
                    count += 1;
                }
            }
        }
        count
    }

    fn show_gif_view(&mut self, view: GifView, cx: &mut Context<Self>) {
        match &view {
            GifView::Favorites => {
                for gif in self.favorites_as_gifs() {
                    self.request_image(format!("gif:{}", gif.id), gif.picture);
                }
            }
            GifView::Trending if !self.fetched.contains_key(TRENDING_GIFS_KEY) => {
                let _ = self.requests.send(Request::Fetch {
                    key: TRENDING_GIFS_KEY.to_owned(),
                    route: format!(
                        "/gifs/trending-gifs?media_format=webm&locale=en-US&limit={SEARCH_LIMIT}"
                    ),
                });
            }
            GifView::Search(query)
                if !self
                    .fetched
                    .contains_key(&format!("{SEARCH_PREFIX}{query}")) =>
            {
                let encoded = encode_query(query);
                let _ = self.requests.send(Request::Fetch {
                    key: format!("{SEARCH_PREFIX}{query}"),
                    route: format!(
                        "/gifs/search?q={encoded}&media_format=webm&locale=en-US&limit={SEARCH_LIMIT}"
                    ),
                });
            }
            _ => {}
        }
        if let Some(gifs) = self.gifs.as_mut() {
            gifs.view = view;
        }
        cx.notify();
    }

    fn current_gifs(&self) -> (String, Vec<Gif>) {
        match self.gifs.as_ref().map(|gifs| gifs.view.clone()) {
            Some(GifView::Favorites) => (String::new(), self.favorites_as_gifs()),
            Some(GifView::Trending) => (
                String::new(),
                self.fetched
                    .get(TRENDING_GIFS_KEY)
                    .map(gifs_of)
                    .unwrap_or_default(),
            ),
            Some(GifView::Search(query)) => {
                let list = self
                    .fetched
                    .get(&format!("{SEARCH_PREFIX}{query}"))
                    .map(gifs_of)
                    .unwrap_or_default();
                (query, list)
            }
            _ => (String::new(), Vec::new()),
        }
    }

    /// `selectGIF`: analytics-side `POST /gifs/select`, then the GIF's page URL as a message.
    fn send_gif(&mut self, gif: &Gif, query: &str, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: "/gifs/select".to_owned(),
            body: Some(json!({"id": gif.id, "q": query})),
        });
        let _ = self.requests.send(Request::Send {
            channel,
            content: gif.url.clone(),
            files: Vec::new(),
            reply: None,
        });
        self.picker = None;
        cx.notify();
    }

    fn gif_tile(
        &self,
        id: String,
        label: String,
        picture: Option<(String, std::sync::Arc<gpui::Image>)>,
        (blurple, trending): (bool, bool),
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(id))
            .relative()
            .w(px(TILE_WIDTH))
            .h(px(110.))
            .rounded(px(5.))
            .overflow_hidden()
            .cursor_pointer()
            .bg(color("--background-mod-subtle"))
            .children(
                picture
                    .map(|(key, image)| self.gif_art(&key, image, (TILE_WIDTH, 110.), window, cx)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(4.))
                    // `categoryFadeBlurple`: brand at 0.8; `categoryFade`: black at 0.72.
                    .bg(if blurple {
                        gpui::rgba(0x5865f2cc)
                    } else {
                        gpui::rgba(0x000000b8)
                    })
                    .hover(|style| style.opacity(0.6)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .when(trending, |this| {
                        this.child(icon("gif_trending", 20., "--white"))
                    })
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(label),
                    ),
            )
    }

    fn gif_categories(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self
            .fetched
            .get(TRENDING_KEY)
            .cloned()
            .unwrap_or(Value::Null);
        let categories: Vec<(String, String)> = body["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["name"].as_str()?.to_owned(),
                    entry["src"].as_str()?.to_owned(),
                ))
            })
            .collect();
        let preview = gifs_of(&json!([body["gifs"][0].clone()]))
            .into_iter()
            .next();
        let mut tiles: Vec<AnyElement> = Vec::new();
        tiles.push(
            self.gif_tile(
                "gif-favorites".to_owned(),
                "Favorites".to_owned(),
                self.favorite_gif_list()
                    .last()
                    .and_then(|favorite| {
                        let key = format!("gif:{}", favorite.url);
                        Some((key.clone(), self.images.get(&key)?.clone()))
                    })
                    .or_else(|| {
                        Some((
                            "gif:favorites".to_owned(),
                            self.images.get("gif:favorites")?.clone(),
                        ))
                    }),
                (true, false),
                window,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.show_gif_view(GifView::Favorites, cx)))
            .into_any_element(),
        );
        tiles.push(
            self.gif_tile(
                "gif-trending".to_owned(),
                "Trending GIFs".to_owned(),
                preview.as_ref().and_then(|gif| {
                    let key = format!("gif:{}", gif.id);
                    Some((key.clone(), self.images.get(&key)?.clone()))
                }),
                (false, true),
                window,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.show_gif_view(GifView::Trending, cx)))
            .into_any_element(),
        );
        for (name, _) in categories {
            let query = name.clone();
            tiles.push(
                self.gif_tile(
                    format!("gif-category-{name}"),
                    name.clone(),
                    self.images
                        .get(&format!("gif-category:{name}"))
                        .map(|image| (format!("gif-category:{name}"), image.clone())),
                    (false, false),
                    window,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let query = query.clone();
                    if let Some(gifs) = this.gifs.as_ref() {
                        gifs.search.update(cx, |editor, _| editor.set_text(&query));
                    }
                    this.show_gif_view(GifView::Search(query), cx);
                }))
                .into_any_element(),
            );
        }
        div()
            .id("gif-categories")
            .size_full()
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(GAP))
            .children(tiles)
            .into_any_element()
    }

    /// `emptyHints_H8nOze`: three 160.7px cells, each a 148.7 x 160 card with 20px padding and 6px around it.
    fn favorites_hints() -> AnyElement {
        let card = |text: &'static str, star: bool| {
            div().w(px(160.67)).h(px(172.)).p(px(6.)).child(
                div()
                    .relative()
                    .size_full()
                    .rounded(px(5.))
                    .bg(gpui::rgba(0x97979f14))
                    .p(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_size(px(15.))
                    .line_height(px(24.))
                    .text_color(color("--text-default"))
                    .when(star, |this| {
                        this.child(div().absolute().top(px(6.)).right(px(6.)).child(icon(
                            "gif_favorite_filled",
                            29.,
                            "--status-warning",
                        )))
                    })
                    .child(div().w_full().child(text)),
            )
        };
        div()
            .id("gif-favorites-hints")
            .size_full()
            .overflow_y_scroll()
            .p(px(6.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .child(card(
                        "Click the star in the corner of a gif to favorite it",
                        true,
                    ))
                    .child(card("Favorites will show up here!", false))
                    .child(card("So uhh\u{2026} maybe go favorite some GIFs?", false)),
            )
            .into_any_element()
    }

    fn gif_results(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (query, list) = self.current_gifs();
        let placed = masonry(&list);
        let favorites = self.favorite_gif_list();
        let height = placed
            .iter()
            .map(|(_, top, height)| top + height)
            .fold(0f32, f32::max);
        if list.is_empty()
            && matches!(
                self.gifs.as_ref().map(|gifs| &gifs.view),
                Some(GifView::Favorites)
            )
        {
            return Self::favorites_hints();
        }
        let tiles: Vec<AnyElement> = list
            .iter()
            .zip(placed)
            .map(|(gif, (column, top, height))| {
                let picture = self.images.get(&format!("gif:{}", gif.id)).cloned();
                let art = picture.map(|image| {
                    self.gif_art(
                        &format!("gif-result:{}", gif.id),
                        image,
                        (TILE_WIDTH, height),
                        window,
                        cx,
                    )
                });
                let chosen = gif.clone();
                let starred = gif.clone();
                let query = query.clone();
                let favorite = favorites.iter().any(|known| known.url == gif.url);
                let group = SharedString::from(format!("gif-group-{}", gif.id));
                div()
                    .id(SharedString::from(format!("gif-{}", gif.id)))
                    .group(group.clone())
                    .absolute()
                    .left(px(column as f32 * (TILE_WIDTH + GAP)))
                    .top(px(top))
                    .w(px(TILE_WIDTH))
                    .h(px(height))
                    .rounded(px(5.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .bg(color("--background-mod-subtle"))
                    .children(art)
                    .child(
                        // `favButton_H8nOze`: 32 square, 5 radius, 4 from the top and right, shown on hover.
                        div()
                            .id(SharedString::from(format!("gif-favorite-{}", gif.id)))
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .size(px(32.))
                            .rounded(px(5.))
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(!favorite, |this| {
                                this.opacity(0.)
                                    .group_hover(group, |style| style.opacity(1.))
                            })
                            .child(icon(
                                if favorite {
                                    "gif_favorite_filled"
                                } else {
                                    "gif_favorite"
                                },
                                20.,
                                "--text-strong",
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_favorite_gif(&starred, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.send_gif(&chosen, &query, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .id("gif-results")
            .size_full()
            .overflow_y_scroll()
            .p(px(12.))
            .child(
                div()
                    .relative()
                    .w(px(2. * TILE_WIDTH + GAP))
                    .h(px(height))
                    .children(tiles),
            )
            .into_any_element()
    }

    /// `container_rfDq2B`: the header (search, or a back arrow and the view's title) over the tiles.
    pub(super) fn gif_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(gifs) = self.gifs.as_ref() else {
            return div().into_any_element();
        };
        let view = gifs.view.clone();
        let search = gifs.search.clone();
        let header =
            match &view {
                GifView::Trending | GifView::Favorites => div()
                    .h(px(41.))
                    .px(px(16.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("gif-back")
                            .w(px(24.))
                            .h(px(28.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_gif_view(GifView::Categories, cx)
                            }))
                            .child(icon("gif_back", 24., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(if view == GifView::Favorites {
                                "Favorites"
                            } else {
                                "Trending GIFs"
                            }),
                    )
                    .into_any_element(),
                _ => div()
                    .px(px(16.))
                    .pb(px(12.))
                    .child(
                        div()
                            .parity_with_border("gif-search", 1.)
                            .h(px(40.))
                            .pl(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-active"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .size(px(30.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("gif_search", 18., "--icon-default")),
                            )
                            .child(div().flex_1().min_w(px(0.)).pr(px(12.)).child(search)),
                    )
                    .into_any_element(),
            };
        let content = match view {
            GifView::Categories => self.gif_categories(window, cx),
            _ => self.gif_results(window, cx),
        };
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(color("--background-surface-high"))
            .child(div().bg(color("--background-surface-higher")).child(header))
            .child(div().flex_1().min_h(px(0.)).child(content))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masonry_fills_the_shorter_column() {
        let gif = |width: f32, height: f32| Gif {
            id: String::new(),
            url: String::new(),
            picture: String::new(),
            width,
            height,
        };
        // 640x370 is 133px tall at 230 wide, as the first tile in the capture.
        let placed = masonry(&[gif(640., 370.), gif(498., 324.), gif(600., 338.)]);
        assert_eq!(placed[0].0, 0);
        assert_eq!(placed[1].0, 1);
        assert_eq!(placed[2].0, 0);
        assert!((placed[0].2 - 133.).abs() < 0.1);
        assert!((placed[2].1 - 145.).abs() < 0.1);
    }

    #[test]
    fn results_read_the_recorded_shape() {
        let body = json!([{"id": "life-saturday", "url": "https://klipy.com/gifs/life-saturday",
            "src": "a.webm", "gif_src": "a.webp", "width": 640, "height": 370}]);
        let list = gifs_of(&body);
        assert_eq!(list[0].picture, "a.webp");
        assert_eq!(list[0].url, "https://klipy.com/gifs/life-saturday");
    }
}
