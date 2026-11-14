//! The layout decisions of a message embed (module 265705's `em` component and `renderEmbeds` of module 679636),
//! apart from drawing: which media slot an embed gets, whether the small thumbnail shows, how inline fields share
//! rows, how large a video is, and which embeds the accessories leave out.

use dc_state::{Embed, EmbedField, fit_rounded};

/// What `renderMedia` draws under the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Media {
    None,
    /// `renderImage`: the embed's image.
    Image,
    /// `renderImages`: the merged gallery (`embedGalleryImagesWrapper_XyW1ZT`).
    Gallery,
    /// `renderVideo({gifv: false})`: the player with the poster and its play button.
    Video,
    /// `renderVideo({gifv: true})`: a looping, silent clip.
    Gifv,
}

/// `renderMedia` (and `renderVideo`'s own guards: a URL and a thumbnail, and for a GIFV a video).
pub(super) fn media(embed: &Embed, hide_media: bool) -> Media {
    if hide_media {
        return Media::None;
    }
    let video_ready = embed.url.is_some() && embed.thumbnail.is_some();
    let proxied_video = embed
        .video
        .as_ref()
        .is_some_and(|video| video.proxy_url.is_some() && video.width > 0 && video.height > 0);
    match embed.kind.as_str() {
        "gifv" => {
            if video_ready && embed.video.is_some() {
                Media::Gifv
            } else {
                Media::None
            }
        }
        // An audio embed without its audio falls back to the image (the audio player is not ported).
        "audio" => image_or_none(embed),
        _ if embed.video.is_some() => {
            if video_ready || proxied_video {
                Media::Video
            } else {
                Media::None
            }
        }
        _ if !embed.images.is_empty() => Media::Gallery,
        _ => image_or_none(embed),
    }
}

fn image_or_none(embed: &Embed) -> Media {
    if embed.image.is_some() {
        Media::Image
    } else {
        Media::None
    }
}

/// `renderAll`: the small right-hand thumbnail shows when no media is drawn; for rich and audio embeds, whenever
/// there is no video (so a rich embed can have both an image and a thumbnail).
pub(super) fn shows_thumbnail(embed: &Embed, hide_media: bool) -> bool {
    if hide_media || embed.thumbnail.is_none() {
        return false;
    }
    if matches!(embed.kind.as_str(), "rich" | "audio") {
        embed.video.is_none()
    } else {
        media(embed, hide_media) == Media::None
    }
}

/// `renderAll`: image, video and GIFV embeds (`Mg` of module 768570) never draw their description.
pub(super) fn shows_description(embed: &Embed) -> bool {
    !matches!(embed.kind.as_str(), "image" | "video" | "gifv")
}

/// `isInline`: drawn as the bare media (`inlineMediaEmbed_XyW1ZT`) unless media is hidden.
pub(super) fn is_inline(embed: &Embed, hide_media: bool) -> bool {
    !hide_media && embed.is_inline_media()
}

/// `usesJustifiedAutoStyle`: the card stretches to its `max-width` instead of shrinking to its content.
pub(super) fn justified(embed: &Embed) -> bool {
    matches!(embed.kind.as_str(), "image" | "video" | "gifv")
        || matches!(embed.kind.as_str(), "rich" | "article")
            && (embed.video.is_some() || embed.image.is_some())
}

/// One field placed in the 12-column grid: its index and its `grid-column` start and end (1-based, end exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Placed {
    pub index: usize,
    pub start: u32,
    pub end: u32,
}

/// `renderFields`: inline fields share a row, up to three; a field that is not inline ends the row before it and
/// stands alone. Each of a row's `n` fields spans `12 / n` columns.
pub(super) fn field_rows(fields: &[EmbedField]) -> Vec<Vec<Placed>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut current: Option<Vec<usize>> = None;
    for (index, field) in fields.iter().enumerate() {
        if !field.inline
            && let Some(row) = current.take()
        {
            rows.push(row);
        }
        let row = current.get_or_insert_with(Vec::new);
        row.push(index);
        if row.len() == 3 || !field.inline {
            rows.extend(current.take());
        }
    }
    rows.extend(current);
    rows.into_iter()
        .map(|row| {
            let span = 12 / row.len() as u32;
            row.into_iter()
                .enumerate()
                .map(|(position, index)| {
                    let start = position as u32 * span;
                    Placed {
                        index,
                        start: start + 1,
                        end: start + span + 1,
                    }
                })
                .collect()
        })
        .collect()
}

