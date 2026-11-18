//! Which platform an activity came from, and what that changes where it is drawn (module 770689, its helpers 287743,
//! 90644, 541806, 61330, 141639, 190915, and the image fallback `P` of module 325158).
//!
//! A linked console account plays a game and Discord's presence carries it as an activity with `platform` set
//! (`ps4`, `ps5`, `xbox`; module 570138) or, for Xbox, the Xbox application (`438122941302046720`, module 487887 `$W`).
//! A presence relayed by a linked platform may also name it in its session (`session_id` `h:<type>,...`, 287743).
//! The meta-quest branches of 770689 are not ported: their strings (`BrHQaq`, `p6vL0e`) are not in the pinned
//! en-US table, and module 573648 lists the platform as disabled. GAP: Meta Quest headings.

use serde_json::Value;

/// Module 487887 `$W`: the Xbox application.
pub(super) const XBOX_APPLICATION: &str = "438122941302046720";
/// Module 539170 `ex`: the Crunchyroll application.
pub(super) const CRUNCHYROLL_APPLICATION: &str = "981509069309354054";

/// A platform of module 573648 an activity can name, with the asset files its activity surfaces draw: the
/// `whiteSVG` the card header masks, the `lightPNG`, and Xbox's `customPNG`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Platform {
    pub kind: &'static str,
    pub name: &'static str,
    pub white_svg: &'static str,
    pub light_png: &'static str,
    pub custom_png: Option<&'static str>,
    /// `enabled` of module 573648: a session provider (287743) must be enabled.
    pub enabled: bool,
}

