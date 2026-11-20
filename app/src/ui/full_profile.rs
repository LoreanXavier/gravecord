//! The full profile (`user-profile-modal-v2`, module 927071): 960 wide and the window's height, a 400px identity
//! column (banner, 120px avatar, names, Add Friend / Message / More, bio, Member Since, the private note) beside
//! tabs for recent activity (the content-inventory outbox), mutual friends and mutual servers. Measured in
//! `design/dom/modals/full-profile-{activity,friends,servers}-darker-cozy.json`.
//!
//! Activity cards follow module 506326: a played game shows its icon, name, the time since (`aJ` over the entry's
//! snowflake) and "New Player" on a first session; a listening session shows the track's art, title and artist.

use super::friends::{RELATIONSHIP_FRIEND, RELATIONSHIP_INCOMING, RELATIONSHIP_OUTGOING};
use super::profile_editor::save::{PersonalSection, WidgetData, parse_widgets};
use super::profile_editor::widgets::{featured_app, widget_data_title};
use super::profile_menu::Which;
use super::{Root, avatar_source, color};
use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, Hsla, IntoElement, Rgba, SharedString, Subscription,
    Window, deferred, div, hsla, img, prelude::*, px, rgba, svg,
};
use serde_json::Value;

/// `UserProfileModalV2`'s editing experiment (module 591179).
const WYSIWYG_EDITING: &str = "2026-03-wysiwyg-user-profile-editing";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    /// `WIDGETS` ("Board"): the profile's widgets, first when it has any.
    Board,
    Activity,
    /// `WISHLIST`: the profile's wishlist, when it has one.
    Wishlist,
    Friends,
    Servers,
}

/// The tabs of module 927071 (`e3`), in order: Board (always on own profile, else when the profile has valid widgets),
/// Activity, Wishlist (always on own profile, else when it has a wishlist), then mutual friends and servers of someone else.
pub(super) fn profile_tabs(body: Option<&Value>, other: bool) -> Vec<Tab> {
    let mut tabs = Vec::new();
    if !other
        || body.is_some_and(|body| {
            parse_widgets(body)
                .iter()
                .any(|widget| widget.data.is_valid())
        })
    {
        tabs.push(Tab::Board);
    }
    tabs.push(Tab::Activity);
    if !other || body.is_some_and(|body| !super::shop::data::wishlist_ids(body).is_empty()) {
        tabs.push(Tab::Wishlist);
    }
    if other && !body.is_some_and(stub_profile) {
        tabs.push(Tab::Friends);
        tabs.push(Tab::Servers);
    }
    tabs
}

/// The tab `o7` mounts on: the first of the tabs the profile has at that moment.
fn initial_tab(body: Option<&Value>, other: bool) -> Tab {
    profile_tabs(body, other)
        .first()
        .copied()
        .unwrap_or(Tab::Activity)
}

/// Module 776879: a profile with nothing to show (empty bio and pronouns, no banner or accent colour key, no badges,
/// and private flags equal to the public ones) gets no mutual tabs.
fn stub_profile(body: &Value) -> bool {
    let profile = &body["user_profile"];
    let empty = |key: &str| profile[key].as_str().is_none_or(str::is_empty);
    let user = &body["user"];
    empty("bio")
        && empty("pronouns")
        && profile.get("banner").is_none()
        && profile.get("accent_color").is_none()
        && user["flags"] == user["public_flags"]
        && body["badges"].as_array().is_none_or(Vec::is_empty)
}

/// The mutual counts of a profile answer: friends (`mutual_friends_count`, else the list) and servers.
pub(super) fn mutual_counts(body: Option<&Value>) -> (Option<usize>, Option<usize>) {
    let Some(body) = body else {
        return (None, None);
    };
    let friends = body["mutual_friends_count"]
        .as_u64()
        .map(|count| count as usize)
        .or_else(|| body["mutual_friends"].as_array().map(Vec::len));
    (friends, body["mutual_guilds"].as_array().map(Vec::len))
}

/// `MU` of module 427262: a status as the screen reader says it (`UNKNOWN` says nothing).
pub(super) fn status_label(status: &str, mobile: bool) -> Option<&'static str> {
    Some(dc_intl::t(match status {
        "online" if mobile => "9hghLD",
        "online" => "WbGtnH",
        "offline" => "Vv0abJ",
        "idle" => "qWbtVU",
        "dnd" => "jaNpQH",
        "invisible" => "bg24HO",
        "streaming" => "XKYej5",
        _ => return None,
    }))
}

/// Module 503062's row name: `9Gofqp` with the presence, else `fGlxFS`.
pub(super) fn mutual_friend_label(name: &str, presence: Option<&str>) -> String {
    match presence {
        Some(presence) => dc_intl::format(
            "9Gofqp",
            &[("name", name.into()), ("presence", presence.into())],
        ),
        None => dc_intl::format("fGlxFS", &[("name", name.into())]),
    }
}

/// Module 837531: Mutual Friends while unknown, No Mutual Friends at none, else the count.
pub(super) fn mutual_friends_label(count: Option<usize>) -> String {
    match count {
        None => dc_intl::t!("0mTJ3j").to_owned(),
        Some(0) => dc_intl::t!("n9g3ay").to_owned(),
        Some(count) => dc_intl::t!("5s9jl+", count = count).to_string(),
    }
}

/// Module 186272: Mutual Servers while unknown, No Mutual Servers at none, else the count.
pub(super) fn mutual_servers_label(count: Option<usize>) -> String {
    match count {
        None => dc_intl::t!("4lTDZq").to_owned(),
        Some(0) => dc_intl::t!("jpY0X5").to_owned(),
        Some(count) => dc_intl::t!("eE3oep", count = count).to_string(),
    }
}

/// `si` / `sl` of module 927071: the empty Mutual Friends / Mutual Servers tab's heading and one of its five lines
/// (`r7` / `r4`, sampled once per opening; here chosen by the person's id so a reopening keeps it).
pub(super) fn mutual_empty_text(servers: bool, user: Id) -> (&'static str, &'static str) {
    let friends = [
        dc_intl::t!("madJdE"),
        dc_intl::t!("NYmfoP"),
        dc_intl::t!("R2PaCg"),
        dc_intl::t!("laSR8h"),
        dc_intl::t!("DnsJE8"),
    ];
    let guilds = [
        dc_intl::t!("nFSbeE"),
        dc_intl::t!("gTcxOz"),
        dc_intl::t!("8T0wYj"),
        dc_intl::t!("BIHl1g"),
        dc_intl::t!("jhBm0+"),
    ];
    let pick = (user % 5) as usize;
    if servers {
        (dc_intl::t!("bFgqYJ"), guilds[pick])
    } else {
        (dc_intl::t!("RnD2yZ"), friends[pick])
    }
}

/// `sr` of module 927071: the empty Activity tab's heading for `name` and one of its sixteen lines (`se`, sampled once
/// per opening; here chosen by the person's id so a reopening keeps it).
pub(super) fn activity_empty_text(name: &str, user: Id) -> (String, String) {
    let pick = (user % 16) as usize;
    let line = match pick {
        0 => dc_intl::t!("AyMGXA").to_owned(),
        1 => dc_intl::t!("aAFW7V").to_owned(),
        2 => dc_intl::t!("h2g0cM", name = name),
        3 => dc_intl::t!("rrYh58").to_owned(),
        4 => dc_intl::t!("HX3K+F").to_owned(),
        5 => dc_intl::t!("/yW3aY").to_owned(),
        6 => dc_intl::t!("PmL/v0").to_owned(),
        7 => dc_intl::t!("IALa3h").to_owned(),
        8 => dc_intl::t!("HRcTFL").to_owned(),
        9 => dc_intl::t!("NuCqPt").to_owned(),
        10 => dc_intl::t!("M1tw+4").to_owned(),
        11 => dc_intl::t!("UBm1y2").to_owned(),
        12 => dc_intl::t!("Cu95PQ").to_owned(),
        13 => dc_intl::t!("R/wFuh").to_owned(),
        14 => dc_intl::t!("HQPAVT").to_owned(),
        _ => dc_intl::t!("YolGh4").to_owned(),
    };
    (dc_intl::t!("sjSitP", name = name), line)
}

pub(super) struct FullProfile {
    pub user: Id,
    pub(super) guild: Option<Id>,
    pub tab: Tab,
    /// A tab was asked for (`initialSection`) or pressed; otherwise the first tab shows.
    pub explicit: bool,
    pub(super) note: Entity<Editor>,
    /// The note as last saved, so a blur only writes a change.
    saved_note: String,
    /// Board widgets whose Show more was pressed (`t9`'s `expanded`), by widget key.
    board_expanded: std::collections::HashSet<String>,
    /// The bot profile modal's tab (`e1` of module 148540, `initialSection ?? BOT_INFO`).
    pub(super) bot_tab: super::profile_bot::BotTab,
    /// Restricted blocked/ignored profile gate dismissed for this open (`ip` of module 148540).
    pub(super) restricted_revealed: bool,
    _events: Subscription,
}

impl FullProfile {
    pub(super) fn board_is_expanded(&self, key: &str) -> bool {
        self.board_expanded.contains(key)
    }

    pub(super) fn toggle_board_expanded(&mut self, key: &str) {
        if !self.board_expanded.remove(key) {
            self.board_expanded.insert(key.to_owned());
        }
    }
}

fn profile_route(user: Id, guild: Option<Id>) -> String {
    let mut route = format!(
        "/users/{user}/profile?type=popout&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=false"
    );
    if let Some(guild) = guild {
        route.push_str(&format!("&guild_id={guild}"));
    }
    route
}

/// 370480.An formats the server's ISO date in the current local timezone. A date without an offset (connections'
/// `metadata.created_at`, `2013-01-06T10:00:00`) is local time, as `new Date()` reads it.
pub(in crate::ui) fn profile_date(value: &str) -> Option<String> {
    let local = chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&chrono::Local).date_naive())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|date| date.date())
        })?;
    Some(local.format("%b %-d, %Y").to_string())
}

/// Whether `body["guild_member"]` belongs to `open_guild` and should be merged into `MemberStore`.
/// A member without a Nitro per-server profile has `guild_member_profile: null`, so `guild_id` is checked only when present.
fn should_apply_guild_member(open_guild: Option<Id>, body: &Value) -> bool {
    let Some(guild) = open_guild else {
        return false;
    };
    if !body["guild_member"].is_object() {
        return false;
    }
    body["guild_member_profile"]["guild_id"]
        .as_str()
        .is_none_or(|id| id == guild.to_string().as_str())
}

fn encode_uri_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One entry of a connection's metadata line (module 169869): `N` (a count and its label), `m` (a tag on a pill) and
/// `A` (a label after a picture).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ConnectionMeta {
    Text(String),
    /// `connectedAccountVanityMetadataTag`; PayPal's adds `paypalVerifiedTag` (green, white text).
    Tag {
        label: String,
        paypal: bool,
    },
    Item {
        label: String,
        icon: &'static str,
        alt: &'static str,
    },
}

/// ShowConnections' view of an account (`connected_account_icons`): platform, name, verified, address, icon.
pub(super) type ConnectionIcon = (
    String,
    String,
    bool,
    Option<String>,
    Option<std::sync::Arc<gpui::Image>>,
);

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConnectedAccount {
    kind: String,
    name: String,
    created_at: Option<String>,
    platform_name: &'static str,
    /// The platform's `icon.lightPNG` (module 573648), which `t0` of module 927071 always draws.
    icon: &'static str,
    /// The platform's `icon.lightSVG` and `icon.darkSVG` (module 573648); only ShowConnections draws them.
    svg: (&'static str, &'static str),
    /// The account's `verified`: the Verified Connection mark of module 46937 and ShowConnections' label.
    verified: bool,
    /// The mark in the Twitter colour with "Verified on Twitter" ([`verified_mark`]).
    twitter_verified: bool,
    url: Option<String>,
    /// The account's `id` on its platform (OpenInApp opens it in the app).
    id: String,
    metadata: Vec<ConnectionMeta>,
}

