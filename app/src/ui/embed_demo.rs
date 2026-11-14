//! Capture only: `embeds demo <set>` hands the root `MESSAGE_CREATE` / `MESSAGE_UPDATE` dispatches whose embeds cover
//! the cases of the embed renderer, shaped like gateway payloads (the GIFV is the one recorded in
//! `design/snapshot/TRAFFIC/gif-send.json`). In memory only: nothing is sent anywhere.
//!
//! Sets: `video` (a FxTwitter video post, a YouTube video, a direct video link), `cards` (an article with a large
//! image, a link with a side thumbnail, a rich embed with fields, footer and time, a gallery of four), `inline` (a
//! GIFV and an image posted as bare links), `own` (an embed on the account's own message, for the remove button),
//! `late` (a message whose embed arrives in an update), `suppress` (an update that removes the embeds of the last
//! demo message).

use super::Root;
use crate::net::Update;
use gpui::Context;
use serde_json::{Value, json};
use std::sync::Arc;

/// Public pictures the harness can fetch: the GIFV poster and clip recorded in TRAFFIC, and a default avatar.
const POSTER: &str = "https://images-ext-1.discordapp.net/external/HiSr_bDAjc2y8gxnBAyPJxT-oJGMWPDklJWsxoQN-no/https/static.klipy.com/ii/e293a233a303a98e471f78d04e13a1b0/13/d1/Sz4nTvkY.webp";
const POSTER_SOURCE: &str =
    "https://static.klipy.com/ii/e293a233a303a98e471f78d04e13a1b0/13/d1/Sz4nTvkY.webp";
const CLIP: &str = "https://images-ext-1.discordapp.net/external/cjwlt1zSiX6ojRkELl3dGJIWXy3uqWFF-G_O7l3xTzw/https/static.klipy.com/ii/e293a233a303a98e471f78d04e13a1b0/13/d1/74Glzt11NLU6TCt.mp4";
const CLIP_SOURCE: &str =
    "https://static.klipy.com/ii/e293a233a303a98e471f78d04e13a1b0/13/d1/74Glzt11NLU6TCt.mp4";
const ICON: &str = "https://cdn.discordapp.com/embed/avatars/0.png";
const ICON_TWO: &str = "https://cdn.discordapp.com/embed/avatars/2.png";

fn picture(width: u32, height: u32) -> Value {
    json!({"url": POSTER_SOURCE, "proxy_url": POSTER, "width": width, "height": height})
}