/// Every platform of module 573648 in its order, with the asset files its activity surfaces draw (the asset modules
/// of each entry resolved through `design/snapshot/modules`; Xbox's `customPNG` is module 783197).
pub(super) const PLATFORMS: &[Platform] = &[
    Platform {
        kind: "twitch",
        name: "Twitch",
        white_svg: "487de5256b2ae540.svg",
        light_png: "45900de231038a6d.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "youtube",
        name: "YouTube",
        white_svg: "9133e8517dabfc4d.svg",
        light_png: "c569f6e10f1fa390.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "battlenet",
        name: "Battle.net",
        white_svg: "49838932f9b480ad.svg",
        light_png: "0e1a419c00a471a2.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "bluesky",
        name: "Bluesky",
        white_svg: "7f4102017aa448cc.svg",
        light_png: "8a42ef6bc4dd4e0d.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "bungie",
        name: "Bungie.net",
        white_svg: "099cd81cf3b4cc98.svg",
        light_png: "9b762e8a42709cc4.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "skype",
        name: "Skype",
        white_svg: "4c67fc57b556eb8f.svg",
        light_png: "4f2cb7753e5d5e61.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "leagueoflegends",
        name: "League of Legends",
        white_svg: "b4b31af39374d156.svg",
        light_png: "fc30b78cd5e1db04.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "steam",
        name: "Steam",
        white_svg: "1f7ec18f3695d4cf.svg",
        light_png: "b68f2f4df4af9085.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "reddit",
        name: "Reddit",
        white_svg: "d71dbd7c4356c7fb.svg",
        light_png: "afc3f4fa75178004.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "facebook",
        name: "Facebook",
        white_svg: "f67afe9ab50d9e47.svg",
        light_png: "bda550439e2a1af0.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "twitter_legacy",
        name: "Twitter",
        white_svg: "437fcd7cc9b62841.svg",
        light_png: "7520545bf8990675.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "twitter",
        name: "X",
        white_svg: "a61999ae9bfb9658.svg",
        light_png: "90d18081c2eb2ea3.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "spotify",
        name: "Spotify",
        white_svg: "f6833ce4dd20c1fb.svg",
        light_png: "40518c6ced9fed93.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "xbox",
        name: "Xbox",
        white_svg: "c4f09fda61827e19.svg",
        light_png: "8cea2073e188eabe.png",
        custom_png: Some("fc0b586dbf05772e.png"),
        enabled: true,
    },
    Platform {
        kind: "samsung",
        name: "Samsung Galaxy",
        white_svg: "098052c801764cc4.svg",
        light_png: "d50e7459b395e5d0.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "github",
        name: "GitHub",
        white_svg: "a35ff3e86ffa1eb2.svg",
        light_png: "1cc92a6d139af44d.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "playstation",
        name: "PlayStation Network",
        white_svg: "eed203a7ec517e23.svg",
        light_png: "ae74d3de4958bf78.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "playstation-stg",
        name: "PlayStation Network (Staging)",
        white_svg: "792da49081c21457.svg",
        light_png: "46a23515a87620b9.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "epicgames",
        name: "Epic Games",
        white_svg: "199eceff4fca1a0c.svg",
        light_png: "e87d94bf0041b63e.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "riotgames",
        name: "Riot Games",
        white_svg: "6da27aeb4b9ca57e.svg",
        light_png: "fa9e14fc0fd3680f.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "roblox",
        name: "Roblox",
        white_svg: "a4d8e9b0404a2d00.svg",
        light_png: "268c81933d752854.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "paypal",
        name: "PayPal",
        white_svg: "7aa7428d47ab2a46.svg",
        light_png: "81a39a4672d62d60.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "ebay",
        name: "eBay",
        white_svg: "bdfe508d89ae4987.svg",
        light_png: "d0f7fa72a2ca6388.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "tiktok",
        name: "TikTok",
        white_svg: "a303feb844769624.svg",
        light_png: "b4002d11fb3b5c1c.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "instagram",
        name: "Instagram",
        white_svg: "b511b1c7270d6908.svg",
        light_png: "ea0692691904fca9.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "letterboxd",
        name: "Letterboxd",
        white_svg: "b80a69c4fb0b9ccd.svg",
        light_png: "cb1a567ff6191fb5.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "mastodon",
        name: "Mastodon",
        white_svg: "b7098b06b15a8307.svg",
        light_png: "53fcd6d102d44c2b.png",
        custom_png: None,
        enabled: false,
    },
    Platform {
        kind: "crunchyroll",
        name: "Crunchyroll",
        white_svg: "94ef3e8b7fa85a2e.svg",
        light_png: "4e56bb1ecb9b9dc3.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "domain",
        name: "Domain",
        white_svg: "b4376756bcbbf1ef.svg",
        light_png: "8d54a5919eb645f9.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "amazon-music",
        name: "Amazon Music",
        white_svg: "e3c4aacc1a54395d.svg",
        light_png: "cda6fb74e92e32f1.png",
        custom_png: None,
        enabled: true,
    },
    Platform {
        kind: "meta_quest_or_horizon",
        name: "Meta Quest",
        white_svg: "ac961aa8d4bb48a5.svg",
        light_png: "9f3c6255a4538cd8.png",
        custom_png: None,
        enabled: false,
    },
];

pub(super) fn platform_of(kind: &str) -> Option<&'static Platform> {
    PLATFORMS.iter().find(|platform| platform.kind == kind)
}

/// The image key of a platform asset file (`/assets/<file>` of the pinned web build).
pub(super) fn asset_key(file: &str) -> String {
    format!("platform-asset:{file}")
}

pub(super) fn asset_url(file: &str) -> String {
    format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN)
}

/// Module 61330: an Xbox activity.
pub(super) fn is_xbox(activity: &Value) -> bool {
    activity["application_id"].as_str() == Some(XBOX_APPLICATION) || activity["platform"] == "xbox"
}

/// Module 141639: a PlayStation activity.
pub(super) fn is_playstation(activity: &Value) -> bool {
    matches!(activity["platform"].as_str(), Some("ps4" | "ps5"))
}

/// Module 541806: a Crunchyroll activity.
pub(super) fn is_crunchyroll(activity: &Value) -> bool {
    activity["application_id"].as_str() == Some(CRUNCHYROLL_APPLICATION)
}