/// The platforms of module 573648 (`type`, `name`, `icon.lightPNG`, `icon.lightSVG`, `icon.darkSVG`): `isSupported` is a
/// type in this list, and module 851767 keeps only supported connections of a profile. The profile draws the PNG;
/// ShowConnections draws the SVG of the theme (`plugin_hooks/show_connections.rs`).
pub(in crate::ui) const CONNECTED_PLATFORMS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "twitch",
        "Twitch",
        "45900de231038a6d.png",
        "4fda00c96319c8ae.svg",
        "4fda00c96319c8ae.svg",
    ),
    (
        "youtube",
        "YouTube",
        "c569f6e10f1fa390.png",
        "0fa530ba9c04ac32.svg",
        "0fa530ba9c04ac32.svg",
    ),
    (
        "battlenet",
        "Battle.net",
        "0e1a419c00a471a2.png",
        "163c8cb9220efc74.svg",
        "163c8cb9220efc74.svg",
    ),
    (
        "bluesky",
        "Bluesky",
        "8a42ef6bc4dd4e0d.png",
        "2709f058378a099d.svg",
        "2709f058378a099d.svg",
    ),
    (
        "bungie",
        "Bungie.net",
        "9b762e8a42709cc4.png",
        "5b0c8422db0e5507.svg",
        "099cd81cf3b4cc98.svg",
    ),
    (
        "skype",
        "Skype",
        "4f2cb7753e5d5e61.png",
        "883a13b030d276ec.svg",
        "883a13b030d276ec.svg",
    ),
    (
        "leagueoflegends",
        "League of Legends",
        "fc30b78cd5e1db04.png",
        "302a27a2e5cc3fb6.svg",
        "302a27a2e5cc3fb6.svg",
    ),
    (
        "steam",
        "Steam",
        "b68f2f4df4af9085.png",
        "dbfa31449965dbd5.svg",
        "1f7ec18f3695d4cf.svg",
    ),
    (
        "reddit",
        "Reddit",
        "afc3f4fa75178004.png",
        "adfd927dcc2049a5.svg",
        "adfd927dcc2049a5.svg",
    ),
    (
        "facebook",
        "Facebook",
        "bda550439e2a1af0.png",
        "17be29f77bee4405.svg",
        "17be29f77bee4405.svg",
    ),
    (
        "twitter_legacy",
        "Twitter",
        "7520545bf8990675.png",
        "0815e2c86f2e0e37.svg",
        "0815e2c86f2e0e37.svg",
    ),
    (
        "twitter",
        "X",
        "90d18081c2eb2ea3.png",
        "5cf814b9693aac47.svg",
        "a61999ae9bfb9658.svg",
    ),
    (
        "spotify",
        "Spotify",
        "40518c6ced9fed93.png",
        "d5719388ffc613da.svg",
        "d5719388ffc613da.svg",
    ),
    (
        "xbox",
        "Xbox",
        "8cea2073e188eabe.png",
        "0133fed7d4dc775c.svg",
        "c4f09fda61827e19.svg",
    ),
    (
        "samsung",
        "Samsung Galaxy",
        "d50e7459b395e5d0.png",
        "4ed35737da17ede2.svg",
        "4ed35737da17ede2.svg",
    ),
    (
        "github",
        "GitHub",
        "1cc92a6d139af44d.png",
        "4db64e0649c64e7e.svg",
        "a35ff3e86ffa1eb2.svg",
    ),
    (
        "playstation",
        "PlayStation Network",
        "ae74d3de4958bf78.png",
        "792da49081c21457.svg",
        "eed203a7ec517e23.svg",
    ),
    (
        "playstation-stg",
        "PlayStation Network (Staging)",
        "46a23515a87620b9.png",
        "eed203a7ec517e23.svg",
        "792da49081c21457.svg",
    ),
    (
        "epicgames",
        "Epic Games",
        "e87d94bf0041b63e.png",
        "4fb7893066c0ef83.svg",
        "199eceff4fca1a0c.svg",
    ),
    (
        "riotgames",
        "Riot Games",
        "fa9e14fc0fd3680f.png",
        "772307b7d902b47b.svg",
        "772307b7d902b47b.svg",
    ),
    (
        "roblox",
        "Roblox",
        "268c81933d752854.png",
        "e04a073ed006173a.svg",
        "a4d8e9b0404a2d00.svg",
    ),
    (
        "paypal",
        "PayPal",
        "81a39a4672d62d60.png",
        "dcb64a4ff8f61b2c.svg",
        "dcb64a4ff8f61b2c.svg",
    ),
    (
        "ebay",
        "eBay",
        "d0f7fa72a2ca6388.png",
        "b28a7a265581b6e3.svg",
        "b28a7a265581b6e3.svg",
    ),
    (
        "tiktok",
        "TikTok",
        "b4002d11fb3b5c1c.png",
        "53641d0496c85aec.svg",
        "a303feb844769624.svg",
    ),
    (
        "instagram",
        "Instagram",
        "ea0692691904fca9.png",
        "c05dded52023ed43.svg",
        "c05dded52023ed43.svg",
    ),
    (
        "letterboxd",
        "Letterboxd",
        "cb1a567ff6191fb5.png",
        "4f8b8634a81979a6.svg",
        "6b6632ec799ccf64.svg",
    ),
    (
        "mastodon",
        "Mastodon",
        "53fcd6d102d44c2b.png",
        "0e6385723fe5cd49.svg",
        "0e6385723fe5cd49.svg",
    ),
    (
        "crunchyroll",
        "Crunchyroll",
        "4e56bb1ecb9b9dc3.png",
        "94ef3e8b7fa85a2e.svg",
        "94ef3e8b7fa85a2e.svg",
    ),
    (
        "domain",
        "Domain",
        "8d54a5919eb645f9.png",
        "10d4972595802406.svg",
        "b4376756bcbbf1ef.svg",
    ),
    (
        "amazon-music",
        "Amazon Music",
        "cda6fb74e92e32f1.png",
        "e3c4aacc1a54395d.svg",
        "e3c4aacc1a54395d.svg",
    ),
    (
        "meta_quest_or_horizon",
        "Meta Quest",
        "9f3c6255a4538cd8.png",
        "d64a395149091d96.svg",
        "ac961aa8d4bb48a5.svg",
    ),
];

/// The Steam item pictures of module 169869 `dy` (modules 455070 and 101554).
const STEAM_DOTA2_ICON: &str = "44e2229fd99bc14e.png";
const STEAM_TF2_ICON: &str = "9eb53b4fa3cc12d5.png";

pub(in crate::ui) fn connection_icon_key(file: &str) -> String {
    format!("connection-icon:{file}")
}

/// `Number(value ?? -1)`: absent is -1, a numeric string its number (empty is 0), anything else NaN.
fn metadata_number(value: &Value) -> f64 {
    match value {
        Value::Null => -1.,
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                0.
            } else {
                text.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}

/// `"1" === value`: the flags are the string "1", a JSON `true` is not one.
fn metadata_flag(value: &Value) -> bool {
    value.as_str() == Some("1")
}

/// `$h` of module 118751: the locale's number below a million, else one decimal and the million abbreviation
/// (`NUMBER_ABBREVIATIONS_MILLION`, "{num}M" in en-US).
fn metadata_count(count: f64) -> String {
    if count < 1e6 {
        dc_intl::number(count)
    } else {
        format!("{:.1}M", count / 1e6)
    }
}

/// The metadata line of one connection, as `renderMetadata` of `t0` (module 927071) picks the builder of module
/// 169869 by platform: Reddit `xE`, Steam `dy`, Letterboxd `uq`, Bluesky / Mastodon / X `ED`, PayPal `gZ`, eBay `ub`,
/// TikTok `HU`. The keys are `pK` of module 783419.
pub(in crate::ui) fn connection_metadata(kind: &str, metadata: &Value) -> Vec<ConnectionMeta> {
    let number = |key: &str| metadata_number(&metadata[key]);
    let flag = |key: &str| metadata_flag(&metadata[key]);
    let tag = |label: &str| ConnectionMeta::Tag {
        label: label.to_owned(),
        paypal: false,
    };
    let mut out = Vec::new();
    match kind {
        "reddit" => {
            let karma = number("total_karma");
            if karma > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "SbCNox",
                    count = karma,
                    value = metadata_count(karma)
                )));
            }
            if flag("gold") {
                out.push(tag(dc_intl::t!("06rDHU")));
            }
            if flag("mod") {
                out.push(tag(dc_intl::t!("oWM95M")));
            }
        }
        "steam" => {
            let games = number("game_count");
            let dota = number("item_count_dota2");
            let tf2 = number("item_count_tf2");
            if games > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "ZatMuX",
                    count = games,
                    value = metadata_count(games)
                )));
            }
            if dota > -1. {
                out.push(ConnectionMeta::Item {
                    label: dc_intl::t!("Y88M5x", count = dota),
                    icon: STEAM_DOTA2_ICON,
                    alt: dc_intl::t("HKUEZo"),
                });
            }
            if tf2 > -1. {
                out.push(ConnectionMeta::Item {
                    label: dc_intl::t!("Y88M5x", count = tf2),
                    icon: STEAM_TF2_ICON,
                    alt: dc_intl::t("C8p1Sh"),
                });
            }
        }
        "letterboxd" => {
            let watches = number("watches");
            let diary = number("films_in_diary_this_year");
            if watches > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "Ck2y4x",
                    count = watches,
                    value = metadata_count(watches)
                )));
            }
            if diary > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "5JpDYR",
                    count = diary,
                    value = metadata_count(diary)
                )));
            }
        }
        "bluesky" | "mastodon" | "twitter" => {
            let posts = number("statuses_count");
            let followers = number("followers_count");
            if posts > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "wJZw02",
                    count = posts,
                    value = metadata_count(posts)
                )));
            }
            if followers > -1. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "grbQMy",
                    count = followers,
                    value = metadata_count(followers)
                )));
            }
        }
        "paypal" => {
            if flag("verified") {
                out.push(ConnectionMeta::Tag {
                    label: dc_intl::t!("IhXLyx").to_owned(),
                    paypal: true,
                });
            }
        }
        "ebay" => {
            let positive = number("positive_feedback_percentage");
            if positive > 0. {
                out.push(ConnectionMeta::Text(dc_intl::t!(
                    "YmL22d",
                    count = positive,
                    value = format!("{}%", metadata_count(positive))
                )));
            }
            if flag("top_rated_seller") {
                out.push(tag(dc_intl::t!("TEEYwa")));
            }
        }
        "tiktok" => {
            let followers = number("follower_count");
            let following = number("following_count");
            let likes = number("likes_count");
            for (count, key) in [
                (followers, "O0/8iD"),
                (following, "ftf12v"),
                (likes, "tEyTTp"),
            ] {
                if count > -1. {
                    out.push(ConnectionMeta::Text(dc_intl::format(
                        key,
                        &[
                            ("count", dc_intl::Arg::from(count)),
                            ("value", dc_intl::Arg::from(metadata_count(count))),
                        ],
                    )));
                }
            }
            if flag("verified") {
                out.push(tag(dc_intl::t!("QHHwRR")));
            }
        }
        _ => {}
    }
    out
}

/// Custom application role-connection metadata (`VW` of module 169869): sorts keys by `application_metadata[key].name`
/// and renders boolean tags (`7`/`8`), ISO dates (`5`/`6`), and integer counters (`1`..=`4`) via `HLoinF`.
pub(in crate::ui) fn application_connection_metadata(
    metadata: &Value,
    app_metadata: &Value,
) -> Vec<ConnectionMeta> {
    let (Some(meta_obj), Some(schema_obj)) = (metadata.as_object(), app_metadata.as_object())
    else {
        return Vec::new();
    };
    if meta_obj.is_empty() || schema_obj.is_empty() {
        return Vec::new();
    }
    let mut keys: Vec<&String> = meta_obj.keys().collect();
    keys.sort_by(|a, b| {
        let na = schema_obj
            .get(*a)
            .and_then(|v| v["name"].as_str())
            .unwrap_or("");
        let nb = schema_obj
            .get(*b)
            .and_then(|v| v["name"].as_str())
            .unwrap_or("");
        na.cmp(nb)
    });
    let mut out = Vec::new();
    for key in keys {
        let Some(schema) = schema_obj.get(key) else {
            continue;
        };
        let name = schema["name"].as_str().unwrap_or(key.as_str());
        let raw_val = meta_obj.get(key);
        let raw_str = raw_val.and_then(Value::as_str).unwrap_or("");
        match schema["type"].as_u64() {
            // `BOOLEAN_EQUAL` (7) / `BOOLEAN_NOT_EQUAL` (8)
            Some(7) => {
                if raw_str == "1" {
                    out.push(ConnectionMeta::Tag {
                        label: name.to_owned(),
                        paypal: false,
                    });
                }
            }
            Some(8) => {
                if raw_str != "1" {
                    out.push(ConnectionMeta::Tag {
                        label: name.to_owned(),
                        paypal: false,
                    });
                }
            }
            // `DATETIME_LESS_THAN_EQUAL` (5) / `DATETIME_GREATER_THAN_EQUAL` (6)
            Some(5 | 6) => {
                if let Some(date) = profile_date(raw_str) {
                    out.push(ConnectionMeta::Text(dc_intl::format(
                        "HLoinF",
                        &[
                            ("value", dc_intl::Arg::Str(std::borrow::Cow::Owned(date))),
                            ("name", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(name))),
                        ],
                    )));
                }
            }
            // `INTEGER_LESS_THAN_EQUAL` (1) .. `INTEGER_NOT_EQUAL` (4)
            Some(1..=4) => {
                let n = metadata_number(raw_val.unwrap_or(&Value::Null));
                if n.is_finite() && n >= 0. {
                    let formatted = metadata_count(n);
                    out.push(ConnectionMeta::Text(dc_intl::format(
                        "HLoinF",
                        &[
                            (
                                "value",
                                dc_intl::Arg::Str(std::borrow::Cow::Owned(formatted)),
                            ),
                            ("name", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(name))),
                        ],
                    )));
                }
            }
            _ => {}
        }
    }
    out
}

/// `getPlatformUserUrl` of module 573648: the profile link a platform builds from the account's name or id.
fn connection_url(kind: &str, id: &str, name: &str) -> Option<String> {
    let name = encode_uri_segment(name);
    let enc_id = encode_uri_segment(id);
    Some(match kind {
        "twitch" => format!("https://www.twitch.tv/{name}"),
        "youtube" => format!("https://www.youtube.com/channel/{enc_id}"),
        "bluesky" => format!("https://bsky.app/profile/{}", enc_id.replace("%3A", ":")),
        "skype" => format!("skype:{enc_id}?userinfo"),
        "steam" => format!("https://steamcommunity.com/profiles/{enc_id}"),
        "reddit" => format!("https://www.reddit.com/u/{name}"),
        "twitter_legacy" => format!("https://twitter.com/{name}"),
        "twitter" => format!("https://x.com/{name}"),
        "spotify" => format!("https://open.spotify.com/user/{enc_id}"),
        "github" => format!("https://github.com/{name}"),
        "roblox" => format!("https://roblox.com/users/{enc_id}/profile"),
        "ebay" => format!("https://www.ebay.com/usr/{name}"),
        "tiktok" => format!("https://www.tiktok.com/@{name}"),
        "instagram" => format!("https://www.instagram.com/{name}"),
        "letterboxd" => format!("https://letterboxd.com/{name}/"),
        "mastodon" => id.to_owned(),
        "domain" => format!("https://{id}/"),
        _ => return None,
    })
}

/// `S` of module 46937: a Bluesky, Mastodon or X account whose `metadata.verified` (`pK.TWITTER_VERIFIED`) is exactly
/// the string `"1"` takes the `PLATFORM_TWITTER` colour and the "Verified on Twitter" tooltip (`Jebrww`).
fn twitter_verified(kind: &str, metadata: &Value) -> bool {
    matches!(kind, "bluesky" | "mastodon" | "twitter") && metadata["verified"].as_str() == Some("1")
}

/// `PLATFORM_TWITTER` of module 646514 (`unsafe_rawColors`, no CSS variable).
const PLATFORM_TWITTER: u32 = 0x1da1f2;

/// The Verified Connection mark of an account (`G` in `S` of module 46937): none unless the account is `verified`;
/// else the tooltip's key, and whether the flower takes the Twitter colour instead of `--icon-muted`.
fn verified_mark(account: &ConnectedAccount) -> Option<(&'static str, bool)> {
    if !account.verified {
        return None;
    }
    Some(if account.twitter_verified {
        ("Jebrww", true)
    } else {
        ("6H6h1r", false)
    })
}

/// The profile's connections as module 851767 keeps them (supported platforms only) and `t0` of module 927071 draws
/// them: the platform, the name with its profile link, "Member since" from `metadata.created_at` and the metadata line.
fn parse_connected_accounts(body: &Value) -> Vec<ConnectedAccount> {
    body["connected_accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let kind = item["type"].as_str()?;
            let &(_, platform_name, icon, light_svg, dark_svg) = CONNECTED_PLATFORMS
                .iter()
                .find(|(known, ..)| *known == kind)?;
            let id = item["id"].as_str().unwrap_or_default();
            let name = item["name"].as_str().unwrap_or_default().to_owned();
            let metadata = &item["metadata"];
            let mut meta_items = connection_metadata(kind, metadata);
            meta_items.extend(application_connection_metadata(
                metadata,
                &item["application_metadata"],
            ));
            Some(ConnectedAccount {
                kind: kind.to_owned(),
                created_at: metadata["created_at"].as_str().and_then(profile_date),
                platform_name,
                icon,
                svg: (light_svg, dark_svg),
                verified: item["verified"] == true,
                twitter_verified: twitter_verified(kind, metadata),
                url: connection_url(kind, id, &name),
                id: id.to_owned(),
                metadata: meta_items,
                name,
            })
        })
        .collect()
}