/// The sets' embeds, as `(content, embeds)`.
pub(super) fn set(name: &str, two_days_ago: &str) -> Option<Vec<(String, Value)>> {
    let tweet = "https://fxtwitter.com/burakalpekn/status/1988000000000000000/video/1";
    Some(match name {
        "video" => vec![
            (
                tweet.to_owned(),
                json!([{
                    "type": "rich", "url": tweet, "color": 6_465_461,
                    "description": "Yeni videom yayında 🎬\n\n💬 17   🔁 2   ❤️ 273   👁️ 57.0K ",
                    "author": {"name": "Heisenberg (@burakalpekn)", "url": "https://x.com/burakalpekn",
                               "icon_url": ICON, "proxy_icon_url": ICON},
                    "footer": {"text": "FxTwitter", "icon_url": ICON_TWO, "proxy_icon_url": ICON_TWO},
                    "timestamp": two_days_ago,
                    "thumbnail": picture(720, 1280),
                    "video": {"url": CLIP_SOURCE, "proxy_url": CLIP, "width": 720, "height": 1280}
                }]),
            ),
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
                json!([{
                    "type": "video", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "color": 16_711_680,
                    "title": "A video on YouTube",
                    "provider": {"name": "YouTube", "url": "https://www.youtube.com"},
                    "author": {"name": "A Channel", "url": "https://www.youtube.com/@channel"},
                    "thumbnail": picture(1280, 720),
                    "video": {"url": "https://www.youtube.com/embed/dQw4w9WgXcQ", "width": 1280, "height": 720}
                }]),
            ),
            (
                CLIP_SOURCE.to_owned(),
                json!([{
                    "type": "video", "url": CLIP_SOURCE,
                    "thumbnail": picture(640, 370),
                    "video": {"url": CLIP_SOURCE, "proxy_url": CLIP, "width": 640, "height": 370}
                }]),
            ),
        ],
        "cards" => vec![
            (
                "https://github.com/zed-industries/zed".to_owned(),
                json!([{
                    "type": "article", "url": "https://github.com/zed-industries/zed",
                    "title": "GitHub - zed-industries/zed: Code at the speed of thought",
                    "description": "Code at the speed of thought – Zed is a high-performance, multiplayer code editor.",
                    "provider": {"name": "GitHub"}, "color": 1_976_615,
                    "thumbnail": picture(1200, 600)
                }]),
            ),
            (
                "https://example.com/page".to_owned(),
                json!([{
                    "type": "link", "url": "https://example.com/page",
                    "title": "A page with a side thumbnail",
                    "description": "Link embeds keep their thumbnail at the top right, at most 80 by 80.",
                    "provider": {"name": "Example", "url": "https://example.com"},
                    "thumbnail": picture(498, 288)
                }]),
            ),
            (
                "a bot's rich embed".to_owned(),
                json!([{
                    "type": "rich", "title": "Fields", "url": "https://example.com/fields", "color": 5_793_266,
                    "author": {"name": "Author without an icon"},
                    "description": "**Bold**, *italic*, `code` and a [link](https://example.com).",
                    "fields": [
                        {"name": "One", "value": "inline", "inline": true},
                        {"name": "Two", "value": "inline", "inline": true},
                        {"name": "Three", "value": "inline", "inline": true},
                        {"name": "Four", "value": "inline, alone in its row", "inline": true},
                        {"name": "Five", "value": "not inline", "inline": false},
                        {"name": "Six", "value": "inline", "inline": true},
                        {"name": "Seven", "value": "inline", "inline": true}
                    ],
                    "thumbnail": picture(249, 144),
                    "footer": {"text": "A footer without an icon"},
                    "timestamp": two_days_ago
                }]),
            ),
            (
                "https://example.com/album".to_owned(),
                json!([
                    {"type": "rich", "url": "https://example.com/album", "title": "A gallery of four",
                     "description": "Four embeds with one address merge into one.", "image": picture(498, 288)},
                    {"type": "rich", "url": "https://example.com/album", "image": picture(498, 288)},
                    {"type": "rich", "url": "https://example.com/album", "image": picture(498, 288)},
                    {"type": "rich", "url": "https://example.com/album", "image": picture(498, 288)}
                ]),
            ),
        ],
        "inline" => vec![
            (
                "https://klipy.com/gifs/life-saturday".to_owned(),
                json!([{
                    "type": "gifv", "url": "https://klipy.com/gifs/life-saturday",
                    "title": "Life When It's Saturday: Relaxing Dog",
                    "provider": {"name": "Klipy", "url": "https://klipy.com"},
                    "thumbnail": picture(498, 288),
                    "video": {"url": CLIP_SOURCE, "proxy_url": CLIP, "width": 640, "height": 370}
                }]),
            ),
            (
                POSTER_SOURCE.to_owned(),
                json!([{"type": "image", "url": POSTER_SOURCE, "thumbnail": picture(498, 288)}]),
            ),
        ],
        "own" | "late" => vec![(
            "https://example.com/own".to_owned(),
            json!([{
                "type": "article", "url": "https://example.com/own", "title": "An embed on my own message",
                "description": "Hover it: the remove button is at its top right.",
                "provider": {"name": "Example"}, "thumbnail": picture(1200, 600)
            }]),
        )],
        _ => return None,
    })
}

fn solid_png(width: u32, height: u32, bg: [u8; 3], accent: [u8; 3]) -> Arc<gpui::Image> {
    let mut img = image::RgbImage::from_pixel(width, height, image::Rgb(bg));
    let bar_w = (width / 5).max(8);
    let y0 = height / 4;
    let y1 = (height * 3) / 4;
    for y in y0..y1 {
        for x in 0..bar_w.min(width) {
            img.put_pixel(x, y, image::Rgb(accent));
        }
    }
    let mut bytes = Vec::new();
    let _ = img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes))
}

fn demo_mp4_fixture() -> Arc<Vec<u8>> {
    let palette = [
        [40, 110, 220],
        [40, 110, 220],
        [40, 110, 220],
        [40, 195, 115],
        [40, 195, 115],
        [40, 195, 115],
        [235, 165, 45],
        [235, 165, 45],
        [235, 165, 45],
        [220, 60, 75],
        [220, 60, 75],
        [220, 60, 75],
    ];
    Arc::new(
        crate::video_vod::synthetic_mp4(320, 180, 3, &palette, 3)
            .expect("offline capture synthetic MP4"),
    )
}