/// Module 287743: the platform a session `h:<type>,...` was relayed by, when it is a known, enabled one.
fn session_provider(activity: &Value) -> Option<&'static Platform> {
    let session = activity["session_id"].as_str()?.strip_prefix("h:")?;
    let kind = session.split(',').next().filter(|kind| !kind.is_empty())?;
    platform_of(kind).filter(|platform| platform.enabled)
}

/// The provider of `x` in module 770689: the session's platform, Spotify, Crunchyroll, Xbox, PlayStation, then a
/// platform whose name is the activity's (only League of Legends, Roblox, Twitch and YouTube, set `g`).
pub(super) fn provider(activity: &Value) -> Option<&'static Platform> {
    if let Some(platform) = session_provider(activity) {
        return Some(platform);
    }
    if dc_state::now_playing::is_spotify(activity) {
        return platform_of("spotify");
    }
    if is_crunchyroll(activity) {
        return platform_of("crunchyroll");
    }
    if is_xbox(activity) {
        return platform_of("xbox");
    }
    if is_playstation(activity) {
        return platform_of("playstation");
    }
    let name = activity["name"].as_str()?;
    PLATFORMS.iter().find(|platform| {
        platform.name == name
            && matches!(
                platform.kind,
                "leagueoflegends" | "roblox" | "twitch" | "youtube"
            )
    })
}

/// Module 190915: how a heading names the platform (Xbox and PlayStation by their short names).
fn heading_name(platform: &Platform) -> String {
    match platform.kind {
        "xbox" => dc_intl::t("Nfvo72").to_owned(),
        "playstation" => dc_intl::t("fFl4jo").to_owned(),
        _ => platform.name.to_owned(),
    }
}

/// The heading of a live activity card (module 770689 `text`), with the platform whose icon follows it.
pub(super) fn heading(activity: &Value) -> (String, Option<&'static Platform>) {
    let kind = activity["type"].as_u64().unwrap_or(0);
    let platform = provider(activity);
    let session = session_provider(activity);
    let with_platform =
        |key: &str, name: String| dc_intl::format(key, &[("platform", name.into())]);
    if let Some(platform) = platform {
        match (kind, platform.kind) {
            (0, "xbox" | "playstation") => {
                return (
                    with_platform("A17aM8", heading_name(platform)),
                    Some(platform),
                );
            }
            (1, "twitch") => {
                return (
                    dc_intl::format("4CQq9Q", &[("name", dc_intl::t("q4pBG3").into())]),
                    Some(platform),
                );
            }
            (1, "youtube") => {
                return (
                    dc_intl::format("4CQq9Q", &[("name", dc_intl::t("aS6cK4").into())]),
                    Some(platform),
                );
            }
            _ => {}
        }
    }
    if let Some(session) = session {
        let key = match kind {
            0 => Some("A17aM8"),
            3 => Some("ENbTKQ"),
            2 => Some("EcHzWI"),
            5 => Some("ikpHeS"),
            1 => Some("Dzgz4u"),
            _ => None,
        };
        if let Some(key) = key {
            return (with_platform(key, heading_name(session)), Some(session));
        }
    }
    let name = activity["name"].as_str().unwrap_or_default();
    let details = !activity["details"].is_null();
    let text = match kind {
        0 => dc_intl::t("BMTj28").to_owned(),
        1 => dc_intl::t("Jpkr/q").to_owned(),
        2 if details => dc_intl::format("b+lA5+", &[("name", name.into())]),
        2 => dc_intl::t("dBISa6").to_owned(),
        3 if details => dc_intl::format("mqdfDc", &[("name", name.into())]),
        3 => dc_intl::t("GpNXjC").to_owned(),
        5 if details => dc_intl::format("oHF7Ch", &[("name", name.into())]),
        5 => dc_intl::t("OzCsIA").to_owned(),
        _ => String::new(),
    };
    (text, platform)
}