fn wishlist_is_private(profile_body: Option<&Value>, list: Option<&Value>) -> bool {
    let check_val = |v: &Value| {
        v["visibility"].as_i64() == Some(2)
            || v["visibility"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("private"))
            || v["privacy"].as_i64() == Some(1)
            || v["is_public"] == false
            || v["private"] == true
    };
    if list.is_some_and(check_val) {
        return true;
    }
    profile_body
        .and_then(|b| b["wishlist_settings"].as_object())
        .and_then(|map| map.values().next())
        .is_some_and(check_val)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CustomStatus {
    text: Option<String>,
    emoji_name: Option<String>,
    emoji_id: Option<String>,
    emoji_animated: bool,
}

fn parse_custom_status(activities: &[Value]) -> Option<CustomStatus> {
    let activity = activities
        .iter()
        .find(|entry| entry["type"].as_u64() == Some(4))?;
    let text = activity["state"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let emoji = &activity["emoji"];
    let emoji_id = emoji["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let emoji_name = emoji["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let emoji_animated = emoji["animated"] == true;
    if text.is_none() && emoji_id.is_none() && emoji_name.is_none() {
        return None;
    }
    Some(CustomStatus {
        text,
        emoji_name,
        emoji_id,
        emoji_animated,
    })
}

impl Root {
    /// Capture only: `full-profile <user> [board|activity|wishlist]` opens someone's full profile, on a tab when named.
    #[cfg(feature = "capture")]
    pub(crate) fn script_full_profile(
        &mut self,
        user: Id,
        tab: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let tab = match tab {
            Some("board") => Some(Tab::Board),
            Some("activity") => Some(Tab::Activity),
            Some("wishlist") => Some(Tab::Wishlist),
            Some("friends") => Some(Tab::Friends),
            Some("servers") => Some(Tab::Servers),
            _ => None,
        };
        match tab {
            Some(tab) => self.open_full_profile_tab(user, tab, window, cx),
            None => self.open_full_profile(user, window, cx),
        }
        format!("full-profile: open for {user}")
    }

    /// The profile on one of its tabs: the Mutual Friends and Mutual Servers links open it there.
    pub(super) fn open_full_profile_tab(
        &mut self,
        user: Id,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.own_profile_edits_in_place(user) {
            self.status_popout = None;
            self.profile = None;
            let tab = match tab {
                Tab::Board => super::profile_editor::Tab::Board,
                Tab::Wishlist => super::profile_editor::Tab::Wishlist,
                _ => super::profile_editor::Tab::Activity,
            };
            self.open_profile_editor_for(self.guild, Some(tab), cx);
            return;
        }
        self.open_full_profile(user, window, cx);
        if let Some(open) = self.full_profile.as_mut() {
            open.tab = tab;
            open.explicit = true;
            // The bot modal's Mutual Servers (`MUTUAL_GUILDS`); any other section falls back to its first tab.
            open.bot_tab = if tab == Tab::Servers {
                super::profile_bot::BotTab::Servers
            } else {
                super::profile_bot::BotTab::Info
            };
        }
        cx.notify();
    }

    /// Edit Profile on your own read-only full profile (module 822775): the editor, or in a server the menu of the
    /// per-server and main profiles under the button.
    pub(super) fn full_profile_edit_clicked(
        &mut self,
        guild: Option<Id>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match guild {
            None => {
                self.close_full_profile(cx);
                self.open_profile_editor_for(None, None, cx);
            }
            Some(guild) => {
                let Some(bounds) = self.tip_anchors.borrow().get("full-profile-edit").copied()
                else {
                    return;
                };
                // GAP: the popout's offset from the button is not in a capture; it opens 8px under it, start-aligned.
                let at = gpui::point(bounds.left(), bounds.bottom() + px(8.));
                let _ = window;
                self.open_context_menu(super::context_menu::Target::EditProfileMenu(guild), at, cx);
            }
        }
    }

    /// Module 927071 `eL`: your own profile opens in editing mode (the profile editor) under the
    /// `2026-03-wysiwyg-user-profile-editing` experiment, unless Streamer Mode hides personal information.
    fn own_profile_edits_in_place(&self, user: Id) -> bool {
        self.stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == user)
            && self.stores.experiments.variant(WYSIWYG_EDITING) == Some(1)
            && !self.streamer_hides_personal()
    }

    pub(super) fn open_full_profile(
        &mut self,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status_popout = None;
        self.profile = None;
        if self.own_profile_edits_in_place(user) {
            self.open_profile_editor_for(self.guild, None, cx);
            return;
        }
        let note_palette = self.profile_palette(user, self.guild, false);
        let note = cx.new(|cx| {
            // `input_CJLDJI`: body-sm on the profile's text colour, the placeholder in italics.
            let style = crate::editor::EditorStyle {
                size: 14.,
                line_height: 18.,
                text: note_palette.text,
                placeholder: note_palette.text,
                placeholder_italic: true,
                caret: note_palette.text,
                ..super::editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(dc_intl::t!("VBhOe2").to_owned());
            editor
        });
        let events = cx.subscribe(&note, |this, editor, event: &EditorEvent, cx| {
            match event {
                // Enter keeps the note; Escape leaves the field.
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                    this.save_note(text);
                }
                EditorEvent::Cancel => this.full_profile = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        // `o7`: the tab is chosen once, as the modal mounts, from the tabs the profile then has (`useState`). A profile
        // not read yet has no Board, so the modal opens on Activity and stays there when the Board arrives.
        let other = self.stores.current_user.as_ref().map(|me| me.id) != Some(user);
        let first = initial_tab(self.fetched.get(&format!("profile:{user}")), other);
        self.full_profile = Some(FullProfile {
            user,
            guild: self.guild,
            tab: first,
            explicit: true,
            note,
            saved_note: String::new(),
            board_expanded: std::collections::HashSet::new(),
            bot_tab: super::profile_bot::BotTab::Info,
            restricted_revealed: false,
            _events: events,
        });
        // The bot modal (`e8` of module 148540) has no activity tab, so an app's outbox is not read.
        let app = self
            .stores
            .users
            .get(&user)
            .is_some_and(super::profile_bot::app_bot);
        for (key, route) in [
            (format!("profile:{user}"), profile_route(user, self.guild)),
            (
                format!("outbox:{user}"),
                format!("/content-inventory/users/{user}/outbox"),
            ),
            (format!("note:{user}"), format!("/users/@me/notes/{user}")),
        ] {
            if app && key.starts_with("outbox:") {
                continue;
            }
            let _ = self.requests.send(Request::Fetch { key, route });
        }
        if let Some(known) = self.stores.users.get(&user).cloned() {
            let (image, url) = self.guild_avatar_source_in(&known, self.guild);
            self.request_image(image, url);
            let (global_image, global_url) = avatar_source(&known);
            self.request_image(global_image, global_url);
        }
        if self.stores.current_user.as_ref().map(|u| u.id) == Some(user) {
            self.shop_request("shop:profile", super::shop::data::own_profile_route(user));
        }
        if let Some(body) = self.fetched.get(&format!("profile:{user}")).cloned() {
            if let Some((key, url)) = super::profile_art::banner_source(user, self.guild, &body) {
                self.request_image(key, url);
            }
            self.request_connected_account_icons(&body);
        }
        self.request_profile_images(user);
        self.request_profile_cards(user);
        self.prefetch_known_wishlist(user);
        let _ = window;
        cx.notify();
    }

    /// Not in the pinned client: ShowConnections' compact icons (`plugin_hooks/show_connections.rs`): each connected
    /// account's name, verified mark, address and the platform's SVG of the theme (asked for when missing), from the
    /// fetched profile.
    pub(super) fn connected_account_icons(
        &self,
        user: Id,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> Vec<ConnectionIcon> {
        let Some(body) = self.fetched.get(&format!("profile:{user}")) else {
            return Vec::new();
        };
        parse_connected_accounts(body)
            .into_iter()
            .map(|account| {
                let file = if dark { account.svg.1 } else { account.svg.0 };
                let key = connection_icon_key(file);
                let picture = self.images.get(&key).cloned();
                if picture.is_none() {
                    self.want_image(key, format!("https://discord.com/assets/{file}"), cx);
                }
                (
                    account.kind,
                    account.name,
                    account.verified,
                    account.url,
                    picture,
                )
            })
            .collect()
    }

    /// The platform pictures and the Steam item pictures the Connections section draws.
    fn request_connected_account_icons(&mut self, body: &Value) {
        for account in parse_connected_accounts(body) {
            let items = account.metadata.iter().filter_map(|item| match item {
                ConnectionMeta::Item { icon, .. } => Some(*icon),
                _ => None,
            });
            for file in std::iter::once(account.icon).chain(items) {
                self.request_image(
                    connection_icon_key(file),
                    format!("https://discord.com/assets/{file}"),
                );
            }
        }
    }

    /// Answers this modal waits for: the profile, wishlist, outbox (then its games' icons) and the note.
    pub(super) fn full_profile_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(open) = self.full_profile.as_ref() else {
            return;
        };
        let (user, open_guild) = (open.user, open.guild);
        if key == format!("profile:{user}") {
            let parsed_user = {
                if let Some(body) = self.fetched.get(key)
                    && body["user"].is_object()
                {
                    self.stores.merge_user(&body["user"]).cloned()
                } else {
                    None
                }
            };
            if let Some(guild) = open_guild
                && let Some(body) = self.fetched.get(key)
                && should_apply_guild_member(open_guild, body)
            {
                self.stores
                    .members
                    .apply_member(guild, &body["guild_member"], Some(&body["user"]));
            }
            if let Some(parsed_user) = parsed_user {
                let (image, url) = self.guild_avatar_source_in(&parsed_user, open_guild);
                self.request_image(image, url);
                let (global_image, global_url) = avatar_source(&parsed_user);
                self.request_image(global_image, global_url);
                if let Some(url) = parsed_user.tag.as_ref().and_then(|tag| tag.badge_url()) {
                    self.request_image(url.clone(), url);
                }
            }
            if let Some(body) = self.fetched.get(key).cloned() {
                if let Some((banner_key, banner_url)) =
                    super::profile_art::banner_source(user, open_guild, &body)
                {
                    self.request_image(banner_key, banner_url);
                }
                self.request_connected_account_icons(&body);
            }
            let friends: Vec<dc_state::User> = self.fetched[key]["mutual_friends"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(super::dm_profile::mutual_user)
                .collect();
            for friend in friends {
                let (image, url) = avatar_source(&friend);
                self.request_image(image, url);
                if let Some(url) = friend.tag.as_ref().and_then(|tag| tag.badge_url()) {
                    self.request_image(url.clone(), url);
                }
            }
            self.request_profile_images(user);
            self.request_profile_cards(user);
        } else if super::dm_profile::is_profile_games_key(key, user)
            || key == format!("profile-wishlist:{user}")
        {
            self.request_profile_cards(user);
        } else if key == format!("outbox:{user}") {
            let entries: Vec<Value> = self.fetched[key]["entries"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let games: Vec<String> = entries
                .iter()
                .filter_map(|entry| entry["extra"]["application_id"].as_str().map(str::to_owned))
                .collect();
            // 692969 reads each game's record (`shouldOpenGameProfile`) for the cards' badges and links.
            for game in &games {
                self.request_game_record(game);
            }
            if !games.is_empty() {
                let query: Vec<String> = games
                    .iter()
                    .map(|id| format!("application_ids={id}"))
                    .collect();
                let _ = self.requests.send(Request::Fetch {
                    key: format!("apps:{user}"),
                    route: format!("/applications/public?{}", query.join("&")),
                });
            }
            for entry in &entries {
                if let Some(art) = entry["extra"]["entries"][0]["media"]["image_url"].as_str() {
                    self.request_image(art.to_owned(), art.to_owned());
                }
            }
        } else if key == format!("apps:{user}") {
            let apps: Vec<Value> = self.fetched[key].as_array().cloned().unwrap_or_default();
            for app in apps {
                if let (Some(id), Some(icon)) = (app["id"].as_str(), app["icon"].as_str()) {
                    // 325158 `O`: the application's `getIconURL(iu.LARGE)`.
                    let url = super::cdn::app_icon(id, icon, super::live_activity::ACTIVITY_ART);
                    self.request_image(format!("app-icon:{id}"), url);
                }
            }
        } else if key == format!("note:{user}") {
            // The answer goes through NoteStore, which the field then reads.
            let note = self.fetched[key]["note"].as_str().map(str::to_owned);
            self.stores.notes.set(user, note);
            self.full_profile_note_changed(user, cx);
        }
        cx.notify();
    }

    /// The open profile's note field takes what NoteStore holds for its user (a read's answer, or another session's
    /// `USER_NOTE_UPDATE`).
    pub(super) fn full_profile_note_changed(&mut self, user: Id, cx: &mut Context<Self>) {
        let note = self.stores.notes.text_of(user).to_owned();
        if let Some(open) = self.full_profile.as_mut()
            && open.user == user
        {
            open.saved_note = note.clone();
            open.note.update(cx, |editor, _| editor.set_text(&note));
        }
    }

    /// `PUT /users/@me/notes/{id}` when the note changed.
    fn save_note(&mut self, text: String) {
        let Some(open) = self.full_profile.as_mut() else {
            return;
        };
        if open.saved_note == text {
            return;
        }
        open.saved_note = text.clone();
        self.stores.notes.set(open.user, Some(text.clone()));
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!("/users/@me/notes/{}", open.user),
            body: Some(serde_json::json!({"note": text})),
        });
    }

    pub(super) fn close_full_profile(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.full_profile.as_ref() {
            let text = open.note.read(cx).text().to_owned();
            self.save_note(text);
        }
        self.full_profile = None;
        cx.notify();
    }

    pub(super) fn section_heading(text: impl Into<SharedString>, tint: Hsla) -> gpui::Div {
        div()
            .h(px(16.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(tint)
            .child(text.into())
    }

    pub(super) fn full_profile_member_since(
        &self,
        user: Id,
        guild: Option<Id>,
        strong: Hsla,
        default: Hsla,
        subtle: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let account_date = super::dm_profile::member_since(user);
        let text = |value: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(default)
                .child(value)
        };
        let member = guild.and_then(|guild| {
            self.stores
                .guild(guild)
                .zip(self.stores.members.get(guild, user))
        });
        let Some((guild, member)) = member else {
            return text(account_date).into_any_element();
        };
        let guild_id = guild.id;
        let guild_name = guild.name.clone();
        let guild_acronym = guild.acronym();
        let guild_has_icon = guild.icon.is_some();
        let joined_date = member.joined_at.as_deref().and_then(profile_date);
        let guild_image = self.images.get(&format!("guild:{guild_id}")).cloned();
        let clyde = self.with_tip(
            format!("full-profile-clyde-tip-{user}"),
            super::tooltip::Tip::text(dc_intl::t!("/7xJCF"), super::tooltip::Side::Top).after(300),
            div()
                .id(SharedString::from(format!("full-profile-clyde-{user}")))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/home.svg")
                        .size(px(16.))
                        .text_color(default),
                ),
            cx,
        );
        let guild_badge = self.with_tip(
            format!("full-profile-guild-tip-{user}-{guild_id}"),
            super::tooltip::Tip::body(
                super::tooltip::Body::Text(guild_name.into()),
                super::tooltip::Side::Top,
            )
            .after(300),
            div()
                .id(SharedString::from(format!(
                    "full-profile-guild-icon-{user}-{guild_id}"
                )))
                .size(px(16.))
                .rounded(px(4.))
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .children(guild_image.map(|image| img(image).size_full()))
                .when(!guild_has_icon, |d| {
                    d.text_size(px(8.)).text_color(strong).child(guild_acronym)
                }),
            cx,
        );
        div()
            .parity("full-profile-member-since")
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(clyde)
                    .child(text(account_date)),
            )
            .child(div().size(px(4.)).rounded_full().bg(subtle))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(guild_badge)
                    .children(joined_date.map(text)),
            )
            .into_any_element()
    }

    /// Custom status thought bubble (`680581.js`, `.container_rmCN9_`).
    fn full_profile_status_bubble(
        &self,
        user: Id,
        guild: Option<Id>,
        me: bool,
        palette: &super::dm_profile::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let light_theme = palette.theme.is_some_and(|t| t.light());
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let status = self
            .stores
            .presence
            .get(user, guild)
            .or_else(|| self.stores.presence.get(user, None))
            .and_then(|presence| parse_custom_status(&presence.activities));
        if status.is_none() && !me {
            return None;
        }
        let bubble_bg = palette.token("--background-surface-highest");
        let bubble_border = palette.token(if palette.themed && !light_theme {
            "--border-strong"
        } else {
            "--border-muted"
        });
        let small_dot = div()
            .absolute()
            .left(px(-3.))
            .top(px(-15.))
            .size(px(10.))
            .rounded_full()
            .bg(bubble_bg)
            .border_1()
            .border_color(bubble_border);
        let large_dot = div()
            .absolute()
            .left(px(10.))
            .top(px(-8.))
            .size(px(20.))
            .rounded_full()
            .bg(bubble_bg)
            .border_1()
            .border_color(bubble_border);
        let inner: AnyElement = match status {
            None => div()
                .id(SharedString::from(format!(
                    "full-profile-add-status-{user}"
                )))
                .group("full-profile-add-status")
                .relative()
                .rounded(px(16.))
                .bg(bubble_bg)
                .border_1()
                .border_color(bubble_border)
                .px(px(10.))
                .py(px(6.))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    svg()
                        .path("icons/status_add.svg")
                        .size(px(16.))
                        .text_color(subtle)
                        .group_hover("full-profile-add-status", move |s| s.text_color(strong)),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(subtle)
                        .group_hover("full-profile-add-status", move |s| s.text_color(strong))
                        .child(dc_intl::t!("evw0oz")),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_custom_status(window, cx);
                }))
                .into_any_element(),
            Some(status) => {
                let emoji_only = status.text.is_none();
                let emoji_px = if emoji_only { 32. } else { 16. };
                let emoji_el: Option<AnyElement> = if let Some(eid) = &status.emoji_id {
                    let key = format!("custom:{eid}");
                    let url = if status.emoji_animated {
                        format!(
                            "https://cdn.discordapp.com/emojis/{eid}.webp?size=96&animated=true"
                        )
                    } else {
                        format!("https://cdn.discordapp.com/emojis/{eid}.webp?size=96")
                    };
                    if let Some(pic) = self.images.get(&key).cloned() {
                        Some(img(pic).size(px(emoji_px)).flex_none().into_any_element())
                    } else {
                        self.want_image(key, url, cx);
                        Some(div().size(px(emoji_px)).flex_none().into_any_element())
                    }
                } else if let Some(ename) = &status.emoji_name {
                    if let Some(file) = dc_tokens::emoji_file(ename) {
                        let key = format!("emoji:{ename}");
                        let url = format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN);
                        if let Some(pic) = self.images.get(&key).cloned() {
                            Some(img(pic).size(px(emoji_px)).flex_none().into_any_element())
                        } else {
                            self.want_image(key, url, cx);
                            Some(
                                div()
                                    .flex_none()
                                    .text_size(px(emoji_px * 0.85))
                                    .line_height(px(emoji_px))
                                    .child(ename.clone())
                                    .into_any_element(),
                            )
                        }
                    } else {
                        Some(
                            div()
                                .flex_none()
                                .text_size(px(emoji_px * 0.85))
                                .line_height(px(emoji_px))
                                .child(ename.clone())
                                .into_any_element(),
                        )
                    }
                } else {
                    None
                };
                let body = div()
                    .id(SharedString::from(format!(
                        "full-profile-status-bubble-{user}"
                    )))
                    .relative()
                    .rounded(px(16.))
                    .bg(bubble_bg)
                    .border_1()
                    .border_color(bubble_border)
                    .map(|d| {
                        if emoji_only {
                            d.px(px(12.))
                                .py(px(8.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(emoji_el)
                        } else {
                            let text = status.text.clone().unwrap_or_default();
                            let line = || {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(default)
                            };
                            let has_emoji = emoji_el.is_some();
                            let (first, rest) = if has_emoji {
                                super::profile_board::float_first_line(&text, 155. - 20., |part| {
                                    super::threads::measure_text(window, part, 14., 400.)
                                })
                            } else {
                                (String::new(), text)
                            };
                            d.px(px(12.))
                                .py(px(8.))
                                .flex()
                                .flex_col()
                                .when(has_emoji, |column| {
                                    column.child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .when_some(emoji_el, |row, art| {
                                                row.child(
                                                    div()
                                                        .mr(px(4.))
                                                        .mt(px(1.))
                                                        .flex_none()
                                                        .child(art),
                                                )
                                            })
                                            .when(!first.is_empty(), |row| {
                                                row.child(line().whitespace_nowrap().child(first))
                                            }),
                                    )
                                })
                                .when(!rest.is_empty(), |column| {
                                    column.child(
                                        line()
                                            .max_w(px(155.))
                                            .line_clamp(if has_emoji { 1 } else { 2 })
                                            .text_ellipsis()
                                            .child(rest),
                                    )
                                })
                        }
                    })
                    .when(me, |d| {
                        d.cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_custom_status(window, cx);
                            }))
                    });
                body.into_any_element()
            }
        };
        Some(
            div()
                .parity("full-profile-status-bubble")
                .absolute()
                .top(px(132.))
                .left(px(172.))
                .max_w(px(181.))
                .min_w(px(42.))
                .child(small_dot)
                .child(large_dot)
                .child(inner)
                .into_any_element(),
        )
    }

    /// The Connections section's list (`t5` of module 927071, `container_O2KI4e` / `connectionList_O2KI4e`: a column,
    /// 16px apart): one `t2` row per connection.
    fn full_profile_connections(
        &self,
        user: Id,
        body: Option<&Value>,
        palette: &super::dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let default: Hsla = palette.token("--text-default").into();
        let accounts = parse_connected_accounts(body?);
        if accounts.is_empty() {
            return None;
        }
        let border = palette.token("--border-subtle");
        let tag_fill = palette.token("--background-mod-subtle");
        let rows: Vec<AnyElement> = accounts
            .into_iter()
            .enumerate()
            .map(|(index, account)| {
                // `platformIconContainer_O2KI4e`: 24px, white with a 1px border-subtle edge and 8px corners, the
                // 18px `lightPNG` in it, and the platform's name as its tooltip.
                let picture = self.images.get(&connection_icon_key(account.icon)).cloned();
                let tile = div()
                    .id(SharedString::from(format!(
                        "full-profile-conn-badge-{user}-{index}"
                    )))
                    .size(px(24.))
                    .flex_none()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(color("--white"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(picture.map(|picture| img(picture).size(px(18.))));
                let tile = self.with_tip(
                    format!("full-profile-conn-platform-{user}-{index}"),
                    super::tooltip::Tip::text(account.platform_name, super::tooltip::Side::Top)
                        .after(300),
                    tile,
                    cx,
                );
                // `accountNameText_O2KI4e`: body-sm/normal, one line.
                let name_text = div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(default)
                    .child(account.name.clone());
                // `G` of module 46937: module 355971's 16px badge (`connectedAccountVerifiedIcon_XvCt_s`, 4px after the
                // name) — module 152578's flower in `--icon-muted` (the Twitter colour for a verified X account) with
                // the 16px CheckmarkSmall (module 478016) centred on it in `--interactive-text-active`, and its tooltip.
                // GAP: the flower's colour is not desaturated by the saturation setting (`$k` of module 317097).
                // GAP: no DOM capture of this mark; the 16px check follows 355971's `width`/`height` props, which the
                // 10px `redesignIconChildContainer_d3vyhc` cannot shrink.
                let mark = verified_mark(&account).map(|(tip, twitter)| {
                    let flower: Hsla = if twitter {
                        gpui::rgb(PLATFORM_TWITTER).into()
                    } else {
                        palette.token("--icon-muted").into()
                    };
                    let badge = div()
                        .relative()
                        .size(px(16.))
                        .flex_none()
                        .child(
                            svg()
                                .path("icons/discover_flower.svg")
                                .size(px(16.))
                                .text_color(flower),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    svg()
                                        .path("icons/discover_flower_check.svg")
                                        .size(px(16.))
                                        .flex_none()
                                        .text_color(palette.token("--interactive-text-active")),
                                ),
                        );
                    self.with_tip(
                        format!("full-profile-conn-verified-{user}-{index}"),
                        super::tooltip::Tip::text(dc_intl::t(tip), super::tooltip::Side::Top),
                        badge,
                        cx,
                    )
                    // The mark is not part of the profile link.
                    .on_click(|_, _, cx| cx.stop_propagation())
                });
                // `platformUrlAnchor_O2KI4e` around `accountName_O2KI4e` (2px gap) with the 16px LinkExternalSmall
                // glyph (module 509434), underlined on hover; a platform without a profile link shows the name alone.
                let (account_kind, account_id) = (account.kind.clone(), account.id.clone());
                let name: AnyElement = match account.url.clone() {
                    Some(url) => {
                        let label = format!(
                            "{}, {}, {}",
                            account.platform_name,
                            account.name,
                            dc_intl::t!("q5jLJB")
                        );
                        div()
                            .id(SharedString::from(format!(
                                "full-profile-conn-name-{user}-{index}"
                            )))
                            .min_w(px(0.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .hover(|style| style.underline())
                            .child(name_text)
                            // 4px after the name: the anchor's 2px gap and 2px more.
                            .children(mark.map(|mark| mark.ml(px(2.))))
                            .child(
                                svg()
                                    .path("icons/link_external_small.svg")
                                    .size(px(16.))
                                    .flex_none()
                                    .text_color(default),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // Not in the pinned client: OpenInApp's account view
                                // (`plugin_hooks/open_in_app.rs`).
                                if this.open_in_app_account(&account_kind, &account_id, cx) {
                                    return;
                                }
                                this.follow_link(&url, &label, cx);
                            }))
                            .into_any_element()
                    }
                    None => div()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .child(name_text)
                        .children(mark.map(|mark| mark.ml(px(4.))))
                        .into_any_element(),
                };
                let meta_count = account.metadata.len();
                let meta: Vec<AnyElement> = account
                    .metadata
                    .into_iter()
                    .enumerate()
                    .map(|(at, item)| {
                        // `connectedAccountVanityMetadata_VlBwYX`: footnote/normal (10px in a 12px line), 4px above,
                        // 8px after each but the last.
                        let base = div()
                            .mt(px(4.))
                            .when(at + 1 < meta_count, |this| this.mr(px(8.)))
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .text_color(default);
                        match item {
                            ConnectionMeta::Text(text) => base.child(text),
                            ConnectionMeta::Tag { label, paypal } => base
                                .rounded(px(12.))
                                .px(px(8.))
                                .py(px(1.))
                                .map(|this| {
                                    if paypal {
                                        this.bg(palette.token("--status-positive-background"))
                                            .text_color(color("--white"))
                                    } else {
                                        this.bg(tag_fill)
                                    }
                                })
                                .child(label),
                            ConnectionMeta::Item { label, icon, alt } => base
                                .flex()
                                .items_center()
                                .children(self.images.get(&connection_icon_key(icon)).cloned().map(
                                    |picture| {
                                        let key = format!("conn-meta-{}-{icon}", account.id);
                                        self.with_tip(
                                            key.clone(),
                                            super::tooltip::Tip::text(
                                                alt,
                                                super::tooltip::Side::Top,
                                            ),
                                            div()
                                                .id(SharedString::from(key))
                                                .mr(px(4.))
                                                .child(img(picture).size(px(18.))),
                                            cx,
                                        )
                                    },
                                ))
                                .child(label),
                        }
                        .into_any_element()
                    })
                    .collect();
                // `connection_O2KI4e`: the tile and `connectionInfo_O2KI4e` (a centred column, 4px apart) 8px apart.
                div()
                    .flex()
                    .gap(px(8.))
                    .child(tile)
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(4.))
                            .child(div().flex().flex_col().child(name).when_some(
                                account.created_at,
                                |this, date| {
                                    this.child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(default)
                                            .child(dc_intl::t!("9rfonh", date = date)),
                                    )
                                },
                            ))
                            .when(!meta.is_empty(), |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .items_center()
                                        .overflow_hidden()
                                        .children(meta),
                                )
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .parity("full-profile-connections")
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .children(rows)
                .into_any_element(),
        )
    }

    /// The Board's cards for `widgets` (`nR` of module 720060 in the card of module 40989), as the full profile draws
    /// them; the Remove Widget dialog (module 380035) shows one of them, not interactive.
    pub(super) fn board_widget_cards(
        &self,
        user: Id,
        me: bool,
        widgets: Vec<super::profile_editor::save::UserWidget>,
        palette: &super::dm_profile::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let records = self.profile_game_records(user);
        let game = |id: &str| {
            records
                .iter()
                .copied()
                .find(|game| game["id"].as_str() == Some(id))
        };
        let tile_bg = palette.tile_background(false);
        let visitor = !me;
        let content_width = super::profile_board::FULL_PROFILE_CARD_CONTENT;
        let expanded = self
            .full_profile
            .as_ref()
            .map(|open| open.board_expanded.clone())
            .unwrap_or_default();
        let game_name = |id: &str| {
            game(id)
                .and_then(|item| item["name"].as_str())
                .filter(|name| !name.is_empty())
                .map_or_else(
                    || {
                        let resolved = self.profile_editor_resolve_game(id);
                        if resolved.name == id {
                            dc_intl::t!("GIWFlF").to_owned()
                        } else {
                            resolved.name
                        }
                    },
                    str::to_owned,
                )
        };
        widgets
            .into_iter()
            .filter(|widget| widget.data.is_valid())
            .enumerate()
            .map(|(index, widget)| {
                let title = widget_data_title(&widget.data);
                let key = widget.data.unique_key();
                let open = expanded.contains(&key);
                // An application widget with its config and identity read draws edge to edge in its card.
                let mut embedded = false;
                let content: AnyElement = match &widget.data {
                    // `ns` / `na` of module 720060: game detail cards, Games in Rotation cut to two.
                    WidgetData::FavoriteGames { games } | WidgetData::CurrentGames { games } => {
                        let limit = if matches!(widget.data, WidgetData::CurrentGames { .. }) {
                            super::profile_board::CARD_LIST_LIMIT
                        } else {
                            1
                        };
                        let shown = if open {
                            games.len()
                        } else {
                            games.len().min(limit)
                        };
                        let cards: Vec<AnyElement> = games
                            .iter()
                            .take(shown)
                            .map(|entry| {
                                self.board_game_card(
                                    &key,
                                    entry,
                                    game_name(&entry.game_id),
                                    content_width,
                                    palette,
                                    window,
                                    cx,
                                )
                            })
                            .collect();
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .children(cards)
                            .when(games.len() > limit, |this| {
                                this.child(self.board_show_more(key.clone(), open, palette, cx))
                            })
                            .into_any_element()
                    }
                    // `nI` / `nE`: a cover grid of eight, then Show more.
                    WidgetData::WantToPlayGames { games } | WidgetData::PlayedGames { games } => {
                        let limit = super::profile_board::COVER_GRID_LIMIT;
                        let shown = if open {
                            games.len()
                        } else {
                            games.len().min(limit)
                        };
                        let (_, gap) = super::profile_board::game_grid(content_width);
                        let (cover, _) = super::profile_board::cover_size(content_width);
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().flex().flex_wrap().gap(px(gap)).children(
                                games.iter().take(shown).map(|entry| {
                                    self.board_game_cover(
                                        &entry.game_id,
                                        &game_name(&entry.game_id),
                                        cover,
                                        palette,
                                    )
                                }),
                            ))
                            .when(games.len() > limit, |this| {
                                this.child(self.board_show_more(key.clone(), open, palette, cx))
                            })
                            .into_any_element()
                    }
                    WidgetData::Application { application_id } => {
                        let resolved = self.profile_editor_resolve_game(application_id);
                        let featured = featured_app(application_id);
                        let (app_name, icon_url) = featured.map_or_else(
                            || (resolved.name, resolved.icon),
                            |(name, url)| (name.to_owned(), Some(url)),
                        );
                        let picture = icon_url
                            .as_deref()
                            .and_then(|url| self.images.get(url).cloned())
                            .or_else(|| {
                                self.images
                                    .get(&format!("profile-game:{application_id}"))
                                    .cloned()
                            });
                        if let Some(laid_out) = self.app_widget_body(
                            user,
                            application_id,
                            &app_name,
                            picture.clone(),
                            content_width + 32.,
                            palette,
                        ) {
                            embedded = true;
                            laid_out
                        } else {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .size(px(48.))
                                        .flex_none()
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .bg(tile_bg)
                                        .children(picture.map(|picture| {
                                            img(picture)
                                                .size(px(48.))
                                                .object_fit(gpui::ObjectFit::Cover)
                                        })),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(strong)
                                                .child(app_name),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(subtle)
                                                .child(dc_intl::t!("EKR8Ps")),
                                        ),
                                )
                                .into_any_element()
                        }
                    }
                    WidgetData::Personal { sections, .. } => {
                        let rendered: Vec<AnyElement> = sections
                            .iter()
                            .filter_map(|section| match section {
                                PersonalSection::Cover { title, subtitle }
                                    if !title.trim().is_empty() || !subtitle.trim().is_empty() =>
                                {
                                    Some(
                                        div()
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .bg(tile_bg)
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .when(!title.trim().is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(strong)
                                                        .child(title.clone()),
                                                )
                                            })
                                            .when(!subtitle.trim().is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(default)
                                                        .child(subtitle.clone()),
                                                )
                                            })
                                            .into_any_element(),
                                    )
                                }
                                PersonalSection::Fields { fields } if !fields.is_empty() => Some(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .children(fields.iter().map(|field| {
                                            div()
                                                .p(px(10.))
                                                .rounded(px(8.))
                                                .bg(tile_bg)
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .when(!field.title.trim().is_empty(), |this| {
                                                    this.child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .line_height(px(16.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(subtle)
                                                            .child(field.title.clone()),
                                                    )
                                                })
                                                .when(
                                                    !field.description.trim().is_empty(),
                                                    |this| {
                                                        this.child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .line_height(px(18.))
                                                                .text_color(default)
                                                                .child(field.description.clone()),
                                                        )
                                                    },
                                                )
                                        }))
                                        .into_any_element(),
                                ),
                                _ => None,
                            })
                            .collect();
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(rendered)
                            .into_any_element()
                    }
                    WidgetData::ClipsGallery { clips } => div()
                        .p(px(10.))
                        .rounded(px(8.))
                        .bg(tile_bg)
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(default)
                        .child(dc_intl::t!("pb2Was", numClips = clips.len()))
                        .into_any_element(),
                };
                self.board_widget_frame(
                    user,
                    index,
                    widget.id.as_deref(),
                    if embedded { "".into() } else { title },
                    visitor,
                    embedded,
                    content,
                    palette,
                    cx,
                )
            })
            .collect()
    }

    /// The Board (`a5` of module 927071): themed widget cards populated from the user's saved `widgets`, or the
    /// official empty state (`oqalC+`) on own profile.
    fn full_profile_board(
        &self,
        user: Id,
        me: bool,
        palette: &super::dm_profile::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let widgets = self.board_widget_cards(
            user,
            me,
            self.fetched
                .get(&format!("profile:{user}"))
                .map(parse_widgets)
                .unwrap_or_default(),
            palette,
            window,
            cx,
        );
        if widgets.is_empty() {
            let secondary_bg = palette.token("--control-secondary-background-default");
            let secondary_hover = palette.token("--control-secondary-background-hover");
            let secondary_border = palette.token("--control-secondary-border-default");
            let secondary_fg = palette.token("--control-secondary-text-default");
            return div()
                .parity("full-profile-board-empty")
                .pt(px(64.))
                .pb(px(32.))
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .max_w(px(328.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(strong)
                                .child(dc_intl::t!("oqalC+")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(default)
                                .child(dc_intl::t!("O9SQ1c")),
                        ),
                )
                .when(me, |d| {
                    d.child(
                        div()
                            .id("full-profile-board-browse-widgets")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(secondary_border)
                            .bg(secondary_bg)
                            .hover(move |s| s.bg(secondary_hover))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                svg()
                                    .path("icons/plus_small.svg")
                                    .size(px(16.))
                                    .text_color(secondary_fg),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(secondary_fg)
                                    .child(dc_intl::t!("/NKLK5")),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_full_profile(cx);
                                this.open_profile_editor(cx);
                            })),
                    )
                })
                .into_any_element();
        }
        // `scroller_9onezO`: 12px between widgets.
        div()
            .pt(px(20.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(widgets)
            .into_any_element()
    }

    /// Official `Wishlist` tab (`o9` of module 927071, `699976.js`, `181554`): header row, 3-column `133x133` card grid
    /// (including owned items at `0.5` rest opacity with `"Owned"` checkmark, hover heart toggle & bottom CTA), and
    /// `o6` empty state (`"No hearts yet 💔"`).
    fn full_profile_wishlist(
        &mut self,
        user: Id,
        me: bool,
        palette: &super::dm_profile::Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let profile_body = self.fetched.get(&format!("profile:{user}")).cloned();
        let list = self
            .fetched
            .get(&format!("profile-wishlist:{user}"))
            .cloned()
            .or_else(|| {
                me.then(|| self.fetched.get("shop:wishlistread").cloned())
                    .flatten()
            });
        let entries: Vec<(String, bool)> = match &list {
            Some(list) => super::dm_profile::wishlist_entries(list),
            None if me => self
                .shop
                .wishlist_order
                .iter()
                .map(|sku| (sku.clone(), false))
                .collect(),
            None => Vec::new(),
        };
        let all_skus: Vec<String> = entries.iter().map(|(sku, _)| sku.clone()).collect();
        if !all_skus.is_empty() {
            self.shop_need_products(&all_skus);
            self.editor_wishlist_need(&all_skus);
        }
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let primary_bg = palette.token("--control-primary-background-default");
        let primary_hover = palette.token("--control-primary-background-hover");
        let primary_active = palette.token("--control-primary-background-active");
        let primary_fg = palette.token("--control-primary-text-default");
        let secondary_bg = palette.token("--control-secondary-background-default");
        let secondary_hover = palette.token("--control-secondary-background-hover");
        let secondary_border = palette.token("--control-secondary-border-default");
        let secondary_fg = palette.token("--control-secondary-text-default");
        let border = palette.token("--border-muted");

        if entries.is_empty() {
            return div()
                .parity("full-profile-wishlist-empty")
                .pt(px(64.))
                .pb(px(32.))
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .w(px(328.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(strong)
                                .child(dc_intl::t!("HGnLLT")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(default)
                                .child(dc_intl::t!("/X1ny6")),
                        ),
                )
                .child(
                    div()
                        .id("full-profile-wishlist-explore-shop")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(border)
                        .bg(primary_bg)
                        .hover(move |s| s.bg(primary_hover))
                        .active(move |s| s.bg(primary_active))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(primary_fg)
                        .child(dc_intl::t!("ZbS4QB"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_full_profile(cx);
                            this.open_shop(cx);
                        })),
                )
                .into_any_element();
        }

        let is_private = me && wishlist_is_private(profile_body.as_ref(), list.as_ref());
        let card_bg = palette.token("--user-profile-overlay-background");
        let heart_bg = palette.token("--background-surface-highest");
        let shop_loaded = self.shop.wishlist_id.is_some() || !self.shop.wishlist.is_empty();
        let mut cards = Vec::with_capacity(entries.len());
        for (sku, is_owned) in &entries {
            let sku = sku.clone();
            let is_owned = *is_owned;
            let preview = self.editor_wishlist_preview(&sku, 133., window, cx);
            let wishlisted = if shop_loaded {
                self.shop_is_wishlisted(&sku)
            } else {
                me
            };
            let group_name = SharedString::from(format!("full-profile-wl-grp-{sku}"));
            let heart_tip = if wishlisted {
                dc_intl::t!("19b82d")
            } else {
                dc_intl::t!("Hcgz2S")
            };
            let sku_heart = sku.clone();
            let heart_btn = self.with_tip(
                format!("full-profile-wl-heart-tip-{sku}"),
                super::tooltip::Tip::text(heart_tip, super::tooltip::Side::Top).after(300),
                div()
                    .id(SharedString::from(format!("full-profile-wl-heart-{sku}")))
                    .size(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(heart_bg)
                    .hover(move |s| s.bg(secondary_hover))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(if wishlisted {
                                "icons/shop_heart_card.svg"
                            } else {
                                "icons/shop_heart.svg"
                            })
                            .size(px(16.))
                            .text_color(if wishlisted {
                                color("--status-danger")
                            } else {
                                secondary_fg
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.shop_wishlist_toggle(&sku_heart, cx);
                    })),
                cx,
            );
            let gift_cta = !me && !is_owned;
            let sku_cta = sku.clone();
            let cta_btn = div()
                .id(SharedString::from(format!("full-profile-wl-cta-{sku}")))
                .absolute()
                .left(px(8.))
                .right(px(8.))
                .bottom(px(8.))
                .h(px(32.))
                .rounded(px(8.))
                .bg(primary_bg)
                .hover(move |s| s.bg(primary_hover))
                .active(move |s| s.bg(primary_active))
                .cursor_pointer()
                .opacity(0.)
                .group_hover(group_name.clone(), |s| s.opacity(1.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .when(gift_cta, |d| {
                    d.child(
                        svg()
                            .path("icons/shop_gift.svg")
                            .size(px(14.))
                            .flex_none()
                            .text_color(primary_fg),
                    )
                })
                .child(
                    div()
                        .min_w(px(0.))
                        .text_ellipsis()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(primary_fg)
                        .child(if gift_cta {
                            dc_intl::t!("ilhtIa")
                        } else {
                            dc_intl::t!("FdGl5A")
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if gift_cta {
                        this.shop_open_gift(&sku_cta, cx);
                    } else {
                        this.shop_open_product(&sku_cta, cx);
                    }
                }));
            let sku_card = sku.clone();
            cards.push(
                div()
                    .id(SharedString::from(format!("full-profile-wishlist-{sku}")))
                    .group(group_name.clone())
                    .relative()
                    .size(px(133.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    .bg(card_bg)
                    .cursor_pointer()
                    .child(
                        div()
                            .size_full()
                            .when(is_owned, |d| {
                                d.opacity(0.5)
                                    .group_hover(group_name.clone(), |s| s.opacity(1.))
                            })
                            .children(preview),
                    )
                    .when(is_owned, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .group_hover(group_name.clone(), |s| s.opacity(0.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .size(px(38.))
                                        .rounded_full()
                                        .bg(hsla(0., 0., 0., 0.56))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            svg()
                                                .path("icons/checkmark_large.svg")
                                                .size(px(20.))
                                                .text_color(color("--white")),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(strong)
                                        .child(dc_intl::t!("L5Pt9L")),
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .right(px(8.))
                            .opacity(0.)
                            .group_hover(group_name, |s| s.opacity(1.))
                            .child(heart_btn),
                    )
                    .child(cta_btn)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_product(&sku_card, cx);
                    }))
                    .into_any_element(),
            );
        }

        let header_row = div()
            .parity("full-profile-wishlist-header")
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(subtle)
                    .child(dc_intl::t!("r6Y1Lg", count = entries.len())),
            )
            .child(
                div()
                    .id("full-profile-wishlist-open-shop")
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(secondary_border)
                    .bg(secondary_bg)
                    .hover(move |s| s.bg(secondary_hover))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        svg()
                            .path(if me {
                                "icons/plus_small.svg"
                            } else {
                                "icons/shop.svg"
                            })
                            .size(px(16.))
                            .text_color(secondary_fg),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(secondary_fg)
                            .child(if me {
                                dc_intl::t!("SDUwM0")
                            } else {
                                dc_intl::t!("i/yzHs")
                            }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_full_profile(cx);
                        this.open_shop(cx);
                    })),
            );

        div()
            .parity("full-profile-wishlist")
            .pt(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(is_private, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            svg()
                                .path("icons/eye_slash.svg")
                                .size(px(16.))
                                .text_color(subtle),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(subtle)
                                .child(dc_intl::t!("RX7D9h")),
                        ),
                )
            })
            .child(header_row)
            .child(
                div()
                    .w(px(133. * 3. + 16. * 2.))
                    .grid()
                    .grid_cols(3)
                    .gap(px(16.))
                    .children(cards),
            )
            .into_any_element()
    }

    pub(super) fn mutual_row(
        &self,
        id: SharedString,
        leading: AnyElement,
        name: String,
        tint: Hsla,
        hover_bg: gpui::Rgba,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .p(px(4.))
            .my(px(1.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(leading)
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(tint)
                    .child(name),
            )
    }

    pub(super) fn full_profile_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // `iN` of module 148540: restricted blocked/ignored profile -> `ip`, non-user bot -> `is`, app bot -> `e8`,
        // regular user -> `ig` (`UserProfileModalV2`).
        if let Some(open) = self.full_profile.as_ref() {
            let open_user = open.user;
            let revealed = open.restricted_revealed;
            let is_restricted = !revealed
                && (self.is_blocked(open_user) || self.stores.is_ignored(open_user))
                && !self.restricted_profile_bypass.contains(&open_user);
            if is_restricted {
                return self.restricted_profile_modal(window, cx);
            }
            if let Some(user) = self.stores.users.get(&open_user) {
                if user.non_user_bot() {
                    return self.non_user_bot_modal(window, cx);
                }
                if super::profile_bot::app_bot(user) {
                    return self.bot_profile_modal(window, cx);
                }
            }
        }
        let (open_user, open_guild, open_tab, explicit, note_entity) = {
            let open = self.full_profile.as_ref()?;
            (
                open.user,
                open.guild,
                open.tab,
                open.explicit,
                open.note.clone(),
            )
        };
        let palette = self.profile_palette(open_user, open_guild, false);
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let placeholder: Hsla = palette.token("--input-placeholder-text-default").into();
        note_entity.update(cx, |editor, _| {
            editor.set_style(EditorStyle {
                size: 14.,
                line_height: 18.,
                text: default,
                placeholder,
                placeholder_italic: true,
                caret: default,
                ..super::editor_style()
            });
        });
        let me_id = self.stores.current_user.as_ref().map(|known| known.id);
        let me = me_id == Some(open_user);
        let tab_list = profile_tabs(self.fetched.get(&format!("profile:{open_user}")), !me);
        let tab = if explicit && tab_list.contains(&open_tab) {
            open_tab
        } else {
            tab_list.first().copied().unwrap_or(Tab::Activity)
        };
        let wishlist_panel = (tab == Tab::Wishlist)
            .then(|| self.full_profile_wishlist(open_user, me, &palette, window, cx));
        let open = self.full_profile.as_ref()?;
        let user = self.stores.users.get(&open.user)?.clone();
        let id = user.id;
        let body = self.fetched.get(&format!("profile:{id}"));
        let light_theme = palette.theme.is_some_and(|t| t.light());
        let blur_px = if light_theme { 24. } else { 36. };
        let presence = self.stores.status(id).to_owned();
        let friends: Vec<dc_state::User> = body
            .and_then(|body| body["mutual_friends"].as_array())
            .into_iter()
            .flatten()
            .filter_map(super::dm_profile::mutual_user)
            .collect();
        let mut servers: Vec<(Id, Option<String>)> = body
            .and_then(|body| body["mutual_guilds"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some((
                    entry["id"].as_str()?.parse().ok()?,
                    entry["nick"].as_str().map(str::to_owned),
                ))
            })
            .collect();
        // Module 913453: mutual servers in the order of the server list (`getFlattenedGuildIds`), unknown ones last.
        servers.sort_by_key(|(guild, _)| {
            self.stores
                .guilds
                .iter()
                .position(|known| known.id == *guild)
                .unwrap_or(usize::MAX)
        });
        let (friend_count, server_count) = mutual_counts(body);
        let display_name = open_guild
            .and_then(|guild| self.stores.members.get(guild, id))
            .and_then(|member| member.nick.as_deref())
            .filter(|nick| !nick.is_empty())
            .map_or_else(|| user.display_name().to_owned(), str::to_owned);
        let activity_name = display_name.clone();
        // `p?.private` (module 927071): the notice of module 501193 under the names.
        let private_profile = !me && body.is_some_and(|body| body["private"] == true);
        let private_name = display_name.clone();
        let for_guild = open_guild.is_some_and(|guild| {
            body.is_some_and(|body| {
                body["guild_member_profile"].is_object()
                    && body["guild_member_profile"]["guild_id"]
                        .as_str()
                        .is_none_or(|gid| gid == guild.to_string().as_str())
            })
        });
        let pronouns = body
            .and_then(|body| {
                for_guild
                    .then(|| {
                        body["guild_member_profile"]["pronouns"]
                            .as_str()
                            .filter(|pronouns| !pronouns.is_empty())
                    })
                    .flatten()
                    .or_else(|| body["user_profile"]["pronouns"].as_str())
            })
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let bio = body
            .and_then(|body| {
                for_guild
                    .then(|| {
                        body["guild_member_profile"]["bio"]
                            .as_str()
                            .filter(|bio| !bio.is_empty())
                    })
                    .flatten()
                    .or_else(|| body["user_profile"]["bio"].as_str())
            })
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let bio = super::dm_profile::shown_bio(id, bio);
        let relationship = self.stores.relationships.get(&id).copied();
        let friend_since = (relationship == Some(RELATIONSHIP_FRIEND))
            .then(|| {
                self.stores
                    .relationship_since
                    .get(&id)
                    .and_then(|since| profile_date(since))
            })
            .flatten();
        let banner_img = self.profile_banner_image_in(id, open_guild);
        let status_bubble =
            self.full_profile_status_bubble(id, open_guild, me, &palette, window, cx);
        // Not in the pinned client: ShowConnections with `showOnModal` puts its compact icons in place of the
        // connection cards (`plugin_hooks/show_connections.rs`).
        let connections = self
            .connections_row(id, true, cx)
            .or_else(|| self.full_profile_connections(id, body, &palette, cx));
        let viewport = window.viewport_size();
        let geo =
            full_profile_modal_geometry(f32::from(viewport.width), f32::from(viewport.height));
        let card_bg = if palette.themed {
            palette.top
        } else {
            color("--background-surface-high")
        };

        // `profileHeader_s3DB6Y`: 220px tall (140px banner + 60px avatar overhang + 20px spacer), 8px cutout ring at (24, 72),
        // 120px decorated avatar at (32, 80), and custom status thought bubble at (172, 132).
        let header = div()
            .relative()
            .h(px(220.))
            .child(
                div()
                    .h(px(140.))
                    .rounded_t(px(16.))
                    .overflow_hidden()
                    .relative()
                    .bg(self.profile_banner_fill(&user, open_guild))
                    .children(
                        banner_img
                            .clone()
                            .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .children(self.live_banner_in(id, open_guild, geo.card_width, 140., cx)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(72.))
                    .size(px(136.))
                    .rounded_full()
                    .bg(card_bg)
                    .child(div().size_full().rounded_full().bg(palette.overlay)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(32.))
                    .top(px(80.))
                    .size(px(120.))
                    .child(self.modal_avatar(&user, open_guild, &presence, cx)),
            )
            .children(status_bubble);
        let names = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(strong)
                    .child(super::name_styles::name(
                        format!("full-profile-name-{}", user.id),
                        display_name,
                        self.name_styles(&user, open_guild).as_ref(),
                        super::name_styles::Display::Animated,
                    )),
            )
            .child(
                div()
                    .min_h(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(default)
                    .child(user.username.clone())
                    .when_some(pronouns, |this, pronouns| {
                        this.child(div().size(px(4.)).rounded_full().bg(default))
                            .child(pronouns)
                    })
                    .children(user.tag.as_ref().map(|tag| {
                        self.profile_server_tag_button(
                            tag,
                            &palette,
                            format!("full-profile-tag-{id}"),
                            cx,
                        )
                    }))
                    .children(self.badge_pill("full-profile", id, &palette, cx)),
            );
        let secondary_bg = palette.token("--control-secondary-background-default");
        let secondary_hover = palette.token("--control-secondary-background-hover");
        let secondary_active = palette.token("--control-secondary-background-active");
        let secondary_border = palette.token("--control-secondary-border-default");
        let secondary_fg = palette.token("--control-secondary-text-default");
        let primary_bg = palette.token("--control-primary-background-default");
        let primary_hover = palette.token("--control-primary-background-hover");
        let primary_active = palette.token("--control-primary-background-active");
        let primary_fg = palette.token("--control-primary-text-default");
        let border_muted = palette.token("--border-muted");
        let themed = palette.themed;
        let secondary_button = |key: &'static str, glyph: &'static str| {
            div()
                .id(key)
                .relative()
                .size(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(secondary_border)
                .bg(secondary_bg)
                .when(themed, |d| d.backdrop_blur(px(blur_px)))
                .hover(move |style| style.bg(secondary_hover))
                .active(move |style| style.bg(secondary_active))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(16.))
                        .text_color(secondary_fg),
                )
        };
        let primary = match relationship {
            None | Some(0) if !me => Some(
                div()
                    .id("full-profile-add-friend")
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border_muted)
                    .bg(primary_bg)
                    .hover(move |style| style.bg(primary_hover))
                    .active(move |style| style.bg(primary_active))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        svg()
                            .path("icons/user_plus.svg")
                            .size(px(16.))
                            .text_color(primary_fg),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(primary_fg)
                            .child(dc_intl::t!("w5uwoI")),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(id, Some(0), cx)),
                    )
                    .into_any_element(),
            ),
            Some(RELATIONSHIP_FRIEND | RELATIONSHIP_INCOMING | RELATIONSHIP_OUTGOING) => Some(
                div()
                    .relative()
                    .child(self.anchor_probe("full-profile-friend"))
                    .child(
                        secondary_button(
                            "full-profile-friend",
                            if relationship == Some(RELATIONSHIP_FRIEND) {
                                "user_check"
                            } else {
                                "user_clock"
                            },
                        )
                        .when(
                            relationship != Some(RELATIONSHIP_OUTGOING),
                            |this| {
                                this.on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_profile_menu_at(
                                        id,
                                        Which::Friend,
                                        "full-profile-friend",
                                        true,
                                        cx,
                                    );
                                }))
                            },
                        ),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };
        let buttons = div()
            .parity("full-profile-buttons")
            .h(px(32.))
            .flex()
            .gap(px(8.))
            .when(relationship == Some(RELATIONSHIP_FRIEND) && !me, |d| {
                d.child(
                    div()
                        .id("full-profile-message-primary")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(border_muted)
                        .bg(primary_bg)
                        .hover(move |style| style.bg(primary_hover))
                        .active(move |style| style.bg(primary_active))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            svg()
                                .path("icons/chat_bubble.svg")
                                .size(px(16.))
                                .text_color(primary_fg),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(primary_fg)
                                .child(dc_intl::t!("BAB0yK")),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_dm_with(id, cx);
                        })),
                )
            })
            // `tG` of module 927071 on your own profile (read-only, no `UserProfileModalV2` editing): Edit Profile
            // (module 822775: primary with the pencil; in a server a menu of the per-server and main profiles), then
            // `tw`, the Shop icon button with its "Shop" tooltip, then More.
            .when(me, |this| {
                let edit_guild = open_guild;
                this.child(
                    div()
                        .relative()
                        .child(self.anchor_probe("full-profile-edit"))
                        .child(
                            div()
                                .id("full-profile-edit")
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(border_muted)
                                .bg(primary_bg)
                                .hover(move |style| style.bg(primary_hover))
                                .active(move |style| style.bg(primary_active))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    svg()
                                        .path("icons/clips_pencil.svg")
                                        .size(px(16.))
                                        .text_color(primary_fg),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(primary_fg)
                                        .child(dc_intl::t!("s5vZlQ")),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.full_profile_edit_clicked(edit_guild, window, cx);
                                })),
                        ),
                )
                .child(self.with_tip(
                    "full-profile-shop-tip",
                    super::tooltip::Tip::text(dc_intl::t!("b2d0N0"), super::tooltip::Side::Top),
                    secondary_button("full-profile-shop", "shop").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_shop(cx);
                        },
                    )),
                    cx,
                ))
            })
            .children(primary)
            .when(!me && relationship != Some(RELATIONSHIP_FRIEND), |this| {
                this.child(
                    secondary_button("full-profile-message", "chat_bubble").on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.full_profile = None;
                            this.open_dm_with(id, cx);
                        },
                    )),
                )
            })
            .child(
                div()
                    .relative()
                    .child(self.anchor_probe("full-profile-more"))
                    .child(
                        secondary_button("full-profile-more", "more_horizontal").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.open_profile_menu_at(
                                    id,
                                    Which::Overflow,
                                    "full-profile-more",
                                    true,
                                    cx,
                                );
                            }),
                        ),
                    ),
            );
        // Module 560296 (`eV.A` after the buttons on your own profile): "Amp up your profile" until dismissed.
        let upsell = (me && self.profile_upsell_shown()).then(|| {
            super::profile::upsell_card_in_modal(self.premium().is_subscriber(), cx)
                .into_any_element()
        });
        // `hidePersonalInformation` (Streamer Mode) hides the bio, connections and apps (`!en` in `u9`).
        let streamer = self.streamer_hides_personal();
        let section = |heading: Option<&'static str>| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(heading.map(|heading| Self::section_heading(heading, subtle)))
        };
        let note_hover = palette.token("--user-profile-background-hover");
        let note_field = div()
            .ml(px(-7.))
            .mt(px(-4.))
            .px(px(7.))
            .py(px(1.))
            .rounded(px(8.))
            .hover(move |style| style.bg(note_hover))
            .child(open.note.clone());
        // `<ProfileEffects>` is the last child of `main.profile` (module 927071, `u8`): the identity column.
        let effect = self.profile_effect_sku(id).and_then(|sku| {
            let hovering = self.profile_effect_hovered("full-profile");
            self.profile_effect_layer(
                "full-profile",
                sku,
                super::profile_effects::Surface::profile(geo.card_width, hovering),
                window,
                cx,
            )
        });
        let identity = div()
            .id("full-profile-identity")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("full-profile", *inside, cx);
            }))
            .relative()
            .w(px(geo.card_width))
            .h_full()
            .flex_none()
            .rounded_t(px(16.))
            .overflow_hidden()
            // `profile_s3DB6Y` has no border, only its shadow (design/dom/profile/full-loreanxavier: the avatar at
            // 240,160 is the card's 208,80 plus 32 and 80).
            .bg(card_bg)
            .children(palette.card_backdrop(140.))
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id("full-profile-body")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .mt(px(-4.))
                            .pt(px(4.))
                            .pl(px(32.))
                            // `profileBody` pads 24 and keeps a stable 8px gutter for its thin scrollbar.
                            .pr(px(32.))
                            .pb(px(32.))
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            .child(names)
                            .when(private_profile, |this| {
                                this.child(super::profile_board::private_notice(
                                    &private_name,
                                    &palette,
                                ))
                            })
                            .child(buttons)
                            .children(upsell)
                            .when_some(bio.filter(|_| !streamer), |this, bio| {
                                this.child(section(None).child(self.bio(&bio, default, cx)))
                            })
                            .child(
                                section(Some(if user.bot {
                                    dc_intl::t!("A//N4k")
                                } else {
                                    dc_intl::t!("a6XYD9")
                                }))
                                .child(
                                    self.full_profile_member_since(
                                        id, open_guild, strong, default, subtle, cx,
                                    ),
                                ),
                            )
                            // Not in the pinned client: VoiceStats' "Voice Time" (`plugin_hooks/voice_stats.rs`).
                            .children(self.voice_time_section(id, false, strong, default))
                            .when_some(friend_since, |d, date| {
                                d.child(
                                    section(Some(dc_intl::t!("wlTO8v"))).child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(default)
                                            .child(date),
                                    ),
                                )
                            })
                            .when_some(
                                open_guild.and_then(|guild| {
                                    self.profile_role_chips_for(guild, id, default)
                                }),
                                |d, roles| {
                                    d.child(
                                        section(Some(dc_intl::t!("2SZsWX")))
                                            .parity("full-profile-roles")
                                            .gap(px(4.))
                                            .child(roles),
                                    )
                                },
                            )
                            .when_some(connections.filter(|_| !streamer), |d, conns| {
                                d.child(section(Some(dc_intl::t!("3fe7U5"))).child(conns))
                            })
                            // `ry`: the note shows on every profile, your own included.
                            .child(section(Some(dc_intl::t!("mQKv+v"))).child(note_field)),
                    ),
            )
            .children(effect);
        // `top_6zZYhF`: the tabs over a 1px line, 24px apart, the chosen one underlined.
        let tab_item = |which: Tab, label: String| {
            let selected = which == tab;
            div()
                .id(SharedString::from(format!("full-profile-tab-{label}")))
                .relative()
                .h(px(30.))
                .mb(px(-1.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(if selected { strong } else { subtle })
                .child(label)
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(2.))
                            .bg(strong),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.full_profile.as_mut() {
                        open.tab = which;
                        open.explicit = true;
                    }
                    cx.notify();
                }))
        };
        let tabs = div()
            .h(px(30.))
            .border_b_1()
            .border_color(palette.token("--border-subtle"))
            .flex()
            .gap(px(24.))
            .when(tab_list.contains(&Tab::Board), |this| {
                this.child(tab_item(Tab::Board, dc_intl::t!("laViwx").to_owned()))
            })
            .child(tab_item(Tab::Activity, dc_intl::t!("chq59f").to_owned()))
            .when(tab_list.contains(&Tab::Wishlist), |this| {
                this.child(tab_item(Tab::Wishlist, dc_intl::t!("7lZ31J").to_owned()))
            })
            // `e3`: both mutual tabs for anyone else, labelled by their counts (modules 837531, 186272).
            .when(tab_list.contains(&Tab::Friends), |this| {
                this.child(tab_item(Tab::Friends, mutual_friends_label(friend_count)))
            })
            .when(tab_list.contains(&Tab::Servers), |this| {
                this.child(tab_item(Tab::Servers, mutual_servers_label(server_count)))
            });
        // `sn`: the empty tab's heading (`heading-sm/medium` `text-strong`) over one line (`body-sm/normal`), centred,
        // at most 320px wide (`empty_7q_A1n`, `textContainer_7q_A1n`), in the panel scroller's 20px top padding
        // (`tabPanelScroller_y4ZEM5`) like every other tab.
        let mutual_empty = |servers: bool| {
            let (heading, line) = mutual_empty_text(servers, id);
            div()
                .size_full()
                .pt(px(20.))
                .min_h(px(240.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
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
                .into_any_element()
        };
        let row_hover = palette.token("--user-profile-background-hover");
        let dot_cutout = palette.theme.map_or("--background-base-lower", |t| {
            if t.light() { "--white" } else { "--black" }
        });
        let tile_bg = palette.tile_background(false);
        let panel: AnyElement = match tab {
            Tab::Board => self.full_profile_board(id, me, &palette, window, cx),
            Tab::Wishlist => wishlist_panel.unwrap_or_else(|| div().into_any_element()),
            Tab::Activity => {
                let outbox = self.fetched.get(&format!("outbox:{id}")).cloned();
                self.profile_activity_panel(
                    id,
                    me,
                    outbox.as_ref(),
                    &activity_name,
                    &palette,
                    20.,
                    cx,
                )
            }
            Tab::Friends if friend_count == Some(0) => mutual_empty(false),
            Tab::Servers if server_count == Some(0) => mutual_empty(true),
            Tab::Friends => div()
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(friends.into_iter().map(|friend| {
                    let (avatar_key, avatar_url) = avatar_source(&friend);
                    let picture = self.images.get(&avatar_key).cloned();
                    if picture.is_none() {
                        self.want_image(avatar_key, avatar_url, cx);
                    }
                    let status = self
                        .stores
                        .presences
                        .get(&friend.id)
                        .cloned()
                        .unwrap_or_else(|| "offline".to_owned());
                    let leading = div()
                        .relative()
                        .size(px(40.))
                        .children(picture.map(|picture| img(picture).size(px(40.)).rounded_full()))
                        .child(super::status_dot_40(&status, dot_cutout))
                        .into_any_element();
                    let friend_id = friend.id;
                    let tag = friend.tag.clone();
                    // Module 503062: "{name}, {presence}, view profile" (no presence for a bot that is not a user).
                    let label = mutual_friend_label(
                        friend.display_name(),
                        (!friend.bot)
                            .then(|| status_label(&status, false))
                            .flatten(),
                    );
                    self.mutual_row(
                        SharedString::from(format!("full-profile-friend-{friend_id}")),
                        leading,
                        friend.display_name().to_owned(),
                        strong,
                        row_hover,
                    )
                    .aria_label(label)
                    .children(
                        tag.as_ref()
                            .map(|tag| self.profile_server_tag_pill(tag, &palette)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_full_profile(friend_id, window, cx);
                    }))
                    .into_any_element()
                }))
                .into_any_element(),
            Tab::Servers => div()
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(servers.into_iter().filter_map(|(guild, nick)| {
                    let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
                    let name = known.name.clone();
                    // Module 51943: the row reads "{guildName}, go to server".
                    let label = dc_intl::t!("Ce2iXp", guildName = name.clone()).to_string();
                    let picture = self.images.get(&format!("guild:{guild}")).cloned();
                    let leading = div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(tile_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(default)
                        .when_some(picture, |this, picture| {
                            this.child(img(picture).size(px(40.)).rounded(px(12.)))
                        })
                        .when(known.icon.is_none(), |this| this.child(known.acronym()))
                        .into_any_element();
                    Some(
                        self.mutual_row(
                            SharedString::from(format!("full-profile-server-{guild}")),
                            leading,
                            name,
                            strong,
                            row_hover,
                        )
                        .aria_label(label)
                        .children(nick.map(|nick| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(subtle)
                                .child(nick)
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.full_profile = None;
                            this.select(guild, None);
                            cx.notify();
                        }))
                        .into_any_element(),
                    )
                }))
                .into_any_element(),
        };
        let columns = div()
            .w_full()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .child(identity)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pt(px(20.))
                    .pl(px(32.))
                    .pr(px(16.))
                    .flex()
                    .flex_col()
                    .child(tabs)
                    .child(
                        // `tabBarPanel_y4ZEM5` starts 2px above the tab line's bottom.
                        div()
                            .id("full-profile-panel")
                            .mt(px(-2.))
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .pb(px(16.))
                            .child(panel),
                    ),
            );
        let modal_bg = palette
            .theme
            .map_or_else(|| color("--background-base-lower"), |t| t.modal_background);
        let modal_fade_end = palette.theme.map_or(modal_bg, |t| {
            super::profile_theme::blend_over(t.modal_background, t.modal_inner_stops().1)
        });
        let top_banner_backdrop =
            banner_img.map(|image| {
                // `.backgroundImage_s3DB6Y` (927071.js:7177-7184): top 280px blurred banner backdrop with vertical fade.
                div()
                    .parity("full-profile-banner-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(280.))
                    .overflow_hidden()
                    .child(
                        img(image)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .opacity(if light_theme { 0.4 } else { 0.3 }),
                    )
                    .child(div().absolute().inset_0().backdrop_blur(px(15.)).bg(
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(
                                Rgba {
                                    a: 0.,
                                    ..modal_fade_end
                                },
                                0.,
                            ),
                            gpui::linear_color_stop(modal_fade_end, 1.),
                        ),
                    ))
            });
        let top_band = geo
            .sys_pad
            .max(if private_profile { PRIVATE_BANNER } else { 0. });
        // The frame of module 744808 over the whole modal (`profileContentOuter`), hidden in windows up to 785px tall.
        let (frame_back, frame_front) = self.profile_frame_layers(
            id,
            geo.width,
            Some(geo.height),
            super::profile_frames::FrameSurface::Modal,
            f32::from(viewport.height),
        );
        let modal = div()
            .parity("full-profile")
            .id("full-profile")
            .occlude()
            .relative()
            .w(px(geo.width))
            .h(px(geo.height))
            .rounded(px(geo.radius))
            .overflow_hidden()
            // Filling the window's height (below 785px) the official modal shows its edge on the sides only: a line at
            // x 159 and 1120 of a 1280x720 window, none at the top, so the card starts at y 80, not 81.
            .map(|modal| {
                if geo.radius == 0. && geo.sys_pad > 0. {
                    modal.border_l_1().border_r_1()
                } else {
                    modal.border_1()
                }
            })
            .border_color(border_muted)
            .bg(modal_bg)
            .children(palette.modal_backdrop())
            .children(top_banner_backdrop)
            .children(frame_back)
            .on_click(|_, _, cx| cx.stop_propagation())
            // anner_B_p0HC in the modal (module 312381): a 48px band across the top, the lock (xs) and Private
            // Profile in ody-sm/semibold, 4px apart; the content and the close button move below it
            // (hasPrivateBanner_ZhNJfE, max(system bar, 48px)).
            .when(private_profile, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(PRIVATE_BANNER))
                        // `.custom-user-profile-theme.user-profile-modal-v2 .banner_B_p0HC`: on a themed profile the
                        // band is the overlay mixed into the primary colour, 65% at the top to 60% at the bottom.
                        .map(|band| match palette.theme.as_ref() {
                            Some(theme) => {
                                let (top, bottom) = theme.private_band_stops();
                                band.bg(gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(top, 0.),
                                    gpui::linear_color_stop(bottom, 1.),
                                ))
                            }
                            None => band.bg(palette.token("--background-surface-higher")),
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_color(palette.token("--text-default"))
                        .child(
                            svg()
                                .path("icons/access_lock.svg")
                                .size(px(16.))
                                .text_color(palette.token("--text-default")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .child(dc_intl::t!("KPnd2O")),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .pt(px(geo.pad + top_band))
                    .pl(px(geo.pad))
                    .pr(px(geo.pad - 16.))
                    .child(columns),
            )
            .child(
                div()
                    .id("full-profile-close")
                    .absolute()
                    .top(px(geo.close_top - geo.sys_pad + top_band))
                    .right(px(8.))
                    .size(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(secondary_border)
                    .bg(secondary_bg)
                    .when(themed, |d| d.backdrop_blur(px(blur_px)))
                    .hover(move |style| style.bg(secondary_hover))
                    .active(move |style| style.bg(secondary_active))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/modal_close.svg")
                            .size(px(16.))
                            .text_color(secondary_fg),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx))),
            )
            .children(frame_front);
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose =
            crate::ui::motion::legacy_modal_pose("full-profile", "full-profile-layer", false);
        Some(
            deferred(
                div()
                    .id("full-profile-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_full_profile(cx)))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// --custom-modal-v2-private-banner-height with hasPrivateBanner_ZhNJfE (--space-48).
const PRIVATE_BANNER: f32 = 48.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ModalGeometry {
    width: f32,
    height: f32,
    card_width: f32,
    pad: f32,
    sys_pad: f32,
    close_top: f32,
    radius: f32,
}

/// `.root_ZhNJfE` / `.outer_ZhNJfE` responsive modal bounds (`f4f30bf79660bbe0.css`).
fn full_profile_modal_geometry(vw: f32, vh: f32) -> ModalGeometry {
    let wide = vw >= 929.0;
    let tall = vh >= 785.0;
    let width = if wide { (vw - 96.0).min(962.0) } else { vw };
    let height = if tall {
        (vh - 96.0).clamp(550.0, 800.0)
    } else {
        vh
    };
    let card_width = if wide {
        (0.455 * vw - 48.0 * 0.91).clamp(300.0, 400.0)
    } else {
        (0.455 * vw).clamp(300.0, 400.0)
    };
    let pad = if vw >= 1025.0 { 48.0 } else { 32.0 };
    let sys_pad = if tall { 0.0 } else { 32.0 };
    let close_top = 8.0 + sys_pad;
    let radius = if wide && tall { 16.0 } else { 0.0 };
    ModalGeometry {
        width,
        height,
        card_width,
        pad,
        sys_pad,
        close_top,
        radius,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionMeta, STEAM_DOTA2_ICON, STEAM_TF2_ICON, Tab, activity_empty_text,
        connection_metadata, full_profile_modal_geometry, initial_tab, mutual_counts,
        mutual_empty_text, mutual_friend_label, mutual_friends_label, mutual_servers_label,
        parse_connected_accounts, parse_custom_status, profile_date, profile_route, profile_tabs,
        should_apply_guild_member, status_label, wishlist_is_private,
    };

    /// ShowConnections reads the theme's SVG and `verified` from the same table and parser as the profile.
    #[test]
    fn every_platform_has_both_svgs_and_verified_is_kept() {
        for &(kind, _, png, light, dark) in super::CONNECTED_PLATFORMS {
            assert!(png.ends_with(".png"), "{kind}");
            assert!(light.ends_with(".svg") && dark.ends_with(".svg"), "{kind}");
        }
        let body = serde_json::json!({"connected_accounts": [
            {"type": "github", "id": "1", "name": "octocat", "verified": true},
            {"type": "steam", "id": "2", "name": "s", "verified": false}
        ]});
        let parsed = parse_connected_accounts(&body);
        assert_eq!(
            parsed
                .iter()
                .map(|a| (a.verified, a.svg))
                .collect::<Vec<_>>(),
            [
                (true, ("4db64e0649c64e7e.svg", "a35ff3e86ffa1eb2.svg")),
                (false, ("dbfa31449965dbd5.svg", "1f7ec18f3695d4cf.svg")),
            ]
        );
    }

    /// Module 46937: only a verified account has the mark; a Bluesky, Mastodon or X account with `metadata.verified`
    /// exactly `"1"` takes the Twitter colour and "Verified on Twitter".
    #[test]
    fn the_verified_connection_mark_follows_module_46937() {
        let body = serde_json::json!({"connected_accounts": [
            {"type": "github", "id": "1", "name": "octocat", "verified": true},
            {"type": "steam", "id": "2", "name": "s", "verified": false},
            {"type": "twitter", "id": "3", "name": "x", "verified": true, "metadata": {"verified": "1"}},
            {"type": "bluesky", "id": "did:plc:a", "name": "b", "verified": true, "metadata": {"verified": "1"}},
            {"type": "mastodon", "id": "https://m.example/@m", "name": "m", "verified": true, "metadata": {"verified": 1}},
            {"type": "twitter", "id": "4", "name": "y", "verified": false, "metadata": {"verified": "1"}},
            {"type": "github", "id": "5", "name": "g", "verified": true, "metadata": {"verified": "1"}}
        ]});
        let marks: Vec<_> = parse_connected_accounts(&body)
            .iter()
            .map(super::verified_mark)
            .collect();
        assert_eq!(
            marks,
            [
                Some(("6H6h1r", false)),
                None,
                Some(("Jebrww", true)),
                Some(("Jebrww", true)),
                Some(("6H6h1r", false)),
                None,
                Some(("6H6h1r", false)),
            ]
        );
        assert_eq!(dc_intl::t("6H6h1r"), "Verified Connection");
        assert_eq!(dc_intl::t("Jebrww"), "Verified on Twitter");
    }
    use serde_json::json;

    #[test]
    fn board_and_wishlist_tabs_always_show_on_own_profile_and_respect_content_on_others() {
        let app_widget = json!({
            "widgets": [{"id": "1", "data": {"type": "application", "application_id": "1346069614634864772"}}]
        });
        assert_eq!(
            profile_tabs(Some(&app_widget), false),
            vec![Tab::Board, Tab::Activity, Tab::Wishlist]
        );

        let empty_fav = json!({
            "widgets": [{"id": "1", "data": {"type": "favorite_games", "games": []}}]
        });
        assert_eq!(
            profile_tabs(Some(&empty_fav), false),
            vec![Tab::Board, Tab::Activity, Tab::Wishlist]
        );
        assert!(!profile_tabs(Some(&empty_fav), true).contains(&Tab::Board));
        assert!(!profile_tabs(Some(&empty_fav), true).contains(&Tab::Wishlist));

        let valid_fav_and_wishlist = json!({
            "user_profile": {"bio": "", "pronouns": "", "banner": null, "accent_color": null},
            "widgets": [{"id": "1", "data": {"type": "favorite_games", "games": [{"game_id": "1001"}]}}],
            "wishlist_settings": {"wl-1": {"visibility": 1}}
        });
        assert_eq!(
            profile_tabs(Some(&valid_fav_and_wishlist), true),
            vec![
                Tab::Board,
                Tab::Activity,
                Tab::Wishlist,
                Tab::Friends,
                Tab::Servers
            ]
        );
    }

    #[test]
    fn mutual_tabs_show_for_others_with_their_counts_and_hide_on_a_stub_profile() {
        let none = json!({
            "user": {"flags": 0, "public_flags": 0},
            "user_profile": {"bio": "", "pronouns": "", "banner": null, "accent_color": null},
            "badges": [], "mutual_friends": [], "mutual_guilds": [{"id": "1"}, {"id": "2"}]
        });
        assert!(profile_tabs(Some(&none), true).contains(&Tab::Friends));
        assert_eq!(mutual_counts(Some(&none)), (Some(0), Some(2)));
        assert_eq!(mutual_counts(None), (None, None));
        assert_eq!(mutual_friends_label(Some(0)), "No Mutual Friends");
        assert_eq!(
            mutual_friend_label("Wren", status_label("dnd", false)),
            "Wren, Do Not Disturb, view profile"
        );
        assert_eq!(mutual_friend_label("Bot", None), "Bot, view profile");
        assert_eq!(status_label("online", true), Some("Online via mobile"));
        assert_eq!(mutual_servers_label(Some(0)), "No Mutual Servers");
        assert_eq!(mutual_servers_label(Some(2)), "2 Mutual Servers");
        assert_eq!(mutual_friends_label(None), "Mutual Friends");
        let counted = json!({"mutual_friends_count": 3, "mutual_friends": []});
        assert_eq!(mutual_counts(Some(&counted)).0, Some(3));
        // No banner or accent colour key at all, nothing else either: module 776879's empty profile.
        let stub = json!({"user": {"flags": 0, "public_flags": 0}, "user_profile": {"bio": "", "pronouns": ""}});
        assert!(!profile_tabs(Some(&stub), true).contains(&Tab::Friends));
        // The modal mounts on the first tab it has then: Activity before the profile is read, the Board after.
        assert_eq!(initial_tab(None, true), Tab::Activity);
        assert_eq!(initial_tab(None, false), Tab::Board);
        let board = json!({
            "user_profile": {"bio": "", "banner": null},
            "widgets": [{"id": "1", "data": {"type": "favorite_games", "games": [{"game_id": "1"}]}}]
        });
        assert_eq!(initial_tab(Some(&board), true), Tab::Board);
        let (heading, line) = activity_empty_text("Ann", 2);
        assert_eq!(heading, "Ann doesn't have any activity to share here");
        assert_eq!(
            line,
            "Ann keeps their profile simple - ask what they're into"
        );
        let (heading, line) = mutual_empty_text(true, 7);
        assert!(!heading.is_empty() && !line.is_empty());
        assert_eq!(mutual_empty_text(false, 7), mutual_empty_text(false, 12));
    }

    #[test]
    fn guild_member_merges_when_per_server_profile_is_null_or_matches_open_guild() {
        let without_server_profile = json!({
            "guild_member_profile": null,
            "guild_member": {"roles": ["10", "20"], "joined_at": "2022-01-01T00:00:00Z"}
        });
        assert!(should_apply_guild_member(Some(99), &without_server_profile));
        assert!(!should_apply_guild_member(None, &without_server_profile));

        let matching_server_profile = json!({
            "guild_member_profile": {"guild_id": "99", "bio": "server bio"},
            "guild_member": {"roles": ["10"]}
        });
        assert!(should_apply_guild_member(
            Some(99),
            &matching_server_profile
        ));
        assert!(!should_apply_guild_member(
            Some(100),
            &matching_server_profile
        ));
    }

    #[test]
    fn connected_accounts_keep_supported_platforms_with_their_dates_and_links() {
        let body = json!({
            "connected_accounts": [
                {
                    "type": "github",
                    "id": "42",
                    "name": "octocat",
                    "verified": true,
                    "metadata": {"created_at": "2020-05-15T12:00:00+00:00"}
                },
                {"type": "steam", "id": "76561198000000000", "name": "Gordon", "verified": true},
                {"type": "playstation", "id": "psn-1", "name": "Hunter", "verified": false},
                {"type": "contacts", "id": "c", "name": "Not a platform"},
                {"type": "skype", "id": "live:someone", "name": "Someone"}
            ]
        });
        let parsed = parse_connected_accounts(&body);
        let kinds: Vec<&str> = parsed.iter().map(|account| account.kind.as_str()).collect();
        assert_eq!(kinds, ["github", "steam", "playstation", "skype"]);
        assert_eq!(parsed[0].platform_name, "GitHub");
        assert_eq!(parsed[0].icon, "1cc92a6d139af44d.png");
        assert_eq!(parsed[0].url.as_deref(), Some("https://github.com/octocat"));
        assert_eq!(parsed[0].created_at.as_deref(), Some("May 15, 2020"));
        assert_eq!(
            parsed[1].url.as_deref(),
            Some("https://steamcommunity.com/profiles/76561198000000000")
        );
        assert_eq!(parsed[2].url, None);
        assert_eq!(
            parsed[3].url.as_deref(),
            Some("skype:live%3Asomeone?userinfo")
        );
    }

    #[test]
    fn connection_metadata_follows_the_platform_builders() {
        let text = |value: &str| ConnectionMeta::Text(value.to_owned());
        let tag = |value: &str| ConnectionMeta::Tag {
            label: value.to_owned(),
            paypal: false,
        };
        assert_eq!(
            connection_metadata(
                "reddit",
                &json!({"total_karma": "1234", "gold": "1", "mod": "0"})
            ),
            [text("1,234 Karma"), tag("Gold")]
        );
        assert_eq!(
            connection_metadata(
                "steam",
                &json!({"game_count": "1", "item_count_dota2": "0", "item_count_tf2": "3"})
            ),
            [
                text("1 Game"),
                ConnectionMeta::Item {
                    label: "0 items".to_owned(),
                    icon: STEAM_DOTA2_ICON,
                    alt: "DOTA 2",
                },
                ConnectionMeta::Item {
                    label: "3 items".to_owned(),
                    icon: STEAM_TF2_ICON,
                    alt: "Team Fortress 2",
                },
            ]
        );
        assert_eq!(
            connection_metadata(
                "twitter",
                &json!({"statuses_count": "2500000", "followers_count": "1", "verified": "1"})
            ),
            [text("2.5M Posts"), text("1 Follower")]
        );
        assert_eq!(
            connection_metadata(
                "tiktok",
                &json!({"follower_count": "10", "following_count": "2", "likes_count": "1", "verified": true})
            ),
            [text("10 Followers"), text("2 Following"), text("1 Like")]
        );
        assert_eq!(
            connection_metadata("paypal", &json!({"verified": "1"})),
            [ConnectionMeta::Tag {
                label: "Verified Bank Account".to_owned(),
                paypal: true
            }]
        );
        assert_eq!(
            connection_metadata(
                "ebay",
                &json!({"positive_feedback_percentage": "0", "top_rated_seller": "1"})
            ),
            [tag("Top Rated Seller")]
        );
        // A missing count is left out, a count that is not a number too.
        assert!(connection_metadata("letterboxd", &json!({"watches": "many"})).is_empty());
        assert!(connection_metadata("github", &json!({"followers_count": "5"})).is_empty());

        // Custom application role-connection metadata (`VW` of module 169869).
        let custom = super::application_connection_metadata(
            &json!({"wins": "42", "pro": "1", "joined": "2024-05-01T00:00:00Z"}),
            &json!({
                "wins": {"key": "wins", "name": "Wins", "type": 2},
                "pro": {"key": "pro", "name": "Pro Tier", "type": 7},
                "joined": {"key": "joined", "name": "Joined", "type": 6}
            }),
        );
        assert_eq!(custom.len(), 3);
        assert_eq!(custom[1], tag("Pro Tier"));
        assert_eq!(custom[2], text("42 Wins"));
    }

    #[test]
    fn custom_status_extracts_emoji_only_and_text_states() {
        let activities = vec![
            json!({"type": 0, "name": "Hades"}),
            json!({"type": 4, "state": "shipping features", "emoji": {"name": "🚀"}}),
        ];
        let status = parse_custom_status(&activities).unwrap();
        assert_eq!(status.text.as_deref(), Some("shipping features"));
        assert_eq!(status.emoji_name.as_deref(), Some("🚀"));
        assert_eq!(
            parse_custom_status(&[json!({"type": 4, "state": "   "})]),
            None
        );
    }

    #[test]
    fn wishlist_privacy_detects_private_settings_and_lists() {
        let private_profile = json!({"wishlist_settings": {"wl": {"visibility": 2}}});
        assert!(wishlist_is_private(Some(&private_profile), None));
        let public_profile = json!({"wishlist_settings": {"wl": {"visibility": 1}}});
        assert!(!wishlist_is_private(Some(&public_profile), None));
        let private_list = json!({"visibility": "PRIVATE", "wishlist_items": []});
        assert!(wishlist_is_private(None, Some(&private_list)));
    }

    #[test]
    fn the_profile_request_names_the_guild_only_when_one_is_in_view() {
        assert!(!profile_route(7, None).contains("guild_id"));
        assert!(profile_route(7, None).starts_with("/users/7/profile?type=popout"));
        assert!(profile_route(7, Some(9)).ends_with("&guild_id=9"));
    }

    #[test]
    fn profile_dates_read_as_month_day_year() {
        // Noon UTC is the same calendar day in every zone from UTC-11 to UTC+11.
        assert_eq!(
            profile_date("2021-10-29T12:00:00.000000+00:00").as_deref(),
            Some("Oct 29, 2021")
        );
        assert_eq!(profile_date("not a date"), None);
        assert_eq!(
            profile_date("2013-01-06T10:00:00").as_deref(),
            Some("Jan 6, 2013")
        );
    }

    #[test]
    fn modal_geometry_matches_responsive_breakpoints() {
        let laptop = full_profile_modal_geometry(1280., 720.);
        assert_eq!(laptop.width, 962.);
        assert_eq!(laptop.height, 720.);
        assert_eq!(laptop.card_width, 400.);
        assert_eq!(laptop.pad, 48.);
        assert_eq!(laptop.sys_pad, 32.);
        assert_eq!(laptop.close_top, 40.);
        assert_eq!(laptop.radius, 0.);

        for (vw, vh) in [(1920., 1080.), (2560., 1440.)] {
            let wide = full_profile_modal_geometry(vw, vh);
            assert_eq!(wide.width, 962.);
            assert_eq!(wide.height, 800.);
            assert_eq!(wide.card_width, 400.);
            assert_eq!(wide.pad, 48.);
            assert_eq!(wide.sys_pad, 0.);
            assert_eq!(wide.close_top, 8.);
            assert_eq!(wide.radius, 16.);
        }

        let compact = full_profile_modal_geometry(800., 600.);
        assert_eq!(compact.width, 800.);
        assert_eq!(compact.height, 600.);
        assert!((compact.card_width - 364.).abs() < 0.01);
        assert_eq!(compact.pad, 32.);
        assert_eq!(compact.sys_pad, 32.);
        assert_eq!(compact.close_top, 40.);
        assert_eq!(compact.radius, 0.);
    }
}
