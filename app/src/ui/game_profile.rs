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
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, deferred, div, img,
    prelude::*, px, rgb,
};
use serde_json::Value;

pub(super) const KEY_PREFIX: &str = "game-profile:";
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
    /// The media item shown in the trailer frame (`n` of `lJ`), an index into [`media_items`].
    media: usize,
    /// Module 679361: when the game has 3+ store platforms and the user clicks "Get Now" (`/hMurx`),
    /// opens the 440px "Choose your platform" (`/4gj6r`) modal.
    pub(super) platform_picker_open: bool,
    /// Capture/override platforms list for multi-platform modal preview.
    pub(super) platform_picker_override: Option<(String, Vec<(u64, String)>)>,
}

/// One item of the trailer frame (`f` of `lJ` in module 581745): the trailers first, then the screenshots.
#[derive(Clone, Debug, PartialEq)]
enum Media {
    Video {
        url: String,
        poster: String,
        width: u32,
        height: u32,
    },
    Image(String),
}

/// `2026-09-game-profiles-v3-communities-tab` (module 581745, `enabled` in variation 1).
const COMMUNITIES_TAB: &str = "2026-09-game-profiles-v3-communities-tab";

/// `eL`: fewer than three community servers, or an account outside the experiment, shows no Communities tab.
fn communities_tab_shown(variant: Option<i64>, communities: usize) -> bool {
    variant == Some(1) && communities >= 3
}

/// `kr` of module 776231: the CDN size ladder, the first step at least `size`.
pub(super) fn cdn_size(size: u32) -> u32 {
    const LADDER: [u32; 31] = [
        16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320,
        480, 512, 600, 640, 1024, 1280, 1536, 2048, 3072, 4096,
    ];
    LADDER
        .iter()
        .copied()
        .find(|step| size <= *step)
        .unwrap_or(4096)
}

/// `YE` of module 371794: `https://{CDN}/app-assets/{application}/store/{asset}.{ext}?size={kr(width)}`.
fn store_asset(application: &str, asset: &str, width: Option<u32>, ext: &str) -> String {
    let mut url =
        format!("https://cdn.discordapp.com/app-assets/{application}/store/{asset}.{ext}");
    if let Some(width) = width {
        url.push_str(&format!("?size={}", cdn_size(width)));
    }
    url
}

/// The trailer frame's items: each trailer as an MP4 with its WebP poster, then each screenshot.
fn media_items(game_id: &str, game: &Value) -> Vec<Media> {
    let trailers = game["trailers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|trailer| {
            let id = trailer["id"].as_str()?;
            let application = trailer["application_id"].as_str().unwrap_or(game_id);
            let width = trailer["width"].as_u64().map(|w| w as u32);
            Some(Media::Video {
                url: store_asset(application, id, width, "mp4"),
                poster: store_asset(application, id, width, "webp"),
                width: width.unwrap_or(1920),
                height: trailer["height"].as_u64().map_or(1080, |h| h as u32),
            })
        });
    let shots = game["screenshot_urls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|url| url.as_str().map(|url| Media::Image(url.to_owned())));
    trailers.chain(shots).collect()
}

/// `lz` at `size: 900`: a screenshot as the frame asks for it.
fn frame_picture(url: &str) -> String {
    format!("{url}?width=900&height=900&keep_aspect_ratio=true&format=webp")
}

/// Synthetic website category for Xbox Game Pass third-party SKU (module 679361).
const STORE_XBOX_GAME_PASS: u64 = 100;

/// `iV`: the store button of a website category, its label and its icon.
fn store_button(category: u64) -> Option<(&'static str, Option<&'static str>)> {
    match category {
        13 => Some(("Steam", Some("activity_steam"))),
        16 => Some(("Epic Games", Some("epic_games"))),
        22 => Some(("Roblox", None)),
        20 => Some(("Battle.net", None)),
        21 => Some(("Riot Games", None)),
        23 => Some(("Minecraft", None)),
        STORE_XBOX_GAME_PASS => Some(("Xbox Game Pass", Some("platform_xbox"))),
        _ => None,
    }
}

/// Module 679361: all store websites of a game in `STORE_ORDER` (plus Xbox Game Pass when present
/// in `third_party_skus`).
pub(crate) fn store_websites(game: &Value) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    if let Some(websites) = game["websites"].as_array() {
        for &category in &STORE_ORDER {
            if let Some(url) = websites
                .iter()
                .find(|site| site["category"].as_u64() == Some(category))
                .and_then(|site| site["url"].as_str())
            {
                out.push((category, url.to_owned()));
            }
        }
    }
    if let Some(skus) = game["third_party_skus"].as_array()
        && skus.iter().any(|s| {
            let dist = s["distributor"].as_str().unwrap_or_default();
            dist.eq_ignore_ascii_case("xbox") || dist.eq_ignore_ascii_case("game_pass")
        })
    {
        out.push((
            STORE_XBOX_GAME_PASS,
            "https://www.xbox.com/xbox-game-pass".to_owned(),
        ));
    }
    out
}

