//! The game profile (module 581745, opened by `openGameProfileModal` of module 60465): a 1200 by 640 dialog over the
//! game's art. This first part draws what the captured first screen shows (`design/golden/windows/game-profile/
//! fortnite-*`): the top bar (the game's icon, the Overview and Communities tabs, Copy Link and Close), the banner art
//! under a gradient (`t8`), the hero (`heroContent`: the cover, the "#N Global Rank" pill `t6`, the name and the
//! genres `t4`), the trailer frame, the description, and in the sidebar the store button (`iV`, the first store
//! website of `iR`'s order) and Reviews (`ii`: the mobile stores, then OpenCritic). The record is
//! `GET /games?game_ids=` (recorded in `design/snapshot/TRAFFIC/game-fortnite.json`).
//!
//! Not yet: the trailer and screenshots playing, Community Announcements, the Community card, Add to Profile, the
//! More menu, and the Communities tab's list.

use super::game_genres::genre_name;
use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, deferred, div, img,
    prelude::*, px, rgb,
};
use serde_json::Value;

const KEY_PREFIX: &str = "game-profile:";
/// Website categories of module 505779 that are stores, in the order `iR` tries them.
const STORE_ORDER: [u64; 6] = [16, 13, 22, 20, 21, 23];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Overview,
    Communities,
}

pub(super) struct GameProfile {
    pub game: String,
    pub tab: Tab,
    scroll: ScrollHandle,
}

/// `iV`: the store button of a website category, its label and its icon.
/// Only Epic's glyph has been captured (`EpicGamesNeutralIcon`); the other stores show their name alone for now.
fn store_button(category: u64) -> Option<(&'static str, Option<&'static str>)> {
    match category {
        13 => Some(("Steam", None)),
        16 => Some(("Epic Games", Some("epic_games"))),
        22 => Some(("Roblox", None)),
        20 => Some(("Battle.net", None)),
        21 => Some(("Riot Games", None)),
        23 => Some(("Minecraft", None)),
        _ => None,
    }
}

/// The first store website in `iR`'s order.
fn store_website(game: &Value) -> Option<(u64, String)> {
    let websites = game["websites"].as_array()?;
    STORE_ORDER.iter().find_map(|category| {
        websites
            .iter()
            .find(|site| site["category"].as_u64() == Some(*category))
            .and_then(|site| site["url"].as_str())
            .map(|url| (*category, url.to_owned()))
    })
}

/// The mobile store reviews (`l1`): App Store (category 10 or 11) then Play Store (12), each with a store page.
fn mobile_reviews(game: &Value) -> Vec<(&'static str, String, f64, u64)> {
    let site = |categories: &[u64]| {
        game["websites"].as_array().and_then(|sites| {
            sites
                .iter()
                .find(|site| {
                    site["category"]
                        .as_u64()
                        .is_some_and(|category| categories.contains(&category))
                })
                .and_then(|site| site["url"].as_str().map(str::to_owned))
        })
    };
    let mut rows = Vec::new();
    for (key, title, categories) in [
        ("app_store", "App Store", &[10u64, 11][..]),
        ("google_play", "Play Store", &[12u64][..]),
    ] {
        let review = &game["reviews"][key];
        if let (Some(rating), Some(count), Some(url)) = (
            review["rating"].as_f64(),
            review["rating_count"].as_u64(),
            site(categories),
        ) {
            rows.push((title, url, rating, count));
        }
    }
    rows
}

/// `zdL4/T` and `u1m2hs`: "4.6 / 5" with one decimal, and the count with thousands separators in brackets.
fn rating_texts(rating: f64, count: u64) -> (String, String) {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    (format!("{rating:.1} / 5"), format!("({grouped})"))
}

