//! Message embeds: the `em` component of module 265705 (`embedFull_XyW1ZT`, `inlineMediaEmbed_XyW1ZT`), normalized by
//! `fK` / `nh` of module 659674 (`dc_state::Embed`), listed by `renderEmbeds` of module 679636. The layout decisions
//! are in `embed_layout.rs`; sizes from the stylesheet `198496.95196da42e0b0983.css` and
//! `design/dom/channel/text-darker-cozy.json` (a GitHub article embed).

use super::embed_layout::{self, Media};
use super::{Root, color, hsla, rgba};
use crate::parity::Parity;
use dc_markdown::{Block, Kind, Span, parse};
use dc_state::{Embed, EmbedMedia, Id, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, div, img, prelude::*, px,
};
use std::sync::Arc;

/// `gridContainer_XyW1ZT` max-width.
const GRID_MAX: f32 = 516.;
/// The 4px coloured edge and the 1px border opposite it.
const BORDERS: f32 = 5.;
/// `grid_XyW1ZT` padding: 12px start, 16px end.
const PADDING_X: f32 = 28.;
/// `MessageFlags.SUPPRESS_EMBEDS` and `EPHEMERAL` (module 652215).
pub(super) const SUPPRESS_EMBEDS: u64 = 4;
const EPHEMERAL: u64 = 64;

pub(super) fn image_key(message: Id, index: usize, part: &str) -> String {
    format!("embed:{message}:{index}:{part}")
}

/// The media proxy URL for a picture drawn at `width` by `height`, with the query the image component adds.
pub(super) fn media_url(media: &EmbedMedia, width: f64, height: f64) -> String {
    super::media_image::image_url(
        media.src(),
        media.is_animated(),
        width.round().max(1.) as u32,
        height.round().max(1.) as u32,
    )
}

/// `eu` in module 265705: a still icon asks the proxy for PNG; an animating one is drawn as it is.
fn icon_url(proxy: &str, animate: bool) -> String {
    if animate {
        return proxy.to_owned();
    }
    match reqwest::Url::parse(proxy) {
        Ok(mut url) => {
            let pairs: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(key, _)| key != "format")
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            url.query_pairs_mut()
                .clear()
                .extend_pairs(pairs)
                .append_pair("format", "png");
            url.into()
        }
        Err(_) => proxy.to_owned(),
    }
}

/// The poster's key and drawn size for an embed's video or GIFV, or the thumbnail's.
fn thumbnail_request(embed: &Embed) -> Option<(f64, f64)> {
    match embed_layout::media(embed, false) {
        Media::Video => embed_layout::video_size(embed),
        Media::Gifv => embed_layout::gifv_size(embed),
        _ => embed_layout::thumbnail_size(embed),
    }
}

/// What to fetch for a message's embeds: keys and URLs.
pub(super) fn wanted(message: &Message, animate: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (index, embed) in message.embeds.iter().enumerate() {
        if embed_layout::left_out(embed) {
            continue;
        }
        if let Some(image) = embed.images.first().or(embed.image.as_ref()) {
            let (width, height) = embed.fit(image);
            out.push((
                image_key(message.id, index, "image"),
                media_url(image, width, height),
            ));
        }
        for (number, image) in embed.images.iter().enumerate().skip(1) {
            let (width, height) = embed.fit(image);
            out.push((
                image_key(message.id, index, &format!("gallery:{number}")),
                media_url(image, width, height),
            ));
        }
        if let Some(thumbnail) = &embed.thumbnail
            && let Some((width, height)) = thumbnail_request(embed)
        {
            out.push((
                image_key(message.id, index, "thumbnail"),
                media_url(thumbnail, width, height),
            ));
        } else if embed_layout::media(embed, false) == Media::Video
            && let Some(video) = &embed.video
            && let Some(proxy) = &video.proxy_url
            && let Some((width, height)) = thumbnail_request(embed)
        {
            out.push((
                image_key(message.id, index, "thumbnail"),
                super::media_image::image_url(
                    proxy,
                    false,
                    width.round().max(1.) as u32,
                    height.round().max(1.) as u32,
                ),
            ));
        }
        if let Some(author) = &embed.author_icon {
            out.push((
                image_key(message.id, index, "author"),
                icon_url(author, animate),
            ));
        }
        if let Some(footer) = embed.footer_icon.as_deref().filter(|icon| !icon.is_empty()) {
            out.push((
                image_key(message.id, index, "footer"),
                icon_url(footer, animate),
            ));
        }
    }
    out
}