/// Modules 960076 and 908289: the url of a `STREAMING` activity that is a Twitch channel or a YouTube video
/// (`/^https?:\/\/(?:www\.)?(twitch\.tv\/(.+)|youtube.com\/watch\?v=)/`, 487887 `yz`).
pub(super) fn stream_url(activity: &Value) -> Option<String> {
    if activity["type"] != 1 {
        return None;
    }
    let url = activity["url"].as_str()?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let matches = rest
        .strip_prefix("twitch.tv/")
        .is_some_and(|channel| !channel.is_empty())
        || rest.starts_with("youtube.com/watch?v=");
    matches.then(|| url.to_owned())
}

impl super::Root {
    /// The status a person's avatar shows (module 935154): `streaming` while one of their activities is a Twitch or
    /// YouTube stream (960076) and they are not offline, else their status.
    pub(super) fn display_status(&self, user: dc_state::Id, guild: Option<dc_state::Id>) -> String {
        let status = self.stores.status(user);
        if matches!(status, "offline" | "invisible") {
            return status.to_owned();
        }
        let streams = self
            .stores
            .presence
            .get(user, guild)
            .is_some_and(|presence| presence.activities.iter().any(|a| stream_url(a).is_some()));
        if streams { "streaming" } else { status }.to_owned()
    }
}

/// Module 682261: an activity with rich presence (details, art, a party, secrets or a state).
pub(super) fn is_rich(activity: &Value) -> bool {
    activity["type"] != 4
        && (!activity["details"].is_null()
            || (activity["assets"].is_object()
                && (!activity["assets"]["large_image"].is_null()
                    || !activity["assets"]["small_text"].is_null()))
            || !activity["party"].is_null()
            || !activity["secrets"].is_null()
            || !activity["state"].is_null())
}

/// `P` of module 325158 when the activity has no art of its own: Xbox's `customPNG`, PlayStation's `lightPNG`
/// (only without a small image). The application icon comes before these; the caller passes whether it has one.
pub(super) fn fallback_art(activity: &Value, has_large: bool) -> Option<&'static str> {
    if has_large {
        return None;
    }
    if is_xbox(activity) {
        return platform_of("xbox").and_then(|platform| platform.custom_png);
    }
    if activity["assets"]["small_image"].is_null() && is_playstation(activity) {
        return platform_of("playstation").map(|platform| platform.light_png);
    }
    None
}