/// `ed` in module 265705: the thumbnail's size, or the video's when there is one, fitted into the embed's media box
/// (`Uj`), then at least 150 by 144.
pub(super) fn video_size(embed: &Embed) -> Option<(f64, f64)> {
    let (width, height) = match (&embed.video, &embed.thumbnail) {
        (Some(video), _) if video.width > 0 && video.height > 0 => {
            (f64::from(video.width), f64::from(video.height))
        }
        (_, Some(poster)) => (f64::from(poster.width), f64::from(poster.height)),
        _ => return None,
    };
    let (max_width, max_height) = embed.max_media();
    let (width, height) = fit_rounded(width, height, max_width, max_height, 0., 0.);
    Some((width.max(150.), height.max(144.)))
}

/// A GIFV's drawn size: its poster's size fitted into the media box (`eh` passes the thumbnail's size).
pub(super) fn gifv_size(embed: &Embed) -> Option<(f64, f64)> {
    let poster = embed.thumbnail.as_ref()?;
    Some(embed.fit(poster))
}

/// The thumbnail fitted into 80 by 80 (`maxThumbnailWidth` / `maxThumbnailHeight`).
pub(super) fn thumbnail_size(embed: &Embed) -> Option<(f64, f64)> {
    let thumbnail = embed.thumbnail.as_ref()?;
    Some(fit_rounded(
        f64::from(thumbnail.width),
        f64::from(thumbnail.height),
        80.,
        80.,
        0.,
        0.,
    ))
}

/// The card's own width rule: `Some(width)` when it is stretched to that width (`justifyAuto` with a `max-width`),
/// `None` when it shrinks to its content up to the grid's 516 plus its borders.
pub(super) fn fixed_width(embed: &Embed) -> Option<f64> {
    if !justified(embed) {
        return None;
    }
    embed.max_width()
}

fn host_and_path(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].to_ascii_lowercase();
    let path = rest[end..]
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_owned();
    Some((host, path))
}

fn discord_host(host: &str) -> bool {
    let host = host
        .strip_prefix("canary.")
        .or_else(|| host.strip_prefix("ptb."))
        .or_else(|| host.strip_prefix("www."))
        .unwrap_or(host);
    host == "discord.com" || host == "discordapp.com"
}

fn digits(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
}

/// `p6`, `V`, `G8` and `Xk` of module 659674: article embeds of a server shop, the shop, a game shop or a game page,
/// which have their own cards.
fn shop_or_game_article(embed: &Embed) -> bool {
    if embed.kind != "article" {
        return false;
    }
    let Some(url) = embed.url.as_deref() else {
        return false;
    };
    let Some((host, path)) = host_and_path(url) else {
        return false;
    };
    let query = url.split_once('?').map_or("", |(_, query)| query);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let discord = discord_host(&host);
    // `p6`: /channels/<guild>/shop and /channels/<guild>/shop/<listing>.
    let guild_shop = discord
        && matches!(parts.as_slice(), ["channels", guild, "shop"] if digits(guild))
        || discord
            && matches!(parts.as_slice(), ["channels", guild, "shop", item] if digits(guild) && digits(item));
    // `V`: https://(canary.|ptb.)discord(app).com/shop or staging.discord.co/shop.
    let store = url.starts_with("https://")
        && (host == "staging.discord.co" || discord && !host.starts_with("www."))
        && path.starts_with("/shop");
    // `G8`: game shops, by channel, by id, by sku list, or the shop's game-shops tab.
    let game_shop = discord
        && (matches!(parts.as_slice(), ["channels", guild, "game-shop", app, sku, ..] if digits(guild) && digits(app) && digits(sku))
            || matches!(parts.as_slice(), ["game-shop", app, sku, ..] if digits(app) && digits(sku))
            || matches!(parts.as_slice(), ["game-shop", app] | ["game-shop", app, ""] if digits(app))
                && query.split('&').any(|pair| pair.starts_with("skuIds="))
            || path == "/shop"
                && query.split('&').any(|pair| pair == "tab=game-shops")
                && query
                    .split('&')
                    .any(|pair| pair.strip_prefix("applicationId=").is_some_and(digits))
                && query
                    .split('&')
                    .any(|pair| pair.strip_prefix("skuId=").is_some_and(digits)));
    // `Xk`: /games/<id> with an optional slug.
    let game = discord
        && match parts.as_slice() {
            ["games", id] => digits(id),
            ["games", id, slug] | ["games", id, slug, ""] => {
                digits(id)
                    && slug
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }
            _ => false,
        };
    guild_shop || store || game_shop || game
}

