//! Server Profile's Games (module 989433 `Iz`): a searchable multi-select (`IB`, options from module 939200 `R`), the
//! picked games as cover tiles (`IH`, `ID`), and "Your Members Play" from `GET /guilds/{id}/top-games` (`Iw`).
//!
//! Before anything is typed the options are module 282435's popular games that have data, are not excluded (module
//! 926675) and are not adult-rated (module 932157's minimal classification). Typing asks `GET /games/autocomplete`
//! (module 518805, `filter_group=default`) for the normalized query (module 929396 `C7`). Game records come from
//! `GET /games?game_ids=` in chunks of 20 (module 920495). Measured in `design/dom/popouts/games-*-darker-cozy.json`
//! and `design/dom/settings/guild-games-picked-darker-cozy.json`.

use super::games_spec::{EXCLUDED, POPULAR};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Window, deferred, div, img,
    prelude::*, px,
};
use serde_json::Value;

/// Every games reply is keyed under this prefix so one hook routes them.
pub(super) const GAMES_PREFIX: &str = "game-";
const DATA_PREFIX: &str = "game-data:";
const SEARCH_PREFIX: &str = "game-search:";
const TOP_PREFIX: &str = "game-top:";
/// Module 989433 `IB`: the select is disabled at 20 games.
const MAX_GAMES: usize = 20;
/// Module 920495: game ids per `GET /games`.
const CHUNK: usize = 20;
const FIELD_PROBE: &str = "server-games-field";

/// What the page needs of a game record.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Game {
    pub name: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub adult: bool,
    /// From `GET /games` (classification known), not only from a search result.
    pub full: bool,
}

/// Module 744700: a game asset, a hash on the app-icons CDN or a URL as given.
fn asset_url(game: &str, asset: &Value, size: u32) -> Option<String> {
    let value = asset["value"].as_str().filter(|value| !value.is_empty())?;
    match asset["type"].as_str()? {
        "hash" => Some(format!(
            "https://cdn.discordapp.com/app-icons/{game}/{value}.png?size={size}"
        )),
        "url" => Some(value.to_owned()),
        _ => None,
    }
}

/// Module 932157 `U1` with the minimal type, compared to ADULT as module 501592 `K` does: Discord's own bits 0, 3 or 4
/// (`RESTRICTED_TO_ADULT`), ESRB rating 5, GOP classification 1, or the IGDB adult theme 21.
fn adult(classification: &Value) -> bool {
    let discord = classification["discord_classifications"]
        .as_str()
        .and_then(|bits| bits.parse::<u64>().ok())
        .or_else(|| classification["discord_classifications"].as_u64())
        .unwrap_or(0);
    let ratings = &classification["agency_ratings"];
    discord & 0b11001 != 0
        || ratings["esrb"]["rating"].as_u64() == Some(5)
        || ratings["gop"]["classification"].as_u64() == Some(1)
        || ratings["igdb"]["themes"]
            .as_array()
            .is_some_and(|themes| themes.iter().any(|theme| theme.as_u64() == Some(21)))
}

/// `GET /games` records.
fn parse_games(body: &Value) -> Vec<(String, Game)> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|game| {
            let id = game["id"].as_str()?.to_owned();
            let parsed = Game {
                name: game["name"].as_str().unwrap_or_default().to_owned(),
                icon: asset_url(&id, &game["media"]["icon"], 64),
                cover: asset_url(&id, &game["media"]["cover"], 256),
                adult: adult(&game["content_classification"]),
                full: true,
            };
            Some((id, parsed))
        })
        .collect()
}

/// Module 929396 `C7`: trimmed, lower case, underscores as spaces, at most 100 characters.
fn normalize(query: &str) -> Option<String> {
    let normal: String = query
        .trim()
        .to_lowercase()
        .replace('_', " ")
        .chars()
        .take(100)
        .collect();
    (!normal.is_empty()).then_some(normal)
}

fn excluded(id: &str) -> bool {
    EXCLUDED.contains(&id)
}

