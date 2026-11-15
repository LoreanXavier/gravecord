//! Uploaded image formats, native animation state (619517/652215), and sent-attachment spoiler / sensitive-content
//! obscuring (`ObscureWrapper`, module 302031 + `880150.css`).
use super::{Root, Side, Tip, color, icon, menu};
use crate::parity::Parity;
use dc_state::{Attachment, Id, Message};
use gpui::{
    AnyElement, Context, EntityId, FontWeight, Image, IntoElement, MouseButton, Pixels,
    SharedString, div, img, prelude::*, px, rgba, svg,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// `AttachmentFlags.IS_SPOILER` (`1 << 3`).
pub(super) const ATTACHMENT_FLAG_SPOILER: u64 = 1 << 3;
/// `AttachmentFlags.CONTAINS_EXPLICIT_MEDIA` (`1 << 4`).
pub(super) const ATTACHMENT_FLAG_EXPLICIT: u64 = 1 << 4;
/// `AttachmentFlags.CONTAINS_GORE_CONTENT` (`1 << 6`).
pub(super) const ATTACHMENT_FLAG_GORE: u64 = 1 << 6;
/// `AttachmentFlags.CONTAINS_SELF_HARM_CONTENT` (`1 << 7`).
pub(super) const ATTACHMENT_FLAG_SELF_HARM: u64 = 1 << 7;
/// Default sensitive-content flags obscured for non-authors outside NSFW channels.
pub(super) const ATTACHMENT_HARM_FLAGS_ALL: u64 =
    ATTACHMENT_FLAG_EXPLICIT | ATTACHMENT_FLAG_GORE | ATTACHMENT_FLAG_SELF_HARM;
/// Sensitive-content flags still obscured in NSFW channels (`CONTAINS_EXPLICIT_MEDIA` is exempt).
pub(super) const ATTACHMENT_HARM_FLAGS_NSFW: u64 = ATTACHMENT_FLAG_GORE | ATTACHMENT_FLAG_SELF_HARM;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObscureKind {
    Spoiler,
    SensitiveContent,
}

/// Resolved `ObscureWrapper` state (`module 302031`, `880150.css`) for one sent attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ObscureWrapper {
    pub(super) attachment_id: Id,
    pub(super) kind: ObscureKind,
    pub(super) hidden: bool,
    pub(super) single_mosaic_item: bool,
    pub(super) over_revealed_video: bool,
}

impl ObscureWrapper {
    pub(super) fn sensitive_toggle_bottom(self) -> f32 {
        if !self.hidden && self.over_revealed_video {
            36.
        } else {
            4.
        }
    }
}

#[derive(Default)]
pub(super) struct ObscureStore {
    revealed_spoilers: HashSet<Id>,
    revealed_sensitive: HashSet<Id>,
}

impl ObscureStore {
    pub(super) fn is_hidden(&self, kind: ObscureKind, attachment_id: Id) -> bool {
        match kind {
            ObscureKind::Spoiler => !self.revealed_spoilers.contains(&attachment_id),
            ObscureKind::SensitiveContent => !self.revealed_sensitive.contains(&attachment_id),
        }
    }

    pub(super) fn reveal_spoiler(&mut self, attachment_id: Id) {
        self.revealed_spoilers.insert(attachment_id);
    }

    pub(super) fn toggle_sensitive(&mut self, attachment_id: Id) -> bool {
        if !self.revealed_sensitive.remove(&attachment_id) {
            self.revealed_sensitive.insert(attachment_id);
            true
        } else {
            false
        }
    }
}

thread_local! {
    static OBSCURE_BY_VIEW: RefCell<HashMap<EntityId, ObscureStore>> = RefCell::new(HashMap::new());
}

