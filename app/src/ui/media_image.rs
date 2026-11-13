//! Uploaded image formats and native animation state (619517/652215).
use super::Root;
use dc_state::Attachment;
use gpui::{AnyElement, Context, Image, IntoElement, Pixels, img, prelude::*, px};
use std::sync::Arc;

pub(super) fn attachment_url(attachment: &Attachment, width: u32, height: u32) -> String {
    image_url(
        &attachment.proxy_url,
        attachment.is_animated(),
        width,
        height,
    )
}

pub(super) fn image_url(base: &str, animated: bool, width: u32, height: u32) -> String {
    let Ok(mut url) = reqwest::Url::parse(base) else {
        return base.to_owned();
    };
    {
        let mut query = url.query_pairs_mut();
        if !animated {
            query
                .append_pair("format", "webp")
                .append_pair("quality", "lossless");
        }
        query
            .append_pair("width", &width.to_string())
            .append_pair("height", &height.to_string());
    }
    url.into()
}

impl Root {
    /// Module 619517 `S`: an animated picture plays while `autoPlay` ("Play GIFs when Discord is focused", turned off
    /// by reduced motion, module 955572) or the pointer is on it, and while it is on screen; else its still frame
    /// shows. An animation is a decoded tile of its own (`gif_tiles`); a still picture is the renderer's `img`.
    pub(super) fn media_wants_play(&self, key: &str) -> bool {
        should_play_media(
            self.pref_on("animate_gifs", true),
            crate::ui::motion::reduced(),
            self.hovered_media.as_deref() == Some(key),
        )
    }

    /// The picture of an attachment `size` points large in a box with `radius` corners; `cover` crops it to the box
    /// (the mosaic). The box must be positioned (an animation fills it absolutely).
    pub(super) fn media_image(
        &self,
        image: Arc<Image>,
        key: &str,
        size: (Pixels, Pixels),
        radius: f32,
        cover: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if super::gif_tiles::animated(image.bytes()) {
            let scale = self.ui_scale.get().max(1.);
            let pixels = (
                (f32::from(size.0) * scale).round().max(1.) as u32,
                (f32::from(size.1) * scale).round().max(1.) as u32,
            );
            return self.media_tile(
                &format!("media-{key}"),
                image,
                super::gif_tiles::TileLook {
                    pixels,
                    radius,
                    cover,
                    post: None,
                    wanted: self.media_wants_play(key),
                },
                cx,
            );
        }
        let picture = img(image).w(size.0).h(size.1).rounded(px(radius));
        if cover {
            picture
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element()
        } else {
            picture.into_any_element()
        }
    }

