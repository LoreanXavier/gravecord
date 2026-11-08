//! Message embeds: `embedFull_XyW1ZT` and the renderer in module 265705, normalized by `fK` in module 659674.
//! Sizes from `design/dom/channel/text-darker-cozy.json` (a GitHub link preview).

use super::{Root, color, hsla, rgba};
use crate::parity::Parity;
use dc_markdown::{Block, Kind, Span, parse};
use dc_state::{Embed, EmbedMedia, Id, Message};
use gpui::{AnyElement, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `gridContainer_XyW1ZT` max-width.
const GRID_MAX: f32 = 516.;
/// The 4px colored edge and the 1px border opposite it.
const BORDERS: f32 = 5.;
/// `grid_XyW1ZT` padding: 12px start, 16px end.
const PADDING_X: f32 = 28.;
/// `embedThumbnail_XyW1ZT` limits.
const THUMBNAIL: f32 = 80.;

pub(super) fn image_key(message: Id, index: usize, part: &str) -> String {
    format!("embed:{message}:{index}:{part}")
}

/// The media proxy URL for a picture drawn at `width` by `height`.
pub(super) fn media_url(media: &EmbedMedia, width: f64, height: f64) -> String {
    let base = media.proxy_url.as_deref().unwrap_or(&media.url);
    let joiner = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{joiner}format=webp&width={}&height={}",
        width.round() as u32,
        height.round() as u32
    )
}

/// What to fetch for a message's embeds: keys and URLs.
pub(super) fn wanted(message: &Message) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (index, embed) in message.embeds.iter().enumerate() {
        if let Some(image) = &embed.image {
            let (width, height) = Embed::fitted(image);
            out.push((
                image_key(message.id, index, "image"),
                media_url(image, width, height),
            ));
        }
        if let Some(thumbnail) = &embed.thumbnail {
            let (width, height) = fit(thumbnail, THUMBNAIL, THUMBNAIL);
            out.push((
                image_key(message.id, index, "thumbnail"),
                media_url(thumbnail, width, height),
            ));
        }
    }
    out
}

fn fit(media: &EmbedMedia, max_width: f32, max_height: f32) -> (f64, f64) {
    let (width, height) = (f64::from(media.width), f64::from(media.height));
    let scale = (f64::from(max_width) / width)
        .min(f64::from(max_height) / height)
        .min(1.);
    (width * scale, height * scale)
}