/// The 20px checkbox (`checkboxIndicator_vTUVyc`).
pub(super) fn checkbox(checked: bool) -> impl IntoElement {
    div()
        .size(px(20.))
        .rounded(px(4.))
        .border_1()
        .when(checked, |this| {
            this.border_color(gpui::rgba(0x97979f1f))
                .bg(color("--control-primary-background-default"))
        })
        .when(!checked, |this| {
            this.border_color(gpui::rgba(0x97979fa3))
                .bg(gpui::rgba(0x00000014))
        })
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |this| {
            this.child(icon("checkbox_check", 18., "--white"))
        })
}

impl Root {
    /// The Games query field, made with the page.
    pub(super) fn new_games_query(cx: &mut Context<Self>) -> (Entity<Editor>, gpui::Subscription) {
        let field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Search for a game...".to_owned());
            editor
        });
        let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    if let Some(open) = this.server_settings.as_mut() {
                        open.games_open = true;
                    }
                    let query = editor.read(cx).text().to_owned();
                    this.search_games(&query);
                }
                EditorEvent::Cancel => {
                    if let Some(open) = this.server_settings.as_mut() {
                        open.games_open = false;
                    }
                }
                EditorEvent::Submit(_) => {}
            }
            cx.notify();
        });
        (field, events)
    }

    /// Records for these ids that are not known yet, 20 to a request.
    pub(super) fn request_games(&mut self, ids: &[String]) {
        let missing: Vec<String> = ids
            .iter()
            .filter(|id| !self.games.get(*id).is_some_and(|game| game.full))
            .filter(|id| !self.requested.contains(&format!("{DATA_PREFIX}{id}")))
            .cloned()
            .collect();
        for chunk in missing.chunks(CHUNK) {
            for id in chunk {
                self.requested.insert(format!("{DATA_PREFIX}{id}"));
            }
            let query: Vec<String> = chunk.iter().map(|id| format!("game_ids={id}")).collect();
            let _ = self.requests.send(Request::Fetch {
                key: format!("{DATA_PREFIX}{}", chunk.join(",")),
                route: format!("/games?{}", query.join("&")),
            });
        }
    }

    /// Opening Server Profile: the popular games and this server's top games.
    pub(super) fn request_profile_games(&mut self, guild: Id) {
        let popular: Vec<String> = POPULAR.iter().map(|id| (*id).to_owned()).collect();
        self.request_games(&popular);
        let _ = self.requests.send(Request::Fetch {
            key: format!("{TOP_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/top-games"),
        });
    }

    fn search_games(&mut self, query: &str) {
        let Some(normal) = normalize(query) else {
            return;
        };
        let key = format!("{SEARCH_PREFIX}{normal}");
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!(
                    "/games/autocomplete?q={}&filter_group=default",
                    super::gifs::encode_query(&normal)
                ),
            });
        }
    }

    pub(super) fn games_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(body) = self.fetched.get(key).cloned() else {
            return;
        };
        if key.starts_with(DATA_PREFIX) {
            for (id, game) in parse_games(&body) {
                self.games.insert(id, game);
            }
        } else if let Some(query) = key.strip_prefix(SEARCH_PREFIX) {
            for found in body.as_array().into_iter().flatten() {
                let Some(id) = found["id"].as_str() else {
                    continue;
                };
                self.games.entry(id.to_owned()).or_insert_with(|| Game {
                    name: found["name"].as_str().unwrap_or_default().to_owned(),
                    icon: asset_url(id, &found["icon"], 64),
                    ..Game::default()
                });
            }
            if let Some(open) = self.server_settings.as_mut() {
                open.games_shown_query = Some(query.to_owned());
            }
        } else if key.starts_with(TOP_PREFIX) {
            let ids: Vec<String> = body["top_games"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry["game_application_id"].as_str().map(str::to_owned))
                .collect();
            self.request_games(&ids);
        }
        self.request_game_images();
        cx.notify();
    }

    /// The dropdown's options: popular games before typing, search results after (module 939200 `R`).
    fn game_options(&self, query: &str) -> Vec<(String, String)> {
        let Some(open) = self.server_settings.as_ref() else {
            return Vec::new();
        };
        if normalize(query).is_none() {
            return POPULAR
                .iter()
                .filter(|id| !excluded(id))
                .filter_map(|id| {
                    let game = self
                        .games
                        .get(*id)
                        .filter(|game| game.full && !game.adult)?;
                    Some(((*id).to_owned(), game.name.clone()))
                })
                .collect();
        }
        // The last answered query's results stay up until the next one arrives.
        let Some(shown) = open.games_shown_query.as_ref() else {
            return Vec::new();
        };
        let Some(body) = self.fetched.get(&format!("{SEARCH_PREFIX}{shown}")) else {
            return Vec::new();
        };
        body.as_array()
            .into_iter()
            .flatten()
            .filter_map(|found| {
                let id = found["id"].as_str()?;
                (!excluded(id)).then(|| {
                    (
                        id.to_owned(),
                        found["name"].as_str().unwrap_or_default().to_owned(),
                    )
                })
            })
            .collect()
    }

    fn toggle_game(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        if let Some(at) = open.games.iter().position(|known| known == id) {
            open.games.remove(at);
        } else if open.games.len() < MAX_GAMES {
            open.games.push(id.to_owned());
        }
        let ids = open.games.clone();
        self.request_games(&ids);
        self.request_game_images();
        cx.notify();
    }

    /// A game as a cover tile with its checkbox and name below (module 989433 `ID`).
    fn game_tile(&self, id: &str, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let game = self.games.get(id);
        let name = game
            .map(|game| game.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown game".to_owned());
        let cover = game
            .and_then(|game| game.cover.as_ref())
            .and_then(|url| self.images.get(&format!("game-cover:{url}")).cloned());
        let target = id.to_owned();
        div()
            .id(SharedString::from(format!("server-game-{id}")))
            .w(px(133.))
            .flex()
            .flex_col()
            .when(!selected, |this| {
                this.opacity(0.5).hover(|style| style.opacity(1.))
            })
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .h(px(182.))
                    .mb(px(8.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--background-base-lowest"))
                    .when_some(cover, |this, cover| {
                        this.child(
                            img(cover)
                                .w(px(133.))
                                .h(px(177.))
                                .object_fit(gpui::ObjectFit::Contain),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .child(checkbox(selected)),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_game(&target, cx)))
            .into_any_element()
    }

    /// Tiles three to a row, 8 apart and 12 between rows (`selectedGames_OJrjgj`).
    fn game_grid(&self, ids: &[String], selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut grid = div().flex().flex_col().gap(px(12.));
        for row in ids.chunks(3) {
            grid = grid.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .children(row.iter().map(|id| self.game_tile(id, selected, cx))),
            );
        }
        grid.into_any_element()
    }

    /// The Games fieldset: legend, description, the select, picked games, then what members play.
    pub(super) fn server_games_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let field = open.games_query.clone();
        let focused = open.games_open;
        let full = open.games.len() >= MAX_GAMES;
        let top: Vec<String> = self
            .fetched
            .get(&format!("{TOP_PREFIX}{}", open.guild))
            .map(|body| {
                let mut entries: Vec<(String, f64)> = body["top_games"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| {
                        Some((
                            entry["game_application_id"].as_str()?.to_owned(),
                            entry["activity_score"].as_f64().unwrap_or(0.),
                        ))
                    })
                    .collect();
                entries.sort_by(|a, b| b.1.total_cmp(&a.1));
                entries
                    .into_iter()
                    .map(|(id, _)| id)
                    .filter(|id| !open.games.contains(id))
                    .collect()
            })
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Games"),
            )
            .child(
                div()
                    .mt(px(-8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("What games does your server play?"),
            )
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(
                        div()
                            .id("server-games-select")
                            .relative()
                            .child(self.anchor_probe(FIELD_PROBE))
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if focused {
                                color("--input-border-active")
                            } else {
                                color("--input-border-default")
                            })
                            .bg(color("--input-background-default"))
                            .when(full, |this| this.opacity(0.5))
                            .pl(px(12.))
                            .flex()
                            .items_center()
                            .cursor_text()
                            .child(div().flex_1().min_w(px(0.)).pr(px(8.)).child(field.clone()))
                            .child(
                                div()
                                    .w(px(36.))
                                    .h_full()
                                    .pr(px(4.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui::svg()
                                            .path("icons/select_chevron.svg")
                                            .size(px(20.))
                                            .text_color(color("--interactive-text-active"))
                                            .when(focused, |this| {
                                                this.with_transformation(
                                                    gpui::Transformation::rotate(gpui::radians(
                                                        std::f32::consts::PI,
                                                    )),
                                                )
                                            }),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this
                                    .server_settings
                                    .as_ref()
                                    .is_some_and(|open| open.games.len() >= MAX_GAMES)
                                {
                                    return;
                                }
                                if let Some(open) = this.server_settings.as_mut() {
                                    open.games_open = !open.games_open;
                                    let handle = gpui::Focusable::focus_handle(
                                        open.games_query.read(cx),
                                        cx,
                                    );
                                    window.focus(&handle, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .when(!open.games.is_empty(), |this| {
                        this.child(self.game_grid(&open.games, true, cx))
                    })
                    .when(!top.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .child(div().h(px(1.)).bg(color("--border-subtle")))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child("Your Members Play"),
                                )
                                .child(self.game_grid(&top, false, cx)),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The open select's list under the field (`selectDropdown_vWC4bk`, rows `listBoxItem_b5PEJH`).
    pub(super) fn server_games_dropdown(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !open.games_open {
            return None;
        }
        // The probe sits inside the field's 1px border.
        let field = self
            .tip_anchors
            .borrow()
            .get(FIELD_PROBE)
            .copied()?
            .dilate(px(1.));
        let query = open.games_query.read(cx).text().to_owned();
        let options = self.game_options(&query);
        let height = (options.len() as f32 * 40. + 8.).min(208.) + 2.;
        let rows = options.into_iter().map(|(id, name)| {
            let selected = open.games.contains(&id);
            div()
                .id(SharedString::from(format!("server-games-option-{id}")))
                .flex_none()
                .h(px(40.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-normal")))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pl(px(8.))
                        .pr(px(4.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-strong"))
                        .child(name),
                )
                .child(div().p(px(4.)).child(checkbox(selected)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_game(&id, cx);
                    // A pick clears the query, as the official combobox does.
                    if let Some(open) = this.server_settings.as_ref() {
                        open.games_query.update(cx, |editor, _| editor.set_text(""));
                    }
                }))
        });
        Some(
            deferred(
                div()
                    .parity_with_border("games-dropdown", 1.)
                    .id("server-games-dropdown")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(field.bottom() + px(4.))
                    .w(field.size.width)
                    .h(px(height))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.16),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(8.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        let on_field = this
                            .tip_anchors
                            .borrow()
                            .get(FIELD_PROBE)
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !on_field && let Some(open) = this.server_settings.as_mut() {
                            open.games_open = false;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("server-games-list")
                            .size_full()
                            .p(px(4.))
                            .pr(px(20.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The preview card's body (module 136696 `V`): the description, then up to six game icons by members'
    /// activity (one game shows its name; past six the sixth carries "+N"), then the traits as pills.
    pub(super) fn server_preview_body(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let description = open.description.read(cx).text().to_owned();
        let traits: Vec<(String, Option<super::server_settings::TraitEmoji>)> = open
            .traits
            .iter()
            .zip(open.trait_emoji.iter())
            .map(|(editor, emoji)| (editor.read(cx).text().to_owned(), emoji.clone()))
            .filter(|(label, _)| !label.is_empty())
            .collect();
        let activity: std::collections::HashMap<String, f64> = self
            .fetched
            .get(&format!("{TOP_PREFIX}{}", open.guild))
            .and_then(|body| body["top_games"].as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["game_application_id"].as_str()?.to_owned(),
                    entry["activity_score"].as_f64().unwrap_or(0.),
                ))
            })
            .collect();
        let mut games: Vec<&String> = open
            .games
            .iter()
            .filter(|id| self.games.contains_key(*id))
            .collect();
        games.sort_by(|a, b| {
            let score = |id: &String| activity.get(id).copied().unwrap_or(0.);
            score(b).total_cmp(&score(a))
        });
        if description.is_empty() && games.is_empty() && traits.is_empty() {
            return None;
        }
        let game_icon = |id: &String| {
            let picture = self
                .games
                .get(id)
                .and_then(|game| game.icon.as_ref())
                .and_then(|url| self.images.get(&format!("game-icon:{url}")).cloned());
            div()
                .size(px(26.))
                .flex_none()
                .rounded(px(4.))
                .border_1()
                .border_color(color("--border-muted"))
                .overflow_hidden()
                .when_some(picture, |this, picture| {
                    this.child(img(picture).size(px(24.)))
                })
        };
        let games_row = (!games.is_empty()).then(|| {
            if games.len() == 1 {
                let name = self
                    .games
                    .get(games[0])
                    .map(|game| game.name.clone())
                    .unwrap_or_default();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(game_icon(games[0]))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(name),
                    )
                    .into_any_element()
            } else {
                let remaining = games.len().saturating_sub(6);
                div()
                    .flex()
                    .gap(px(4.))
                    .children(games.iter().take(6).enumerate().map(|(index, id)| {
                        let tile = game_icon(id).relative();
                        if index == 5 && remaining > 0 {
                            tile.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(4.))
                                    .bg(color("--background-scrim"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-overlay-light"))
                                    .child(format!("+{remaining}")),
                            )
                            .into_any_element()
                        } else {
                            tile.into_any_element()
                        }
                    }))
                    .into_any_element()
            }
        });
        let pills = (!traits.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children(traits.iter().map(|(label, emoji)| {
                    let picture = emoji
                        .as_ref()
                        .and_then(|emoji| self.trait_emoji_image(emoji));
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .max_w(px(268.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(16.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .when_some(picture, |this, picture| {
                            this.child(img(picture).size(px(16.)))
                        })
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(label.clone()),
                        )
                }))
                .into_any_element()
        });
        Some(
            div()
                .mb(px(16.))
                .px(px(16.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .when(!description.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    )
                })
                .children(games_row)
                .children(pills)
                .into_any_element(),
        )
    }

    /// Covers and icons of the games the page shows, asked for once each.
    pub(super) fn request_game_images(&mut self) {
        let Some(open) = self.server_settings.as_ref() else {
            return;
        };
        let mut wanted: Vec<(String, String)> = Vec::new();
        let top: Vec<String> = self
            .fetched
            .get(&format!("{TOP_PREFIX}{}", open.guild))
            .and_then(|body| body["top_games"].as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["game_application_id"].as_str().map(str::to_owned))
            .collect();
        for id in open.games.iter().chain(top.iter()) {
            if let Some(game) = self.games.get(id) {
                if let Some(cover) = &game.cover {
                    wanted.push((format!("game-cover:{cover}"), cover.clone()));
                }
                if let Some(icon) = &game.icon {
                    wanted.push((format!("game-icon:{icon}"), icon.clone()));
                }
            }
        }
        for (key, url) in wanted {
            self.request_image(key, url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_marks_adult_as_module_932157_does() {
        assert!(!adult(
            &serde_json::json!({"agency_ratings": {"esrb": {"rating": 3}, "pegi": {"rating": 3}}})
        ));
        assert!(adult(
            &serde_json::json!({"agency_ratings": {"esrb": {"rating": 5}}})
        ));
        assert!(adult(
            &serde_json::json!({"agency_ratings": {"igdb": {"themes": [1, 21]}}})
        ));
        assert!(adult(&serde_json::json!({"discord_classifications": "8"})));
        assert!(!adult(&serde_json::json!({"discord_classifications": "2"})));
    }

    #[test]
    fn queries_normalize_like_c7() {
        assert_eq!(normalize("  Mine_Craft "), Some("mine craft".to_owned()));
        assert_eq!(normalize("   "), None);
    }

    #[test]
    fn assets_resolve_by_type() {
        let hash = serde_json::json!({"type": "hash", "value": "f2b6"});
        assert_eq!(
            asset_url("1", &hash, 64).as_deref(),
            Some("https://cdn.discordapp.com/app-icons/1/f2b6.png?size=64")
        );
        let url = serde_json::json!({"type": "url", "value": "https://x/y.jpg"});
        assert_eq!(asset_url("1", &url, 64).as_deref(), Some("https://x/y.jpg"));
        assert!(POPULAR.len() > 90 && !EXCLUDED.is_empty());
    }
}
