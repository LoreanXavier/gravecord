//! The CDN addresses of pictures, as the official client builds them.
//!
//! - Module 776231 `kr`: the `size=` a picture is asked for, the smallest step of its ladder at least the wanted
//!   size (or, when asked to, the step just below when the wanted size is at most a quarter over it).
//! - Module 486020: user and member avatars, application icons (`getApplicationIconURL`, `getGameAssetURL`), each
//!   at `kr(size * devicePixelRatio)`, `webp` (Electron can use WebP), `animated=true` only for an `a_` hash.
//! - Module 913659 `getAssetImage` (`uD`): a rich presence asset: `spotify:<id>` is Spotify's image CDN, `mp:<path>`
//!   the media proxy (`format=webp` for a GIF, `animated=true` for a GIF, WebP or AVIF), `twitch:` and `youtube:`
//!   their preview pictures, and a bare asset id the application's asset at `kr(size)` (no pixel ratio here).
//!
//! Everything here is pure string work, so it can be tested and called from a render without cost worth counting.

/// Module 776231 `I`: every `size=` the client asks for.
pub(super) const SIZES: [u32; 31] = [
    16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320, 480,
    512, 600, 640, 1024, 1280, 1536, 2048, 3072, 4096,
];

/// Module 776231 `kr(e, t)`: with `round_down`, the step below `size` when `size` is at most 1.25 times it; else the
/// first step at least `size`, and the largest step past the end of the ladder.
pub(super) fn size_step(size: f32, round_down: bool) -> u32 {
    if round_down
        && let Some(below) = SIZES.iter().rev().find(|step| **step as f32 <= size)
        && size / *below as f32 <= 1.25
    {
        return *below;
    }
    SIZES
        .iter()
        .copied()
        .find(|step| size <= *step as f32)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// `kr(size * devicePixelRatio)`: the file side for a picture drawn `drawn` logical pixels wide in a window of this
/// scale factor (486020 `g`, `R`, `y`, `b`).
pub(crate) fn side_for(drawn: f32, scale: f32) -> u32 {
    size_step(drawn * scale.max(0.5), false)
}

pub(super) const CDN: &str = "https://cdn.discordapp.com";
const MEDIA_PROXY: &str = "https://media.discordapp.net/";
/// 913659 `p`: Spotify's image CDN.
pub(crate) const SPOTIFY_IMAGES: &str = "https://i.scdn.co/image/";
/// The largest side an activity card draws its picture at (`live_activity`'s large card).
pub(crate) const ACTIVITY_ART_DRAWN: f32 = 100.;

fn is_animated(hash: &str) -> bool {
    hash.starts_with("a_")
}

/// 486020 `C`/`g` (`getUserAvatarURL` with a hash): `avatars/{id}/{hash}.webp?size=..`, `&animated=true` when
/// `animate` and the hash is animated.
pub(super) fn user_avatar(user: u64, hash: &str, side: u32, animate: bool) -> String {
    let animated = if animate && is_animated(hash) {
        "&animated=true"
    } else {
        ""
    };
    format!("{CDN}/avatars/{user}/{hash}.webp?size={side}{animated}")
}

/// 486020 `R` (`getGuildMemberAvatarURLSimple`).
pub(super) fn member_avatar(guild: u64, user: u64, hash: &str, side: u32, animate: bool) -> String {
    let animated = if animate && is_animated(hash) {
        "&animated=true"
    } else {
        ""
    };
    format!("{CDN}/guilds/{guild}/users/{user}/avatars/{hash}.webp?size={side}{animated}")
}

/// 486020 `k` (`getGameAssetURL`, the icon of an application record, module 327891 `getIconURL`): a PNG, never
/// WebP (`canWebP: false`), `keep_aspect_ratio=false` unless asked.
pub(super) fn app_icon(application: &str, hash: &str, side: u32) -> String {
    format!("{CDN}/app-icons/{application}/{hash}.png?size={side}&keep_aspect_ratio=false")
}

/// `badge-icons/{icon}.png`: a profile badge (module 652215 `BADGE_ICON`), asked for without a size as the official
/// badge list does.
pub(super) fn badge_icon(icon: &str) -> String {
    format!("{CDN}/badge-icons/{icon}.png")
}

/// The part of a URL path a value takes: `encodeURIComponent`.
fn component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 913659 `g.mp.deserialize`: the path against the media proxy, `format=webp` for a GIF, `animated=true` for a
/// GIF, WebP or AVIF.
fn media_proxy(path: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(MEDIA_PROXY).ok()?.join(path).ok()?;
    let lower = path.to_ascii_lowercase();
    let gif = lower.ends_with(".gif");
    if gif {
        url.query_pairs_mut().append_pair("format", "webp");
    }
    if gif || lower.ends_with(".webp") || lower.ends_with(".avif") {
        url.query_pairs_mut().append_pair("animated", "true");
    }
    Some(url.into())
}

/// 913659 `uD(application, asset, size, ext = "png")`: the picture of a rich presence asset. `size` is the largest
/// side the card asks for (`[160, 160]` for the large image of every activity card, 487887 `iu.LARGE`); a Twitch
/// preview needs both sides, so it is built at `size`x`size`.
pub(super) fn activity_asset(application: Option<&str>, asset: &str, size: u32) -> Option<String> {
    if asset.contains(':') {
        // `let [e, i] = t.split(":")`: the scheme and the part up to a second colon, as the official keeps it.
        let mut parts = asset.split(':');
        let scheme = parts.next()?;
        let value = parts.next().unwrap_or_default();
        return match scheme {
            "spotify" => Some(format!("{SPOTIFY_IMAGES}{}", component(value))),
            "twitch" => Some(format!(
                "https://static-cdn.jtvnw.net/previews-ttv/live_user_{}-{size}x{size}.jpg",
                component(value)
            )),
            "youtube" => Some(format!(
                "https://i.ytimg.com/vi/{}/hqdefault_live.jpg",
                component(value)
            )),
            "mp" => media_proxy(value),
            _ => None,
        };
    }
    let application = application.filter(|id| !id.is_empty())?;
    if asset.is_empty() {
        return None;
    }
    Some(format!(
        "{CDN}/app-assets/{application}/{asset}.png?size={}",
        size_step(size as f32, false)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_take_the_next_step_of_the_ladder() {
        assert_eq!(size_step(16., false), 16);
        assert_eq!(size_step(17., false), 20);
        assert_eq!(size_step(40., false), 40);
        assert_eq!(size_step(120., false), 128);
        assert_eq!(size_step(150., false), 160);
        assert_eq!(size_step(180., false), 240);
        assert_eq!(size_step(241., false), 256);
        assert_eq!(size_step(5000., false), 4096);
        // Round down only when the wanted size is at most a quarter over the step below.
        assert_eq!(size_step(50., true), 48);
        assert_eq!(size_step(70., true), 64);
        assert_eq!(size_step(78., true), 64);
        assert_eq!(size_step(90., true), 80);
        assert_eq!(size_step(110., true), 100);
    }

    #[test]
    fn the_side_follows_the_window_scale() {
        // The full profile's 120px avatar: 128 at 100%, 160 at 125%, 240 at 150% and 200%.
        assert_eq!(side_for(120., 1.), 128);
        assert_eq!(side_for(120., 1.25), 160);
        assert_eq!(side_for(120., 1.5), 240);
        assert_eq!(side_for(120., 2.), 240);
        // The popout's 80px avatar and a 32px member row.
        assert_eq!(side_for(80., 1.), 80);
        assert_eq!(side_for(80., 1.25), 100);
        assert_eq!(side_for(32., 1.), 32);
        assert_eq!(side_for(32., 1.5), 48);
    }

    #[test]
    fn avatars_are_webp_and_animate_only_an_animated_hash() {
        assert_eq!(
            user_avatar(7, "abc", 128, true),
            "https://cdn.discordapp.com/avatars/7/abc.webp?size=128"
        );
        assert_eq!(
            user_avatar(7, "a_abc", 160, true),
            "https://cdn.discordapp.com/avatars/7/a_abc.webp?size=160&animated=true"
        );
        assert_eq!(
            user_avatar(7, "a_abc", 160, false),
            "https://cdn.discordapp.com/avatars/7/a_abc.webp?size=160"
        );
        assert_eq!(
            member_avatar(1, 2, "h", 240, false),
            "https://cdn.discordapp.com/guilds/1/users/2/avatars/h.webp?size=240"
        );
    }

    #[test]
    fn application_icons_and_badges() {
        // As the captures show them (design/dom: `app-icons/{id}/{hash}.png?size=160&keep_aspect_ratio=false`).
        assert_eq!(
            app_icon("1124356208988532856", "e7bb", 160),
            "https://cdn.discordapp.com/app-icons/1124356208988532856/e7bb.png?size=160&keep_aspect_ratio=false"
        );
        assert_eq!(
            badge_icon("2ba85e8026a8614b640c2837bcdfe21b"),
            "https://cdn.discordapp.com/badge-icons/2ba85e8026a8614b640c2837bcdfe21b.png"
        );
    }

    #[test]
    fn activity_assets_resolve_as_get_asset_image() {
        assert_eq!(
            activity_asset(Some("1"), "spotify:ab67616d0000b273", 160).as_deref(),
            Some("https://i.scdn.co/image/ab67616d0000b273")
        );
        assert_eq!(
            activity_asset(None, "spotify:a b", 160).as_deref(),
            Some("https://i.scdn.co/image/a%20b")
        );
        assert_eq!(
            activity_asset(Some("1"), "mp:external/x/https/a.example/p.gif", 160).as_deref(),
            Some(
                "https://media.discordapp.net/external/x/https/a.example/p.gif?format=webp&animated=true"
            )
        );
        assert_eq!(
            activity_asset(Some("1"), "mp:external/x/p.png", 160).as_deref(),
            Some("https://media.discordapp.net/external/x/p.png")
        );
        assert_eq!(
            activity_asset(Some("1"), "mp:attachments/1/2/p.webp", 160).as_deref(),
            Some("https://media.discordapp.net/attachments/1/2/p.webp?animated=true")
        );
        assert_eq!(
            activity_asset(Some("383226320970055681"), "565945770067623946", 160).as_deref(),
            Some(
                "https://cdn.discordapp.com/app-assets/383226320970055681/565945770067623946.png?size=160"
            )
        );
        assert_eq!(
            activity_asset(Some("1"), "twitch:streamer", 160).as_deref(),
            Some("https://static-cdn.jtvnw.net/previews-ttv/live_user_streamer-160x160.jpg")
        );
        assert_eq!(
            activity_asset(Some("1"), "youtube:dQw4w9WgXcQ", 160).as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault_live.jpg")
        );
        // An unknown scheme, or an id with no application, has no picture.
        assert_eq!(activity_asset(Some("1"), "steam:1", 160), None);
        assert_eq!(activity_asset(None, "565945770067623946", 160), None);
    }
}