/// The pictures of an embed the media viewer pages through, with their keys: the gallery, else the image, else a
/// GIFV's poster.
pub(super) fn viewer_pictures(
    message: Id,
    index: usize,
    embed: &Embed,
) -> Vec<(String, EmbedMedia)> {
    if !embed.images.is_empty() {
        return embed
            .images
            .iter()
            .enumerate()
            .map(|(number, image)| {
                let part = if number == 0 {
                    "image".to_owned()
                } else {
                    format!("gallery:{number}")
                };
                (image_key(message, index, &part), image.clone())
            })
            .collect();
    }
    if let Some(image) = &embed.image {
        return vec![(image_key(message, index, "image"), image.clone())];
    }
    if embed.kind == "gifv"
        && let Some(poster) = &embed.thumbnail
    {
        return vec![(image_key(message, index, "thumbnail"), poster.clone())];
    }
    Vec::new()
}

/// `(width, height)` scaled down to `available` wide, keeping the shape (`responsive`).
fn shrink(size: (f64, f64), available: f32) -> (f32, f32) {
    let (width, height) = (size.0 as f32, size.1 as f32);
    let shown = width.min(available.max(1.));
    (shown, height * shown / width.max(1.))
}

impl Root {
    /// Description and field text: markdown at 14/18 in the default text color, line breaks kept (`pre-line`).
    pub(super) fn embed_text(&self, text: &str, key: (u64, u32)) -> AnyElement {
        self.embed_text_lined(text, key, 18.)
    }

    /// `embed_text` with another line height (a component's reduced text is 14/19.25).
    pub(super) fn embed_text_lined(&self, text: &str, key: (u64, u32), line: f32) -> AnyElement {
        self.embed_text_sized(text, key, 14., line)
    }

    /// `embed_text_lined` with another font size (a component's plain text outside a container is 16/22).
    pub(super) fn embed_text_sized(
        &self,
        text: &str,
        key: (u64, u32),
        size: f32,
        line: f32,
    ) -> AnyElement {
        let base = hsla("--text-default");
        let blocks = parse(text);
        div()
            .flex()
            .flex_col()
            .children(blocks.into_iter().enumerate().map(|(index, block)| {
                let spans: Vec<Span> = match block {
                    Block::Paragraph(spans)
                    | Block::Quote(spans)
                    | Block::Subtext(spans)
                    | Block::Heading { spans, .. }
                    | Block::ListItem { spans, .. } => spans,
                    Block::Code { text, .. } => vec![Span {
                        text,
                        style: dc_markdown::Style::default(),
                        kind: Kind::Text,
                    }],
                };
                let pieces = spans
                    .iter()
                    .map(|span| self.piece(span, base, size, 400., line))
                    .collect();
                crate::inline::inline(pieces, px(line)).selectable(
                    (key.0, key.1 + index as u32),
                    self.selection.clone(),
                    super::hsla_from(super::SELECTION),
                )
            }))
            .into_any_element()
    }

