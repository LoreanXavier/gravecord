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

impl Root {
    pub(crate) fn script_embeds(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let ["demo", name] = words else {
            return "embeds: usage `embeds demo <video|cards|inline|own|late|suppress>`".to_owned();
        };
        let Some(channel) = self.channel else {
            return "embeds: no channel is open".to_owned();
        };
        let now = chrono::Utc::now();
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
