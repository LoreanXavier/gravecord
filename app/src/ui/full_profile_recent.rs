//! The full profile's Recent activity cards (`$` of module 637865, `UserProfileRecentActivityCard`): one card per
//! outbox entry, its picture from 325158 `nO`, its title and the badges row of module 506326 at the `USER_PROFILE`
//! location (4).
//!
//! - a played or top game (`U`): the matching live activity's art, else the application's icon (the cover is not
//!   used, `showCoverImage: false`); the game's name; the badges `iq Zc Xy $X tR K7 fg sp MK` (322789 `n`), or for a
//!   top game `Xr` (977001).
//! - a watched show (`w`): the media art at 2:3, title and subtitle, `zi Rq` (514243).
//! - a listened session (`Q`): the first track's album art, title and first artist, `R_` (363670).
//! - a launched activity (`F`): the art, the activity's name, `iq Zc Xy $X fg MK` (468581).
//!
//! A card without a picture, or whose picture failed, shows the `UnknownGameIcon` (module 263577), white on a dark
//! theme. The card is `overlay_P1YEjQ card_Bu4Al8`: 12px padding, `--radius-md`, `--user-profile-overlay-background`,
//! a 1px `--border-muted` edge only on a light theme; its "..." sits 16px from the top-right corner.

use super::Root;
use super::dm_profile::Palette;
use dc_state::content_inventory::Entry;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, SharedString, div, img, prelude::*, px, svg,
};
use serde_json::Value;

/// `ContentInventoryEntryType` (module 681154).
pub(super) const PLAYED_GAME: u64 = 1;
pub(super) const WATCHED_MEDIA: u64 = 2;
pub(super) const TOP_GAME: u64 = 3;
pub(super) const LISTENED_SESSION: u64 = 5;

/// `ContentInventoryTraitType` (module 974690).
const TRAIT_DURATION_SECONDS: u64 = 2;
const TRAIT_RESURRECTED: u64 = 5;
const TRAIT_MARATHON: u64 = 6;
const TRAIT_STREAK_DAYS: u64 = 8;
const TRAIT_TRENDING_CONTENT: u64 = 9;

/// `$R` of module 20805: the entries 637865 draws a card for (a game, a listened session, a watched show, or anything
/// naming an application).
pub(super) fn has_card(entry: &Value) -> bool {
    matches!(
        entry["content_type"].as_u64(),
        Some(PLAYED_GAME | TOP_GAME | LISTENED_SESSION | WATCHED_MEDIA)
    ) || entry["extra"].get("application_id").is_some()
}

/// Where a press on part of a card goes.
#[derive(Clone, Debug)]
enum Link {
    Game(String),
    Url(String),
}

/// One badge of the row: its icon (in the `icons/` folder) and text, and whether it is drawn in the positive colour.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Badge {
    pub icon: &'static str,
    pub text: String,
    pub positive: bool,
    /// Only the icon takes the positive colour (`te` with `STATUS_POSITIVE` at the card and game-profile locations;
    /// never at this one, kept for the shape of `J`).
    pub tabular: bool,
}

impl Badge {
    fn plain(icon: &'static str, text: String) -> Self {
        Self {
            icon,
            text,
            positive: false,
            tabular: false,
        }
    }
}

/// `Hd` of module 583846: the entry is live and not expired.
fn live(entry: &Entry, now_ms: i64) -> bool {
    entry.is_live() && !entry.expired(now_ms)
}

/// `H.A` of module 693879: a running clock from the matching activity's start (or `created_at`) when the author is
/// doing it now, else from the entry while it is live, else "Nh ago".
fn time_text(entry: &Entry, matching: Option<&Value>, now_ms: i64) -> String {
    let start = matching.and_then(|activity| {
        super::live_activity::millis(&activity["timestamps"]["start"])
            .or_else(|| super::live_activity::millis(&activity["created_at"]))
    });
    if let Some(start) = start {
        return super::inventory_feed::live_timer(start, now_ms);
    }
    if live(entry, now_ms) {
        return super::inventory_feed::live_timer(entry.timestamp_ms(), now_ms);
    }
    entry.age_text(now_ms)
}

/// `us` of module 583846 with `intervalToDuration` (date-fns, module 665697): "Returning after" the months part of the
/// gap, else its days part; the duration has no weeks, so the weeks plural is always empty.
pub(super) fn returning_text(last_played_ms: i64, now_ms: i64) -> String {
    use chrono::{Datelike, TimeZone};
    let (Some(start), Some(end)) = (
        chrono::Local.timestamp_millis_opt(last_played_ms).single(),
        chrono::Local.timestamp_millis_opt(now_ms).single(),
    ) else {
        return String::new();
    };
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let mut months = (end.year() - start.year()) * 12 + end.month() as i32 - start.month() as i32;
    let anchor = |months: i32| {
        start
            .checked_add_months(chrono::Months::new(months.max(0) as u32))
            .unwrap_or(start)
    };
    if months > 0 && anchor(months) > end {
        months -= 1;
    }
    let after_months = anchor(months);
    let days = (end - after_months).num_days();
    let months = months % 12;
    let days = if months > 0 { 0 } else { days };
    dc_intl::format(
        "NXBtjF",
        &[
            ("months", (months as f64).into()),
            ("weeks", 0.0.into()),
            ("days", (days as f64).into()),
        ],
    )
}