impl Root {
    pub(crate) fn script_embeds(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        if let ["video", rest @ ..] = words {
            return self.script_video_cx(rest, cx);
        }
        let ["demo", name] = words else {
            return "embeds: usage `embeds demo <video|video-att|video-fxtwitter|video-mosaic|spoiler-audio|cards|inline|own|late|suppress>`".to_owned();
        };
        let Some(channel) = self.channel else {
            return "embeds: no channel is open".to_owned();
        };
        let now = chrono::Utc::now();
        if *name == "spoiler-audio" {
            let base = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
            let spoiler_embed_id = (base + 301).to_string();
            let audio_embed_id = (base + 302).to_string();
            let spoiler_att_msg_id = (base + 303).to_string();
            let spoiler_att_id = (base + 401).to_string();
            let poster_spoiler = solid_png(400, 220, [185, 55, 95], [250, 225, 110]);
            let poster_att = solid_png(360, 200, [40, 145, 175], [240, 250, 255]);
            self.images.insert(
                format!("embed:{spoiler_embed_id}:0:thumbnail"),
                poster_spoiler,
            );
            self.images
                .insert(format!("attachment:{spoiler_att_id}"), poster_att);
            self.update(
                Update::Dispatch {
                    name: "MESSAGE_CREATE".to_owned(),
                    data: json!({
                        "id": spoiler_embed_id,
                        "channel_id": channel.to_string(),
                        "type": 0,
                        "flags": 0,
                        "content": "Check ||https://example.com/secret|| spoiler link",
                        "author": {"id": "900", "username": "ada"},
                        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                        "embeds": [{
                            "type": "article",
                            "url": "https://example.com/secret",
                            "title": "Hidden Plot **Twist** & *Surprise*",
                            "description": "This card embed is blurred under a 44px Gaussian spoiler veil until clicked.",
                            "provider": {"name": "Example"},
                            "color": 0x5865F2,
                            "thumbnail": {
                                "url": "https://media.example/secret.png",
                                "proxy_url": "https://media.example/secret.png",
                                "width": 400,
                                "height": 220
                            }
                        }],
                        "attachments": [],
                        "mentions": [],
                        "mention_roles": []
                    }),
                },
                cx,
            );
            self.update(
                Update::Dispatch {
                    name: "MESSAGE_CREATE".to_owned(),
                    data: json!({
                        "id": audio_embed_id,
                        "channel_id": channel.to_string(),
                        "type": 0,
                        "flags": 0,
                        "content": "https://audio.example/track.mp3",
                        "author": {"id": "900", "username": "ada"},
                        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                        "embeds": [{
                            "type": "audio",
                            "url": "https://audio.example/track",
                            "title": "Ambient **Synth** Preview",
                            "provider": {"name": "Bandcamp"},
                            "color": 0x1DA0C3,
                            "audio": {
                                "url": "https://audio.example/track.mp3",
                                "proxy_url": "https://media.example/track.mp3",
                                "content_type": "audio/mpeg"
                            }
                        }],
                        "attachments": [],
                        "mentions": [],
                        "mention_roles": []
                    }),
                },
                cx,
            );
            self.update(
                Update::Dispatch {
                    name: "MESSAGE_CREATE".to_owned(),
                    data: json!({
                        "id": spoiler_att_msg_id,
                        "channel_id": channel.to_string(),
                        "type": 0,
                        "flags": 0,
                        "content": "Sent spoiler image attachment",
                        "author": {"id": "900", "username": "ada"},
                        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                        "embeds": [],
                        "attachments": [{
                            "id": spoiler_att_id,
                            "filename": "SPOILER_ending.png",
                            "flags": 8,
                            "size": 24_000,
                            "url": "https://cdn.example/SPOILER_ending.png",
                            "proxy_url": "https://media.example/SPOILER_ending.png",
                            "content_type": "image/png",
                            "width": 360,
                            "height": 200
                        }],
                        "mentions": [],
                        "mention_roles": []
                    }),
                },
                cx,
            );
            cx.notify();
            return format!("embeds: dispatched spoiler-audio demo to {channel}");
        }
        if matches!(*name, "video-att" | "video-fxtwitter" | "video-mosaic") {
            let base = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
            let author = match self.stores.current_user.as_ref() {
                Some(me) => json!({"id": me.id.to_string(), "username": me.username}),
                None => json!({"id": "900", "username": "ada"}),
            };
            let mp4 = demo_mp4_fixture();
            let poster_att = solid_png(320, 180, [32, 72, 160], [235, 240, 255]);
            let poster_yt = solid_png(320, 180, [130, 38, 46], [250, 215, 215]);
            match *name {
                "video-att" => {
                    let msg_id = (base + 101).to_string();
                    let att_id = (base + 201).to_string();
                    let proxy = "https://media.example/demo_clip.mp4";
                    self.seed_video_bytes(proxy, mp4);
                    self.images
                        .insert(format!("attachment:{att_id}"), poster_att);
                    self.update(
                        Update::Dispatch {
                            name: "MESSAGE_CREATE".to_owned(),
                            data: json!({
                                "id": msg_id,
                                "channel_id": channel.to_string(),
                                "type": 0,
                                "flags": 0,
                                "content": "Offline H.264 attachment clip",
                                "author": author,
                                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                                "embeds": [],
                                "attachments": [{
                                    "id": att_id,
                                    "filename": "demo_clip.mp4",
                                    "size": 48_000,
                                    "url": "https://cdn.example/demo_clip.mp4",
                                    "proxy_url": proxy,
                                    "content_type": "video/mp4",
                                    "width": 320,
                                    "height": 180
                                }],
                                "mentions": [],
                                "mention_roles": []
                            }),
                        },
                        cx,
                    );
                }
                "video-fxtwitter" => {
                    let fx_id = (base + 102).to_string();
                    let yt_id = (base + 103).to_string();
                    let fx_proxy = "https://media.example/fx_clip.mp4";
                    self.seed_video_bytes(fx_proxy, mp4);
                    self.images
                        .insert(format!("embed:{fx_id}:0:thumbnail"), poster_att);
                    self.images
                        .insert(format!("embed:{yt_id}:0:thumbnail"), poster_yt);
                    self.update(
                        Update::Dispatch {
                            name: "MESSAGE_CREATE".to_owned(),
                            data: json!({
                                "id": fx_id,
                                "channel_id": channel.to_string(),
                                "type": 0,
                                "flags": 0,
                                "content": "https://fxtwitter.com/example/status/1",
                                "author": {"id": "900", "username": "ada"},
                                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                                "embeds": [{
                                    "type": "video",
                                    "url": "https://fxtwitter.com/example/status/1",
                                    "title": "FxTwitter Inline Video",
                                    "description": "Proxied MP4 stream plays inline.",
                                    "provider": {"name": "FxEmbed"},
                                    "color": 0x1DA1F2,
                                    "thumbnail": {
                                        "url": "https://media.example/fx_thumb.webp",
                                        "proxy_url": "https://media.example/fx_thumb.webp",
                                        "width": 320,
                                        "height": 180
                                    },
                                    "video": {
                                        "url": "https://video.example/fx_clip.mp4",
                                        "proxy_url": fx_proxy,
                                        "width": 320,
                                        "height": 180
                                    }
                                }],
                                "attachments": [],
                                "mentions": [],
                                "mention_roles": []
                            }),
                        },
                        cx,
                    );
                    self.update(
                        Update::Dispatch {
                            name: "MESSAGE_CREATE".to_owned(),
                            data: json!({
                                "id": yt_id,
                                "channel_id": channel.to_string(),
                                "type": 0,
                                "flags": 0,
                                "content": "https://www.youtube.com/watch?v=example",
                                "author": {"id": "900", "username": "ada"},
                                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                                "embeds": [{
                                    "type": "video",
                                    "url": "https://www.youtube.com/watch?v=example",
                                    "title": "External YouTube Embed",
                                    "provider": {"name": "YouTube"},
                                    "color": 0xFF0000,
                                    "thumbnail": {
                                        "url": "https://i.ytimg.com/vi/example/hqdefault.jpg",
                                        "proxy_url": "https://media.example/yt_thumb.jpg",
                                        "width": 320,
                                        "height": 180
                                    },
                                    "video": {
                                        "url": "https://www.youtube.com/embed/example",
                                        "width": 320,
                                        "height": 180
                                    }
                                }],
                                "attachments": [],
                                "mentions": [],
                                "mention_roles": []
                            }),
                        },
                        cx,
                    );
                }
                _ => {
                    let msg_id = (base + 104).to_string();
                    let vid_id = (base + 204).to_string();
                    let img_id = (base + 205).to_string();
                    let proxy = "https://media.example/mosaic_clip.mp4";
                    self.seed_video_bytes(proxy, mp4);
                    self.images
                        .insert(format!("attachment:{vid_id}"), poster_att);
                    self.images
                        .insert(format!("attachment:{img_id}"), poster_yt);
                    self.update(
                        Update::Dispatch {
                            name: "MESSAGE_CREATE".to_owned(),
                            data: json!({
                                "id": msg_id,
                                "channel_id": channel.to_string(),
                                "type": 0,
                                "flags": 0,
                                "content": "Mosaic with video and image",
                                "author": author,
                                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                                "embeds": [],
                                "attachments": [
                                    {
                                        "id": vid_id,
                                        "filename": "mosaic_clip.mp4",
                                        "size": 48_000,
                                        "url": "https://cdn.example/mosaic_clip.mp4",
                                        "proxy_url": proxy,
                                        "content_type": "video/mp4",
                                        "width": 320,
                                        "height": 180
                                    },
                                    {
                                        "id": img_id,
                                        "filename": "still.png",
                                        "size": 12_000,
                                        "url": "https://cdn.example/still.png",
                                        "proxy_url": "https://media.example/still.png",
                                        "content_type": "image/png",
                                        "width": 320,
                                        "height": 180
                                    }
                                ],
                                "mentions": [],
                                "mention_roles": []
                            }),
                        },
                        cx,
                    );
                }
            }
            cx.notify();
            return format!("embeds: dispatched offline video demo {name} to {channel}");
        }
        if *name == "suppress" {
            let last = self
                .stores
                .messages
                .get(&channel)
                .and_then(|list| list.iter().rev().find(|known| !known.embeds.is_empty()))
                .map(|known| (known.id, known.flags));
            let Some((id, flags)) = last else {
                return "embeds: no message with embeds".to_owned();
            };
            self.update(
                Update::Dispatch {
                    name: "MESSAGE_UPDATE".to_owned(),
                    data: json!({"id": id.to_string(), "channel_id": channel.to_string(),
                                 "flags": flags | super::embed::SUPPRESS_EMBEDS, "embeds": []}),
                },
                cx,
            );
            return format!("embeds: suppressed the embeds of {id}");
        }
        let two_days_ago =
            (now - chrono::Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let Some(messages) = set(name, &two_days_ago) else {
            return format!("embeds: no demo set {name}");
        };
        let own = matches!(*name, "own" | "late");
        let author = match (own, self.stores.current_user.as_ref()) {
            (true, Some(me)) => json!({"id": me.id.to_string(), "username": me.username}),
            _ => json!({"id": "900", "username": "ada"}),
        };
        let base = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
        let count = messages.len();
        for (offset, (content, embeds)) in messages.into_iter().enumerate() {
            let id = (base + offset as u64 + 1).to_string();
            let mut data = json!({
                "id": id, "channel_id": channel.to_string(), "type": 0, "flags": 0,
                "content": content, "author": author,
                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "embeds": if *name == "late" { json!([]) } else { embeds.clone() },
                "attachments": [], "mentions": [], "mention_roles": []
            });
            if *name == "late" {
                self.update(
                    Update::Dispatch {
                        name: "MESSAGE_CREATE".to_owned(),
                        data: data.clone(),
                    },
                    cx,
                );
                // The unfurl arrives later, in an update that carries only the embeds.
                data = json!({"id": id, "channel_id": channel.to_string(), "embeds": embeds});
                self.update(
                    Update::Dispatch {
                        name: "MESSAGE_UPDATE".to_owned(),
                        data,
                    },
                    cx,
                );
                continue;
            }
            self.update(
                Update::Dispatch {
                    name: "MESSAGE_CREATE".to_owned(),
                    data,
                },
                cx,
            );
        }
        self.request_images();
        cx.notify();
        format!("embeds: dispatched {count} demo messages to {channel}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_demo_set_parses_into_messages_with_embeds() {
        for name in ["video", "cards", "inline", "own", "late"] {
            for (content, embeds) in set(name, "2026-11-11T08:28:00.000Z").unwrap() {
                let message = dc_state::Message::from_value(&json!({
                    "id": "1438000000000000000", "channel_id": "1", "type": 0, "content": content,
                    "timestamp": "2026-11-13T08:28:00+00:00", "author": {"id": "1", "username": "a"},
                    "embeds": embeds
                }))
                .unwrap();
                assert!(!message.embeds.is_empty(), "{name}");
            }
        }
        assert!(set("nothing", "").is_none());
        // The gallery of four merges into one embed.
        let cards = set("cards", "2026-11-11T08:28:00.000Z").unwrap();
        let gallery = dc_state::Embed::list(&cards[3].1, None);
        assert_eq!(gallery.len(), 1);
        assert_eq!(gallery[0].images.len(), 4);
    }
}