/// The first store website in `iR`'s order.
#[cfg(test)]
fn store_website(game: &Value) -> Option<(u64, String)> {
    store_websites(game).into_iter().next()
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

/// The thumbnails under the trailer: the trailers' posters (`app-assets/{game}/store/{id}`), then the screenshots
/// at 128 (`thumbnail_67wC2_`).
fn thumbnails(id: &str, game: &Value) -> Vec<String> {
    let trailers = game["trailers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|trailer| {
            trailer["id"].as_str().map(|trailer| {
                format!("https://cdn.discordapp.com/app-assets/{id}/store/{trailer}.png?size=128")
            })
        });
    let shots = game["screenshot_urls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|url| {
            url.as_str()
                .map(|url| format!("{url}?width=128&height=128&keep_aspect_ratio=true&format=webp"))
        });
    trailers.chain(shots).collect()
}

/// `lC`: the Details rows, each a label (singular or plural by the count) and its value, "-" when unknown.
fn detail_rows(game: &Value) -> Vec<(&'static str, String)> {
    let companies = |role: u64| -> Vec<String> {
        game["companies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|company| {
                company["roles"]
                    .as_array()
                    .is_some_and(|roles| roles.iter().any(|known| known.as_u64() == Some(role)))
            })
            .filter_map(|company| company["name"].as_str().map(str::to_owned))
            .collect()
    };
    let genre_count = game["genres"].as_array().map_or(0, Vec::len);
    let publishers = companies(1);
    let developers = companies(2);
    let or_dash = |text: String| {
        if text.is_empty() {
            "-".to_owned()
        } else {
            text
        }
    };
    let released = game["first_release_date"]
        .as_str()
        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
        // moment's `LL` in English: "June 29, 2020".
        .map(|date| date.format("%B %-d, %Y").to_string())
        .unwrap_or_else(|| "-".to_owned());
    vec![
        (
            if genre_count != 1 { "Genres" } else { "Genre" },
            or_dash(genres(game)),
        ),
        (
            if publishers.len() != 1 {
                "Publishers"
            } else {
                "Publisher"
            },
            or_dash(publishers.join(", ")),
        ),
        (
            if developers.len() != 1 {
                "Developers"
            } else {
                "Developer"
            },
            or_dash(developers.join(", ")),
        ),
        ("Release Date", released),
    ]
}

/// The platform glyphs of `lC`, in `n4`'s order: desktop (Windows, macOS or Linux), mobile (iOS or Android), Xbox,
/// PlayStation, Nintendo.
fn platform_glyphs(game: &Value) -> Vec<&'static str> {
    let platforms: Vec<u64> = game["platforms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect();
    let has = |platform: u64| platforms.contains(&platform);
    let mut glyphs = Vec::new();
    if has(0) || has(6) || has(7) {
        glyphs.push("platform_desktop");
    }
    if has(3) || has(4) {
        glyphs.push("platform_mobile");
    }
    if has(1) {
        glyphs.push("platform_xbox");
    }
    if has(2) {
        glyphs.push("platform_playstation");
    }
    if has(5) {
        glyphs.push("platform_nintendo");
    }
    glyphs
}

/// The link glyphs of `lx`, in the order of module 505779's `pF` categories; categories whose glyph has not been
/// captured yet are left out.
fn link_glyphs(game: &Value) -> Vec<(&'static str, String)> {
    const ORDER: [(u64, Option<&str>); 14] = [
        (1, Some("link_official")),
        (4, Some("link_facebook")),
        (5, Some("link_x")),
        (8, Some("link_instagram")),
        (9, Some("link_youtube")),
        (19, None),
        (14, Some("link_reddit")),
        (6, Some("link_twitch")),
        (22, None),
        (15, None),
        (2, Some("link_fandom")),
        (10, Some("link_apple")),
        (11, Some("link_apple")),
        (12, Some("link_google_play")),
    ];
    let websites: Vec<&Value> = game["websites"].as_array().into_iter().flatten().collect();
    ORDER
        .iter()
        .flat_map(|(category, glyph)| {
            websites
                .iter()
                .filter(move |site| site["category"].as_u64() == Some(*category))
                .filter_map(move |site| Some((*glyph.as_ref()?, site["url"].as_str()?.to_owned())))
        })
        .collect()
}