impl Root {
    /// Description and field text: markdown at 14/18 in the default text color, line breaks kept (`pre-line`).
    fn embed_text(&self, text: &str, key: (u64, u32)) -> AnyElement {
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
                    .map(|span| self.piece(span, base, 14., 400., 18.))
                    .collect();
                crate::inline::inline(pieces, px(18.)).selectable(
                    (key.0, key.1 + index as u32),
                    self.selection.clone(),
                    super::hsla_from(super::SELECTION),
                )
            }))
            .into_any_element()
    }

    pub(super) fn embeds(&self, message: &Message) -> Option<AnyElement> {
        let cards: Vec<AnyElement> = message
            .embeds
            .iter()
            .enumerate()
            .map(|(index, embed)| self.embed(message.id, index, embed))
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

    /// `inlineMediaEmbed_XyW1ZT`: the media alone, fitted into 400x300, radius 4, 2px under the line above.
    fn inline_media(&self, message: Id, index: usize, embed: &Embed) -> Option<AnyElement> {
        let image = embed.image.as_ref()?;
        let (width, height) = Embed::fitted(image);
        // A narrow chat scales the media down to its width.
        let shown = (width as f32).min(self.content_width.max(1.));
        let size = (px(shown), px(height as f32 * shown / width as f32));
        let picture = self
            .images
            .get(&image_key(message, index, "image"))
            .cloned();
        Some(
            div()
                .pt(px(2.))
                .child(
                    div()
                        .w(size.0)
                        .h(size.1)
                        .rounded(px(4.))
                        .overflow_hidden()
                        .bg(color("--background-base-low"))
                        .children(
                            picture.map(|picture| img(picture).w(size.0).h(size.1).rounded(px(4.))),
                        ),
                )
                .into_any_element(),
        )
    }

    fn embed(&self, message: Id, index: usize, embed: &Embed) -> AnyElement {
        if embed.is_inline_media()
            && let Some(media) = self.inline_media(message, index, embed)
        {
            return media;
        }
        let max_width = embed
            .max_width()
            .map_or(GRID_MAX + BORDERS, |width| width as f32);
        let content_width = max_width - BORDERS - PADDING_X;
        // Text keys start past the message's own blocks so selections never collide.
        let text_key = (message, 1000 + index as u32 * 100);
        let edge = match embed.color {
            Some(value) if value != 0x00ff_ffff => rgba((value << 8) | 0xff),
            _ => color("--border-normal"),
        };
        let mut column = div().flex_1().min_w(px(0.)).flex().flex_col();
        if let Some(provider) = &embed.provider {
            column = column.child(
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(provider.clone()),
            );
        }
        if let Some(author) = &embed.author {
            column = column.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(author.clone()),
            );
        }
        if let Some(title) = &embed.title {
            // The title links to the embed's address, as the message's own link does.
            let link = embed.url.clone();
            let root = self.weak.clone();
            column = column.child(
                div()
                    .id(SharedString::from(format!("embed-title-{message}-{index}")))
                    .mt(px(8.))
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(600.))
                    .text_color(if embed.url.is_some() {
                        color("--text-link")
                    } else {
                        color("--text-strong")
                    })
                    .when(link.is_some(), |this| this.cursor_pointer())
                    .on_click(move |_, _, cx| {
                        if let Some(url) = &link {
                            let _ = root.update(cx, |this, cx| this.follow_link(url, url, cx));
                        }
                    })
                    .child(title.clone()),
            );
        }
        if let Some(description) = &embed.description {
            column = column.child(
                div()
                    .mt(px(8.))
                    .child(self.embed_text(description, text_key)),
            );
        }
        if !embed.fields.is_empty() {
            column = column.child(div().mt(px(8.)).flex().flex_col().gap(px(8.)).children(
                embed.fields.iter().enumerate().map(|(field_index, field)| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(field.name.clone()),
                        )
                        .child(self.embed_text(
                            &field.value,
                            (text_key.0, text_key.1 + 50 + field_index as u32),
                        ))
                }),
            ));
        }
        if let Some(image) = &embed.image {
            let (width, height) = Embed::fitted(image);
            let shown = (width as f32).min(content_width);
            let scale = shown / width as f32;
            let size = (px(shown), px(height as f32 * scale));
            let picture = self
                .images
                .get(&image_key(message, index, "image"))
                .cloned();
            column = column.child(
                div()
                    .mt(px(16.))
                    .w(size.0)
                    .h(size.1)
                    .rounded(px(4.))
                    .bg(color("--background-base-low"))
                    .children(
                        picture.map(|picture| img(picture).w(size.0).h(size.1).rounded(px(4.))),
                    ),
            );
        }
        if let Some(footer) = &embed.footer {
            column = column.child(
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(footer.clone()),
            );
        }
        let thumbnail = embed.thumbnail.as_ref().map(|thumbnail| {
            let (width, height) = fit(thumbnail, THUMBNAIL, THUMBNAIL);
            let size = (px(width as f32), px(height as f32));
            let picture = self
                .images
                .get(&image_key(message, index, "thumbnail"))
                .cloned();
            div()
                .flex_none()
                .ml(px(16.))
                .mt(px(8.))
                .w(size.0)
                .h(size.1)
                .rounded(px(4.))
                .children(picture.map(|picture| img(picture).w(size.0).h(size.1).rounded(px(4.))))
        });
        let grid = div()
            .pt(px(2.))
            .pb(px(16.))
            .pl(px(12.))
            .pr(px(16.))
            .flex()
            .child(column)
            .children(thumbnail);
        // The colored edge is the card's own background showing through 4px at the start.
        div()
            .flex()
            .child(
                div()
                    .flex_none()
                    .max_w(px(max_width))
                    .rounded(px(4.))
                    .bg(edge)
                    .pl(px(4.))
                    .child(
                        div()
                            .rounded_r(px(4.))
                            .border_1()
                            .border_l_0()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .child(grid),
                    ),
            )
            .into_any_element()
    }
}