/// The game badges in 322789's order (`iq Zc Xy $X tR K7 fg sp MK`), or the launched activity's (`iq Zc Xy $X fg MK`);
/// `Xy` (the voice channel) never shows at this location.
pub(super) fn game_badges(
    entry: &Entry,
    matching: Option<&Value>,
    launched: bool,
    now_ms: i64,
) -> Vec<Badge> {
    let mut badges = Vec::new();
    // `$`: the game controller (the apps glyph for a launched activity) with the time, positive while live.
    badges.push(Badge {
        icon: if launched {
            "activity_launched"
        } else {
            "activity_game"
        },
        text: time_text(entry, matching, now_ms),
        positive: live(entry, now_ms),
        tabular: true,
    });
    // `tt`: the matching activity's state and party size.
    if let Some(activity) = matching
        && let Some(text) =
            super::live_activity::party_text(activity["state"].as_str(), &activity["party"])
    {
        badges.push(Badge::plain("activity_party", text));
    }
    // `ts`: an application made in the last seven days.
    if let Some(app) = entry.extra["application_id"]
        .as_str()
        .and_then(|app| app.parse::<u64>().ok())
    {
        let made = (app >> 22) as i64 + 1_420_070_400_000;
        if (now_ms - made) / 86_400_000 <= 7 {
            badges.push(Badge::plain(
                "activity_star_shooting",
                dc_intl::t!("vYuyWf").to_owned(),
            ));
        }
    }
    if !launched {
        // `te`: the first session ("New Player").
        if entry.first_time() {
            badges.push(Badge::plain(
                "activity_new_player",
                dc_intl::t!("keY6mW").to_owned(),
            ));
        }
        // `tr`: three days or more in a row, shown for 48 hours after the entry.
        if let Some(days) = entry
            .trait_value(TRAIT_STREAK_DAYS, "streak_count_days")
            .and_then(Value::as_i64)
            && days >= 3
            && now_ms - entry.timestamp_ms() <= 48 * 3_600_000
        {
            badges.push(Badge::plain(
                "activity_flash",
                dc_intl::t!("Klie/P", days = days).to_owned(),
            ));
        }
    }
    // `ta`: trending content (any value but `TRENDING_TYPE_UNSPECIFIED`).
    if entry
        .trait_value(TRAIT_TRENDING_CONTENT, "trending")
        .and_then(Value::as_i64)
        .is_some_and(|trending| trending != 0)
    {
        badges.push(Badge::plain(
            "discover_fire",
            dc_intl::t!("kAlUsy").to_owned(),
        ));
    }
    if !launched {
        // `tl`: back after a break, written out away from the card location.
        if let Some(last) = entry
            .trait_value(TRAIT_RESURRECTED, "resurrected_last_played")
            .and_then(Value::as_str)
            .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
        {
            badges.push(Badge::plain(
                "msg_retry",
                returning_text(last.timestamp_millis(), now_ms),
            ));
        }
    }
    // `tn`: a marathon of at least an hour.
    if entry
        .trait_value(TRAIT_MARATHON, "marathon")
        .and_then(Value::as_bool)
        == Some(true)
        && let Some(seconds) = entry
            .trait_value(TRAIT_DURATION_SECONDS, "duration_seconds")
            .and_then(Value::as_f64)
    {
        let hours = (seconds / 3_600.).round() as i64;
        if hours > 0 {
            badges.push(Badge::plain(
                "slowmode",
                dc_intl::t!("vZaMem", hours = hours).to_owned(),
            ));
        }
    }
    badges
}

/// `Xr` of module 506326 away from the card: "Most played — Nh this week" with the trophy.
pub(super) fn top_game_badges(entry: &Entry) -> Vec<Badge> {
    entry
        .trait_value(TRAIT_DURATION_SECONDS, "duration_seconds")
        .and_then(Value::as_f64)
        .map(|seconds| {
            let hours = (seconds / 3_600.).round() as i64;
            vec![Badge::plain(
                "inventory_trophy",
                format!(
                    "{}{}{}",
                    dc_intl::t!("/50eHi"),
                    " \u{2014} ",
                    dc_intl::t!("C0AxoR", hours = hours)
                ),
            )]
        })
        .unwrap_or_default()
}

/// `kR` of module 583846: "Season N, Episode M" read from the show's large text.
fn season_episode(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    let mut numbers = Vec::new();
    while let Some(word) = words.next() {
        if let Some(next) = words.clone().next() {
            let digits = next.trim_end_matches(',');
            if word.chars().all(char::is_alphabetic)
                && !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
            {
                numbers.push(digits.to_owned());
                words.next();
            }
        }
        if numbers.len() == 2 {
            break;
        }
    }
    let [season, episode] = numbers.as_slice() else {
        return None;
    };
    Some(
        dc_intl::t!(
            "ijVm6y",
            seasonNum = season.as_str(),
            episodeNum = episode.as_str()
        )
        .to_owned(),
    )
}