/// Pure rule of module 302031 (`getObscureReason`): sensitive-content flags apply to attachments from other users,
/// and spoiler flags / `SPOILER_` prefix apply whenever `spoilers_shown` (`renderSpoilers`) does not reveal them.
pub(super) fn attachment_obscure_kind(
    attachment: &Attachment,
    author_id: Id,
    current_user_id: Option<Id>,
    spoilers_shown: bool,
    enabled_harm_flags: u64,
) -> Option<ObscureKind> {
    if current_user_id != Some(author_id) && (attachment.flags & enabled_harm_flags) != 0 {
        return Some(ObscureKind::SensitiveContent);
    }
    let is_spoiler = (attachment.flags & ATTACHMENT_FLAG_SPOILER) != 0
        || attachment.filename.starts_with("SPOILER_");
    if is_spoiler && !spoilers_shown {
        return Some(ObscureKind::Spoiler);
    }
    None
}

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
    // A proxy URL may already describe a thumbnail. Replace its sizing parameters instead of sending two
    // widths (the proxy can keep the first), while retaining the attachment's signed query.
    let retained: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            !matches!(
                key.as_ref(),
                "width" | "height" | "size" | "quality" | "format"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(retained);
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
        self.media_image_frozen(image, key, size, radius, (cover, false), cx)
    }

    fn media_image_frozen(
        &self,
        image: Arc<Image>,
        key: &str,
        size: (Pixels, Pixels),
        radius: f32,
        (cover, frozen): (bool, bool),
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
                    wanted: !frozen && self.media_wants_play(key),
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

    fn attachment_enabled_harm_flags(&self) -> u64 {
        if self.current_channel().is_some_and(|channel| channel.nsfw) {
            ATTACHMENT_HARM_FLAGS_NSFW
        } else {
            ATTACHMENT_HARM_FLAGS_ALL
        }
    }

    fn attachment_obscure_state(
        &self,
        message: &Message,
        attachment: &Attachment,
        cx: &Context<Self>,
    ) -> Option<(ObscureKind, bool)> {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let kind = attachment_obscure_kind(
            attachment,
            message.author.id,
            me,
            self.spoilers_shown(),
            self.attachment_enabled_harm_flags(),
        )?;
        let view_id = cx.entity_id();
        let hidden = OBSCURE_BY_VIEW.with(|map| {
            map.borrow()
                .get(&view_id)
                .is_none_or(|store| store.is_hidden(kind, attachment.id))
        });
        Some((kind, hidden))
    }

    fn attachment_obscure_wrapper(
        &self,
        message: &Message,
        attachment: &Attachment,
        single_mosaic_item: bool,
        over_revealed_video: bool,
        cx: &Context<Self>,
    ) -> Option<ObscureWrapper> {
        let (kind, hidden) = self.attachment_obscure_state(message, attachment, cx)?;
        Some(ObscureWrapper {
            attachment_id: attachment.id,
            kind,
            hidden,
            single_mosaic_item,
            over_revealed_video,
        })
    }

    fn reveal_spoiler_attachment(&mut self, attachment_id: Id, cx: &mut Context<Self>) {
        let view_id = cx.entity_id();
        OBSCURE_BY_VIEW.with(|map| {
            map.borrow_mut()
                .entry(view_id)
                .or_default()
                .reveal_spoiler(attachment_id);
        });
        cx.notify();
    }

    fn toggle_sensitive_attachment(&mut self, attachment_id: Id, cx: &mut Context<Self>) {
        let view_id = cx.entity_id();
        OBSCURE_BY_VIEW.with(|map| {
            map.borrow_mut()
                .entry(view_id)
                .or_default()
                .toggle_sensitive(attachment_id);
        });
        cx.notify();
    }

    fn attachment_sensitive_toggle_button(
        &self,
        wrapper: ObscureWrapper,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let attachment_id = wrapper.attachment_id;
        let hidden = wrapper.hidden;
        let bottom = wrapper.sensitive_toggle_bottom();
        let button = div()
            .id(SharedString::from(format!(
                "obscure-sensitive-toggle-{attachment_id}"
            )))
            .parity("attachment-sensitive-toggle")
            .occlude()
            .size(px(32.))
            .rounded(px(8.))
            .bg(color("--control-overlay-secondary-background-default"))
            .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tip = None;
                this.toggle_sensitive_attachment(attachment_id, cx);
                cx.stop_propagation();
            }))
            .child(icon(
                if hidden { "eye_slash" } else { "eye" },
                20.,
                "--white",
            ));
        let tipped = self.with_tip(
            format!("obscure-sensitive-tip-{attachment_id}"),
            Tip::text(dc_intl::t!("ex5G9m"), Side::Top),
            button,
            cx,
        );
        div()
            .absolute()
            .right(px(4.))
            .bottom(px(bottom))
            .child(tipped)
            .into_any_element()
    }

    fn attachment_obscure_overlay(
        &self,
        wrapper: ObscureWrapper,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let attachment_id = wrapper.attachment_id;
        match (wrapper.kind, wrapper.hidden) {
            (ObscureKind::Spoiler, false) => None,
            (ObscureKind::SensitiveContent, false) => {
                Some(self.attachment_sensitive_toggle_button(wrapper, cx))
            }
            (ObscureKind::Spoiler, true) => {
                let group = SharedString::from(format!("spoiler-group-{attachment_id}"));
                let mut tint = color("--spoiler-hidden-background");
                if tint.a > 0.65 {
                    tint.a = 0.45;
                }
                Some(
                    div()
                        .id(SharedString::from(format!(
                            "obscure-spoiler-{attachment_id}"
                        )))
                        .parity("attachment-spoiler-cover")
                        .group(group.clone())
                        .occlude()
                        .absolute()
                        .inset_0()
                        .rounded(px(8.))
                        .backdrop_blur(px(44.))
                        .bg(tint)
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.reveal_spoiler_attachment(attachment_id, cx);
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(px(20.))
                                .bg(rgba(0x00000099))
                                .group_hover(group.clone(), |style| style.bg(rgba(0x000000e0)))
                                .text_size(px(15.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--primary-200"))
                                .group_hover(group, |style| style.text_color(color("--white")))
                                .child(dc_intl::t!("F+x38C").to_uppercase()),
                        )
                        .into_any_element(),
                )
            }
            (ObscureKind::SensitiveContent, true) => Some(
                div()
                    .id(SharedString::from(format!(
                        "obscure-sensitive-{attachment_id}"
                    )))
                    .parity("attachment-sensitive-cover")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .rounded(px(8.))
                    .backdrop_blur(px(44.))
                    .bg(color("--spoiler-hidden-background"))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(8.))
                            .bg(rgba(0x000000d9))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .px(px(16.))
                            .child(icon("warning", 24., "--interactive-icon-default"))
                            .when(wrapper.single_mosaic_item, |this| {
                                this.child(
                                    div()
                                        .text_center()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t!("SpxcUR")),
                                )
                            }),
                    )
                    .child(self.attachment_sensitive_toggle_button(wrapper, cx))
                    .into_any_element(),
            ),
        }
    }

    /// Single inline video attachment with `ObscureWrapper` (`module 302031`): while hidden, playback is frozen on its
    /// poster frame under the blur/opaque cover; when revealed on a sensitive video, the eye toggle sits at `bottom: 36px`.
    pub(super) fn attachment_video_obscured(
        &self,
        message: &Message,
        attachment: &Attachment,
        size: (Pixels, Pixels),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.attachment_obscure_wrapper(message, attachment, true, true, cx) {
            None => self.attachment_video_player(message, attachment, size, cx),
            Some(wrapper) if !wrapper.hidden => div()
                .relative()
                .w(size.0)
                .h(size.1)
                .child(self.attachment_video_player(message, attachment, size, cx))
                .children(self.attachment_obscure_overlay(wrapper, cx))
                .into_any_element(),
            Some(wrapper) => {
                let key = format!("attachment:{}", attachment.id);
                let poster = self.images.get(&key).cloned();
                div()
                    .id(SharedString::from(format!("attachment-{}", attachment.id)))
                    .relative()
                    .w(size.0)
                    .h(size.1)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--spoiler-hidden-background"))
                    .children(poster.map(|image| {
                        self.media_image_frozen(image, &key, size, 8., (true, true), cx)
                    }))
                    .children(self.attachment_obscure_overlay(wrapper, cx))
                    .into_any_element()
            }
        }
    }

    /// Single image or mosaic cell with `ObscureWrapper` (`module 302031`, `880150.css`).
    pub(super) fn attachment_tile_obscured(
        &self,
        message: &Message,
        attachment: &Attachment,
        size: (Pixels, Pixels),
        mosaic_cell: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let wrapper = self.attachment_obscure_wrapper(message, attachment, !mosaic_cell, false, cx);
        let hidden = wrapper.is_some_and(|w| w.hidden);
        let key = format!("attachment:{}", attachment.id);
        let hovered = key.clone();
        let image = self.images.get(&key).cloned();
        let clicked = message.clone();
        let (url, media_key) = (attachment.url.clone(), key.clone());
        let (w, h) = size;
        let id_prefix = if mosaic_cell { "mosaic" } else { "attachment" };
        let base_bg = if hidden {
            color("--spoiler-hidden-background")
        } else {
            color("--background-base-low")
        };

        let inner_media = div()
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .children(image.map(|image| {
                self.media_image_frozen(image, &key, (w, h), 8., (mosaic_cell, hidden), cx)
            }))
            .children((mosaic_cell && !hidden && attachment.is_video()).then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(48.))
                            .rounded_full()
                            .bg(gpui::hsla(0., 0., 0., 0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path("icons/clips_play.svg")
                                    .size(px(24.))
                                    .ml(px(1.))
                                    .mr(px(-1.))
                                    .text_color(gpui::white()),
                            ),
                    )
            }));

        div()
            .id(SharedString::from(format!("{id_prefix}-{}", attachment.id)))
            .relative()
            .w(w)
            .h(h)
            .when(mosaic_cell, |this| this.flex_none())
            .rounded(px(8.))
            .overflow_hidden()
            .when(!hidden, |this| this.cursor_pointer())
            .bg(base_bg)
            .child(inner_media)
            .when(!hidden, |this| {
                let open_key = key.clone();
                this.children(self.alt_text_overlay(attachment, cx))
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        this.media_hover(&hovered, *inside, cx)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_lightbox(&clicked, &open_key, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                            this.menu_hit = Some((
                                event.position,
                                menu::Hit::Media {
                                    url: url.clone(),
                                    key: media_key.clone(),
                                },
                            ));
                        }),
                    )
            })
            .children(wrapper.and_then(|wrapper| self.attachment_obscure_overlay(wrapper, cx)))
            .into_any_element()
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
    fn viewer_size_replaces_thumbnail_parameters_without_losing_signatures() {
        let result = image_url(
            "https://media.discordapp.net/attachments/a/b.png?ex=abc&hm=def&width=240&height=120&format=webp&quality=low",
            false,
            1920,
            960,
        );
        let url = reqwest::Url::parse(&result).unwrap();
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs.iter().filter(|(key, _)| key == "width").count(), 1);
        assert!(
            pairs
                .iter()
                .any(|(key, value)| key == "width" && value == "1920")
        );
        assert!(
            pairs
                .iter()
                .any(|(key, value)| key == "ex" && value == "abc")
        );
        assert!(
            pairs
                .iter()
                .any(|(key, value)| key == "hm" && value == "def")
        );
        let animated = image_url(&result, true, 960, 480);
        assert!(!animated.contains("format="));
        assert!(!animated.contains("quality="));
    }

    fn sample_attachment(filename: &str, flags: u64, proxy_url: &str) -> Attachment {
        let value = serde_json::json!({
            "id": "100",
            "channel_id": "200",
            "type": 0,
            "content": "",
            "timestamp": "2026-01-01T00:00:00Z",
            "author": {"id": "300", "username": "ada"},
            "attachments": [{
                "id": "1",
                "filename": filename,
                "size": 10,
                "url": "",
                "proxy_url": proxy_url,
                "content_type": "image/gif",
                "width": 160,
                "height": 96,
                "flags": flags,
            }],
        });
        Message::from_value(&value)
            .expect("valid message")
            .attachments
            .into_iter()
            .next()
            .expect("one attachment")
    }

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
        let mut attachment = sample_attachment(
            "motion.GIF",
            32,
            "https://example.com/motion.gif?signature=preserve&",
        );
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

    #[test]
    fn obscure_kind_handles_spoilers_harm_flags_nsfw_and_author_exemption() {
        let plain = sample_attachment("cat.png", 0, "https://example.com/cat.png");
        assert_eq!(
            attachment_obscure_kind(&plain, 300, Some(999), false, ATTACHMENT_HARM_FLAGS_ALL),
            None
        );

        let spoiler_flag =
            sample_attachment("cat.png", ATTACHMENT_FLAG_SPOILER, "https://example.com/a");
        assert_eq!(
            attachment_obscure_kind(
                &spoiler_flag,
                300,
                Some(300),
                false,
                ATTACHMENT_HARM_FLAGS_ALL
            ),
            Some(ObscureKind::Spoiler)
        );
        assert_eq!(
            attachment_obscure_kind(
                &spoiler_flag,
                300,
                Some(999),
                true,
                ATTACHMENT_HARM_FLAGS_ALL
            ),
            None
        );

        let spoiler_prefix = sample_attachment("SPOILER_cat.png", 0, "https://example.com/b");
        assert_eq!(
            attachment_obscure_kind(
                &spoiler_prefix,
                300,
                Some(999),
                false,
                ATTACHMENT_HARM_FLAGS_ALL
            ),
            Some(ObscureKind::Spoiler)
        );

        for flag in [
            ATTACHMENT_FLAG_EXPLICIT,
            ATTACHMENT_FLAG_GORE,
            ATTACHMENT_FLAG_SELF_HARM,
        ] {
            let harm = sample_attachment("warn.png", flag, "https://example.com/warn.png");
            assert_eq!(
                attachment_obscure_kind(&harm, 300, Some(999), true, ATTACHMENT_HARM_FLAGS_ALL),
                Some(ObscureKind::SensitiveContent)
            );
            // Author's own uploads are exempt from sensitive-content obscuring.
            assert_eq!(
                attachment_obscure_kind(&harm, 300, Some(300), true, ATTACHMENT_HARM_FLAGS_ALL),
                None
            );
        }

        // In NSFW channels explicit (`16`) is exempt, while gore (`64`) and self-harm (`128`) stay obscured.
        let explicit = sample_attachment(
            "nsfw.png",
            ATTACHMENT_FLAG_EXPLICIT,
            "https://example.com/n",
        );
        assert_eq!(
            attachment_obscure_kind(&explicit, 300, Some(999), false, ATTACHMENT_HARM_FLAGS_NSFW),
            None
        );
        let gore = sample_attachment("gore.png", ATTACHMENT_FLAG_GORE, "https://example.com/g");
        assert_eq!(
            attachment_obscure_kind(&gore, 300, Some(999), false, ATTACHMENT_HARM_FLAGS_NSFW),
            Some(ObscureKind::SensitiveContent)
        );

        // Sensitive content takes priority over spoiler when both flags are set for non-authors.
        let both = sample_attachment(
            "SPOILER_warn.png",
            ATTACHMENT_FLAG_SPOILER | ATTACHMENT_FLAG_EXPLICIT,
            "https://example.com/both.png",
        );
        assert_eq!(
            attachment_obscure_kind(&both, 300, Some(999), false, ATTACHMENT_HARM_FLAGS_ALL),
            Some(ObscureKind::SensitiveContent)
        );
        assert_eq!(
            attachment_obscure_kind(&both, 300, Some(300), false, ATTACHMENT_HARM_FLAGS_ALL),
            Some(ObscureKind::Spoiler)
        );
    }

    #[test]
    fn obscure_store_reveals_spoilers_one_way_and_toggles_sensitive_content() {
        let mut store = ObscureStore::default();
        assert!(store.is_hidden(ObscureKind::Spoiler, 77));
        store.reveal_spoiler(77);
        assert!(!store.is_hidden(ObscureKind::Spoiler, 77));
        store.reveal_spoiler(77);
        assert!(!store.is_hidden(ObscureKind::Spoiler, 77));
        assert!(store.is_hidden(ObscureKind::Spoiler, 78));

        assert!(store.is_hidden(ObscureKind::SensitiveContent, 88));
        assert!(store.toggle_sensitive(88));
        assert!(!store.is_hidden(ObscureKind::SensitiveContent, 88));
        assert!(!store.toggle_sensitive(88));
        assert!(store.is_hidden(ObscureKind::SensitiveContent, 88));

        let hidden_video = ObscureWrapper {
            attachment_id: 88,
            kind: ObscureKind::SensitiveContent,
            hidden: true,
            single_mosaic_item: true,
            over_revealed_video: true,
        };
        assert_eq!(hidden_video.sensitive_toggle_bottom(), 4.);
        let revealed_video = ObscureWrapper {
            hidden: false,
            ..hidden_video
        };
        assert_eq!(revealed_video.sensitive_toggle_bottom(), 36.);
        let revealed_image = ObscureWrapper {
            over_revealed_video: false,
            ..revealed_video
        };
        assert_eq!(revealed_image.sensitive_toggle_bottom(), 4.);
    }

    #[test]
    fn obscure_wrapper_tokens_resolve_cleanly() {
        for token in [
            "--spoiler-hidden-background",
            "--primary-200",
            "--white",
            "--interactive-icon-default",
            "--text-muted",
            "--control-overlay-secondary-background-default",
            "--control-overlay-secondary-background-hover",
        ] {
            let _ = color(token);
        }
    }
}