/// The genres as `t4` joins them.
fn genres(game: &Value) -> String {
    game["genres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|genre| genre.as_u64().and_then(genre_name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn asset(game: &str, value: &Value, size: u32) -> Option<String> {
    let hash = value["value"].as_str().filter(|hash| !hash.is_empty())?;
    match value["type"].as_str()? {
        "hash" => Some(format!(
            "https://cdn.discordapp.com/app-icons/{game}/{hash}.png?size={size}&keep_aspect_ratio=true"
        )),
        "url" => Some(hash.to_owned()),
        _ => None,
    }
}

impl Root {
    /// Asks for a game's record ahead of time, so a menu or a card can tell whether its profile opens
    /// (`useShouldOpenGameProfile` reads the game store).
    pub(super) fn request_game_record(&mut self, game: &str) {
        if game.parse::<u64>().is_err() {
            return;
        }
        let key = format!("{KEY_PREFIX}{game}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/games?game_ids={game}"),
            });
        }
    }

    /// `openGameProfileModal`: asks for the game's record and opens the profile.
    pub(super) fn open_game_profile(&mut self, game: String, cx: &mut Context<Self>) {
        let key = format!("{KEY_PREFIX}{game}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/games?game_ids={game}"),
            });
        }
        self.profile = None;
        self.game_profile = Some(GameProfile {
            game,
            tab: Tab::Overview,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    /// The reply of `GET /games`: the pictures the profile draws.
    pub(super) fn game_profile_fetched(
        &mut self,
        key: &str,
        body: Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(id) = key.strip_prefix(KEY_PREFIX) else {
            return false;
        };
        if let Some(game) = body.as_array().and_then(|games| games.first()) {
            let media = &game["media"];
            let mut wanted = Vec::new();
            if let Some(url) = asset(id, &media["banner"], 1536) {
                wanted.push(url);
            }
            if let Some(url) = media["cover_url"].as_str() {
                wanted.push(url.to_owned());
            }
            if let Some(url) = asset(id, &media["icon"], 64) {
                wanted.push(url);
            }
            for url in wanted {
                self.request_image(url.clone(), url);
            }
        }
        self.fetched.insert(key.to_owned(), body);
        cx.notify();
        true
    }

    fn game_record(&self, id: &str) -> Option<Value> {
        self.fetched
            .get(&format!("{KEY_PREFIX}{id}"))?
            .as_array()?
            .first()
            .cloned()
    }

    /// `shouldOpenGameProfile` of module 611656: a record that is not disabled (`GAME_DISABLED`, bit 0 of
    /// `game_flags`) and not adult-rated.
    pub(super) fn game_profile_available(&self, id: &str) -> bool {
        self.game_record(id).is_some_and(|game| {
            game["game_flags"].as_u64().unwrap_or(0) & 1 == 0
                && game["content_classification"]["discord_classifications"]
                    .as_str()
                    .and_then(|bits| bits.parse::<u64>().ok())
                    .unwrap_or(0)
                    & 0b11001
                    == 0
        })
    }

    fn game_profile_bar_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `overlay-secondary`: black at 56% with white ink, 32px, 8px corners.
        div()
            .id(id)
            .size(px(32.))
            .rounded(px(8.))
            .bg(gpui::rgba(0x0000008f))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon(glyph, 16., "--white"))
            .on_click(cx.listener(move |this, _, _, cx| {
                on_click(this, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    pub(super) fn game_profile_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.game_profile.as_ref()?;
        let id = open.game.clone();
        let tab = open.tab;
        let game = self.game_record(&id);
        let name = game
            .as_ref()
            .and_then(|game| game["name"].as_str())
            .unwrap_or_default()
            .to_owned();
        let media = game
            .as_ref()
            .map(|game| game["media"].clone())
            .unwrap_or_default();
        let picture = |url: Option<String>| url.and_then(|url| self.images.get(&url).cloned());
        let banner = picture(asset(&id, &media["banner"], 1536));
        let cover = picture(media["cover_url"].as_str().map(str::to_owned));
        let dock = picture(asset(&id, &media["icon"], 64));
        let rank = game.as_ref().and_then(|game| game["l30_rank"].as_u64());
        let genre_text = game.as_ref().map(genres).unwrap_or_default();
        let description = game
            .as_ref()
            .and_then(|game| game["description"].as_str())
            .unwrap_or_default()
            .to_owned();
        let store = game.as_ref().and_then(store_website);
        let reviews = game.as_ref().map(mobile_reviews).unwrap_or_default();
        let tab_item = |label: &'static str, which: Tab| {
            let selected = tab == which;
            div()
                .id(SharedString::from(format!("game-profile-tab-{label}")))
                .h(px(56.))
                .flex()
                .items_center()
                .cursor_pointer()
                .border_b_2()
                .border_color(if selected {
                    color("--text-strong")
                } else {
                    gpui::rgba(0x00000000)
                })
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.game_profile.as_mut() {
                        open.tab = which;
                    }
                    cx.notify();
                }))
        };
        // Copy Link: the web app's `GAME_PROFILE` route (`/games/{id}`), then "Link Copied!" (`+5kSoW`).
        let link = format!("https://discord.com/games/{id}");
        let bar = div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(1190.))
            .h(px(56.))
            .bg(color("--background-base-lower").opacity(0.8))
            .px(px(24.))
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .size(px(24.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .children(dock.map(|dock| img(dock).size_full().rounded(px(4.)))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(32.))
                    .child(tab_item("Overview", Tab::Overview))
                    .child(tab_item("Communities", Tab::Communities)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.game_profile_bar_button(
                        "game-profile-link",
                        "share_link",
                        move |this, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                            this.show_success_toast("Link Copied!".to_owned(), cx);
                        },
                        cx,
                    ))
                    .child(self.game_profile_bar_button(
                        "game-profile-close",
                        "modal_close",
                        |this, _| this.game_profile = None,
                        cx,
                    )),
            );
        let hero = div()
            .px(px(24.))
            .pt(px(24.))
            .mb(px(12.))
            .flex()
            .items_end()
            .gap(px(12.))
            .child(
                div()
                    .w(px(85.5))
                    .h(px(114.))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .children(cover.map(|cover| {
                        img(cover)
                            .size_full()
                            .rounded(px(8.))
                            .object_fit(gpui::ObjectFit::Cover)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(rank.map(|rank| {
                        // `t6`: a white pill with a trophy, black 12px semibold text.
                        div()
                            .h(px(16.))
                            .px(px(8.))
                            .rounded_full()
                            .bg(rgb(0xffffff))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .self_start()
                            .child(icon("inventory_trophy", 12., "--black"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(rgb(0x000000))
                                    .child(format!("#{rank} Global Rank")),
                            )
                    }))
                    .child(
                        div()
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(name.clone()),
                    )
                    .when(!genre_text.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(genre_text.clone()),
                        )
                    }),
            );
        let main = div()
            .w(px(766.66))
            .flex()
            .flex_col()
            .gap(px(24.))
            // `trailerSection`: 16px above the black 16px-cornered video frame, 431px high.
            .child(
                div()
                    .pt(px(16.))
                    .child(div().h(px(431.23)).rounded(px(16.)).bg(rgb(0x000000))),
            )
            .when(!description.is_empty(), |this| {
                this.child(
                    div()
                        .line_clamp(3)
                        .text_ellipsis()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(description.clone()),
                )
            });
        let section_title = |text: &'static str| {
            div()
                .px(px(8.))
                .h(px(20.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-subtle"))
                .child(text)
        };
        let review_rows: Vec<AnyElement> = reviews
            .iter()
            .enumerate()
            .map(|(index, (title, url, rating, count))| {
                let (rating_text, count_text) = rating_texts(*rating, *count);
                let url = url.clone();
                div()
                    .id(("game-profile-review", index))
                    .h(px(58.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(color("--border-subtle"))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon(
                                if *title == "App Store" {
                                    "apple"
                                } else {
                                    "google_play"
                                },
                                18.,
                                "--text-strong",
                            ))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(*title),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .child(div().text_color(color("--text-strong")).child(rating_text))
                            .child(div().text_color(color("--text-subtle")).child(count_text)),
                    )
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element()
            })
            .collect();
        let sidebar = div()
            .w(px(359.34))
            .ml(px(24.))
            .pt(px(16.))
            .flex()
            .flex_col()
            .gap(px(32.))
            .children(store.and_then(|(category, url)| {
                let (label, glyph) = store_button(category)?;
                Some(
                    div()
                        .id("game-profile-store")
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .children(
                            glyph.map(|glyph| icon(glyph, 20., "--control-secondary-text-default")),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(label),
                        )
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            }))
            .when(!review_rows.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(section_title("Reviews"))
                        .child(
                            div()
                                .rounded(px(16.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .children(review_rows),
                        ),
                )
            });
        let content = div()
            .id("game-profile-scroll")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .track_scroll(&open.scroll)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().h(px(57.)).flex_none())
                    .child(hero)
                    .when(tab == Tab::Overview, |this| {
                        this.child(
                            div()
                                .pl(px(24.))
                                .pr(px(16.))
                                .pb(px(24.))
                                .flex()
                                .child(main)
                                .child(sidebar),
                        )
                    }),
            );
        let panel = div()
            .id("game-profile")
            .occlude()
            .relative()
            .w(px(1200.))
            .h(px(640.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .overflow_hidden()
            // `t8`: the banner art 500px high under a gradient to the dialog's colour 200px down.
            .children(banner.map(|banner| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(500.))
                    .child(img(banner).size_full().object_fit(gpui::ObjectFit::Cover))
            }))
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(gpui::hsla(0., 0., 0., 0.), 0.),
                gpui::linear_color_stop(color("--background-base-lower"), 200. / 638.),
            )))
            .child(content)
            .child(bar);
        Some(
            deferred(
                div()
                    .id("game-profile-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.game_profile = None;
                        cx.notify();
                    }))
                    .child(panel.on_click(|_, _, cx| cx.stop_propagation())),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{genres, mobile_reviews, rating_texts, store_website};
    use serde_json::Value;

    fn fortnite() -> Value {
        serde_json::json!({
            "genres": [28, 23],
            "websites": [
                {"url": "https://play.google.com/x", "category": 12},
                {"url": "https://www.epicgames.com/fortnite", "category": 16},
                {"url": "https://apps.apple.com/x", "category": 10}
            ],
            "reviews": {
                "app_store": {"rating": 4.599999904632568, "rating_count": 421571},
                "google_play": {"rating": 4.599999904632568, "rating_count": 552025}
            }
        })
    }

    #[test]
    fn the_hero_and_sidebar_read_the_record_as_the_capture_shows() {
        let game = fortnite();
        assert_eq!(genres(&game), "Simulation, Shooter");
        assert_eq!(store_website(&game).map(|(category, _)| category), Some(16));
        let reviews = mobile_reviews(&game);
        assert_eq!(
            reviews.iter().map(|row| row.0).collect::<Vec<_>>(),
            ["App Store", "Play Store"]
        );
        assert_eq!(
            rating_texts(reviews[0].2, reviews[0].3),
            ("4.6 / 5".to_owned(), "(421,571)".to_owned())
        );
    }
}