/// `t` of module 729160: a link to a Conjure game.
fn conjure_link(url: &str) -> bool {
    let Some((host, path)) = host_and_path(url) else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host != "conjure.town" && host != "conjure.city" {
        return false;
    }
    let parts: Vec<&str> = path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .collect();
    let id = |part: &str| digits(part) && (17..=20).contains(&part.len());
    let slug = |part: &str| {
        (1..=48).contains(&part.len())
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    let token = |part: &str| {
        part.split_once('.').is_some_and(|(head, tail)| {
            let ok = |text: &str, range: std::ops::RangeInclusive<usize>| {
                range.contains(&text.len())
                    && text
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            };
            ok(head, 16..=200) && ok(tail, 16..=64)
        })
    };
    match parts.as_slice() {
        [first, second] => id(first) && slug(second),
        [first, second, third] => id(first) && slug(second) && token(third),
        _ => false,
    }
}

/// `renderEmbeds` in module 679636: the embeds drawn by other components (automod alerts, safety notices, voice
/// channel cards, poll results and the others of `zx` in module 581619), gifts, shop and game pages, and Conjure links
/// are left out of the embed list. (`nL`, profile links, is behind an experiment the pin does not enable by default;
/// content inventory entries need the message's components.)
pub(super) fn left_out(embed: &Embed) -> bool {
    if matches!(
        embed.kind.as_str(),
        "auto_moderation_message"
            | "auto_moderation_notification"
            | "age_verification_system_notification"
            | "application_news"
            | "post_preview"
            | "safety_policy_notice"
            | "safety_system_notification"
            | "voice_channel"
            | "poll_result"
            | "gift"
    ) {
        return true;
    }
    if shop_or_game_article(embed) {
        return true;
    }
    embed.kind != "rich" && embed.url.as_deref().is_some_and(conjure_link)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn embed(value: serde_json::Value) -> Embed {
        Embed::from_value(&value)
    }

    fn field(inline: bool) -> EmbedField {
        EmbedField {
            name: "n".into(),
            value: "v".into(),
            inline,
        }
    }

    #[test]
    fn a_video_tweet_gets_the_video_slot_not_the_side_thumbnail() {
        // The fault the owner reported: FxTwitter's rich embed with a video and its poster as thumbnail.
        let tweet = embed(json!({
            "type": "rich", "url": "https://fxtwitter.com/a/status/1/video/1",
            "author": {"name": "Heisenberg (@burakalpekn)", "proxy_icon_url": "https://p/a.jpg"},
            "description": "text",
            "thumbnail": {"url": "https://p/t.jpg", "width": 720, "height": 1280},
            "video": {"url": "https://v/v.mp4", "proxy_url": "https://p/v.mp4", "width": 720, "height": 1280},
            "footer": {"text": "FxTwitter", "proxy_icon_url": "https://p/f.png"},
            "timestamp": "2026-11-11T08:28:00+00:00"
        }));
        assert_eq!(media(&tweet, false), Media::Video);
        assert!(!shows_thumbnail(&tweet, false));
        assert!(!is_inline(&tweet, false));
        assert!(shows_description(&tweet));
        assert_eq!(video_size(&tweet), Some((169., 300.)));
        assert!(justified(&tweet));
        assert_eq!(fixed_width(&tweet), None, "169 wide: no fixed width");
        // With "When posted as links to chat" off the media goes, and so does the thumbnail.
        assert_eq!(media(&tweet, true), Media::None);
        assert!(!shows_thumbnail(&tweet, true));
    }

    #[test]
    fn media_slots_follow_render_media_and_thumbnails_follow_render_all() {
        let link = embed(json!({"type": "link", "url": "https://x", "title": "t",
            "thumbnail": {"url": "https://x/t.png", "width": 400, "height": 400}}));
        assert_eq!(media(&link, false), Media::None);
        assert!(shows_thumbnail(&link, false));
        assert_eq!(thumbnail_size(&link), Some((80., 80.)));
        let article = embed(json!({"type": "article", "url": "https://x", "title": "t",
            "thumbnail": {"url": "https://x/t.png", "width": 1200, "height": 600}}));
        assert_eq!(media(&article, false), Media::Image);
        assert!(!shows_thumbnail(&article, false));
        assert_eq!(fixed_width(&article), Some(432.));
        // A rich embed may draw both its image and its thumbnail.
        let rich = embed(json!({"type": "rich", "title": "t",
            "image": {"url": "https://x/i.png", "width": 200, "height": 100},
            "thumbnail": {"url": "https://x/t.png", "width": 40, "height": 40}}));
        assert_eq!(media(&rich, false), Media::Image);
        assert!(shows_thumbnail(&rich, false));
        assert_eq!(fixed_width(&rich), None, "200 wide is under 300");
        let gifv = embed(json!({"type": "gifv", "url": "https://tenor.com/x",
            "thumbnail": {"url": "https://x/p.png", "width": 498, "height": 280},
            "video": {"url": "https://x/v.mp4", "width": 498, "height": 280}}));
        assert_eq!(media(&gifv, false), Media::Gifv);
        assert!(is_inline(&gifv, false) && !shows_thumbnail(&gifv, false));
        assert_eq!(gifv_size(&gifv), Some((400., 225.)));
        assert!(!shows_description(&gifv));
        // A YouTube video: the video slot, 400x225 in a 432 card.
        let youtube = embed(
            json!({"type": "video", "url": "https://www.youtube.com/watch?v=x",
            "title": "t", "provider": {"name": "YouTube"},
            "thumbnail": {"url": "https://i.ytimg.com/t.jpg", "width": 1280, "height": 720},
            "video": {"url": "https://www.youtube.com/embed/x", "width": 1280, "height": 720}}),
        );
        assert_eq!(media(&youtube, false), Media::Video);
        assert_eq!(video_size(&youtube), Some((400., 225.)));
        assert_eq!(fixed_width(&youtube), Some(432.));
        assert!(!is_inline(&youtube, false), "a title keeps the card");
        // A small video is at least 150 by 144.
        let tiny = embed(json!({"type": "video", "url": "https://x",
            "thumbnail": {"url": "https://x/t.png", "width": 64, "height": 48},
            "video": {"url": "https://x/v.mp4", "width": 64, "height": 48}}));
        assert_eq!(video_size(&tiny), Some((150., 144.)));
        // A video embed without a URL has no media and no thumbnail.
        let lost = embed(json!({"type": "video",
            "thumbnail": {"url": "https://x/t.png", "width": 64, "height": 48},
            "video": {"url": "https://x/v.mp4", "width": 64, "height": 48}}));
        assert_eq!(media(&lost, false), Media::None);
        assert!(shows_thumbnail(&lost, false));
    }

    #[test]
    fn inline_fields_share_rows_of_up_to_three() {
        let placed = |rows: Vec<Vec<Placed>>| -> Vec<Vec<(usize, u32, u32)>> {
            rows.into_iter()
                .map(|row| row.into_iter().map(|p| (p.index, p.start, p.end)).collect())
                .collect()
        };
        assert_eq!(
            placed(field_rows(&[
                field(true),
                field(true),
                field(true),
                field(true)
            ])),
            vec![vec![(0, 1, 5), (1, 5, 9), (2, 9, 13)], vec![(3, 1, 13)]]
        );
        assert_eq!(
            placed(field_rows(&[
                field(true),
                field(false),
                field(true),
                field(true)
            ])),
            vec![
                vec![(0, 1, 13)],
                vec![(1, 1, 13)],
                vec![(2, 1, 7), (3, 7, 13)]
            ]
        );
        assert_eq!(
            placed(field_rows(&[field(false), field(false)])),
            vec![vec![(0, 1, 13)], vec![(1, 1, 13)]]
        );
        assert!(field_rows(&[]).is_empty());
    }

    #[test]
    fn embeds_drawn_elsewhere_are_left_out() {
        for kind in [
            "auto_moderation_message",
            "poll_result",
            "voice_channel",
            "gift",
        ] {
            assert!(left_out(&embed(json!({"type": kind}))), "{kind}");
        }
        for url in [
            "https://discord.com/channels/123/shop",
            "https://canary.discord.com/channels/123/shop/456",
            "https://discord.com/shop",
            "https://discord.com/game-shop/1/2",
            "https://discord.com/game-shop/1?skuIds=2",
            "https://discord.com/shop?tab=game-shops&applicationId=1&skuId=2",
            "https://discord.com/games/1/some-game",
        ] {
            assert!(
                left_out(&embed(json!({"type": "article", "url": url}))),
                "{url}"
            );
        }
        assert!(!left_out(&embed(
            json!({"type": "link", "url": "https://discord.com/shop"})
        )));
        assert!(!left_out(&embed(
            json!({"type": "article", "url": "https://example.com/shop"})
        )));
        assert!(left_out(&embed(json!({"type": "link",
            "url": "https://www.conjure.town/12345678901234567/game"}))));
        assert!(!left_out(&embed(json!({"type": "rich",
            "url": "https://conjure.town/12345678901234567/game"}))));
    }

    #[test]
    fn proxied_video_without_thumbnail_still_resolves_size() {
        let bare = embed(json!({
            "type": "video",
            "url": "https://cdn.example/clip.mp4",
            "video": {"url": "https://cdn.example/clip.mp4", "proxy_url": "https://media.example/clip.mp4", "width": 640, "height": 360}
        }));
        assert_eq!(media(&bare, false), Media::Video);
        assert_eq!(media(&bare, true), Media::None);
        assert!(video_size(&bare).is_some_and(|(w, h)| w >= 150. && h >= 144.));
    }
}
