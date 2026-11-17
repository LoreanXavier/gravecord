//! The full profile (`user-profile-modal-v2`, module 927071): 960 wide and the window's height, a 400px identity
//! column (banner, 120px avatar, names, Add Friend / Message / More, bio, Member Since, the private note) beside
//! tabs for recent activity (the content-inventory outbox), mutual friends and mutual servers. Measured in
//! `design/dom/modals/full-profile-{activity,friends,servers}-darker-cozy.json`.
//!
//! Activity cards follow module 506326: a played game shows its icon, name, the time since (`aJ` over the entry's
//! snowflake) and "New Player" on a first session; a listening session shows the track's art, title and artist.

use super::friends::{RELATIONSHIP_FRIEND, RELATIONSHIP_INCOMING, RELATIONSHIP_OUTGOING};
use super::profile_editor::save::{PersonalSection, WidgetData, parse_widgets};
use super::profile_editor::widgets::{featured_app, tag_choices, widget_data_title};
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
    if other {
        tabs.push(Tab::Friends);
        tabs.push(Tab::Servers);
    }
    tabs
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
    _events: Subscription,
}

/// `PLAYED_GAME` and `LISTENED_SESSION` in `ContentInventoryEntryType`.
const PLAYED_GAME: u64 = 1;
const LISTENED_SESSION: u64 = 5;
/// `ContentInventoryTraitType.FIRST_TIME`.
const TRAIT_FIRST_TIME: u64 = 1;

/// `aJ` with the `E` format set: "Ns ago" up to a minute, then minutes, hours under 12, days under 9, weeks under 4,
/// then months of 30 days, each rounded.
pub(super) fn time_ago(seconds: i64) -> String {
    let span = seconds.abs();
    let round = |unit: f64| (seconds as f64 / unit).round() as i64;
    if span < 60 {
        format!("{seconds}s ago")
    } else if span < 3_600 {
        format!("{}m ago", round(60.))
    } else if span < 12 * 3_600 {
        format!("{}h ago", round(3_600.))
    } else if span < 9 * 86_400 {
        format!("{}d ago", round(86_400.))
    } else if span < 4 * 7 * 86_400 {
        format!("{}w ago", round(7. * 86_400.))
    } else {
        format!("{}mo ago", round(30. * 86_400.))
    }
}

/// Milliseconds since the Unix epoch of a snowflake.
fn snowflake_ms(id: Id) -> i64 {
    (id >> 22) as i64 + 1_420_070_400_000
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

/// 370480.An formats the server's ISO date in the current local timezone.
fn profile_date(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %-d, %Y")
                .to_string()
        })
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConnectedAccount {
    kind: String,
    id: String,
    name: String,
    verified: bool,
    created_at: Option<String>,
    platform_name: &'static str,
    monogram: &'static str,
    brand_rgb: u32,
    light_asset: Option<&'static str>,
    dark_asset: Option<&'static str>,
    icon_svg: Option<&'static str>,
    url: Option<String>,
}