/// `n` of module 261020 with `RQ` of 272984: the Spotify app's `spotify:` address when the protocol is registered,
/// else the web player's, both with `?utm_source=discord&utm_medium=desktop`.
pub(super) fn spotify_open_url(kind: &str, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    let encode = |text: &str| {
        url::form_urlencoded::byte_serialize(text.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    let suffix = "?utm_source=discord&utm_medium=desktop";
    Some(if crate::spotify::protocol_registered() {
        format!("spotify:{}:{}{suffix}", encode(kind), encode(id))
    } else {
        format!(
            "https://open.spotify.com/{}/{}{suffix}",
            encode(kind),
            encode(id)
        )
    })
}

/// `st` of module 927071 with each platform's `icon.lightPNG` and `icon.darkPNG` (module 573648): the platforms the
/// empty Activity tab of your own profile shows.
const EMPTY_PLATFORMS: [(&str, &str, &str); 13] = [
    ("Steam", "b68f2f4df4af9085.png", "07ca603d9273b502.png"),
    (
        "PlayStation Network",
        "ae74d3de4958bf78.png",
        "46a23515a87620b9.png",
    ),
    ("Xbox", "8cea2073e188eabe.png", "09781ebf5509f48e.png"),
    ("Twitch", "45900de231038a6d.png", "45900de231038a6d.png"),
    ("Battle.net", "0e1a419c00a471a2.png", "0e1a419c00a471a2.png"),
    (
        "League of Legends",
        "fc30b78cd5e1db04.png",
        "fc30b78cd5e1db04.png",
    ),
    ("Epic Games", "e87d94bf0041b63e.png", "ecf035f3cf08a860.png"),
    ("Riot Games", "fa9e14fc0fd3680f.png", "fa9e14fc0fd3680f.png"),
    ("Roblox", "268c81933d752854.png", "5d078bf414a0124c.png"),
    ("Spotify", "40518c6ced9fed93.png", "40518c6ced9fed93.png"),
    ("YouTube", "c569f6e10f1fa390.png", "c569f6e10f1fa390.png"),
    (
        "Crunchyroll",
        "4e56bb1ecb9b9dc3.png",
        "4e56bb1ecb9b9dc3.png",
    ),
    ("Bungie.net", "9b762e8a42709cc4.png", "a35b652323167799.png"),
];

impl Root {
    /// `sa` of module 927071: your own Activity tab with nothing in it (`empty_7q_A1n`): `VB6LWY` (heading-sm/medium
    /// `text-strong`) over `KpjsU9` (body-sm/normal `text-default`), at most 320px wide and centred, the platforms'
    /// 36px pictures (12px apart, wrapping in 324px, 8px above and below), then Connect Accounts and Add Game (small
    /// secondary buttons, 8px apart), 24px between the three.
    pub(super) fn own_activity_empty(
        &self,
        palette: &Palette,
        strong: Hsla,
        default: Hsla,
        top: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let light = palette.theme.as_ref().map_or_else(
            || super::theme() == super::Theme::Light,
            |theme| theme.light(),
        );
        let icons: Vec<AnyElement> = EMPTY_PLATFORMS
            .iter()
            .map(|(name, light_png, dark_png)| {
                let file = if light { light_png } else { dark_png };
                let key = super::full_profile::connection_icon_key(file);
                let picture = self.images.get(&key).cloned();
                if picture.is_none() {
                    self.want_image(key, format!("https://discord.com/assets/{file}"), cx);
                }
                div()
                    .id(SharedString::from(format!(
                        "full-profile-empty-platform-{name}"
                    )))
                    .size(px(36.))
                    .children(picture.map(|picture| img(picture).size(px(36.))))
                    .into_any_element()
            })
            .collect();
        let border = palette.token("--control-secondary-border-default");
        let fill = palette.token("--control-secondary-background-default");
        let hover = palette.token("--control-secondary-background-hover");
        let text = palette.token("--control-secondary-text-default");
        let button = move |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(border)
                .bg(fill)
                .hover(move |style| style.bg(hover))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(text)
                .child(label)
        };
        div()
            .size_full()
            .pt(px(top))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(24.))
            .child(
                div()
                    .max_w(px(320.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_center()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(strong)
                            .child(dc_intl::t!("VB6LWY")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(default)
                            .child(dc_intl::t!("KpjsU9")),
                    ),
            )
            .child(
                div()
                    .max_w(px(324.))
                    .py(px(8.))
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(12.))
                    .children(icons),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button("full-profile-connect-accounts", dc_intl::t!("/Hl24U")).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.close_full_profile(cx);
                                this.open_settings_category(
                                    dc_intl::tr("Connected Apps"),
                                    dc_intl::tr("Connections"),
                                    cx,
                                );
                            }),
                        ),
                    )
                    .child(
                        button("full-profile-add-game", dc_intl::t!("GTCx0p")).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.close_full_profile(cx);
                                // `CONNECTED_GAMES_CATEGORY`: "Messaging in Connected Games".
                                this.open_settings_category(
                                    dc_intl::tr("Messaging Permissions"),
                                    dc_intl::tr("Messaging in Connected Games"),
                                    cx,
                                );
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// The rows of a Recent activity card's menu (`recent_activity_items`) for the entry kept in the menu state.
    pub(super) fn recent_activity_menu_items(&self, user: Id) -> Vec<super::context_menu::Entry> {
        let Some(entry) = self.context.recent_entry.as_ref() else {
            return Vec::new();
        };
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == user);
        // 83971 `F3`: a Spotify listening session (provider 1).
        let spotify = (entry["content_type"].as_u64() == Some(LISTENED_SESSION)
            && entry["extra"]["entries"][0]["media"]["provider"] == 1)
            .then(|| self.stores.spotify.has_connected_account());
        let game = matches!(entry["content_type"].as_u64(), Some(PLAYED_GAME | TOP_GAME));
        let profile = game
            && entry["extra"]["application_id"]
                .as_str()
                .is_some_and(|app| self.game_profile_available(app));
        super::context_menu::recent_activity_items(own, spotify, game, profile)
    }

    /// The card's "..." (285909 `H`): its menu beside the button, nothing when the menu has no rows or the author is a
    /// bot.
    pub(super) fn open_recent_activity_menu(
        &mut self,
        user: Id,
        entry: &Value,
        anchor: &SharedString,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.stores.users.get(&user).is_some_and(|known| known.bot) {
            return;
        }
        self.context.recent_entry = Some(entry.clone());
        if self.recent_activity_menu_items(user).is_empty() {
            self.context.recent_entry = None;
            return;
        }
        let Some(bounds) = self.tip_anchors.borrow().get(anchor).copied() else {
            return;
        };
        let viewport = window.viewport_size();
        let width = 188.;
        let right = bounds.right() + px(8.);
        let x = if right + px(width) > viewport.width - px(8.) {
            bounds.left() - px(8. + width)
        } else {
            right
        };
        self.open_context_menu(
            super::context_menu::Target::RecentActivity { user },
            gpui::point(x, bounds.top()),
            cx,
        );
    }

    /// `getMatchingActivity` (module 574520): what the author is doing now that this entry is about.
    fn matching_activity(&self, entry: &Entry) -> Option<Value> {
        self.stores
            .presence
            .get(entry.author, None)?
            .activities
            .iter()
            .find(|activity| entry.matches_activity(activity))
            .cloned()
    }

    /// `recent` of module 146655: an outbox entry is listed unless it is live (583846 `Hd`), a listened session with
    /// no tracks or one the author is listening to now (83971 `qb`), or a show they are watching now (`SU`).
    pub(super) fn shows_in_recent(&self, value: &Value, now_ms: i64) -> bool {
        let Some(entry) = Entry::from_value(value) else {
            return false;
        };
        if live(&entry, now_ms) {
            return false;
        }
        match entry.kind {
            LISTENED_SESSION => {
                entry.extra["entries"]
                    .as_array()
                    .is_some_and(|tracks| !tracks.is_empty())
                    && self.matching_activity(&entry).is_none()
            }
            WATCHED_MEDIA => self.matching_activity(&entry).is_none(),
            _ => has_card(value),
        }
    }

    /// The URL 325158 `nO` shows for an entry, and the image key it is kept under.
    pub(super) fn recent_activity_art(&self, entry: &Entry) -> Option<(String, String)> {
        // The live activity's own art comes first (`b(activity)`).
        if let Some(url) = self
            .matching_activity(entry)
            .and_then(|activity| super::live_activity::asset_url(&activity, "large_image"))
        {
            return Some((url.clone(), url));
        }
        match entry.kind {
            LISTENED_SESSION => entry.extra["entries"][0]["media"]["image_url"]
                .as_str()
                .map(|url| (url.to_owned(), url.to_owned())),
            WATCHED_MEDIA => {
                let app = entry.extra["application_id"].as_str()?;
                let asset = entry.extra["media_assets_large_image"].as_str()?;
                let url = super::live_activity::asset_url(
                    &serde_json::json!({"application_id": app, "assets": {"large_image": asset}}),
                    "large_image",
                )?;
                Some((url.clone(), url))
            }
            _ => {
                // The application's icon (`getIconURL(LARGE)`), read with the outbox's `/applications/public`
                // (`full_profile_fetched` asks for it under `app-icon:<id>`). An application without an icon, or one
                // the lookup did not return, has no picture; while the lookup is out the card waits.
                let app = entry.extra["application_id"].as_str()?;
                let key = format!("app-icon:{app}");
                if self.images.contains_key(&key) {
                    return Some((key.clone(), key));
                }
                match self.fetched.get(&format!("apps:{}", entry.author)) {
                    Some(apps) if !crate::net::is_fetch_failure(apps) => apps
                        .as_array()?
                        .iter()
                        .find(|known| known["id"].as_str() == Some(app))
                        .and_then(|known| known["icon"].as_str())
                        .map(|_| (key.clone(), key)),
                    _ => Some((key.clone(), key)),
                }
            }
        }
    }

    /// `$` of module 637865 for one outbox entry.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn recent_activity_card(
        &self,
        user: Id,
        value: &Value,
        index: usize,
        now_ms: i64,
        palette: &Palette,
        strong: Hsla,
        subtle: Hsla,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !has_card(value) {
            return None;
        }
        let entry = Entry::from_value(value)?;
        let matching = self.matching_activity(&entry);
        let app = entry.extra["application_id"].as_str().map(str::to_owned);
        // 692969: the game's profile opens (its record is known and allowed); without it 637865 `U` draws the plain
        // card, the title alone, nothing clickable.
        let game_profile = matches!(entry.kind, PLAYED_GAME | TOP_GAME)
            && app
                .as_deref()
                .is_some_and(|app| self.game_profile_available(app));
        let (title, subtitle, badges, crunchyroll) = match entry.kind {
            PLAYED_GAME | TOP_GAME if !game_profile => (
                entry.extra["game_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                None,
                Vec::new(),
                false,
            ),
            PLAYED_GAME | TOP_GAME => {
                let name = entry.extra["game_name"]
                    .as_str()
                    .or_else(|| matching.as_ref().and_then(|a| a["name"].as_str()))
                    .unwrap_or_default()
                    .to_owned();
                let badges = if entry.kind == TOP_GAME {
                    top_game_badges(&entry)
                } else {
                    game_badges(&entry, matching.as_ref(), false, now_ms)
                };
                (name, None, badges, false)
            }
            LISTENED_SESSION => {
                let media = &entry.extra["entries"][0]["media"];
                if media.is_null() {
                    return None;
                }
                let badges = vec![Badge {
                    icon: "activity_music",
                    text: time_text(&entry, matching.as_ref(), now_ms),
                    positive: live(&entry, now_ms),
                    tabular: true,
                }];
                (
                    media["title"].as_str().unwrap_or_default().to_owned(),
                    media["artists"][0]["name"].as_str().map(str::to_owned),
                    badges,
                    false,
                )
            }
            WATCHED_MEDIA => {
                let mut badges = vec![Badge {
                    icon: "activity_tv",
                    text: time_text(&entry, matching.as_ref(), now_ms),
                    positive: live(&entry, now_ms),
                    tabular: true,
                }];
                if let Some(text) = entry.extra["media_assets_large_text"]
                    .as_str()
                    .and_then(season_episode)
                {
                    badges.push(Badge::plain("activity_topics", text));
                }
                (
                    entry.extra["media_title"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    entry.extra["media_subtitle"].as_str().map(str::to_owned),
                    badges,
                    true,
                )
            }
            _ => (
                entry.extra["activity_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                None,
                game_badges(&entry, matching.as_ref(), true, now_ms),
                false,
            ),
        };
        let art_key = self.recent_activity_art(&entry);
        let picture = art_key
            .as_ref()
            .and_then(|(key, _)| self.images.get(key).cloned());
        if picture.is_none()
            && let Some((key, url)) = art_key.clone()
            && !key.starts_with("app-icon:")
        {
            self.want_image(key, url, cx);
        }
        // `V` of 263577: no address, or an address that failed, draws the UnknownGameIcon; while it loads, nothing.
        let unknown = match &art_key {
            None => true,
            Some((key, _)) => picture.is_none() && self.images.retries.failures(key) > 0,
        };
        let light = palette.theme.as_ref().map_or_else(
            || super::theme() == super::Theme::Light,
            |theme| theme.light(),
        );
        let positive: Hsla = palette.token("--text-feedback-positive").into();
        let (art_w, art_h) = if crunchyroll { (40., 60.) } else { (60., 60.) };
        let art: AnyElement = match picture {
            Some(picture) => div()
                .w(px(art_w))
                .h(px(art_h))
                .flex_none()
                .rounded(px(8.))
                .overflow_hidden()
                .child(
                    img(picture)
                        .w(px(art_w))
                        .h(px(art_h))
                        .rounded(px(8.))
                        .object_fit(gpui::ObjectFit::Cover),
                )
                .into_any_element(),
            None if unknown => svg()
                .path("icons/inventory_unknown_game.svg")
                .flex_none()
                .size(px(60.))
                .text_color(if light { gpui::black() } else { gpui::white() })
                .into_any_element(),
            None => div()
                .w(px(art_w))
                .h(px(art_h))
                .flex_none()
                .into_any_element(),
        };
        let badge = |badge: Badge| {
            let tint = if badge.positive { positive } else { subtle };
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    svg()
                        .path(SharedString::from(format!("icons/{}.svg", badge.icon)))
                        .flex_none()
                        .size(px(12.))
                        .text_color(tint),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(tint)
                        .when(badge.tabular, |text| {
                            text.font_features(super::voice_message::tnum())
                        })
                        .truncate()
                        .child(badge.text),
                )
        };
        let more_anchor = SharedString::from(format!("full-profile-recent-more-{user}-{index}"));
        let entry_value = value.clone();
        // What a press on the picture, the title and the artist opens: the game's profile (the modal stays open), or
        // the Spotify album, track and artist (`OPEN_SPOTIFY_*` of 637865 `Q`).
        let media = &entry.extra["entries"][0]["media"];
        let spotify = |kind: &'static str, id: Option<&str>| {
            id.filter(|id| !id.is_empty())
                .and_then(|id| spotify_open_url(kind, id))
        };
        let (art_link, title_link, subtitle_link): (Option<Link>, Option<Link>, Option<Link>) =
            match entry.kind {
                PLAYED_GAME | TOP_GAME if game_profile => {
                    let game = app.clone().map(Link::Game);
                    (game.clone(), game, None)
                }
                LISTENED_SESSION => (
                    spotify("album", media["external_parent_id"].as_str()).map(Link::Url),
                    spotify("track", media["external_id"].as_str()).map(Link::Url),
                    spotify("artist", media["artists"][0]["external_id"].as_str()).map(Link::Url),
                ),
                WATCHED_MEDIA => {
                    let url = entry.extra["url"]
                        .as_str()
                        .filter(|url| !url.is_empty())
                        .map(|url| Link::Url(url.to_owned()));
                    (url.clone(), url, None)
                }
                _ => (None, None, None),
            };
        let linked =
            |element: gpui::Stateful<gpui::Div>, link: Option<Link>, cx: &mut Context<Self>| {
                match link {
                    Some(link) => {
                        element
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                match &link {
                                    Link::Game(app) => this.open_game_profile(app.clone(), cx),
                                    Link::Url(url) => cx.open_url(url),
                                }
                            }))
                    }
                    None => element,
                }
            };
        Some(
            div()
                .id(SharedString::from(format!(
                    "full-profile-recent-{user}-{index}"
                )))
                .relative()
                .p(px(12.))
                .rounded(px(12.))
                .when(light, |card| {
                    card.border_1()
                        .border_color(palette.token("--border-muted"))
                })
                .bg(palette.token("--user-profile-overlay-background"))
                .flex()
                .gap(px(12.))
                .child(linked(
                    div()
                        .id(SharedString::from(format!(
                            "full-profile-recent-art-{user}-{index}"
                        )))
                        .flex_none()
                        .child(art),
                    art_link,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .mr(px(24.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(linked(
                                    div()
                                        .id(SharedString::from(format!(
                                            "full-profile-recent-title-{user}-{index}"
                                        )))
                                        .when(title_link.is_some(), |text| {
                                            text.hover(move |style| style.underline())
                                        })
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(strong)
                                        .truncate()
                                        .child(title),
                                    title_link.clone(),
                                    cx,
                                ))
                                .children(subtitle.map(|subtitle| {
                                    linked(
                                        div()
                                            .id(SharedString::from(format!(
                                                "full-profile-recent-subtitle-{user}-{index}"
                                            )))
                                            .when(subtitle_link.is_some(), |text| {
                                                text.hover(move |style| style.underline())
                                            })
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(strong)
                                            .truncate()
                                            .child(subtitle),
                                        subtitle_link.clone(),
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(8.))
                                .children(badges.into_iter().map(badge)),
                        ),
                )
                .child(
                    // `headerContextMenu absolute`: the 16px "..." with 4px after it, 12px inside the card.
                    div()
                        .id(more_anchor.clone())
                        .absolute()
                        .top(px(12.))
                        .right(px(16.))
                        .size(px(16.))
                        .cursor_pointer()
                        .child(self.anchor_probe(more_anchor.to_string()))
                        .child(
                            svg()
                                .path("icons/more_horizontal.svg")
                                .size(px(16.))
                                .text_color(subtle),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_recent_activity_menu(
                                user,
                                &entry_value,
                                &more_anchor,
                                window,
                                cx,
                            );
                        })),
                )
                .into_any_element(),
        )
    }
}

use dc_state::Id;

impl Root {
    /// The Activity tab of module 927071 (`sc`), for the full profile and for the editor's own profile: Current
    /// activity and Recent activity, or the empty state (`sa` on your own profile, `sr` on someone else's).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn profile_activity_panel(
        &self,
        id: Id,
        me: bool,
        outbox: Option<&Value>,
        activity_name: &str,
        palette: &Palette,
        // The panel scroller's 20px top padding (`tabPanelScroller_y4ZEM5`), when the caller's scroller lacks it.
        top: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let current =
            self.live_activity_cards(id, super::live_activity::Surface::Modal, strong, subtle, cx);
        let entries: Vec<Value> = outbox
            .and_then(|outbox| outbox["entries"].as_array())
            .cloned()
            .unwrap_or_default();
        let cards: Vec<AnyElement> = entries
            .iter()
            .filter(|entry| self.shows_in_recent(entry, now_ms))
            .enumerate()
            .filter_map(|(index, entry)| {
                self.recent_activity_card(id, entry, index, now_ms, palette, strong, subtle, cx)
            })
            .collect();
        let outbox_read = outbox.is_some();
        if me && outbox_read && current.is_empty() && cards.is_empty() {
            self.own_activity_empty(palette, strong, default, top, cx)
        } else if !me && outbox_read && current.is_empty() && cards.is_empty() {
            // `sr` of module 927071: "<name> doesn't have any activity to share here", one of its sixteen
            // lines (sampled once per opening; here by the person's id) and a Message button.
            let (heading, line) = super::full_profile::activity_empty_text(activity_name, id);
            let message_bg = palette.token("--control-secondary-background-default");
            let message_hover = palette.token("--control-secondary-background-hover");
            let message_fg = palette.token("--control-secondary-text-default");
            div()
                // `empty_7q_A1n` sits in the tab panel itself (no scroller), 448px wide from the panel's
                // 32px inset: its centre is 8px right of the scroller's.
                .size_full()
                .pt(px(top))
                .relative()
                .left(px(8.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(24.))
                .child(
                    div()
                        .max_w(px(320.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .text_center()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(strong)
                                .child(heading),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(default)
                                .child(line),
                        ),
                )
                .child(
                    // `button_F7qvhY` small secondary: 32px, 12px sides, the 16px chat glyph 4px before it.
                    div()
                        .id("full-profile-activity-message")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(message_bg)
                        .hover(move |style| style.bg(message_hover))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            svg()
                                .path("icons/chat_bubble.svg")
                                .size(px(16.))
                                .text_color(message_fg),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(message_fg)
                                .child(dc_intl::t!("g33r/P")),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_dm_with(id, cx);
                        })),
                )
                .into_any_element()
        } else {
            // `sc` of module 927071: the panel scroller (`tabPanelScroller_y4ZEM5`, 24px between sections)
            // holds Current activity and Recent activity (`td` sections, 8px inside); on your own profile
            // Recent activity opens with `4bk9Ak` (body-xs/normal) and its Learn more link to the
            // `ACTIVITY_STATUS_SETTINGS` article.
            let link: Hsla = palette.token("--text-link").into();
            let intro = me.then(|| {
                div().flex().flex_wrap().text_size(px(12.)).line_height(px(16.)).text_color(default).children(
                    dc_intl::format_runs("4bk9Ak", &[]).into_iter().enumerate().map(
                        |(index, (text, tag))| match tag {
                            None => div().whitespace_nowrap().child(text).into_any_element(),
                            Some(_) => div()
                                .id(("full-profile-activity-learn-more", index))
                                .cursor_pointer()
                                .text_color(link)
                                .hover(|style| style.underline())
                                .child(text)
                                .on_click(|_, _, cx| {
                                    cx.open_url(
                                        "https://support.discord.com/hc/en-us/articles/7931156448919",
                                    );
                                })
                                .into_any_element(),
                        },
                    ),
                )
            });
            let section =
                |heading: &'static str, intro: Option<gpui::Div>, cards: Vec<AnyElement>| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .child(Self::section_heading(heading, subtle))
                                .children(intro),
                        )
                        .child(div().flex().flex_col().gap(px(8.)).children(cards))
                };
            div()
                .pt(px(top))
                .flex()
                .flex_col()
                .gap(px(24.))
                .when(!current.is_empty(), |this| {
                    this.child(section(dc_intl::t!("J6STd9"), None, current))
                })
                .when(!cards.is_empty(), |this| {
                    this.child(section(dc_intl::t!("jzgEoL"), intro, cards))
                })
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(value: Value) -> Entry {
        Entry::from_value(&value).unwrap()
    }

    #[test]
    fn game_badges_follow_the_official_order_and_rules() {
        // Made now (snowflake of now), first session, a five day streak, trending.
        let now = 1_795_000_000_000_i64;
        let id = ((now - 1_420_070_400_000 - 3_600_000) as u64) << 22;
        let e = entry(json!({
            "id": id.to_string(), "author_id": "1", "author_type": 1, "content_type": 1,
            "extra": {"application_id": "1124351960078897173", "game_name": "The Outlast Trials"},
            "traits": [{"type": 1, "first_time": true}, {"type": 8, "streak_count_days": 5},
                       {"type": 9, "trending": 1}]
        }));
        let badges = game_badges(&e, None, false, now);
        let texts: Vec<&str> = badges.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["1h ago", "New Player", "5x Streak", "Trending"]);
        assert_eq!(badges[0].icon, "activity_game");
        assert!(!badges[0].positive);
    }

    #[test]
    fn a_streak_shows_from_three_days_and_for_two_days_only() {
        let now = 1_795_000_000_000_i64;
        let made = |hours: i64| ((now - 1_420_070_400_000 - hours * 3_600_000) as u64) << 22;
        let with = |days: i64, hours: i64| {
            entry(
                json!({"id": made(hours).to_string(), "author_id": "1", "author_type": 1, "content_type": 1,
                "extra": {"application_id": "1"}, "traits": [{"type": 8, "streak_count_days": days}]}),
            )
        };
        let has = |e: &Entry| {
            game_badges(e, None, false, now)
                .iter()
                .any(|b| b.icon == "activity_flash")
        };
        assert!(!has(&with(2, 1)));
        assert!(has(&with(3, 47)));
        assert!(!has(&with(9, 49)));
    }

    #[test]
    fn a_live_entry_counts_up_in_the_positive_colour() {
        let now = 1_795_000_000_000_i64;
        let id = ((now - 1_420_070_400_000 - 65_000) as u64) << 22;
        let e = entry(
            json!({"id": id.to_string(), "author_id": "1", "author_type": 1, "content_type": 1,
            "extra": {"application_id": "1"}, "traits": [{"type": 3, "is_live": true}]}),
        );
        let badges = game_badges(&e, None, false, now);
        assert_eq!(badges[0].text, "1:05");
        assert!(badges[0].positive);
        // A matching activity's start wins.
        let activity =
            json!({"type": 0, "application_id": "1", "timestamps": {"start": now - 3_725_000}});
        assert_eq!(
            game_badges(&e, Some(&activity), false, now)[0].text,
            "1:02:05"
        );
    }

    #[test]
    fn a_marathon_and_a_new_release_read_their_traits() {
        let now = 1_795_000_000_000_i64;
        let app = ((now - 1_420_070_400_000 - 2 * 86_400_000) as u64) << 22;
        let id = ((now - 1_420_070_400_000 - 86_400_000 * 3) as u64) << 22;
        let e = entry(
            json!({"id": id.to_string(), "author_id": "1", "author_type": 1, "content_type": 1,
            "extra": {"application_id": app.to_string()},
            "traits": [{"type": 6, "marathon": true}, {"type": 2, "duration_seconds": 5.6 * 3600.0}]}),
        );
        let texts: Vec<String> = game_badges(&e, None, false, now)
            .into_iter()
            .map(|b| b.text)
            .collect();
        assert_eq!(texts, ["3d ago", "New Release", "6h Marathon"]);
    }

    #[test]
    fn cards_are_drawn_for_the_kinds_637865_knows() {
        assert!(has_card(&json!({"content_type": 1, "extra": {}})));
        assert!(has_card(&json!({"content_type": 5, "extra": {}})));
        assert!(has_card(&json!({"content_type": 2, "extra": {}})));
        assert!(has_card(
            &json!({"content_type": 8, "extra": {"application_id": "1"}})
        ));
        assert!(!has_card(&json!({"content_type": 6, "extra": {}})));
    }

    #[test]
    fn returning_names_months_else_days() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let ms = |date: chrono::NaiveDateTime| {
            use chrono::TimeZone;
            chrono::Local
                .from_local_datetime(&date)
                .single()
                .unwrap()
                .timestamp_millis()
        };
        let back = |y, m, d| {
            ms(chrono::NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap())
        };
        assert_eq!(
            returning_text(back(2026, 8, 10), ms(now)),
            "Returning after 3 months"
        );
        assert_eq!(
            returning_text(back(2026, 11, 1), ms(now)),
            "Returning after 18 days"
        );
    }

    fn labels(items: Vec<super::super::context_menu::Entry>) -> Vec<String> {
        items
            .into_iter()
            .filter_map(|item| match item {
                super::super::context_menu::Entry::Row(row) => Some(row.label),
                super::super::context_menu::Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn the_recent_card_menu_follows_285909() {
        use super::super::context_menu::recent_activity_items;
        // Someone else's Spotify entry, our account not linked: Connect Spotify only.
        assert_eq!(
            labels(recent_activity_items(false, Some(false), false, false)),
            ["Connect Spotify"]
        );
        // Linked: Play on Spotify; on our own entry Manage Privacy instead.
        assert_eq!(
            labels(recent_activity_items(false, Some(true), false, false)),
            ["Play on Spotify"]
        );
        assert_eq!(
            labels(recent_activity_items(true, Some(true), false, false)),
            ["Manage Privacy"]
        );
        // A game: View Game Profile when it opens, Report Issue, and Manage Privacy on our own.
        assert_eq!(
            labels(recent_activity_items(true, None, true, true)),
            ["View Game Profile", "Report Issue", "Manage Privacy"]
        );
        assert_eq!(
            labels(recent_activity_items(false, None, true, false)),
            ["Report Issue"]
        );
    }

    #[test]
    fn edit_profile_in_a_server_offers_both_profiles() {
        let rows: Vec<(String, Option<String>)> = super::super::context_menu::edit_profile_items()
            .into_iter()
            .filter_map(|item| match item {
                super::super::context_menu::Entry::Row(row) => Some((row.label, row.sub)),
                super::super::context_menu::Entry::Separator => None,
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "Edit Per-server Profile".to_owned(),
                    Some("Visible only to those in this server".to_owned())
                ),
                (
                    "Edit Main Profile".to_owned(),
                    Some("Default profile and visible to everyone".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn spotify_links_carry_the_official_tracking_suffix() {
        let url = spotify_open_url("track", "4mL25h2hl6gdyu9tNTudnI").unwrap();
        assert!(
            url.ends_with("track/4mL25h2hl6gdyu9tNTudnI?utm_source=discord&utm_medium=desktop")
                || url.ends_with(
                    "track:4mL25h2hl6gdyu9tNTudnI?utm_source=discord&utm_medium=desktop"
                )
        );
        assert_eq!(spotify_open_url("track", ""), None);
    }

    #[test]
    fn a_show_names_its_season_and_episode() {
        assert_eq!(
            season_episode("Season 2, Episode 7").as_deref(),
            Some("S2E7")
        );
        assert_eq!(season_episode("Special"), None);
    }
}