/// `background-size: cover; background-position: 50% 50%` of a `width` by `height` box: the picture at the scale that
/// covers it, centred, for the box to clip. Placed by hand because the image element here keeps the picture's own
/// height under `object_fit(Cover)`.
fn cover_fit(picture: std::sync::Arc<gpui::Image>, width: f32, height: f32) -> gpui::AnyElement {
    let natural = image::ImageReader::new(std::io::Cursor::new(&picture.bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    let Some((natural_width, natural_height)) = natural.filter(|(w, h)| *w > 0 && *h > 0) else {
        return img(picture).w(px(width)).h(px(height)).into_any_element();
    };
    let scale = (width / natural_width as f32).max(height / natural_height as f32);
    let (shown_width, shown_height) = (natural_width as f32 * scale, natural_height as f32 * scale);
    img(picture)
        .absolute()
        .left(px((width - shown_width) / 2.))
        .top(px((height - shown_height) / 2.))
        .w(px(shown_width))
        .h(px(shown_height))
        .into_any_element()
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

pub(super) fn asset(game: &str, value: &Value, size: u32) -> Option<String> {
    let hash = value["value"].as_str().filter(|hash| !hash.is_empty())?;
    match value["type"].as_str()? {
        "hash" => Some(format!(
            "https://cdn.discordapp.com/app-icons/{game}/{hash}.png?size={size}&keep_aspect_ratio=true"
        )),
        "url" => Some(hash.to_owned()),
        _ => None,
    }
}

/// The OpenCritic tier's badge (`it`) and the colour of its rating ring (`il`), by `l8` (MIGHTY 1 to WEAK 4).
fn open_critic_tier(tier: u64) -> Option<(&'static str, u32)> {
    match tier {
        1 => Some((
            "https://cdn.discordapp.com/assets/content/35c42952234dc88292af091e1f0a5eb2189dbe0e40253245f51637c4ff587173.png",
            0xfc430a,
        )),
        2 => Some((
            "https://cdn.discordapp.com/assets/content/8d450a540daa7b1a93e760d85891273058b41ed329141c86dae484c23817e0bb.png",
            0x9e00b4,
        )),
        3 => Some((
            "https://cdn.discordapp.com/assets/content/9008eb4e7484a2c84cc11e8dff3831398fedd2de795ebf7c70436fd747bab475.png",
            0x4aa1ce,
        )),
        4 => Some((
            "https://cdn.discordapp.com/assets/content/1538fd1d5a67d65aebc33a9b47ab87cafbd83433f31f064f8deba3f89104ac8f.png",
            0x80b06a,
        )),
        _ => None,
    }
}

/// `l6`: a 30px ring (r 16 of a 36 viewBox, so 13.3px, 2px wide) whose arc is the rating out of 100, turned by
/// `(1 - rating) * 180deg` so it stays centred at the bottom; SVG circles start at three o'clock and run clockwise.
fn paint_open_critic_arc(
    bounds: gpui::Bounds<gpui::Pixels>,
    rating: f32,
    stroke: u32,
    window: &mut gpui::Window,
) {
    let share = (rating / 100.).clamp(0., 1.);
    if share <= 0. {
        return;
    }
    let radius = 16. * 30. / 36.;
    let cx = bounds.origin.x + bounds.size.width * 0.5;
    let cy = bounds.origin.y + bounds.size.height * 0.5;
    let start = (1. - share) * std::f32::consts::PI;
    let sweep = share * std::f32::consts::TAU;
    let segments = ((share * 96.).ceil() as usize).max(2);
    let mut path = gpui::PathBuilder::stroke(px(2.));
    for step in 0..=segments {
        let angle = start + sweep * (step as f32 / segments as f32);
        let (sin, cos) = angle.sin_cos();
        let at = gpui::point(cx + px(radius * cos), cy + px(radius * sin));
        if step == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    if let Ok(built) = path.build() {
        window.paint_path(built, rgb(stroke));
    }
}

impl Root {
    /// `ie`: OpenCritic with the tier's badge and the top critics' rating ring, or "Not Enough Top Critic Reviews"
    /// without either; shown when the record has an OpenCritic review and page, beside the store reviews (`h`).
    fn open_critic_row(&self, game: &Value, divider: bool) -> Option<AnyElement> {
        let review = &game["reviews"]["opencritic"];
        let url = game["opencritic_url"].as_str()?.to_owned();
        if review.is_null() {
            return None;
        }
        let rating = review["top_critic_rating"].as_f64().unwrap_or(-1.);
        let count = review["top_critic_rating_count"].as_i64().unwrap_or(-1);
        let tier = review["tier"].as_u64().and_then(open_critic_tier);
        let badge = tier.and_then(|(src, _)| self.images.get(src).cloned());
        let ring = tier
            .filter(|_| rating > 0. && count > 0)
            .map(|(_, stroke)| {
                let shown = rating as f32;
                div()
                    .relative()
                    .size(px(32.))
                    .rounded_full()
                    .bg(rgb(0x2e2e2e))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                paint_open_critic_arc(bounds, shown, stroke, window)
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-overlay-light"))
                            .child(format!("{}", rating.floor() as i64)),
                    )
            });
        let none = tier.is_none() && (rating <= 0. || count <= 0);
        Some(
            div()
                .id("game-profile-opencritic")
                .h(px(56.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .when(divider, |this| {
                    this.border_t_1().border_color(color("--border-subtle"))
                })
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child("OpenCritic"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .children(badge.map(|badge| img(badge).size(px(32.)).rounded_full()))
                        .children(ring)
                        .when(none, |this| {
                            this.child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-subtle"))
                                    .child("Not Enough Top Critic Reviews"),
                            )
                        }),
                )
                .on_click(move |_, _, cx| cx.open_url(&url))
                .into_any_element(),
        )
    }

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
    pub(crate) fn open_game_profile(&mut self, game: String, cx: &mut Context<Self>) {
        let key = format!("{KEY_PREFIX}{game}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/games?game_ids={game}"),
            });
        }
        self.profile = None;
        self.game_announcements_fetch(&game);
        self.game_add_to_profile_wanted();
        self.game_announcements_scroll = 0.;
        self.game_similar_fetch(&game);
        self.game_similar_scroll = 0.;
        self.game_profile = Some(GameProfile {
            game,
            tab: Tab::Overview,
            scroll: ScrollHandle::new(),
            media: 0,
            platform_picker_open: false,
            platform_picker_override: None,
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
            wanted.extend(thumbnails(id, game));
            // The first item's picture: a trailer's poster or the first screenshot at the frame's size.
            match media_items(id, game).first() {
                Some(Media::Video { poster, .. }) => wanted.push(poster.clone()),
                Some(Media::Image(url)) => wanted.push(frame_picture(url)),
                None => {}
            }
            if let Some((badge, _)) = game["reviews"]["opencritic"]["tier"]
                .as_u64()
                .and_then(open_critic_tier)
            {
                wanted.push(badge.to_owned());
            }
            // A similar game's cover (`ik`) at 256.
            if let Some(url) = asset(id, &media["cover"], 256) {
                wanted.push(url);
            }
            // ... and its icon at 256, the cover's stand-in (459746 `x`).
            if let Some(url) = asset(id, &media["icon"], 256) {
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

    /// The game's name, for the Report Issue dialog its More menu opens.
    pub(super) fn game_profile_name(&self, id: &str) -> String {
        self.game_record(id)
            .and_then(|game| game["name"].as_str().map(str::to_owned))
            .unwrap_or_default()
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
        let media_list = self
            .game_record(&id)
            .map(|game| media_items(&id, &game))
            .unwrap_or_default();
        // `eL` / `eS`: the Communities tab is for accounts in its experiment and games with three community servers.
        let communities_tab = communities_tab_shown(
            self.stores.experiments.variant(COMMUNITIES_TAB),
            self.game_record(&id)
                .and_then(|game| game["community_guild_ids"].as_array().map(Vec::len))
                .unwrap_or(0),
        );
        // `j = Math.min(n, f.length - 1)`.
        let shown = open.media.min(media_list.len().saturating_sub(1));
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
        let stores = game.as_ref().map(store_websites).unwrap_or_default();
        let platform_picker_open = open.platform_picker_open;
        let platform_picker_override = open.platform_picker_override.clone();
        let thumbs: Vec<Option<std::sync::Arc<gpui::Image>>> = game
            .as_ref()
            .map(|game| thumbnails(&id, game))
            .unwrap_or_default()
            .into_iter()
            .map(|url| self.images.get(&url).cloned())
            .collect();
        let details = game.as_ref().map(detail_rows).unwrap_or_default();
        let platforms = game.as_ref().map(platform_glyphs).unwrap_or_default();
        let links = game.as_ref().map(link_glyphs).unwrap_or_default();
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
                    let mut game = None;
                    if let Some(open) = this.game_profile.as_mut() {
                        open.tab = which;
                        game = Some(open.game.clone());
                    }
                    if which == Tab::Communities
                        && let Some(game) = game
                    {
                        this.game_communities_fetch(&game);
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
                    .when(communities_tab, |this| {
                        this.child(tab_item("Communities", Tab::Communities))
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    // `K.$` `overlay-secondary` `sm` with text: 12px in, the large plus 16px, 4px, then the label.
                    .when(self.game_add_to_profile_shown(&id), |this| {
                        let menu_game = id.clone();
                        this.child(
                            div()
                                .id("game-profile-add-to-profile")
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(gpui::rgba(0x0000008f))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .child(icon("plus_large", 16., "--white"))
                                .child(dc_intl::tr("Add to Profile"))
                                .on_click(cx.listener(
                                    move |this, event: &gpui::ClickEvent, _, cx| {
                                        if let Ok(game) = menu_game.parse::<u64>() {
                                            this.open_context_menu(
                                                super::context_menu::Target::GameAddToProfile(game),
                                                event.position(),
                                                cx,
                                            );
                                        }
                                    },
                                )),
                        )
                    })
                    .child(self.game_profile_bar_button(
                        "game-profile-link",
                        "share_link",
                        move |this, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                            this.show_success_toast("Link Copied!".to_owned(), cx);
                        },
                        cx,
                    ))
                    .child({
                        let menu_game = id.clone();
                        div()
                            .id("game-profile-more")
                            .size(px(32.))
                            .rounded(px(8.))
                            .bg(gpui::rgba(0x0000008f))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(icon("more_horizontal", 16., "--white"))
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                if let Ok(game) = menu_game.parse::<u64>() {
                                    this.open_context_menu(
                                        super::context_menu::Target::GameProfile(game),
                                        event.position(),
                                        cx,
                                    );
                                }
                            }))
                    })
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
                                    // `gameRankPill`: upper case, as the capture shows ("#2 GLOBAL RANK").
                                    .child(format!("#{rank} Global Rank").to_uppercase()),
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
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div().pt(px(16.)).child(
                            div()
                                .h(px(431.23))
                                .rounded(px(16.))
                                .overflow_hidden()
                                .bg(rgb(0x000000))
                                .children(self.game_media_frame(&id, media_list.get(shown), cx)),
                        ),
                    )
                    // `children_s64Ydp`: 106 by 60 thumbnails 8px apart; the shown one is ringed 2px in
                    // `--border-strong` with its picture 4px in.
                    .child(
                        div()
                            .id("game-profile-thumbnails")
                            .h(px(60.))
                            .flex()
                            .gap(px(8.))
                            .overflow_x_scroll()
                            .children(thumbs.into_iter().enumerate().map(|(index, picture)| {
                                let selected = index == shown;
                                div()
                                    .id(("game-profile-thumb", index))
                                    .cursor_pointer()
                                    // `I`: choosing a thumbnail shows its item.
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(open) = this.game_profile.as_mut() {
                                            open.media = index;
                                        }
                                        this.game_media_wanted();
                                        cx.notify();
                                    }))
                                    .flex_none()
                                    .w(px(106.))
                                    .h(px(60.))
                                    .rounded(px(12.))
                                    .when(selected, |this| {
                                        this.border_2()
                                            .border_color(color("--border-strong"))
                                            .p(px(2.))
                                    })
                                    .child(
                                        div()
                                            .size_full()
                                            .rounded(px(if selected { 8. } else { 12. }))
                                            .overflow_hidden()
                                            .bg(color("--background-mod-normal"))
                                            .children(picture.map(|picture| {
                                                img(picture)
                                                    .size_full()
                                                    .object_fit(gpui::ObjectFit::Cover)
                                            })),
                                    )
                            })),
                    ),
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
            })
            .children(self.game_announcements_section(&id, cx))
            .children(self.game_similar_section(&id, cx));
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
        let mut review_rows = review_rows;
        if let Some(row) = game
            .as_ref()
            .and_then(|game| self.open_critic_row(game, !review_rows.is_empty()))
        {
            review_rows.push(row);
        }
        let render_store_btn = |id: SharedString, category: u64, url: String, flex_fill: bool| {
            let (label, glyph) = store_button(category)?;
            Some(
                div()
                    .id(id)
                    .when(flex_fill, |d| d.flex_1().min_w(px(0.)))
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .children(
                        glyph.map(|glyph| icon(glyph, 20., "--control-secondary-text-default")),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(label),
                    )
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element(),
            )
        };
        let store_section: Option<AnyElement> = match stores.len() {
            0 => None,
            1 => {
                let (category, url) = stores[0].clone();
                render_store_btn("game-profile-store".into(), category, url, false)
            }
            2 => {
                let btns: Vec<AnyElement> = stores
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (cat, url))| {
                        render_store_btn(
                            SharedString::from(format!("game-profile-store-{i}")),
                            *cat,
                            url.clone(),
                            true,
                        )
                    })
                    .collect();
                Some(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(btns)
                        .into_any_element(),
                )
            }
            _ => Some(
                div()
                    .id("game-profile-get-now")
                    .h(px(40.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|style| style.bg(color("--control-primary-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("/hMurx"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(gp) = this.game_profile.as_mut() {
                            gp.platform_picker_open = true;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            ),
        };
        let sidebar = div()
            .w(px(359.34))
            .ml(px(24.))
            .pt(px(16.))
            .flex()
            .flex_col()
            .gap(px(32.))
            .children(store_section)
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
            })
            .when(game.is_some(), |this| {
                let row = |label: &'static str, value: AnyElement, first: bool| {
                    div()
                        .p(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(!first, |this| {
                            this.border_t_1().border_color(color("--border-subtle"))
                        })
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(label),
                        )
                        .child(value)
                        .into_any_element()
                };
                let text = |value: String| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(value)
                        .into_any_element()
                };
                let glyph_row = |glyphs: Vec<AnyElement>| {
                    if glyphs.is_empty() {
                        text("-".to_owned())
                    } else {
                        div()
                            .flex()
                            .gap(px(12.))
                            .children(glyphs)
                            .into_any_element()
                    }
                };
                let mut rows: Vec<AnyElement> = details
                    .iter()
                    .enumerate()
                    .map(|(index, (label, value))| row(label, text(value.clone()), index == 0))
                    .collect();
                rows.push(row(
                    if platforms.len() > 1 {
                        "Platforms"
                    } else {
                        "Platform"
                    },
                    glyph_row(
                        platforms
                            .iter()
                            .map(|glyph| icon(glyph, 16., "--text-subtle").into_any_element())
                            .collect(),
                    ),
                    false,
                ));
                rows.push(row(
                    "Links",
                    glyph_row(
                        links
                            .iter()
                            .enumerate()
                            .map(|(index, (glyph, url))| {
                                let url = url.clone();
                                div()
                                    .id(("game-profile-link-site", index))
                                    .cursor_pointer()
                                    .child(icon(glyph, 16., "--text-subtle"))
                                    .on_click(move |_, _, cx| cx.open_url(&url))
                                    .into_any_element()
                            })
                            .collect(),
                    ),
                    false,
                ));
                rows.push(row(
                    "Game metadata by",
                    div()
                        .id("game-profile-igdb")
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-link"))
                        .child("IGDB")
                        .on_click(|_, _, cx| cx.open_url("https://www.igdb.com/about"))
                        .into_any_element(),
                    false,
                ));
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(section_title("Details"))
                        .child(
                            div()
                                .rounded(px(16.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .children(rows),
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
                    .relative()
                    .flex()
                    .flex_col()
                    // `t8`: the banner art 500px high under a gradient to the dialog's colour 200px down; it sits
                    // at the top of the page and scrolls away with it (the scrolled capture shows none of it).
                    .children(banner.filter(|_| tab == Tab::Overview).map(|banner| {
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(500.))
                            // gpui draws a covered picture past its box: the box clips it, as CSS does.
                            .overflow_hidden()
                            .child(cover_fit(banner, 1198., 500.))
                    }))
                    .child(div().absolute().top_0().left_0().right_0().h(px(638.)).bg(
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(gpui::hsla(0., 0., 0., 0.), 0.),
                            gpui::linear_color_stop(color("--background-base-lower"), 200. / 638.),
                        ),
                    ))
                    .child(div().h(px(57.)).flex_none())
                    // Module 647474: sticky promotion notice banner & live countdown when game carries active promo.
                    .children(
                        game.as_ref()
                            .and_then(|g| self.game_profile_promo_banner(g)),
                    )
                    // `tH` is its own scroller under the top bar: the hero belongs to the Overview.
                    .when(tab == Tab::Overview, |this| this.child(hero))
                    .when(tab == Tab::Communities, |this| {
                        this.child(self.game_communities_section(&id, 1198., cx))
                    })
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
            .child(content)
            .child(bar);
        let platform_picker_overlay = platform_picker_open.then(|| {
            let (picker_game_name, picker_stores) =
                platform_picker_override.unwrap_or_else(|| (name.clone(), stores.clone()));
            let subtitle = dc_intl::format(
                "0acM2Y",
                &[(
                    "gameName",
                    dc_intl::Arg::Str(std::borrow::Cow::Owned(picker_game_name)),
                )],
            );
            let rows: Vec<AnyElement> = picker_stores
                .into_iter()
                .enumerate()
                .filter_map(|(i, (cat, url))| {
                    let (label, glyph) = store_button(cat)?;
                    Some(
                        div()
                            .id(("game-platform-option", i))
                            .w_full()
                            .h(px(44.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .children(
                                glyph.map(|g| icon(g, 20., "--control-secondary-text-default")),
                            )
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(label),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.open_url(&url);
                                if let Some(gp) = this.game_profile.as_mut() {
                                    gp.platform_picker_open = false;
                                }
                                cx.notify();
                            }))
                            .into_any_element(),
                    )
                })
                .collect();
            div()
                .id("game-platform-picker-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x000000aa))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(gp) = this.game_profile.as_mut() {
                        gp.platform_picker_open = false;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .parity("game-platform-picker-modal")
                        .id("game-platform-picker-modal")
                        .occlude()
                        .w(px(440.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .shadow_lg()
                        .overflow_hidden()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(24.))
                                .pb(px(12.))
                                .flex()
                                .items_start()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(20.))
                                                .line_height(px(24.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("/4gj6r")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-default"))
                                                .child(subtitle),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("game-platform-picker-close")
                                        .size(px(24.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .child(icon("lb_close", 20., "--text-muted"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(gp) = this.game_profile.as_mut() {
                                                gp.platform_picker_open = false;
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(8.))
                                .pb(px(24.))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .children(rows),
                        ),
                )
        });
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
                    .child(panel.on_click(|_, _, cx| cx.stop_propagation()))
                    .children(platform_picker_overlay),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

impl Root {
    /// `lJ`'s frame: a trailer plays in the media player (`l$`, `autoPlay: !reducedMotion && !obscured`, muted at
    /// first), a screenshot is drawn to cover the frame.
    fn game_media_frame(
        &self,
        game: &str,
        item: Option<&Media>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match item? {
            Media::Video {
                url,
                poster,
                width,
                height,
            } => Some(self.render_media_player(
                super::video_player::PlayerSpec {
                    key: format!("game-trailer:{game}:{url}"),
                    src: url.clone(),
                    width: 766.66,
                    height: 431.23,
                    aspect_size: ((*width).max(1), (*height).max(1)),
                    radius: 16.0,
                    poster_key: Some(poster.clone()),
                    auto_play: true,
                    allow_fullscreen: true,
                    is_fullscreen_view: false,
                    actions: None,
                },
                cx,
            )),
            Media::Image(url) => {
                let picture = self.images.get(&frame_picture(url)).cloned()?;
                Some(cover_fit(picture, 766.66, 431.23))
            }
        }
    }

    /// The shown item's picture, asked for once it is chosen.
    fn game_media_wanted(&mut self) {
        let Some(open) = self.game_profile.as_ref() else {
            return;
        };
        let id = open.game.clone();
        let index = open.media;
        let Some(item) = self
            .game_record(&id)
            .and_then(|game| media_items(&id, &game).into_iter().nth(index))
        else {
            return;
        };
        let url = match item {
            Media::Video { poster, .. } => poster,
            Media::Image(url) => frame_picture(&url),
        };
        self.request_image(url.clone(), url);
    }

    /// Module 647474 — Game Profile / Game Shop Sticky Promotion Notice Banner (`A`) & Countdown (`S`).
    /// Extracts promotion metadata (`promotion.text` / `promo_text`, `promotion.ends_at`, `promotion.flavor`)
    /// from the game record and renders the sticky notice strip when active.
    fn game_profile_promo_banner(&self, game: &Value) -> Option<AnyElement> {
        let promo = &game["promotion"];
        let text = promo["text"]
            .as_str()
            .or_else(|| game["promo_text"].as_str())
            .filter(|s| !s.is_empty())?;
        let flavor = promo["flavor"]
            .as_str()
            .or_else(|| promo["color"].as_str())
            .unwrap_or("brand");
        let ends_at_ms = promo["ends_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp_millis())
            .or_else(|| promo["ends_at_ms"].as_i64());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Some(render_promo_notice_banner(
            text,
            PromoBannerTheme::parse(flavor),
            ends_at_ms.and_then(|end| compute_promo_countdown(end, now_ms)),
        ))
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_game_platform_picker(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.eq_ignore_ascii_case("close") {
            self.game_profile = None;
            cx.notify();
            return "game-platform-picker: closed".to_owned();
        }
        self.game_profile = Some(GameProfile {
            game: "capture-multi-platform".to_owned(),
            tab: Tab::Overview,
            scroll: ScrollHandle::new(),
            media: 0,
            platform_picker_open: true,
            platform_picker_override: Some((
                "Apex Legends".to_owned(),
                vec![
                    (13, "https://store.steampowered.com/app/1172470".to_owned()),
                    (16, "https://store.epicgames.com/p/apex-legends".to_owned()),
                    (
                        STORE_XBOX_GAME_PASS,
                        "https://www.xbox.com/xbox-game-pass".to_owned(),
                    ),
                ],
            )),
        });
        cx.notify();
        "game-platform-picker: open".to_owned()
    }
}

/// Module 647474 banner theme (`"nitro-pink"`, custom `#RRGGBB` hex, or default Brand).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromoBannerTheme {
    Brand,
    NitroPink,
    Hex { rgb: u32, light_text: bool },
}

impl PromoBannerTheme {
    pub(super) fn parse(spec: &str) -> Self {
        let s = spec.trim();
        if s.eq_ignore_ascii_case("nitro-pink") {
            return Self::NitroPink;
        }
        if let Some(hex) = s.strip_prefix('#')
            && hex.len() == 6
            && let Ok(rgb) = u32::from_str_radix(hex, 16)
        {
            let r = ((rgb >> 16) & 0xff) as f32 / 255.;
            let g = ((rgb >> 8) & 0xff) as f32 / 255.;
            let b = (rgb & 0xff) as f32 / 255.;
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            return Self::Hex {
                rgb,
                light_text: luma <= 0.5,
            };
        }
        Self::Brand
    }
}

/// Module 647474 live countdown (`S`): `Days(d)` when `days > 0`, else boxed `HH:MM:SS` digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromoCountdown {
    Days(u64),
    Hms { hours: u64, mins: u64, secs: u64 },
}

pub(super) fn compute_promo_countdown(ends_at_ms: i64, now_ms: i64) -> Option<PromoCountdown> {
    let rem_ms = ends_at_ms - now_ms;
    if rem_ms <= 0 {
        return None;
    }
    let total_secs = (rem_ms / 1000) as u64;
    let days = total_secs / 86_400;
    if days > 0 {
        Some(PromoCountdown::Days(days))
    } else {
        Some(PromoCountdown::Hms {
            hours: (total_secs % 86_400) / 3600,
            mins: (total_secs % 3600) / 60,
            secs: total_secs % 60,
        })
    }
}

/// Module 647474 sticky promotion notice banner (`bannerContainer_ikb72w`, `10px 24px` padding)
/// with live countdown (`N` clock + `BXpdIg` `"{days}d left"` or `k` boxed `HH:MM:SS` digits).
pub(super) fn render_promo_notice_banner(
    text: &str,
    theme: PromoBannerTheme,
    countdown: Option<PromoCountdown>,
) -> AnyElement {
    let text_color = match theme {
        PromoBannerTheme::Hex {
            light_text: false, ..
        } => color("--black"),
        _ => color("--white"),
    };
    let mut bar = div()
        .id("promo-notice-banner")
        .w_full()
        .px(px(24.))
        .py(px(10.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.));
    bar = match theme {
        PromoBannerTheme::NitroPink => bar.bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(gpui::rgba(0xeb459eff), 0.),
            gpui::linear_color_stop(gpui::rgba(0x5865f2ff), 1.),
        )),
        PromoBannerTheme::Hex { rgb: hex, .. } => bar.bg(rgb(hex)),
        PromoBannerTheme::Brand => bar.bg(color("--brand-500")),
    };
    bar = bar.child(
        div()
            .flex_1()
            .min_w(px(0.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .text_color(text_color)
            .child(text.to_owned()),
    );
    if let Some(cd) = countdown {
        let right = match cd {
            PromoCountdown::Days(days) => {
                let copy = dc_intl::format("BXpdIg", &[("days", (days as f64).into())]);
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon(
                        "user_clock",
                        16.,
                        match theme {
                            PromoBannerTheme::Hex {
                                light_text: false, ..
                            } => "--black",
                            _ => "--white",
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(text_color)
                            .child(copy),
                    )
                    .into_any_element()
            }
            PromoCountdown::Hms { hours, mins, secs } => {
                let digit_box = |d: char, fixed: bool| {
                    div()
                        .when(fixed, |el| el.w(px(16.)).py(px(4.)))
                        .when(!fixed, |el| el.p(px(4.)))
                        .rounded(px(4.))
                        .bg(color("--background-base-lowest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .line_height(px(14.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child(d.to_string())
                };
                let colon = || {
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--background-base-lowest"))
                        .child(":")
                };
                let h = format!("{hours:02}");
                let m = format!("{mins:02}");
                let s = format!("{secs:02}");
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .children(h.chars().map(|c| digit_box(c, false)))
                    .child(colon())
                    .children(m.chars().map(|c| digit_box(c, false)))
                    .child(colon())
                    .children(s.chars().map(|c| digit_box(c, true)))
                    .into_any_element()
            }
        };
        bar = bar.child(right);
    }
    bar.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        Media, cdn_size, communities_tab_shown, detail_rows, genres, link_glyphs, media_items,
        mobile_reviews, platform_glyphs, rating_texts, store_website,
    };
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
    fn the_communities_tab_needs_the_experiment_and_three_servers() {
        assert!(communities_tab_shown(Some(1), 3));
        assert!(!communities_tab_shown(Some(1), 2));
        assert!(!communities_tab_shown(Some(0), 9));
        assert!(!communities_tab_shown(None, 9));
    }

    #[test]
    fn trailers_come_first_as_mp4_with_a_webp_poster_then_the_screenshots() {
        let game = serde_json::json!({
            "trailers": [{"id": "5", "application_id": "9", "width": 1280, "height": 720}],
            "screenshot_urls": ["https://cdn.example/shot.png"],
        });
        let items = media_items("1", &game);
        assert_eq!(
            items[0],
            Media::Video {
                url: "https://cdn.discordapp.com/app-assets/9/store/5.mp4?size=1280".to_owned(),
                poster: "https://cdn.discordapp.com/app-assets/9/store/5.webp?size=1280".to_owned(),
                width: 1280,
                height: 720,
            }
        );
        assert_eq!(
            items[1],
            Media::Image("https://cdn.example/shot.png".to_owned())
        );
        assert_eq!(cdn_size(1000), 1024);
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
        let mut full = game.clone();
        full["companies"] = serde_json::json!([{"name": "Epic Games", "roles": [1, 2]}]);
        full["first_release_date"] = serde_json::json!("2020-06-29T00:00:00+00:00");
        full["platforms"] = serde_json::json!([0, 1, 2, 3, 4, 5, 7]);
        assert_eq!(
            detail_rows(&full),
            [
                ("Genres", "Simulation, Shooter".to_owned()),
                ("Publisher", "Epic Games".to_owned()),
                ("Developer", "Epic Games".to_owned()),
                ("Release Date", "June 29, 2020".to_owned()),
            ]
        );
        assert_eq!(
            platform_glyphs(&full),
            [
                "platform_desktop",
                "platform_mobile",
                "platform_xbox",
                "platform_playstation",
                "platform_nintendo"
            ]
        );
        assert_eq!(
            link_glyphs(&full)
                .iter()
                .map(|(glyph, _)| *glyph)
                .collect::<Vec<_>>(),
            ["link_apple", "link_google_play"]
        );
        assert_eq!(
            rating_texts(reviews[0].2, reviews[0].3),
            ("4.6 / 5".to_owned(), "(421,571)".to_owned())
        );
    }

    #[test]
    fn promo_banner_647474_theme_and_countdown() {
        assert!(dc_intl::has_key("BXpdIg"));
        assert_eq!(
            super::PromoBannerTheme::parse("nitro-pink"),
            super::PromoBannerTheme::NitroPink
        );
        assert_eq!(
            super::PromoBannerTheme::parse("#FFFFFF"),
            super::PromoBannerTheme::Hex {
                rgb: 0xffffff,
                light_text: false
            }
        );
        assert_eq!(
            super::PromoBannerTheme::parse("#101020"),
            super::PromoBannerTheme::Hex {
                rgb: 0x101020,
                light_text: true
            }
        );
        let now = 1_700_000_000_000i64;
        assert_eq!(
            super::compute_promo_countdown(now + 3 * 86_400_000 + 10_000, now),
            Some(super::PromoCountdown::Days(3))
        );
        assert_eq!(
            super::compute_promo_countdown(now + (2 * 3600 + 15 * 60 + 9) * 1000, now),
            Some(super::PromoCountdown::Hms {
                hours: 2,
                mins: 15,
                secs: 9
            })
        );
        assert_eq!(super::compute_promo_countdown(now - 1000, now), None);
    }

    #[test]
    fn multi_platform_store_picker_matches_module_679361() {
        let game = serde_json::json!({
            "websites": [
                {"category": 13, "url": "https://store.steampowered.com/app/1"},
                {"category": 16, "url": "https://store.epicgames.com/p/1"},
                {"category": 20, "url": "https://shop.battle.net/product/1"}
            ],
            "third_party_skus": [
                {"distributor": "xbox", "sku": "123"}
            ]
        });
        let stores = super::store_websites(&game);
        assert_eq!(stores.len(), 4);
        assert_eq!(stores[0].0, 16);
        assert_eq!(stores[1].0, 13);
        assert_eq!(stores[2].0, 20);
        assert_eq!(stores[3].0, super::STORE_XBOX_GAME_PASS);
        assert_eq!(dc_intl::t("/4gj6r"), "Choose your platform");
        assert_eq!(dc_intl::t("/hMurx"), "Get Now");
    }
}