type ConnectedPlatformSpec = (
    &'static str,
    &'static str,
    &'static str,
    u32,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

/// Platform metadata from module 573648 (`connected_accounts` in 927071).
const CONNECTED_PLATFORMS: &[ConnectedPlatformSpec] = &[
    (
        "twitch",
        "Twitch",
        "Tw",
        0x9146ff,
        Some("4fda00c96319c8ae.svg"),
        Some("4fda00c96319c8ae.svg"),
        None,
    ),
    (
        "youtube",
        "YouTube",
        "YT",
        0xff0033,
        Some("0fa530ba9c04ac32.svg"),
        Some("0fa530ba9c04ac32.svg"),
        None,
    ),
    (
        "battlenet",
        "Battle.net",
        "BN",
        0x009ae4,
        Some("163c8cb9220efc74.svg"),
        Some("163c8cb9220efc74.svg"),
        None,
    ),
    (
        "bluesky",
        "Bluesky",
        "BS",
        0x0285ff,
        Some("2709f058378a099d.svg"),
        Some("2709f058378a099d.svg"),
        None,
    ),
    (
        "bungie",
        "Bungie.net",
        "Bg",
        0x1a1e24,
        Some("5b0c8422db0e5507.svg"),
        Some("099cd81cf3b4cc98.svg"),
        None,
    ),
    (
        "skype",
        "Skype",
        "Sk",
        0x00aff0,
        Some("883a13b030d276ec.svg"),
        Some("883a13b030d276ec.svg"),
        None,
    ),
    (
        "leagueoflegends",
        "League of Legends",
        "LoL",
        0x0bc6e3,
        Some("302a27a2e5cc3fb6.svg"),
        Some("302a27a2e5cc3fb6.svg"),
        None,
    ),
    (
        "steam",
        "Steam",
        "St",
        0x171a21,
        Some("dbfa31449965dbd5.svg"),
        Some("1f7ec18f3695d4cf.svg"),
        Some("icons/activity_steam.svg"),
    ),
    (
        "reddit",
        "Reddit",
        "r/",
        0xff4500,
        Some("adfd927dcc2049a5.svg"),
        Some("adfd927dcc2049a5.svg"),
        None,
    ),
    (
        "facebook",
        "Facebook",
        "fb",
        0x1877f2,
        Some("17be29f77bee4405.svg"),
        Some("17be29f77bee4405.svg"),
        None,
    ),
    (
        "twitter_legacy",
        "Twitter",
        "Tw",
        0x1da1f2,
        Some("0815e2c86f2e0e37.svg"),
        Some("0815e2c86f2e0e37.svg"),
        None,
    ),
    (
        "twitter",
        "X",
        "X",
        0x111214,
        Some("5cf814b9693aac47.svg"),
        Some("a61999ae9bfb9658.svg"),
        None,
    ),
    (
        "x",
        "X",
        "X",
        0x111214,
        Some("5cf814b9693aac47.svg"),
        Some("a61999ae9bfb9658.svg"),
        None,
    ),
    (
        "spotify",
        "Spotify",
        "Sp",
        0x1db954,
        Some("d5719388ffc613da.svg"),
        Some("d5719388ffc613da.svg"),
        Some("icons/inventory_spotify.svg"),
    ),
    (
        "xbox",
        "Xbox",
        "XB",
        0x107c10,
        Some("0133fed7d4dc775c.svg"),
        Some("c4f09fda61827e19.svg"),
        None,
    ),
    (
        "samsung",
        "Samsung Galaxy",
        "SG",
        0x1428a0,
        Some("4ed35737da17ede2.svg"),
        Some("4ed35737da17ede2.svg"),
        None,
    ),
    (
        "github",
        "GitHub",
        "GH",
        0x24292f,
        Some("4db64e0649c64e7e.svg"),
        Some("a35ff3e86ffa1eb2.svg"),
        None,
    ),
    (
        "playstation",
        "PlayStation Network",
        "PS",
        0x003791,
        Some("792da49081c21457.svg"),
        Some("eed203a7ec517e23.svg"),
        None,
    ),
    (
        "epicgames",
        "Epic Games",
        "EG",
        0x2a2a2a,
        Some("4fb7893066c0ef83.svg"),
        Some("199eceff4fca1a0c.svg"),
        None,
    ),
    (
        "riotgames",
        "Riot Games",
        "RG",
        0xd32936,
        Some("772307b7d902b47b.svg"),
        Some("772307b7d902b47b.svg"),
        None,
    ),
    (
        "roblox",
        "Roblox",
        "Rb",
        0x232527,
        Some("e04a073ed006173a.svg"),
        Some("a4d8e9b0404a2d00.svg"),
        None,
    ),
    (
        "paypal",
        "PayPal",
        "PP",
        0x003087,
        Some("dcb64a4ff8f61b2c.svg"),
        Some("dcb64a4ff8f61b2c.svg"),
        None,
    ),
    (
        "ebay",
        "eBay",
        "eB",
        0xe53238,
        Some("b28a7a265581b6e3.svg"),
        Some("b28a7a265581b6e3.svg"),
        None,
    ),
    (
        "tiktok",
        "TikTok",
        "TT",
        0x111111,
        Some("53641d0496c85aec.svg"),
        Some("a303feb844769624.svg"),
        None,
    ),
    (
        "instagram",
        "Instagram",
        "IG",
        0xe1306c,
        Some("c05dded52023ed43.svg"),
        Some("c05dded52023ed43.svg"),
        None,
    ),
    (
        "letterboxd",
        "Letterboxd",
        "Lb",
        0x202830,
        Some("4f8b8634a81979a6.svg"),
        Some("6b6632ec799ccf64.svg"),
        None,
    ),
    (
        "mastodon",
        "Mastodon",
        "M",
        0x6364ff,
        Some("0e6385723fe5cd49.svg"),
        Some("0e6385723fe5cd49.svg"),
        None,
    ),
    (
        "crunchyroll",
        "Crunchyroll",
        "CR",
        0xf47521,
        Some("94ef3e8b7fa85a2e.svg"),
        Some("94ef3e8b7fa85a2e.svg"),
        None,
    ),
    (
        "domain",
        "Domain",
        "www",
        0x2b2d31,
        Some("10d4972595802406.svg"),
        Some("b4376756bcbbf1ef.svg"),
        Some("icons/oauth_entry_globe.svg"),
    ),
    (
        "amazon-music",
        "Amazon Music",
        "AM",
        0x25d1da,
        Some("e3c4aacc1a54395d.svg"),
        Some("e3c4aacc1a54395d.svg"),
        None,
    ),
    (
        "amazon_music",
        "Amazon Music",
        "AM",
        0x25d1da,
        Some("e3c4aacc1a54395d.svg"),
        Some("e3c4aacc1a54395d.svg"),
        None,
    ),
    (
        "meta-quest",
        "Meta Quest",
        "MQ",
        0x0081fb,
        Some("d64a395149091d96.svg"),
        Some("ac961aa8d4bb48a5.svg"),
        None,
    ),
    (
        "meta_quest",
        "Meta Quest",
        "MQ",
        0x0081fb,
        Some("d64a395149091d96.svg"),
        Some("ac961aa8d4bb48a5.svg"),
        None,
    ),
];

const DEFAULT_CONNECTED_PLATFORM: (&str, &str, u32, Option<&str>, Option<&str>, Option<&str>) = (
    "Connected Account",
    "•",
    0x4e5058,
    None,
    None,
    Some("icons/oauth_entry_globe.svg"),
);

fn connection_platform_icon_key(kind: &str, dark: bool) -> String {
    format!(
        "connection-icon:{kind}:{}",
        if dark { "dark" } else { "light" }
    )
}

/// Platform metadata and profile URLs from module 573648 (`connected_accounts` in 927071).
fn parse_connected_accounts(body: &Value) -> Vec<ConnectedAccount> {
    body["connected_accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if item["revoked"] == true {
                return None;
            }
            let kind = item["type"].as_str()?.to_ascii_lowercase();
            let id = item["id"].as_str().unwrap_or_default().to_owned();
            let name = item["name"].as_str().filter(|s| !s.is_empty())?.to_owned();
            let verified = item["verified"] == true;
            let created_at = item["metadata"]["created_at"]
                .as_str()
                .and_then(profile_date);
            let enc_name = encode_uri_segment(&name);
            let enc_id = encode_uri_segment(&id);
            let (platform_name, monogram, brand_rgb, light_asset, dark_asset, icon_svg) =
                CONNECTED_PLATFORMS
                    .iter()
                    .find(|(known, ..)| *known == kind)
                    .map(|(_, title, mono, rgb, light, dark, svg)| {
                        (*title, *mono, *rgb, *light, *dark, *svg)
                    })
                    .unwrap_or(DEFAULT_CONNECTED_PLATFORM);
            let url = match kind.as_str() {
                "twitch" => Some(format!("https://www.twitch.tv/{enc_name}")),
                "youtube" => {
                    (!id.is_empty()).then(|| format!("https://www.youtube.com/channel/{enc_id}"))
                }
                "bluesky" => (!id.is_empty())
                    .then(|| format!("https://bsky.app/profile/{}", enc_id.replace("%3A", ":"))),
                "steam" => (!id.is_empty())
                    .then(|| format!("https://steamcommunity.com/profiles/{enc_id}")),
                "reddit" => Some(format!("https://www.reddit.com/u/{enc_name}")),
                "twitter_legacy" => Some(format!("https://twitter.com/{enc_name}")),
                "twitter" | "x" => Some(format!("https://x.com/{enc_name}")),
                "spotify" => {
                    (!id.is_empty()).then(|| format!("https://open.spotify.com/user/{enc_id}"))
                }
                "github" => Some(format!("https://github.com/{enc_name}")),
                "roblox" => (!id.is_empty())
                    .then(|| format!("https://www.roblox.com/users/{enc_id}/profile")),
                "ebay" => Some(format!("https://www.ebay.com/usr/{enc_name}")),
                "tiktok" => Some(format!("https://www.tiktok.com/@{enc_name}")),
                "instagram" => Some(format!("https://www.instagram.com/{enc_name}")),
                "letterboxd" => Some(format!("https://letterboxd.com/{enc_name}/")),
                "mastodon" => {
                    (id.starts_with("https://") || id.starts_with("http://")).then_some(id.clone())
                }
                "domain" => (!id.is_empty()).then(|| format!("https://{id}/")),
                _ => None,
            };
            Some(ConnectedAccount {
                kind,
                id,
                name,
                verified,
                created_at,
                platform_name,
                monogram,
                brand_rgb,
                light_asset,
                dark_asset,
                icon_svg,
                url,
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
    /// The profile on one of its tabs: the Mutual Friends and Mutual Servers links open it there.
    pub(super) fn open_full_profile_tab(
        &mut self,
        user: Id,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_full_profile(user, window, cx);
        if let Some(open) = self.full_profile.as_mut() {
            open.tab = tab;
            open.explicit = true;
        }
        cx.notify();
    }

    pub(super) fn open_full_profile(
        &mut self,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status_popout = None;
        self.profile = None;
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
        self.full_profile = Some(FullProfile {
            user,
            guild: self.guild,
            tab: Tab::Activity,
            explicit: false,
            note,
            saved_note: String::new(),
            _events: events,
        });
        for (key, route) in [
            (format!("profile:{user}"), profile_route(user, self.guild)),
            (
                format!("outbox:{user}"),
                format!("/content-inventory/users/{user}/outbox"),
            ),
            (format!("note:{user}"), format!("/users/@me/notes/{user}")),
        ] {
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

    fn request_connected_account_icons(&mut self, body: &Value) {
        for account in parse_connected_accounts(body) {
            if let Some(light) = account.light_asset {
                self.request_image(
                    connection_platform_icon_key(&account.kind, false),
                    format!("https://discord.com/assets/{light}"),
                );
            }
            if let Some(dark) = account.dark_asset {
                self.request_image(
                    connection_platform_icon_key(&account.kind, true),
                    format!("https://discord.com/assets/{dark}"),
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
        } else if key == format!("profile-games:{user}")
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
                    let url = format!(
                        "https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=160&keep_aspect_ratio=false"
                    );
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

    fn section_heading(text: impl Into<SharedString>, tint: Hsla) -> gpui::Div {
        div()
            .h(px(16.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(tint)
            .child(text.into())
    }

    fn full_profile_member_since(
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
                            d.p(px(8.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(emoji_el)
                        } else {
                            let text = status.text.clone().unwrap_or_default();
                            d.px(px(12.))
                                .py(px(8.))
                                .flex()
                                .items_start()
                                .when_some(emoji_el, |row, art| {
                                    row.child(div().mr(px(4.)).mt(px(1.)).flex_none().child(art))
                                })
                                .child(
                                    div()
                                        .min_w(px(0.))
                                        .line_clamp(2)
                                        .text_ellipsis()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(default)
                                        .child(text),
                                )
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

    /// Connected accounts list (`927071.js:760-945`, `.connectedAccounts_O2KI4e`).
    fn full_profile_connections(
        &self,
        user: Id,
        body: Option<&Value>,
        palette: &super::dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let accounts = parse_connected_accounts(body?);
        if accounts.is_empty() {
            return None;
        }
        let border = palette.token(if palette.themed {
            "--user-profile-border"
        } else {
            "--border-subtle"
        });
        let dark = palette
            .theme
            .map_or(!matches!(super::theme(), dc_tokens::Theme::Light), |t| {
                !t.light()
            });
        let rows: Vec<AnyElement> = accounts
            .into_iter()
            .enumerate()
            .map(|(index, account)| {
                let platform_picture = self
                    .images
                    .get(&connection_platform_icon_key(&account.kind, dark))
                    .cloned();
                let badge_el = if let Some(picture) = platform_picture {
                    div()
                        .id(SharedString::from(format!(
                            "full-profile-conn-badge-{user}-{index}"
                        )))
                        .size(px(24.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(img(picture).size(px(24.)))
                } else {
                    div()
                        .id(SharedString::from(format!(
                            "full-profile-conn-badge-{user}-{index}"
                        )))
                        .size(px(24.))
                        .flex_none()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(border)
                        .bg(gpui::rgb(account.brand_rgb))
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|d| match account.icon_svg {
                            Some(path) => {
                                d.child(svg().path(path).size(px(14.)).text_color(color("--white")))
                            }
                            None => d
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(account.monogram),
                        })
                };
                let badge = self.with_tip(
                    format!("full-profile-conn-platform-{user}-{index}"),
                    super::tooltip::Tip::text(account.platform_name, super::tooltip::Side::Top)
                        .after(300),
                    badge_el,
                    cx,
                );
                let verified = account.verified.then(|| {
                    svg()
                        .path("icons/mod_info_verified.svg")
                        .size(px(16.))
                        .flex_none()
                        .text_color(subtle)
                });
                let name_text = account.name.clone();
                let open_link = account.url.clone().map(|url| {
                    let label = format!("{}: {}", account.platform_name, account.name);
                    let btn = div()
                        .id(SharedString::from(format!(
                            "full-profile-conn-link-{user}-{index}"
                        )))
                        .size(px(24.))
                        .flex_none()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path("icons/open_external.svg")
                                .size(px(16.))
                                .text_color(subtle)
                                .hover(move |s| s.text_color(strong)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.follow_link(&url, &label, cx);
                        }));
                    self.with_tip(
                        format!("full-profile-conn-open-tip-{user}-{index}"),
                        super::tooltip::Tip::text(dc_intl::t!("q5jLJB"), super::tooltip::Side::Top)
                            .after(300),
                        btn,
                        cx,
                    )
                });
                let name_row: AnyElement = if let Some(url) = account.url.clone() {
                    let label = format!("{}: {}", account.platform_name, account.name);
                    div()
                        .id(SharedString::from(format!(
                            "full-profile-conn-name-{user}-{index}"
                        )))
                        .min_w(px(0.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_ellipsis()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(default)
                                .hover(move |s| s.text_color(strong))
                                .child(name_text),
                        )
                        .children(verified)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.follow_link(&url, &label, cx);
                        }))
                        .into_any_element()
                } else {
                    div()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_ellipsis()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(default)
                                .child(name_text),
                        )
                        .children(verified)
                        .into_any_element()
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(badge)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(name_row)
                            .when_some(account.created_at, |d, date| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(subtle)
                                        .child(dc_intl::t!("9rfonh", date = date)),
                                )
                            }),
                    )
                    .children(open_link)
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .parity("full-profile-connections")
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows)
                .into_any_element(),
        )
    }

    /// One `card_Bu4Al8`: art, title, subtitle and the badges row.
    fn activity_card(
        &self,
        entry: &Value,
        now_ms: i64,
        palette: &super::dm_profile::Palette,
        strong: Hsla,
        subtle: Hsla,
    ) -> Option<AnyElement> {
        let kind = entry["content_type"].as_u64()?;
        let id: Id = entry["id"].as_str()?.parse().ok()?;
        let ago = time_ago((now_ms - snowflake_ms(id)) / 1_000);
        let first_time = entry["traits"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|item| {
                item["type"].as_u64() == Some(TRAIT_FIRST_TIME) && item["first_time"] == true
            });
        let (art, title, subtitle, glyph) = match kind {
            PLAYED_GAME => {
                let app = entry["extra"]["application_id"]
                    .as_str()
                    .unwrap_or_default();
                (
                    self.images.get(&format!("app-icon:{app}")).cloned(),
                    entry["extra"]["game_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    None,
                    "activity_game",
                )
            }
            LISTENED_SESSION => {
                let media = &entry["extra"]["entries"][0]["media"];
                let url = media["image_url"].as_str().unwrap_or_default();
                (
                    self.images.get(url).cloned(),
                    media["title"].as_str().unwrap_or_default().to_owned(),
                    media["artists"][0]["name"].as_str().map(str::to_owned),
                    "activity_music",
                )
            }
            _ => return None,
        };
        let badge = |glyph: &'static str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(12.))
                        .text_color(subtle),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(subtle)
                        .child(text),
                )
        };
        Some(
            div()
                .relative()
                .p(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(palette.token("--border-muted"))
                .bg(palette.token("--user-profile-overlay-background"))
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(60.))
                        .flex_none()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(palette.tile_background(false))
                        .children(art.map(|picture| img(picture).size(px(60.)).rounded(px(8.)))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(strong)
                                        .child(title),
                                )
                                .children(subtitle.map(|subtitle| {
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(strong)
                                        .child(subtitle)
                                })),
                        )
                        .child(div().flex().gap(px(8.)).child(badge(glyph, ago)).when(
                            kind == PLAYED_GAME && first_time,
                            |this| {
                                this.child(badge(
                                    "activity_new_player",
                                    dc_intl::t!("keY6mW").to_owned(),
                                ))
                            },
                        )),
                )
                .child(
                    div().absolute().top(px(12.)).right(px(12.)).child(
                        svg()
                            .path("icons/more_horizontal.svg")
                            .size(px(16.))
                            .text_color(subtle),
                    ),
                )
                .into_any_element(),
        )
    }

    /// The Board (`a5` of module 927071): themed widget cards populated from the user's saved `widgets`, or the
    /// official empty state (`oqalC+`) on own profile.
    fn full_profile_board(
        &self,
        user: Id,
        me: bool,
        palette: &super::dm_profile::Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strong: Hsla = palette.token("--text-strong").into();
        let default: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let body = self.fetched.get(&format!("profile:{user}"));
        let answer = self.fetched.get(&format!("profile-games:{user}"));
        let game = |id: &str| {
            answer
                .and_then(Value::as_array)
                .and_then(|games| games.iter().find(|game| game["id"].as_str() == Some(id)))
        };
        let tag_map = tag_choices();
        let card_bg = palette.token("--user-profile-overlay-background");
        let border = palette.token("--border-muted");
        let tile_bg = palette.tile_background(false);
        let pill_bg = palette.pill_background(false);
        let widgets: Vec<AnyElement> = body
            .map(parse_widgets)
            .unwrap_or_default()
            .into_iter()
            .filter(|widget| widget.data.is_valid())
            .map(|widget| {
                let title = widget_data_title(&widget.data);
                let content: AnyElement = match &widget.data {
                    WidgetData::FavoriteGames { games }
                    | WidgetData::CurrentGames { games }
                    | WidgetData::WantToPlayGames { games }
                    | WidgetData::PlayedGames { games } => {
                        let rows: Vec<AnyElement> = games
                            .iter()
                            .map(|entry| {
                                let fetched = game(&entry.game_id);
                                let resolved = self.profile_editor_resolve_game(&entry.game_id);
                                let name = fetched
                                    .and_then(|item| item["name"].as_str())
                                    .filter(|name| !name.is_empty())
                                    .map_or(resolved.name, str::to_owned);
                                let picture = self
                                    .images
                                    .get(&format!("profile-game:{}", entry.game_id))
                                    .cloned()
                                    .or_else(|| {
                                        resolved
                                            .cover
                                            .as_deref()
                                            .and_then(|url| self.images.get(url).cloned())
                                    })
                                    .or_else(|| {
                                        resolved
                                            .icon
                                            .as_deref()
                                            .and_then(|url| self.images.get(url).cloned())
                                    });
                                let comment = entry
                                    .comment
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|text| !text.is_empty())
                                    .map(str::to_owned);
                                let tag_chips: Vec<AnyElement> = entry
                                    .tags
                                    .iter()
                                    .filter_map(|tag_id| {
                                        let label = tag_map
                                            .iter()
                                            .find(|(id, _)| id == tag_id)
                                            .map(|(_, label)| *label)?;
                                        Some(
                                            div()
                                                .px(px(6.))
                                                .py(px(2.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(border)
                                                .bg(pill_bg)
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(default)
                                                .child(label)
                                                .into_any_element(),
                                        )
                                    })
                                    .collect();
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .w(px(56.))
                                            .h(px(74.66))
                                            .flex_none()
                                            .rounded(px(8.))
                                            .overflow_hidden()
                                            .bg(tile_bg)
                                            .children(picture.map(|picture| {
                                                img(picture)
                                                    .w(px(56.))
                                                    .h(px(74.66))
                                                    .object_fit(gpui::ObjectFit::Cover)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(strong)
                                                    .child(name),
                                            )
                                            .when(!tag_chips.is_empty(), |this| {
                                                this.child(
                                                    div()
                                                        .flex()
                                                        .flex_wrap()
                                                        .gap(px(4.))
                                                        .children(tag_chips),
                                                )
                                            })
                                            .when_some(comment, |this, comment| {
                                                this.child(
                                                    div()
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .text_color(default)
                                                        .child(comment),
                                                )
                                            }),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .children(rows)
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
                div()
                    .p(px(16.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(border)
                    .bg(card_bg)
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(Self::section_heading(title, subtle))
                    .child(content)
                    .into_any_element()
            })
            .collect();
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
        div()
            .pt(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
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

    fn mutual_row(
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
        let servers: Vec<(Id, Option<String>)> = body
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
        let display_name = open_guild
            .and_then(|guild| self.stores.members.get(guild, id))
            .and_then(|member| member.nick.as_deref())
            .filter(|nick| !nick.is_empty())
            .map_or_else(|| user.display_name().to_owned(), str::to_owned);
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
        let status_bubble = self.full_profile_status_bubble(id, open_guild, me, &palette, cx);
        let connections = self.full_profile_connections(id, body, &palette, cx);
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
                            .child(buttons)
                            .when_some(bio, |this, bio| {
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
                            .when_some(connections, |d, conns| {
                                d.child(section(Some(dc_intl::t!("3fe7U5"))).child(conns))
                            })
                            .when(!me, |this| {
                                this.child(section(Some(dc_intl::t!("mQKv+v"))).child(note_field))
                            }),
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
            .when(!friends.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Friends,
                    dc_intl::t!("5s9jl+", count = friends.len()),
                ))
            })
            .when(!servers.is_empty(), |this| {
                this.child(tab_item(
                    Tab::Servers,
                    dc_intl::t!("eE3oep", count = servers.len()),
                ))
            });
        let now_ms = chrono::Utc::now().timestamp_millis();
        let row_hover = palette.token("--user-profile-background-hover");
        let dot_cutout = palette.theme.map_or("--background-base-lower", |t| {
            if t.light() { "--white" } else { "--black" }
        });
        let tile_bg = palette.tile_background(false);
        let panel: AnyElement = match tab {
            Tab::Board => self.full_profile_board(id, me, &palette, cx),
            Tab::Wishlist => wishlist_panel.unwrap_or_else(|| div().into_any_element()),
            Tab::Activity => {
                let current = self.live_activity_cards(
                    id,
                    super::live_activity::Surface::Modal,
                    strong,
                    subtle,
                    cx,
                );
                let cards: Vec<AnyElement> = self
                    .fetched
                    .get(&format!("outbox:{id}"))
                    .and_then(|outbox| outbox["entries"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| self.activity_card(entry, now_ms, &palette, strong, subtle))
                    .collect();
                div()
                    .pt(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(!current.is_empty(), |this| {
                        this.child(Self::section_heading(dc_intl::t!("J6STd9"), subtle))
                            .child(div().flex().flex_col().gap(px(8.)).children(current))
                    })
                    .when(!cards.is_empty(), |this| {
                        this.child(Self::section_heading(dc_intl::t!("jzgEoL"), subtle))
                            .child(div().flex().flex_col().gap(px(8.)).children(cards))
                    })
                    .into_any_element()
            }
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
                    self.mutual_row(
                        SharedString::from(format!("full-profile-friend-{friend_id}")),
                        leading,
                        friend.display_name().to_owned(),
                        strong,
                        row_hover,
                    )
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
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .pt(px(geo.pad + geo.sys_pad))
                    .pl(px(geo.pad))
                    .pr(px(geo.pad - 16.))
                    .child(columns),
            )
            .child(
                div()
                    .id("full-profile-close")
                    .absolute()
                    .top(px(geo.close_top))
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
            );
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
        Tab, full_profile_modal_geometry, parse_connected_accounts, parse_custom_status,
        profile_date, profile_route, profile_tabs, should_apply_guild_member, time_ago,
        wishlist_is_private,
    };
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
    fn connected_accounts_parse_verified_badges_dates_and_external_urls() {
        let body = json!({
            "connected_accounts": [
                {
                    "type": "github",
                    "id": "42",
                    "name": "octocat",
                    "verified": true,
                    "metadata": {"created_at": "2020-05-15T12:00:00+00:00"}
                },
                {
                    "type": "steam",
                    "id": "76561198000000000",
                    "name": "Gordon",
                    "verified": true
                },
                {
                    "type": "playstation",
                    "id": "psn-1",
                    "name": "Hunter",
                    "verified": false
                }
            ]
        });
        let parsed = parse_connected_accounts(&body);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].platform_name, "GitHub");
        assert_eq!(parsed[0].url.as_deref(), Some("https://github.com/octocat"));
        assert!(parsed[0].verified);
        assert_eq!(parsed[0].created_at.as_deref(), Some("May 15, 2020"));
        assert_eq!(
            parsed[1].url.as_deref(),
            Some("https://steamcommunity.com/profiles/76561198000000000")
        );
        assert_eq!(parsed[2].url, None);
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
    }

    #[test]
    fn time_ago_rounds_into_the_official_bands() {
        assert_eq!(time_ago(42), "42s ago");
        assert_eq!(time_ago(90), "2m ago");
        assert_eq!(time_ago(8 * 3_600 + 1_000), "8h ago");
        assert_eq!(time_ago(30 * 3_600), "1d ago");
        assert_eq!(time_ago(10 * 86_400), "1w ago");
        assert_eq!(time_ago(40 * 86_400), "1mo ago");
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