/// The platform of the "Connect {platform}" button of module 802880 `tP` (and 961272 `O`): an Xbox or PlayStation
/// activity without rich presence, for a viewer who has no account of that platform linked.
pub(super) fn connect_offer(activity: &Value, linked: &[&str]) -> Option<&'static Platform> {
    if is_rich(activity) {
        return None;
    }
    let kind = if is_xbox(activity) {
        "xbox"
    } else if is_playstation(activity) {
        "playstation"
    } else {
        return None;
    };
    (!linked.contains(&kind))
        .then(|| platform_of(kind))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn consoles_read_playing_on_their_platform() {
        let ps5 = json!({"type": 0, "name": "Astro Bot", "platform": "ps5"});
        let (text, platform) = heading(&ps5);
        assert_eq!(text, "Playing on PlayStation");
        assert_eq!(platform.map(|p| p.kind), Some("playstation"));
        let ps4 = json!({"type": 0, "name": "Bloodborne", "platform": "ps4"});
        assert_eq!(heading(&ps4).0, "Playing on PlayStation");
        let xbox = json!({"type": 0, "name": "Halo", "platform": "xbox"});
        assert_eq!(heading(&xbox).0, "Playing on Xbox");
        let xbox_app = json!({"type": 0, "name": "Halo", "application_id": XBOX_APPLICATION});
        assert_eq!(heading(&xbox_app).0, "Playing on Xbox");
        let desktop = json!({"type": 0, "name": "Deadlock", "platform": "desktop"});
        assert_eq!(heading(&desktop), ("Playing".to_owned(), None));
    }

    #[test]
    fn a_relayed_session_names_its_platform() {
        let steam = json!({"type": 0, "name": "x", "session_id": "h:steam,abc"});
        let (text, platform) = heading(&steam);
        assert_eq!(text, "Playing on Steam");
        assert_eq!(platform.map(|p| p.kind), Some("steam"));
        let twitch = json!({"type": 1, "name": "x", "session_id": "h:twitch,abc"});
        assert_eq!(heading(&twitch).0, "Streaming Twitch");
        let disabled = json!({"type": 0, "name": "x", "session_id": "h:skype,abc"});
        assert_eq!(heading(&disabled).0, "Playing");
        let watching = json!({"type": 3, "name": "x", "session_id": "h:crunchyroll,1"});
        assert_eq!(heading(&watching).0, "Watching on Crunchyroll");
        let unknown = json!({"type": 0, "name": "x", "session_id": "h:nope,1"});
        assert_eq!(heading(&unknown).0, "Playing");
    }

    #[test]
    fn streams_and_listening_keep_their_headings() {
        let twitch = json!({"type": 1, "name": "Twitch", "url": "https://twitch.tv/a"});
        assert_eq!(heading(&twitch).0, "Streaming Twitch");
        let other = json!({"type": 1, "name": "Other"});
        assert_eq!(heading(&other).0, "Streaming");
        let spotify =
            json!({"type": 2, "name": "Spotify", "details": "Song", "party": {"id": "spotify:1"}});
        let (text, platform) = heading(&spotify);
        assert_eq!(text, "Listening to Spotify");
        assert_eq!(platform.map(|p| p.kind), Some("spotify"));
        let crunchy = json!({"type": 3, "name": "Crunchyroll", "details": "Ep 1", "application_id": CRUNCHYROLL_APPLICATION});
        let (text, platform) = heading(&crunchy);
        assert_eq!(text, "Watching Crunchyroll");
        assert_eq!(platform.map(|p| p.kind), Some("crunchyroll"));
        assert_eq!(heading(&json!({"type": 5})).0, "Competing in");
    }

    #[test]
    fn only_twitch_and_youtube_streams_have_a_watch_url() {
        let twitch = json!({"type": 1, "url": "https://www.twitch.tv/fixture"});
        assert_eq!(
            stream_url(&twitch).as_deref(),
            Some("https://www.twitch.tv/fixture")
        );
        assert!(stream_url(&json!({"type": 1, "url": "http://youtube.com/watch?v=x"})).is_some());
        assert!(stream_url(&json!({"type": 1, "url": "https://kick.com/a"})).is_none());
        assert!(stream_url(&json!({"type": 0, "url": "https://twitch.tv/a"})).is_none());
    }

    #[test]
    fn console_art_falls_back_to_the_platform_picture() {
        let xbox = json!({"type": 0, "name": "Halo", "platform": "xbox"});
        assert_eq!(fallback_art(&xbox, false), Some("fc0b586dbf05772e.png"));
        assert_eq!(fallback_art(&xbox, true), None);
        let ps = json!({"type": 0, "name": "Astro", "platform": "ps5"});
        assert_eq!(fallback_art(&ps, false), Some("ae74d3de4958bf78.png"));
        let ps_small = json!({"type": 0, "platform": "ps5", "assets": {"small_image": "a"}});
        assert_eq!(fallback_art(&ps_small, false), None);
    }

    #[test]
    fn connect_is_offered_for_plain_console_games_without_a_link() {
        let xbox = json!({"type": 0, "name": "Halo", "platform": "xbox"});
        assert_eq!(connect_offer(&xbox, &[]).map(|p| p.kind), Some("xbox"));
        assert_eq!(connect_offer(&xbox, &["xbox"]), None);
        let rich = json!({"type": 0, "name": "Halo", "platform": "xbox", "details": "Campaign"});
        assert_eq!(connect_offer(&rich, &[]), None);
        let ps = json!({"type": 0, "name": "Astro", "platform": "ps4"});
        assert_eq!(
            connect_offer(&ps, &["xbox"]).map(|p| p.kind),
            Some("playstation")
        );
        assert_eq!(connect_offer(&json!({"type": 0, "name": "PC"}), &[]), None);
    }
}