    /// A description of an embed that is not rich: `renderEmbedDescription` hands it over unparsed; `pre-line`
    /// keeps its line breaks.
    fn embed_plain_text(text: &str) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-default"))
            .children(
                text.split('\n')
                    .map(|line| div().min_h(px(18.)).child(line.trim().to_owned())),
            )
            .into_any_element()
    }

    /// `canSuppressEmbeds` of module 679636 (`j`): the author, or someone who may manage messages, on a message
    /// that is not the system's and not ephemeral, with at least one embed.
    fn can_suppress_embeds(&self, message: &Message) -> bool {
        let own = self
            .stores
            .me()
            .is_some_and(|me| me.id == message.author.id);
        let manage = self.current_channel().is_some_and(|channel| {
            self.can_in_channel(channel, dc_state::permissions::MANAGE_MESSAGES)
        });
        (own || manage)
            && message.author.id != 1
            && message.flags & EPHEMERAL == 0
            && !message.embeds.is_empty()
    }

    /// `handleEmbedSuppressed`: shift removes the embeds at once, otherwise the question opens first.
    fn suppress_embeds(&mut self, message: &Message, shift: bool, cx: &mut Context<Self>) {
        let request = suppress_request(message);
        if shift {
            let _ = self.requests.send(request);
            cx.notify();
        } else {
            self.ask_destructive(super::destructive::Pending::suppress_embeds(request), cx);
        }
    }

    pub(super) fn hides_inline_media_content(&self, message: &Message) -> bool {
        self.pref_on("chat_embeds_render_embeds", true)
            && self.pref_on("chat_inline_media_links", true)
            && message.content_is_inline_media()
    }

    pub(super) fn embeds(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        // "Show embeds and link previews" off hides them under every message.
        if !self.pref_on("chat_embeds_render_embeds", true) {
            return None;
        }
        let suppress = self.can_suppress_embeds(message);
        let cards: Vec<AnyElement> = message
            .embeds
            .iter()
            .enumerate()
            .filter(|(_, embed)| !embed_layout::left_out(embed))
            .filter_map(|(index, embed)| self.embed(message, index, embed, suppress, cx))
            .collect();
        (!cards.is_empty()).then(|| {
            div()
                .parity("embeds")
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(cards)
                .into_any_element()
        })
    }

    /// "When posted as links to chat" off: `hideMedia`, no media and no thumbnail in any embed.
    fn embed_media_hidden(&self) -> bool {
        !self.pref_on("chat_inline_media_links", true)
    }

    /// A picture of the embed at its drawn size, opening the media viewer on click; the box keeps the declared size
    /// while the picture loads so the message does not move.
    fn embed_picture(
        &self,
        message: &Message,
        key: String,
        size: (f32, f32),
        cover: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let picture = self.images.get(&key).cloned();
        let (clicked, open, hovered) = (message.clone(), key.clone(), key.clone());
        let (width, height) = (px(size.0), px(size.1));
        div()
            .id(SharedString::from(format!("picture-{key}")))
            .flex_none()
            .relative()
            .cursor_pointer()
            .w(width)
            .h(height)
            .rounded(px(4.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                this.media_hover(&hovered, *inside, cx);
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_lightbox(&clicked, &open, cx);
            }))
            .children(
                picture
                    .map(|picture| self.media_image(picture, &key, (width, height), 4., cover, cx)),
            )
    }

    /// `ed` and module 153440's player before it plays: the poster at the video's size with the round play button
    /// in the middle (`cover_ifLhOx`, `iconWrapper_ifLhOx`), or, for a provider without a proxied file (YouTube and
    /// the like), the poster with the pill of module 875149 holding Play and Open in browser.
    fn embed_video(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        available: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let size = shrink(embed_layout::video_size(embed)?, available);
        let video = embed.video.as_ref()?;
        let key = image_key(message.id, index, "thumbnail");
        let force_external = video.width > 6016 || video.height > 3384;
        if let Some(proxy_url) = video.proxy_url.as_ref().filter(|_| !force_external) {
            return Some(self.render_media_player(
                super::video_player::PlayerSpec {
                    key: format!("embed:{}:{index}", message.id),
                    src: proxy_url.clone(),
                    width: size.0.max(150.0),
                    height: size.1.max(144.0),
                    aspect_size: (video.width.max(1), video.height.max(1)),
                    radius: 4.0,
                    poster_key: Some(key),
                    auto_play: false,
                    allow_fullscreen: true,
                    is_fullscreen_view: false,
                    actions: None,
                },
                cx,
            ));
        }
        let poster = self.images.get(&key).cloned();
        let (width, height) = (px(size.0), px(size.1));
        let proxied = video.proxy_url.is_some();
        let target = if proxied {
            video.url.clone()
        } else {
            embed.url.clone().unwrap_or_else(|| video.url.clone())
        };
        let page = embed.url.clone();
        let group = SharedString::from(format!("embed-video-{}-{index}", message.id));
        let play_icon = || {
            gpui::svg()
                .path("icons/clips_play.svg")
                .size(px(24.))
                .flex_none()
                .ml(px(1.))
                .mr(px(-1.))
                .text_color(color("--white"))
        };
        let overlay = if proxied {
            let target = target.clone();
            div()
                .id(SharedString::from(format!("{group}-play")))
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.follow_link(&target, &target, cx);
                }))
                .child(
                    div()
                        .p(px(12.))
                        .rounded(px(24.))
                        .bg(color("--black"))
                        .opacity(0.6)
                        .group_hover(group.clone(), |style| style.opacity(0.8))
                        .child(play_icon()),
                )
                .into_any_element()
        } else {
            let (play, open) = (target.clone(), page.clone());
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .h(px(48.))
                        .p(px(12.))
                        .rounded(px(24.))
                        .bg(color("--opacity-black-60"))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id(SharedString::from(format!("{group}-play")))
                                .flex()
                                .cursor_pointer()
                                .opacity(0.6)
                                .hover(|style| style.opacity(1.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.follow_link(&play, &play, cx);
                                }))
                                .child(play_icon()),
                        )
                        .children(open.map(|open| {
                            div()
                                .id(SharedString::from(format!("{group}-open")))
                                .flex()
                                .cursor_pointer()
                                .opacity(0.6)
                                .hover(|style| style.opacity(1.))
                                .ml(px(2.))
                                .mr(px(4.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.follow_link(&open, &open, cx);
                                }))
                                .child(
                                    gpui::svg()
                                        .path("icons/embed_open_external.svg")
                                        .size(px(24.))
                                        .text_color(color("--white")),
                                )
                        })),
                )
                .into_any_element()
        };
        Some(
            div()
                .id(SharedString::from(format!("{group}-box")))
                .group(group.clone())
                .flex_none()
                .relative()
                .w(width)
                .h(height)
                .rounded(px(if proxied { 3. } else { 4. }))
                .overflow_hidden()
                .bg(color("--background-base-lower"))
                .children(poster.map(|poster| {
                    img(poster)
                        .w(width)
                        .h(height)
                        .object_fit(ObjectFit::Cover)
                        .rounded(px(4.))
                }))
                .child(overlay)
                .into_any_element(),
        )
    }

    /// `renderImages`: two columns 4px apart, the merged pictures cut to fill them, as tall as the media box.
    fn embed_gallery(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        available: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = embed.max_media().1 as f32;
        let side = ((available - 4.) / 2.).max(1.);
        let keys: Vec<String> = (0..embed.images.len().min(4))
            .map(|number| {
                if number == 0 {
                    image_key(message.id, index, "image")
                } else {
                    image_key(message.id, index, &format!("gallery:{number}"))
                }
            })
            .collect();
        let count = keys.len();
        let left: Vec<usize> = if count == 4 { vec![0, 2] } else { vec![0] };
        let right: Vec<usize> = match count {
            4 => vec![1, 3],
            3 => vec![1, 2],
            2 => vec![1],
            _ => vec![],
        };
        let column = |cells: Vec<usize>, cx: &mut Context<Self>| {
            let cell_height = if cells.len() > 1 {
                (height - 4.) / 2.
            } else {
                height
            };
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .overflow_hidden()
                .children(cells.into_iter().map(|cell| {
                    self.embed_picture(message, keys[cell].clone(), (side, cell_height), true, cx)
                        .rounded(px(0.))
                }))
        };
        let left = column(left, cx);
        let right = column(right, cx);
        div()
            .flex()
            .gap(px(4.))
            .h(px(height))
            .rounded(px(4.))
            .overflow_hidden()
            .child(left)
            .child(right)
            .into_any_element()
    }

    /// `renderMedia` inside the card (or alone for an inline embed).
    fn embed_media(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        slot: Media,
        available: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match slot {
            Media::None => None,
            Media::Image => {
                let image = embed.image.as_ref()?;
                let size = shrink(embed.fit(image), available);
                Some(
                    self.embed_picture(
                        message,
                        image_key(message.id, index, "image"),
                        size,
                        false,
                        cx,
                    )
                    .into_any_element(),
                )
            }
            Media::Gallery => Some(self.embed_gallery(message, index, embed, available, cx)),
            Media::Video => self.embed_video(message, index, embed, available, cx),
            Media::Gifv => {
                let size = shrink(embed_layout::gifv_size(embed)?, available);
                Some(
                    self.embed_picture(
                        message,
                        image_key(message.id, index, "thumbnail"),
                        size,
                        false,
                        cx,
                    )
                    .children(self.inline_video(message.id, index, embed, cx))
                    .into_any_element(),
                )
            }
        }
    }

    /// `inlineMediaEmbed_XyW1ZT`: the media alone, at most 520 wide, 2px under the line above.
    fn inline_media(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let slot = embed_layout::media(embed, false);
        let limit = embed
            .inline_max_width()
            .map_or(520., |width| width.min(520.) as f32)
            .min(self.content_width.max(1.));
        let media = self.embed_media(message, index, embed, slot, limit, cx)?;
        Some(div().pt(px(2.)).flex().child(media).into_any_element())
    }

    fn embed_link_text(
        &self,
        id: String,
        text: String,
        url: Option<String>,
        idle: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let base = div()
            .id(SharedString::from(id))
            .text_color(color(idle))
            .child(text);
        match url {
            Some(url) => base
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, &url, cx)))
                .into_any_element(),
            None => base.into_any_element(),
        }
    }

    fn embed_small_icon(
        &self,
        key: &str,
        picture: Arc<gpui::Image>,
        side: f32,
        animate: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if animate && super::gif_tiles::animated(picture.bytes()) {
            let pixels = (side * self.ui_scale.get().max(1.)).round().max(1.) as u32;
            self.gif_art_pixels(key, picture, (pixels, pixels), side / 2., None, cx)
        } else {
            gpui::img(picture)
                .size(px(side))
                .rounded_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        }
    }

    fn embed(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        suppress: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let message_id = message.id;
        let hide_media = self.embed_media_hidden();
        if embed_layout::is_inline(embed, hide_media) {
            return self.inline_media(message, index, embed, cx);
        }
        let animate_icons = self.pref_on("animate_gifs", true)
            && !super::motion::reduced()
            && !super::voice_video::client_unfocused(cx);
        let max_width = embed.max_width().map(|width| width as f32);
        let fixed = embed_layout::fixed_width(embed)
            .map(|width| (width as f32).min(self.content_width.max(1.)));
        let outer = fixed
            .or(max_width)
            .unwrap_or(GRID_MAX + BORDERS)
            .min(GRID_MAX + BORDERS)
            .min(self.content_width.max(1.));
        let content_width = (outer - BORDERS - PADDING_X).max(1.);
        // Text keys start past the message's own blocks so selections never collide.
        let text_key = (message_id, 1000 + index as u32 * 100);
        let edge = match embed.color {
            Some(value) if value != 0x00ff_ffff => rgba((value << 8) | 0xff),
            _ => color("--border-normal"),
        };
        let mut column = div().flex_1().min_w(px(0.)).flex().flex_col();
        let mut has_content = false;
        // `embedProvider_XyW1ZT`: body-xs/normal, a link in `--interactive-text-default`.
        if let Some(provider) = &embed.provider {
            has_content = true;
            let link = embed.provider_url.is_some();
            column = column.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(self.embed_link_text(
                        format!("embed-provider-{message_id}-{index}"),
                        provider.clone(),
                        embed.provider_url.clone(),
                        if link {
                            "--interactive-text-default"
                        } else {
                            "--text-default"
                        },
                        cx,
                    )),
            );
        }
        // `embedAuthor_XyW1ZT`: the round 24px icon 8px before the name (14px semibold, 22px lines).
        if let Some(author) = &embed.author {
            has_content = true;
            let icon_key = image_key(message_id, index, "author");
            let picture = embed
                .author_icon
                .as_ref()
                .map(|_| self.images.get(&icon_key).cloned());
            column = column.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .children(picture.map(|picture| {
                        div()
                            .flex_none()
                            .relative()
                            .size(px(24.))
                            .mr(px(8.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(picture.map(|picture| {
                                self.embed_small_icon(&icon_key, picture, 24., animate_icons, cx)
                            }))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .text_size(px(14.))
                            .line_height(px(22.))
                            .font_weight(FontWeight(600.))
                            .child(self.embed_link_text(
                                format!("embed-author-{message_id}-{index}"),
                                author.clone(),
                                embed.author_url.clone(),
                                "--text-strong",
                                cx,
                            )),
                    ),
            );
        }
        // `embedTitle_XyW1ZT`: 16px semibold; a link in `--text-link` when the embed has an address.
        if let Some(title) = &embed.title {
            has_content = true;
            column = column.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(600.))
                    .child(self.embed_link_text(
                        format!("embed-title-{message_id}-{index}"),
                        title.clone(),
                        embed.url.clone(),
                        if embed.url.is_some() {
                            "--text-link"
                        } else {
                            "--text-strong"
                        },
                        cx,
                    )),
            );
        }
        if embed_layout::shows_description(embed)
            && let Some(description) = &embed.description
        {
            has_content = true;
            let text = if embed.kind == "rich" {
                self.embed_text(description, text_key)
            } else {
                Self::embed_plain_text(description)
            };
            column = column.child(div().mt(px(8.)).child(text));
        }
        // `embedFields_XyW1ZT`: rows of up to three inline fields, 8px apart both ways.
        if !embed.fields.is_empty() {
            has_content = true;
            let rows = embed_layout::field_rows(&embed.fields);
            column = column.child(div().mt(px(8.)).flex().flex_col().gap(px(8.)).children(
                rows.into_iter().map(|row| {
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(row.into_iter().map(|placed| {
                            let field = &embed.fields[placed.index];
                            let value = if embed.kind == "rich" {
                                self.embed_text(
                                    &field.value,
                                    (text_key.0, text_key.1 + 50 + placed.index as u32),
                                )
                            } else {
                                Self::embed_plain_text(&field.value)
                            };
                            div()
                                .flex_grow((placed.end - placed.start) as f32)
                                .flex_basis(px(0.))
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .mb(px(2.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(field.name.clone()),
                                )
                                .child(value)
                        }))
                }),
            ));
        }
        let slot = embed_layout::media(embed, hide_media);
        let media = self
            .embed_media(message, index, embed, slot, content_width, cx)
            .map(|media| div().mt(px(16.)).flex().child(media));
        let thumbnail = embed_layout::shows_thumbnail(embed, hide_media)
            .then(|| embed_layout::thumbnail_size(embed))
            .flatten()
            .map(|size| {
                let size = (size.0 as f32, size.1 as f32);
                div()
                    .flex_none()
                    .ml(px(16.))
                    .mt(px(8.))
                    .child(self.embed_picture(
                        message,
                        image_key(message_id, index, "thumbnail"),
                        size,
                        false,
                        cx,
                    ))
            });
        let footer = self.embed_footer(message_id, index, embed, animate_icons, cx);
        if !has_content && thumbnail.is_none() && media.is_none() && footer.is_none() {
            return None;
        }
        let grid = div()
            .pt(px(2.))
            .pb(px(16.))
            .pl(px(12.))
            .pr(px(16.))
            .flex()
            .flex_col()
            .child(div().flex().items_start().child(column).children(thumbnail))
            .children(media)
            .children(footer);
        let group = SharedString::from(format!("embed-{message_id}-{index}"));
        // `embedSuppressButton_XyW1ZT`: 2px round a 16px cross, 2px above the card and 20px past its end, seen while
        // the card is hovered.
        let suppress_button = suppress.then(|| {
            let clicked = message.clone();
            div()
                .id(SharedString::from(format!("{group}-suppress")))
                .absolute()
                .top(px(-1.))
                .right(px(-19.))
                .w(px(20.))
                .h(px(26.))
                .p(px(2.))
                .cursor_pointer()
                .opacity(0.)
                .group_hover(group.clone(), |style| style.opacity(1.))
                .text_color(color("--interactive-text-default"))
                .hover(|style| style.text_color(color("--interactive-text-hover")))
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                    this.suppress_embeds(&clicked, event.modifiers().shift, cx);
                }))
                .child(
                    gpui::svg()
                        .path("icons/x_small.svg")
                        .size(px(16.))
                        .text_color(color("--interactive-text-default")),
                )
        });
        // The coloured edge is the card's own background showing through 4px at the start.
        let card = div()
            .id(SharedString::from(format!("{group}-card")))
            .group(group.clone())
            .parity("embed")
            .relative()
            .flex_none()
            .rounded(px(4.))
            .bg(edge)
            .pl(px(4.))
            .when_some(fixed, |this, width| this.w(px(width)))
            .max_w(px(outer))
            .child(
                div()
                    .rounded_r(px(4.))
                    .border_1()
                    .border_l_0()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .child(grid),
            )
            .children(suppress_button);
        Some(div().flex().child(card).into_any_element())
    }

    /// `renderFooter`: the round 20px icon 8px before the text, then " • " and the time (`mk` of module 5377, the
    /// calendar form: "Today at 11:28 AM", "Yesterday at", then the date and time); the time alone without a footer.
    fn embed_footer(
        &self,
        message: Id,
        index: usize,
        embed: &Embed,
        animate_icons: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let time = embed.timestamp.map(|time| {
            super::timestamps::calendar_time(
                time.with_timezone(&chrono::Local),
                chrono::Local::now(),
                false,
            )
        });
        if embed.footer.is_none() && time.is_none() {
            return None;
        }
        let icon_key = image_key(message, index, "footer");
        let icon = embed
            .footer_icon
            .as_deref()
            .filter(|icon| !icon.is_empty())
            .map(|_| self.images.get(&icon_key).cloned());
        let text = div()
            .flex()
            .flex_wrap()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-default"))
            .children(embed.footer.clone())
            .when(embed.footer.is_some() && time.is_some(), |this| {
                this.child(div().mx(px(4.)).child("\u{2022}"))
            })
            .children(time);
        Some(
            div()
                .mt(px(8.))
                .flex()
                .items_center()
                .when(embed.footer.is_some(), |this| {
                    this.children(icon.map(|picture| {
                        div()
                            .flex_none()
                            .relative()
                            .size(px(20.))
                            .mr(px(8.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(picture.map(|picture| {
                                self.embed_small_icon(&icon_key, picture, 20., animate_icons, cx)
                            }))
                    }))
                })
                .child(text)
                .into_any_element(),
        )
    }
}

/// `suppressEmbeds` of module 138399: `PATCH /channels/{c}/messages/{m}` with `SUPPRESS_EMBEDS` added to the flags.
pub(super) fn suppress_request(message: &Message) -> crate::net::Request {
    crate::net::Request::Rest {
        key: None,
        method: reqwest::Method::PATCH,
        route: format!("/channels/{}/messages/{}", message.channel_id, message.id),
        body: Some(serde_json::json!({"flags": message.flags | SUPPRESS_EMBEDS})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_video_tweet_fetches_its_poster_at_the_video_size_and_both_icons() {
        let mut message = Message::from_value(&json!({
            "id": "1438000000000000000", "channel_id": "5", "content": "https://fxtwitter.com/a/status/1/video/1",
            "timestamp": "2026-11-11T08:28:00+00:00", "type": 0, "flags": 0,
            "author": {"id": "10", "username": "a"},
            "embeds": [{
                "type": "rich", "url": "https://fxtwitter.com/a/status/1/video/1",
                "author": {"name": "Heisenberg (@burakalpekn)", "proxy_icon_url": "https://images-ext-1.discordapp.net/a.jpg"},
                "footer": {"text": "FxTwitter", "proxy_icon_url": "https://images-ext-1.discordapp.net/f.png"},
                "thumbnail": {"url": "https://pbs/t.jpg", "proxy_url": "https://images-ext-1.discordapp.net/t.jpg", "width": 720, "height": 1280},
                "video": {"url": "https://video/v.mp4", "proxy_url": "https://images-ext-1.discordapp.net/v.mp4", "width": 720, "height": 1280}
            }]
        }))
        .unwrap();
        let wanted = wanted(&message, true);
        let keys: Vec<&str> = wanted.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "embed:1438000000000000000:0:thumbnail",
                "embed:1438000000000000000:0:author",
                "embed:1438000000000000000:0:footer"
            ]
        );
        assert!(wanted[0].1.contains("width=169") && wanted[0].1.contains("height=300"));
        assert!(
            !message.content_is_inline_media(),
            "the link stays above a rich embed"
        );
        // Suppressing patches the flags with bit 4 added.
        message.flags = 16;
        let crate::net::Request::Rest {
            method,
            route,
            body,
            ..
        } = suppress_request(&message)
        else {
            panic!("not a REST request");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/channels/5/messages/1438000000000000000");
        assert_eq!(body, Some(json!({"flags": 20})));
    }

    #[test]
    fn still_icons_ask_for_png_and_galleries_page_in_order() {
        assert_eq!(
            icon_url(
                "https://images-ext-1.discordapp.net/a.gif?format=webp",
                false
            ),
            "https://images-ext-1.discordapp.net/a.gif?format=png"
        );
        assert_eq!(icon_url("https://x/a.gif", true), "https://x/a.gif");
        let list = Embed::list(
            &json!([
                {"type": "rich", "url": "https://x/p", "image": {"url": "https://x/a.png", "width": 10, "height": 10}},
                {"type": "rich", "url": "https://x/p", "image": {"url": "https://x/b.png", "width": 10, "height": 10}}
            ]),
            None,
        );
        let pictures = viewer_pictures(9, 0, &list[0]);
        let keys: Vec<&str> = pictures.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["embed:9:0:image", "embed:9:0:gallery:1"]);
        assert_eq!(shrink((400., 300.), 200.), (200., 150.));
    }
}