    pub(super) fn media_hover(&mut self, key: &str, inside: bool, cx: &mut Context<Self>) {
        if inside {
            self.hovered_media = Some(key.to_owned());
        } else if self.hovered_media.as_deref() == Some(key) {
            self.hovered_media = None;
        }
        cx.notify();
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only, memory only (nothing is sent):
    /// - `media local <key> <path>`: a picture file from disk under an image key (`attachment:<id>`), as if it had
    ///   been downloaded;
    /// - `media message <channel> [<attachment id> <file name> <width> <height>]`: a `MESSAGE_CREATE` from the other
    ///   person of a DM, with one picture attachment when given;
    /// - `media typing <channel>`: a `TYPING_START` from that person;
    /// - `media hover <key>|off`: the pointer over an attachment;
    /// - `media state`: the tiles, the DM's read state and the typing list.
    pub(crate) fn script_media(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let other = |root: &Self, channel: u64| {
            root.stores
                .private_channels
                .iter()
                .find(|known| known.id == channel)
                .and_then(|known| known.recipients.first().copied())
        };
        match words {
            ["local", key, path] => match std::fs::read(path) {
                Ok(bytes) => {
                    let format = if bytes.starts_with(b"GIF8") {
                        gpui::ImageFormat::Gif
                    } else if bytes.starts_with(b"RIFF") {
                        gpui::ImageFormat::Webp
                    } else {
                        gpui::ImageFormat::Png
                    };
                    let length = bytes.len();
                    self.images.insert(
                        (*key).to_owned(),
                        Arc::new(Image::from_bytes(format, bytes)),
                    );
                    format!("{key} {length} bytes")
                }
                Err(error) => format!("{key}: {error}"),
            },
            ["message", channel, rest @ ..] => {
                let Some((channel, author)) = channel
                    .parse::<u64>()
                    .ok()
                    .and_then(|channel| Some((channel, other(self, channel)?)))
                else {
                    return "no such DM".into();
                };
                let id = dc_state::read_state::snowflake_at(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |elapsed| elapsed.as_millis() as u64),
                );
                let attachments = match rest {
                    [attachment, name, width, height] => serde_json::json!([{
                        "id": attachment, "filename": name, "size": 1000,
                        "url": format!("https://cdn.discordapp.com/attachments/{channel}/{attachment}/{name}"),
                        "proxy_url": format!("https://media.discordapp.net/attachments/{channel}/{attachment}/{name}"),
                        "width": width.parse::<u64>().unwrap_or(1),
                        "height": height.parse::<u64>().unwrap_or(1),
                        "content_type": "image/gif",
                    }]),
                    _ => serde_json::json!([]),
                };
                let author = self.stores.users.get(&author).cloned();
                let data = serde_json::json!({
                    "id": id.to_string(), "channel_id": channel.to_string(), "type": 0,
                    "content": if attachments.as_array().is_some_and(|list| list.is_empty()) { "hello" } else { "" },
                    "author": {
                        "id": author.as_ref().map_or(String::new(), |user| user.id.to_string()),
                        "username": author.as_ref().map_or(String::new(), |user| user.username.clone()),
                        "discriminator": "0", "avatar": author.as_ref().and_then(|user| user.avatar.clone()),
                    },
                    "attachments": attachments, "embeds": [], "mentions": [], "mention_roles": [],
                    "timestamp": chrono::Utc::now().to_rfc3339(), "edited_timestamp": null,
                    "pinned": false, "mention_everyone": false, "tts": false,
                });
                self.update(
                    crate::net::Update::Dispatch {
                        name: "MESSAGE_CREATE".into(),
                        data,
                    },
                    cx,
                );
                format!("message {id}")
            }
            ["typing", channel] => {
                let Some((channel, user)) = channel
                    .parse::<u64>()
                    .ok()
                    .and_then(|channel| Some((channel, other(self, channel)?)))
                else {
                    return "no such DM".into();
                };
                self.update(
                    crate::net::Update::Dispatch {
                        name: "TYPING_START".into(),
                        data: serde_json::json!({
                            "channel_id": channel.to_string(), "user_id": user.to_string(),
                            "timestamp": chrono::Utc::now().timestamp(),
                        }),
                    },
                    cx,
                );
                "typing".into()
            }
            ["hover", "off"] => {
                self.hovered_media = None;
                "hover off".into()
            }
            ["hover", key] => {
                self.hovered_media = Some((*key).to_owned());
                format!("hover {key}")
            }
            ["state"] => {
                let dms: Vec<String> = self
                    .stores
                    .private_channels
                    .iter()
                    .take(4)
                    .map(|channel| {
                        format!(
                            "{}: mentions={} typing={:?}",
                            channel.id,
                            self.stores.read_states.mentions(channel.id),
                            self.stores
                                .typing_users(channel.id, std::time::Instant::now())
                                .len()
                        )
                    })
                    .collect();
                format!(
                    "open={:?} dms=[{}] tiles=[{}]",
                    self.channel,
                    dms.join(", "),
                    self.gif_tiles.borrow().len()
                )
            }
            _ => "usage: media local|message|typing|hover|state".into(),
        }
    }
}

/// `(autoPlay || hasMouseOver) && shouldAnimate` with `autoPlay` the setting unless reduced motion overrides it.
pub(super) fn should_play_media(setting: bool, reduced: bool, hovered: bool) -> bool {
    (setting && !reduced) || hovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_plays_by_the_setting_or_under_the_pointer() {
        assert!(should_play_media(true, false, false));
        assert!(!should_play_media(false, false, false));
        assert!(should_play_media(false, false, true));
        // Reduced motion turns the setting off (module 955572); the pointer still plays it.
        assert!(!should_play_media(true, true, false));
        assert!(should_play_media(true, true, true));
    }

    #[test]
    fn animated_uploads_keep_their_format_and_signed_queries() {
        let mut attachment = Attachment {
            id: 1,
            filename: "motion.GIF".into(),
            url: String::new(),
            proxy_url: "https://example.com/motion.gif?signature=preserve&".into(),
            content_type: Some("image/gif".into()),
            width: Some(160),
            height: Some(96),
            size: 10,
            flags: 32,
            description: None,
            duration_secs: None,
            waveform: None,
        };
        let url = reqwest::Url::parse(&attachment_url(&attachment, 160, 96)).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs.get("signature").unwrap(), "preserve");
        assert_eq!(pairs.get("width").unwrap(), "160");
        assert!(!pairs.contains_key("format"));
        attachment.filename = "motion.webp".into();
        assert!(attachment.is_animated());
        attachment.flags = 0;
        assert!(!attachment.is_animated());
        attachment.proxy_url = "https://example.com/static.webp".into();
        let url = attachment_url(&attachment, 160, 96);
        assert!(url.contains("?format=webp&quality=lossless&width=160&height=96"));
    }
}
