//! Message embeds: the `em` component of module 265705 (`embedFull_XyW1ZT`, `inlineMediaEmbed_XyW1ZT`), normalized by
//! `fK` / `nh` of module 659674 (`dc_state::Embed`), listed by `renderEmbeds` of module 679636. The layout decisions
//! are in `embed_layout.rs`; sizes from the stylesheet `198496.95196da42e0b0983.css` and
//! `design/dom/channel/text-darker-cozy.json` (a GitHub article embed).

use super::embed_layout::{self, Media};
use super::tooltip::{Side, Tip};
use super::{Root, color, hsla, icon, rgba};
use crate::parity::Parity;
use dc_markdown::{Block, Kind, Span, embed_title, parse};
use dc_state::{Embed, EmbedMedia, Id, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, ObjectFit, SharedString, div, img,
    prelude::*, px,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// `gridContainer_XyW1ZT` max-width.
const GRID_MAX: f32 = 516.;
/// The 4px coloured edge and the 1px border opposite it.
const BORDERS: f32 = 5.;
/// `grid_XyW1ZT` padding: 12px start, 16px end.
const PADDING_X: f32 = 28.;
/// `MessageFlags.SUPPRESS_EMBEDS` and `EPHEMERAL` (module 652215).
pub(super) const SUPPRESS_EMBEDS: u64 = 4;
const EPHEMERAL: u64 = 64;

/// `EmbedFlags` (modules 652215 and 811602): explicit (`1 << 4`), gore (`1 << 6`), self-harm (`1 << 7`).
const EMBED_FLAG_EXPLICIT: u64 = 16;
const EMBED_FLAG_GORE: u64 = 64;
const EMBED_FLAG_SELF_HARM: u64 = 128;

/// Sensitive content harm mask (`LO` in module 811602).
const HARM_EXPLICIT: u8 = 1;
const HARM_GORE: u8 = 2;
const HARM_SELF_HARM: u8 = 4;

/// Content filter redaction levels (`SHOW = 1`, `BLUR = 2`, `BLOCK = 3`; modules 632119 and 444802).
const REDACTION_SHOW: u64 = 1;
const REDACTION_BLUR: u64 = 2;
const REDACTION_BLOCK: u64 = 3;
const EXPLICIT_FILTER_DEFAULT: u64 = 1;

/// `ObscureReason` (`av` in module 294520, resolved by modules 282108 and 863439).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObscureReason {
    Spoiler,
    ExplicitContent,
    GoreContent,
    SelfHarmContent,
    PotentialExplicitContent,
}

impl ObscureReason {
    fn is_sensitive(self) -> bool {
        !matches!(self, Self::Spoiler)
    }
}

static EMBED_REVEALED: OnceLock<Mutex<HashMap<(Id, usize), bool>>> = OnceLock::new();

fn revealed_map() -> &'static Mutex<HashMap<(Id, usize), bool>> {
    EMBED_REVEALED.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn is_embed_revealed(message_id: Id, index: usize) -> bool {
    revealed_map()
        .lock()
        .ok()
        .and_then(|map| map.get(&(message_id, index)).copied())
        .unwrap_or(false)
}

fn set_embed_revealed(message_id: Id, index: usize, revealed: bool) {
    if let Ok(mut map) = revealed_map().lock() {
        if revealed {
            map.insert((message_id, index), true);
        } else {
            map.remove(&(message_id, index));
        }
    }
}

fn toggle_embed_revealed(message_id: Id, index: usize) -> bool {
    if let Ok(mut map) = revealed_map().lock() {
        let next = !map.get(&(message_id, index)).copied().unwrap_or(false);
        if next {
            map.insert((message_id, index), true);
        } else {
            map.remove(&(message_id, index));
        }
        next
    } else {
        false
    }
}

/// `d` in module 354777 (`hasSpoilerEmbeds`): true when the message's paragraph content wraps a link inside `||...||`.
pub(super) fn message_has_spoiler_link(content: &str) -> bool {
    let blocks = parse(content);
    !blocks.is_empty()
        && blocks
            .iter()
            .all(|block| matches!(block, Block::Paragraph(_)))
        && blocks.iter().any(|block| {
            matches!(
                block,
                Block::Paragraph(spans)
                    if spans
                        .iter()
                        .any(|span| span.style.spoiler && matches!(span.kind, Kind::Link(_)))
            )
        })
}

fn embed_has_visual_media(embed: &Embed) -> bool {
    embed.image.is_some()
        || embed.thumbnail.is_some()
        || embed.video.is_some()
        || !embed.images.is_empty()
}

fn redaction_column(is_guild: bool, is_friend_dm: bool) -> usize {
    dc_proto::settings::CONTENT_REDACTION[0]
        .iter()
        .position(|pref| {
            if is_guild {
                pref.key.contains("guild")
            } else if is_friend_dm {
                pref.key.contains("friend") && !pref.key.contains("non_friend")
            } else {
                pref.key.contains("non_friend")
            }
        })
        .unwrap_or(if is_guild {
            0
        } else if is_friend_dm {
            1
        } else {
            2
        })
}

fn default_content_redaction(kind: usize, is_guild: bool, is_friend_dm: bool, filter: u64) -> u64 {
    if is_guild {
        if kind == 0 && filter == 2 {
            REDACTION_BLUR
        } else {
            REDACTION_SHOW
        }
    } else if is_friend_dm {
        if filter == 2 {
            REDACTION_BLUR
        } else {
            REDACTION_SHOW
        }
    } else if filter >= 1 {
        REDACTION_BLUR
    } else {
        REDACTION_SHOW
    }
}

/// Modules 282108, 33358, 632119 and 444802: active sensitive-content harm mask for `message` in `stores`.
pub(super) fn sensitive_harm_flags(stores: &dc_state::Stores, message: &Message) -> u8 {
    let Some(me) = stores.current_user.as_ref() else {
        return 0;
    };
    // Own messages never obscure for sensitive content (`module 282108`).
    if me.id == message.author.id {
        return 0;
    }
    let (is_guild, is_friend_dm, is_nsfw_guild) = if stores
        .private_channels
        .iter()
        .any(|channel| channel.id == message.channel_id)
    {
        let is_friend =
            stores.relationships.get(&message.author.id) == Some(&dc_state::RELATIONSHIP_FRIEND);
        (false, is_friend, false)
    } else if let Some(nsfw) = stores.guilds.iter().find_map(|guild| {
        if let Some(channel) = guild
            .channels
            .iter()
            .find(|channel| channel.id == message.channel_id)
        {
            return Some(channel.nsfw);
        }
        guild
            .threads
            .iter()
            .find(|thread| thread.id == message.channel_id)
            .map(|thread| {
                guild
                    .channels
                    .iter()
                    .find(|channel| channel.id == thread.parent_id)
                    .is_some_and(|channel| channel.nsfw)
            })
    }) {
        (true, false, nsfw)
    } else {
        return 0;
    };

    let col = redaction_column(is_guild, is_friend_dm);
    let filter = stores
        .prefs
        .get(dc_proto::settings::EXPLICIT_CONTENT_FILTER.key)
        .copied()
        .unwrap_or(EXPLICIT_FILTER_DEFAULT);
    let active_redaction = |kind: usize| {
        let pref = &dc_proto::settings::CONTENT_REDACTION[kind][col];
        let raw = stores.prefs.get(pref.key).copied().unwrap_or(0);
        let value = if raw == 0 {
            default_content_redaction(kind, is_guild, is_friend_dm, filter)
        } else {
            raw
        };
        value == REDACTION_BLUR || value == REDACTION_BLOCK
    };
    let self_harm_active = dc_proto::settings::PROTO_PREFS
        .iter()
        .filter(|pref| pref.key.contains("self_harm"))
        .find(|pref| {
            if is_guild {
                pref.key.contains("guild")
            } else if is_friend_dm {
                pref.key.contains("friend") && !pref.key.contains("non_friend")
            } else {
                pref.key.contains("non_friend")
            }
        })
        .and_then(|pref| stores.prefs.get(pref.key).copied())
        .is_some_and(|val| val == REDACTION_BLUR || val == REDACTION_BLOCK);

    let mut flags = 0u8;
    // Guild NSFW channels skip explicit flag 16 (`module 282108`).
    if !is_nsfw_guild && active_redaction(0) {
        flags |= HARM_EXPLICIT;
    }
    if active_redaction(1) {
        flags |= HARM_GORE;
    }
    if self_harm_active {
        flags |= HARM_SELF_HARM;
    }
    flags
}

/// `sC` in module 294520 and `S`/`N` in module 282108: resolves the embed's obscure reason from harm flags and spoilers.
pub(super) fn obscure_reason_for(
    embed: &Embed,
    author_is_bot: bool,
    has_spoiler_embeds: bool,
    harm_flags: u8,
) -> Option<ObscureReason> {
    if harm_flags & HARM_EXPLICIT != 0 && embed.flags & EMBED_FLAG_EXPLICIT != 0 {
        return Some(ObscureReason::ExplicitContent);
    }
    if harm_flags & HARM_GORE != 0 && embed.flags & EMBED_FLAG_GORE != 0 {
        return Some(ObscureReason::GoreContent);
    }
    if harm_flags & HARM_SELF_HARM != 0 && embed.flags & EMBED_FLAG_SELF_HARM != 0 {
        return Some(ObscureReason::SelfHarmContent);
    }
    if !author_is_bot && harm_flags != 0 && embed_has_visual_media(embed) {
        let version = embed.content_scan_version;
        let pending = version != Some(-1)
            && if harm_flags & (HARM_GORE | HARM_SELF_HARM) != 0 {
                version.is_none_or(|v| v < 4)
            } else {
                version.is_none_or(|v| v < 1)
            };
        if pending {
            return Some(ObscureReason::PotentialExplicitContent);
        }
    }
    has_spoiler_embeds.then_some(ObscureReason::Spoiler)
}

pub(super) fn image_key(message: Id, index: usize, part: &str) -> String {
    format!("embed:{message}:{index}:{part}")
}

/// The media proxy URL for a picture drawn at `width` by `height`, with the query the image component adds.
pub(super) fn media_url(media: &EmbedMedia, width: f64, height: f64) -> String {
    super::media_image::scaled_image_url(
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
                super::media_image::scaled_image_url(
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
                let block_key = (key.0, key.1 + index as u32);
                let spoilers = Self::spoiler_run_starts(&spans);
                let pieces = spans
                    .iter()
                    .zip(&spoilers)
                    .map(|(span, start)| {
                        self.piece_with_spoiler(span, *start, block_key, base, (size, 400., line))
                    })
                    .collect();
                let element = crate::inline::inline(pieces, px(line)).selectable(
                    block_key,
                    self.selection.clone(),
                    super::hsla_from(super::SELECTION),
                );
                self.wire_inline_spans(element, &spans, &spoilers, block_key, &[])
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
        let mut cards: Vec<AnyElement> = message
            .embeds
            .iter()
            .enumerate()
            .filter(|(_, embed)| !embed_layout::left_out(embed))
            .filter_map(|(index, embed)| self.embed(message, index, embed, suppress, cx))
            .collect();
        // Module 107543 + 562293 (`QUESTS_EMBED`): Quest Promoted Card or Fallback Card for
        // `https://discord.com/quests/{id}` links.
        if message.content.contains("/quests/")
            && let Some(qid) = super::quest_promoted::extract_quest_link_id(&message.content)
        {
            let prefix = format!("msg-quest-{}", message.id);
            if let Some(quest) = self.quests.store.quests.iter().find(|q| q.id == qid) {
                cards.push(self.quest_promoted_card(quest, &prefix, cx));
            } else {
                let reason = if self.quests.store.excluded_quests.iter().any(|e| e == qid) {
                    super::quest_promoted::QuestFallbackReason::Excluded
                } else {
                    super::quest_promoted::QuestFallbackReason::NotFound
                };
                cards.push(self.quest_embed_fallback_card(qid, reason, &prefix, cx));
            }
        }
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

    /// `863439`: whether spoiler embeds start obscured under the user's `renderSpoilers` setting.
    fn should_obscure_spoilers(&self) -> bool {
        match self.pref_index("chat_spoilers_show_spoilers", 0) {
            // `ALWAYS`
            1 => false,
            // `IF_MODERATOR`
            2 => !self.current_channel().is_some_and(|channel| {
                self.can_in_channel(channel, dc_state::permissions::MANAGE_MESSAGES)
            }),
            // `ON_CLICK` (default)
            _ => true,
        }
    }

    fn embed_obscure_reason(&self, message: &Message, embed: &Embed) -> Option<ObscureReason> {
        let harm_flags = sensitive_harm_flags(&self.stores, message);
        let spoiler_embeds =
            message_has_spoiler_link(&message.content) && self.should_obscure_spoilers();
        obscure_reason_for(embed, message.author.bot, spoiler_embeds, harm_flags)
    }

    fn spoiler_overlay(
        &self,
        group: &SharedString,
        message_id: Id,
        index: usize,
        radius: f32,
        card_embed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = dc_intl::t!("F+x38C").to_uppercase();
        div()
            .id(SharedString::from(format!("{group}-spoiler-overlay")))
            .parity("embed-spoiler-overlay")
            .absolute()
            .inset_0()
            .rounded(px(radius))
            .backdrop_blur(px(44.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |_, _, _, cx| {
                set_embed_revealed(message_id, index, true);
                cx.notify();
            }))
            .child(
                div()
                    .when(card_embed, |this| this.ml(px(4.)))
                    .py(px(8.))
                    .px(px(12.))
                    .rounded(px(20.))
                    .bg(rgba(0x00000099))
                    .group_hover(group.clone(), |style| {
                        style.bg(rgba(0x000000e0)).text_color(color("--white"))
                    })
                    .text_size(px(15.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--primary-200"))
                    .flex()
                    .items_center()
                    .gap(px(0.5))
                    .children(label.chars().map(|ch| ch.to_string())),
            )
            .into_any_element()
    }

    fn sensitive_cover(reason: ObscureReason, group: &SharedString, radius: f32) -> AnyElement {
        let show_warning = reason != ObscureReason::PotentialExplicitContent;
        div()
            .id(SharedString::from(format!("{group}-sensitive-cover")))
            .parity("embed-sensitive-cover")
            .absolute()
            .inset_0()
            .rounded(px(radius))
            .backdrop_blur(px(44.))
            .bg(rgba(0x000000eb))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(16.))
            .text_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(show_warning, |this| {
                this.child(
                    gpui::svg()
                        .path("icons/warning_triangle.svg")
                        .size(px(24.))
                        .flex_none()
                        .text_color(color("--text-overlay-light")),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-overlay-light"))
                        .child(dc_intl::t!("SpxcUR")),
                )
            })
            .into_any_element()
    }

    fn embed_sensitive_toggle_button(
        &self,
        group: &SharedString,
        message_id: Id,
        index: usize,
        visible: bool,
        over_video: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let glyph = if visible { "clips_eye" } else { "eye_slash" };
        let button = div()
            .id(SharedString::from(format!("{group}-obscure-toggle")))
            .parity("embed-sensitive-toggle")
            .size(px(32.))
            .rounded(px(8.))
            .bg(color("--control-overlay-secondary-background-default"))
            .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |_, _, _, cx| {
                toggle_embed_revealed(message_id, index);
                cx.notify();
            }))
            .child(icon(glyph, 18., "--control-overlay-secondary-text-default"));
        div()
            .absolute()
            .bottom(px(if visible && over_video { 36. } else { 4. }))
            .right(px(4.))
            .child(self.with_tip(
                format!("{group}-obscure-tip"),
                Tip::text(dc_intl::t!("ex5G9m"), Side::Top),
                button,
                cx,
            ))
            .into_any_element()
    }

    fn embed_rich_inline(
        &self,
        text: &str,
        allow_links: bool,
        base: gpui::Hsla,
        (size, line): (f32, f32),
        key: (u64, u32),
        interactive: bool,
    ) -> AnyElement {
        let lines: Vec<&str> = text.split('\n').collect();
        div()
            .flex()
            .flex_col()
            .min_w(px(0.))
            .children(lines.into_iter().enumerate().map(|(row, raw_line)| {
                let spans = embed_title(raw_line, allow_links && interactive);
                let row_key = (key.0, key.1.saturating_add(row as u32));
                let spoilers = if interactive {
                    Self::spoiler_run_starts(&spans)
                } else {
                    vec![None; spans.len()]
                };
                let pieces = spans
                    .iter()
                    .zip(&spoilers)
                    .map(|(span, start)| {
                        let rendered = if interactive {
                            span.clone()
                        } else {
                            let mut inert = span.clone();
                            inert.style.spoiler = false;
                            if matches!(inert.kind, Kind::Link(_) | Kind::Channel(_)) {
                                inert.kind = Kind::Text;
                            }
                            inert
                        };
                        self.piece_with_spoiler(
                            &rendered,
                            *start,
                            row_key,
                            base,
                            (size, 600., line),
                        )
                    })
                    .collect();
                let element = crate::inline::inline(pieces, px(line)).selectable(
                    row_key,
                    self.selection.clone(),
                    super::hsla_from(super::SELECTION),
                );
                if interactive {
                    self.wire_inline_spans(element, &spans, &spoilers, row_key, &[])
                } else {
                    element
                }
            }))
            .into_any_element()
    }

    /// A picture of the embed at its drawn size, opening the media viewer on click; the box keeps the declared size
    /// while the picture loads so the message does not move.
    fn embed_picture(
        &self,
        message: &Message,
        key: String,
        size: (f32, f32),
        cover: bool,
        playable: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let picture = self.images.get(&key).cloned();
        let (clicked, open, hovered) = (message.clone(), key.clone(), key.clone());
        let media_hit = super::lightbox::items_of(message)
            .into_iter()
            .find(|item| item.key == key)
            .map(|item| {
                if item.is_video || !super::menu::can_save_image(&item.url) {
                    super::menu::Hit::Video {
                        url: item.url.clone(),
                        src: item.url,
                    }
                } else {
                    super::menu::Hit::Media {
                        url: item.url,
                        key: item.key,
                    }
                }
            });
        let (width, height) = (px(size.0), px(size.1));
        div()
            .id(SharedString::from(format!("picture-{key}")))
            .flex_none()
            .relative()
            .w(width)
            .h(height)
            .rounded(px(4.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                    if let Some(hit) = media_hit.clone() {
                        this.menu_hit = Some((event.position, hit));
                    }
                }),
            )
            .when(playable, |this| {
                this.cursor_pointer()
                    .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                        this.media_hover(&hovered, *inside, cx);
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_lightbox(&clicked, &open, cx);
                    }))
            })
            .children(picture.map(|picture| {
                if playable {
                    self.media_image(picture, &key, (width, height), 4., cover, cx)
                } else {
                    img(picture)
                        .w(width)
                        .h(height)
                        .object_fit(if cover {
                            ObjectFit::Cover
                        } else {
                            ObjectFit::Contain
                        })
                        .rounded(px(4.))
                        .into_any_element()
                }
            }))
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
        playable: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let size = shrink(embed_layout::video_size(embed)?, available);
        let video = embed.video.as_ref()?;
        let key = image_key(message.id, index, "thumbnail");
        let force_external = video.width > 6016 || video.height > 3384;
        if playable && let Some(proxy_url) = video.proxy_url.as_ref().filter(|_| !force_external) {
            let menu_url = video.url.clone();
            let menu_src = proxy_url.clone();
            return Some(
                div()
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                            this.menu_hit = Some((
                                event.position,
                                super::menu::Hit::Video {
                                    url: menu_url.clone(),
                                    src: menu_src.clone(),
                                },
                            ));
                        }),
                    )
                    .child(self.render_media_player(
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
                    ))
                    .into_any_element(),
            );
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
        // `ee`: Play put the provider's player in this box; the web view sits over the anchor drawn here.
        let player_key = format!("embed:{}:{index}", message.id);
        if !proxied && self.provider_playing(&player_key) {
            return Some(
                div()
                    .id(SharedString::from(format!("{group}-player")))
                    .flex_none()
                    .relative()
                    .w(width)
                    .h(height)
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--black"))
                    .child(
                        self.web_frames
                            .probe(super::provider_player::ANCHOR.to_owned(), None),
                    )
                    .into_any_element(),
            );
        }
        let player_src = super::provider_player::player_url(
            &video.url,
            embed.provider.as_deref().unwrap_or_default(),
        );
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
                .when(playable, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.follow_link(&target, &target, cx);
                        }))
                })
                .child(
                    div()
                        .p(px(12.))
                        .rounded(px(24.))
                        .bg(color("--black"))
                        .opacity(0.6)
                        .when(playable, |this| {
                            this.group_hover(group.clone(), |style| style.opacity(0.8))
                        })
                        .child(play_icon()),
                )
                .into_any_element()
        } else {
            let (play, open) = (player_src.clone(), page.clone());
            let play_key = player_key.clone();
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
                                .opacity(0.6)
                                .when(playable, |this| {
                                    this.cursor_pointer()
                                        .hover(|style| style.opacity(1.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.provider_play(play_key.clone(), play.clone(), cx);
                                        }))
                                })
                                .child(play_icon()),
                        )
                        .children(open.map(|open| {
                            div()
                                .id(SharedString::from(format!("{group}-open")))
                                .flex()
                                .opacity(0.6)
                                .ml(px(2.))
                                .mr(px(4.))
                                .when(playable, |this| {
                                    this.cursor_pointer()
                                        .hover(|style| style.opacity(1.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.follow_link(&open, &open, cx);
                                        }))
                                })
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
        let menu_video_url = target.clone();
        let menu_video_src = video.proxy_url.clone().unwrap_or_else(|| target.clone());
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
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                        this.menu_hit = Some((
                            event.position,
                            super::menu::Hit::Video {
                                url: menu_video_url.clone(),
                                src: menu_video_src.clone(),
                            },
                        ));
                    }),
                )
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
        playable: bool,
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
                    self.embed_picture(
                        message,
                        keys[cell].clone(),
                        (side, cell_height),
                        true,
                        playable,
                        cx,
                    )
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
        (slot, available, playable): (Media, f32, bool),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match slot {
            Media::None => None,
            Media::Image => {
                let image = embed.image.as_ref()?;
                let size = shrink(embed.fit(image), available);
                let key = image_key(message.id, index, "image");
                let group = SharedString::from(format!("embed-img-{}-{index}", message.id));
                let favorite = (playable
                    && (image.is_animated()
                        || self
                            .images
                            .get(&key)
                            .is_some_and(|pic| super::gif_tiles::animated(pic.bytes()))))
                .then(|| {
                    // Module 343552 `b`: favorite key prefers embed URL, falling back to proxy/original image URL.
                    let src = image.src().to_owned();
                    let url = embed
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&src)
                        .to_owned();
                    super::gifs::FavoriteTarget {
                        url,
                        src,
                        width: image.width.max(1),
                        height: image.height.max(1),
                        format: 1,
                    }
                });
                Some(
                    self.embed_picture(message, key, size, false, playable, cx)
                        .group(group.clone())
                        .children(favorite.map(|target| {
                            self.gif_favorite_button(
                                format!("embed-gif-fav-{}-{index}", message.id),
                                group,
                                target,
                                cx,
                            )
                        }))
                        .into_any_element(),
                )
            }
            Media::Gallery => {
                Some(self.embed_gallery(message, index, embed, available, playable, cx))
            }
            Media::Video => self.embed_video(message, index, embed, available, playable, cx),
            Media::Gifv => {
                let size = shrink(embed_layout::gifv_size(embed)?, available);
                let group = SharedString::from(format!("embed-gifv-{}-{index}", message.id));
                let favorite = playable
                    .then(|| embed.video.as_ref().or(embed.thumbnail.as_ref()))
                    .flatten()
                    .map(|media| {
                        let src = media.src().to_owned();
                        let url = embed
                            .url
                            .as_deref()
                            .filter(|u| !u.is_empty())
                            .unwrap_or(&src)
                            .to_owned();
                        super::gifs::FavoriteTarget {
                            url,
                            src,
                            width: media.width.max(1),
                            height: media.height.max(1),
                            format: if embed.video.is_some() { 2 } else { 1 },
                        }
                    });
                Some(
                    self.embed_picture(
                        message,
                        image_key(message.id, index, "thumbnail"),
                        size,
                        false,
                        playable,
                        cx,
                    )
                    .group(group.clone())
                    .when(playable, |this| {
                        this.children(self.inline_video(message.id, index, embed, cx))
                    })
                    .children(favorite.map(|target| {
                        self.gif_favorite_button(
                            format!("embed-gifv-fav-{}-{index}", message.id),
                            group,
                            target,
                            cx,
                        )
                    }))
                    .into_any_element(),
                )
            }
            Media::Audio => {
                let audio = embed.audio.as_ref()?;
                Some(self.embed_audio_player(message.id, index, &audio.proxy_url, playable))
            }
        }
    }

    /// `inlineMediaEmbed_XyW1ZT`: the media alone, at most 520 wide, with no margin of its own: the 2px above it are
    /// the accessories' padding (`container_YUQhib`; frame channel/pt-gifv: the embed 2px under the container top).
    fn inline_media(
        &self,
        message: &Message,
        index: usize,
        embed: &Embed,
        reason: Option<ObscureReason>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let slot = embed_layout::media(embed, false);
        let limit = embed
            .inline_max_width()
            .map_or(520., |width| width.min(520.) as f32)
            .min(self.content_width.max(1.));
        let visible = reason.is_none_or(|_| is_embed_revealed(message.id, index));
        let media = self.embed_media(message, index, embed, (slot, limit, visible), cx)?;
        let Some(reason) = reason else {
            return Some(div().flex().child(media).into_any_element());
        };
        let group = SharedString::from(format!("embed-inline-{}-{index}", message.id));
        let hidden_spoiler = reason == ObscureReason::Spoiler && !visible;
        let hidden_sensitive = reason.is_sensitive() && !visible;
        let allow_min_size = hidden_sensitive && limit >= 146.;
        let mut wrapper = div()
            .id(SharedString::from(format!("{group}-box")))
            .group(group.clone())
            .relative()
            .flex()
            .rounded(px(8.))
            .overflow_hidden()
            .when(hidden_spoiler, |this| {
                this.bg(color("--spoiler-hidden-background"))
                    .group_hover(group.clone(), |style| {
                        style.bg(color("--spoiler-hidden-background-hover"))
                    })
            })
            .when(hidden_sensitive, |this| {
                this.bg(color("--spoiler-hidden-background"))
            })
            .when(allow_min_size, |this| this.min_w(px(146.)).min_h(px(212.)))
            .child(media);
        if hidden_spoiler {
            wrapper = wrapper.child(self.spoiler_overlay(&group, message.id, index, 8., false, cx));
        } else if reason.is_sensitive() {
            if !visible {
                wrapper = wrapper.child(Self::sensitive_cover(reason, &group, 8.));
            }
            wrapper = wrapper.child(self.embed_sensitive_toggle_button(
                &group,
                message.id,
                index,
                visible,
                slot == Media::Video,
                cx,
            ));
        }
        Some(div().pt(px(2.)).flex().child(wrapper).into_any_element())
    }

    fn embed_link_text(
        &self,
        id: String,
        text: String,
        url: Option<String>,
        idle: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `min-width: 0` (the `embedProvider`/`embedAuthor`/`embedTitle` rule): in the row it sits in, the text wraps
        // inside its column instead of running on at its one-line width (under the thumbnail, out of the card).
        let label_for_menu = text.clone();
        let base = div()
            .id(SharedString::from(id))
            .min_w(px(0.))
            .text_color(color(idle))
            .child(text);
        match url {
            Some(url) => {
                let menu_url = url.clone();
                base.cursor_pointer()
                    .hover(|style| style.underline())
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                            this.menu_hit = Some((
                                event.position,
                                super::menu::Hit::Link {
                                    url: menu_url.clone(),
                                    label: label_for_menu.clone(),
                                },
                            ));
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, &url, cx)))
                    .into_any_element()
            }
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
        // 265705: a Spotify link is Spotify's own player, not a card.
        if let Some((url, height)) = super::web_frames::spotify_frame(embed) {
            return Some(self.web_frame_box(format!("{message_id}:{index}"), url, height));
        }
        let hide_media = self.embed_media_hidden();
        let reason = self.embed_obscure_reason(message, embed);
        if embed_layout::is_inline(embed, hide_media) {
            return self.inline_media(message, index, embed, reason, cx);
        }
        let visible = reason.is_none_or(|_| is_embed_revealed(message_id, index));
        let animate_icons = visible
            && self.pref_on("animate_gifs", true)
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
                        embed.provider_url.clone().filter(|_| visible),
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
                                embed.author_url.clone().filter(|_| visible),
                                "--text-strong",
                                cx,
                            )),
                    ),
            );
        }
        // `embedTitle_XyW1ZT`: 16px semibold; rich embeds parse `EMBED_TITLE_RULES` (`module 679636` / `605275`).
        if let Some(title) = &embed.title {
            has_content = true;
            let embed_url = embed.url.as_deref().filter(|url| !url.is_empty());
            let title_el = if embed.kind == "rich" {
                let allow_links = embed_url.is_none();
                let base = if embed_url.is_some() {
                    hsla("--text-link")
                } else {
                    hsla("--text-strong")
                };
                let inline_title = self.embed_rich_inline(
                    title,
                    allow_links,
                    base,
                    (16., 22.),
                    (text_key.0, text_key.1 + 90),
                    visible,
                );
                let wrapper = div()
                    .id(SharedString::from(format!(
                        "embed-title-{message_id}-{index}"
                    )))
                    .min_w(px(0.))
                    .child(inline_title);
                if let Some(url) = embed_url.filter(|_| visible) {
                    let url = url.to_owned();
                    wrapper
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.follow_link(&url, &url, cx)),
                        )
                        .into_any_element()
                } else {
                    wrapper.into_any_element()
                }
            } else {
                self.embed_link_text(
                    format!("embed-title-{message_id}-{index}"),
                    title.clone(),
                    embed_url.filter(|_| visible).map(str::to_owned),
                    if embed_url.is_some() {
                        "--text-link"
                    } else {
                        "--text-strong"
                    },
                    cx,
                )
            };
            column = column.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(600.))
                    .child(title_el),
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
        } else if embed.kind == "video"
            && let Some(description) = &embed.description
            && let Some(shown) =
                super::plugin_hooks::video_description(message_id, index, description)
        {
            // Not in the pinned client: FixYoutubeEmbeds' `youtubeDescription` (`plugin_hooks/fix_youtube_embeds.rs`).
            has_content = true;
            let folded = matches!(shown, super::plugin_hooks::VideoDescription::Folded(_));
            let inner = match shown {
                super::plugin_hooks::VideoDescription::Whole => {
                    div().mt(px(8.)).child(Self::embed_plain_text(description))
                }
                super::plugin_hooks::VideoDescription::Folded(text) => div().child(text),
            };
            column = column.child(
                div()
                    .id(SharedString::from(format!(
                        "yt-description-{message_id}-{index}"
                    )))
                    .cursor_pointer()
                    .when(folded, |this| this.mt(px(8.)))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .child(inner)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        super::plugin_hooks::toggle_video_description(message_id, index);
                        cx.notify();
                    })),
            );
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
                            let name_el = if embed.kind == "rich" {
                                self.embed_rich_inline(
                                    &field.name,
                                    true,
                                    hsla("--text-strong"),
                                    (14., 18.),
                                    (text_key.0, text_key.1 + 20 + placed.index as u32),
                                    visible,
                                )
                            } else {
                                div().child(field.name.clone()).into_any_element()
                            };
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
                                        .child(name_el),
                                )
                                .child(value)
                        }))
                }),
            ));
        }
        let slot = embed_layout::media(embed, hide_media);
        let media = self
            .embed_media(message, index, embed, (slot, content_width, visible), cx)
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
                        visible,
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
        let hidden_spoiler = reason == Some(ObscureReason::Spoiler) && !visible;
        let hidden_sensitive = reason.is_some_and(ObscureReason::is_sensitive) && !visible;
        let hide_accent = hidden_spoiler || hidden_sensitive;
        let allow_min_size = hidden_sensitive && outer >= 146.;

        let mut body = if hide_accent {
            div()
                .relative()
                .rounded(px(4.))
                .overflow_hidden()
                .border_1()
                .border_color(color("--spoiler-hidden-background"))
                .bg(color("--spoiler-hidden-background"))
                .when(hidden_spoiler, |this| {
                    this.group_hover(group.clone(), |style| {
                        style
                            .bg(color("--spoiler-hidden-background-hover"))
                            .border_color(color("--spoiler-hidden-background-hover"))
                    })
                })
                .when(allow_min_size, |this| this.min_w(px(146.)).min_h(px(212.)))
                .child(grid)
        } else {
            div()
                .relative()
                .rounded_r(px(4.))
                .border_1()
                .border_l_0()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .child(grid)
        };
        if hidden_spoiler {
            body = body.child(self.spoiler_overlay(&group, message_id, index, 4., true, cx));
        } else if let Some(reason) = reason.filter(|r| r.is_sensitive()) {
            if !visible {
                body = body.child(Self::sensitive_cover(reason, &group, 4.));
            }
            body = body.child(self.embed_sensitive_toggle_button(
                &group,
                message_id,
                index,
                visible,
                slot == Media::Video,
                cx,
            ));
        }

        // The coloured edge is the card's own background showing through 4px at the start (suppressed while obscured).
        let card = div()
            .id(SharedString::from(format!("{group}-card")))
            .group(group.clone())
            .parity("embed")
            .relative()
            .flex_none()
            .rounded(px(4.))
            .when(!hide_accent, |this| this.bg(edge).pl(px(4.)))
            .when_some(fixed, |this, width| this.w(px(width)))
            .max_w(px(outer))
            .child(body)
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
    fn unflagged_gif_embeds_request_animation_through_the_proxy() {
        let message = Message::from_value(&json!({
            "id": "1438000000000000000", "channel_id": "5",
            "content": "https://example.com/loop.GIF?key=value",
            "timestamp": "2026-11-11T08:28:00+00:00",
            "author": {"id": "10", "username": "a"},
            "embeds": [{
                "type": "image",
                "url": "https://example.com/loop.GIF?key=value",
                "thumbnail": {
                    "url": "https://example.com/loop.GIF?key=value",
                    "proxy_url": "https://media.example/preview.webp?format=webp",
                    "width": 360,
                    "height": 300
                }
            }]
        }))
        .unwrap();
        assert_eq!(
            wanted(&message, true),
            vec![(
                image_key(message.id, 0, "image"),
                "https://media.example/preview.webp?width=360&height=300".to_owned()
            )]
        );
    }

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

    #[test]
    fn spoiler_link_detection_and_reveal_state_toggle() {
        assert!(message_has_spoiler_link("check ||https://example.com/a||"));
        assert!(message_has_spoiler_link(
            "||[secret](https://example.com)||"
        ));
        assert!(!message_has_spoiler_link(
            "||just text|| https://example.com"
        ));
        assert!(!message_has_spoiler_link("> ||https://example.com||"));

        set_embed_revealed(987_654, 2, false);
        assert!(!is_embed_revealed(987_654, 2));
        assert!(toggle_embed_revealed(987_654, 2));
        assert!(is_embed_revealed(987_654, 2));
        assert!(!toggle_embed_revealed(987_654, 2));
        assert!(!is_embed_revealed(987_654, 2));
    }

    #[test]
    fn obscure_reason_resolves_explicit_gore_self_harm_pending_and_spoiler() {
        let base = Embed {
            kind: "image".into(),
            image: Some(EmbedMedia {
                url: "https://x/a.png".into(),
                proxy_url: None,
                width: 400,
                height: 300,
                description: None,
                flags: 0,
            }),
            content_scan_version: Some(4),
            ..Embed::default()
        };

        let explicit = Embed {
            flags: EMBED_FLAG_EXPLICIT,
            ..base.clone()
        };
        assert_eq!(
            obscure_reason_for(&explicit, false, true, HARM_EXPLICIT),
            Some(ObscureReason::ExplicitContent)
        );

        let gore = Embed {
            flags: EMBED_FLAG_GORE,
            ..base.clone()
        };
        assert_eq!(
            obscure_reason_for(&gore, false, false, HARM_GORE),
            Some(ObscureReason::GoreContent)
        );

        let self_harm = Embed {
            flags: EMBED_FLAG_SELF_HARM,
            ..base.clone()
        };
        assert_eq!(
            obscure_reason_for(&self_harm, false, false, HARM_SELF_HARM),
            Some(ObscureReason::SelfHarmContent)
        );

        let pending = Embed {
            content_scan_version: None,
            ..base.clone()
        };
        assert_eq!(
            obscure_reason_for(&pending, false, false, HARM_EXPLICIT),
            Some(ObscureReason::PotentialExplicitContent)
        );
        // Bots skip pending scan obscuring.
        assert_eq!(
            obscure_reason_for(&pending, true, false, HARM_EXPLICIT),
            None
        );
        // Version -1 skips pending scan obscuring.
        let exempt = Embed {
            content_scan_version: Some(-1),
            ..base.clone()
        };
        assert_eq!(
            obscure_reason_for(&exempt, false, false, HARM_EXPLICIT),
            None
        );
        assert_eq!(
            obscure_reason_for(&base, false, true, 0),
            Some(ObscureReason::Spoiler)
        );
    }

    #[test]
    fn sensitive_harm_flags_exempt_own_messages_and_skip_explicit_in_nsfw_guild_channels() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me", "nsfw_allowed": true},
                "relationships": [{"id": "2", "type": 1}],
                "private_channels": [
                    {"id": "10", "type": 1, "recipient_ids": ["2"]},
                    {"id": "11", "type": 1, "recipient_ids": ["3"]}
                ],
                "guilds": [{
                    "id": "50",
                    "name": "Server",
                    "channels": [
                        {"id": "51", "type": 0, "name": "general", "nsfw": false},
                        {"id": "52", "type": 0, "name": "nsfw", "nsfw": true}
                    ]
                }]
            }),
        );
        for row in &dc_proto::settings::CONTENT_REDACTION {
            for pref in row {
                stores.prefs.insert(pref.key, REDACTION_BLUR);
            }
        }

        let make_msg = |channel_id: &str, author_id: &str| {
            Message::from_value(&json!({
                "id": "1438000000000000001",
                "channel_id": channel_id,
                "content": "",
                "timestamp": "2026-11-11T08:28:00+00:00",
                "type": 0,
                "flags": 0,
                "author": {"id": author_id, "username": "u"}
            }))
            .unwrap()
        };

        // Own message never obscures for sensitive content.
        assert_eq!(sensitive_harm_flags(&stores, &make_msg("51", "1")), 0);
        // Normal guild channel blurs both explicit and gore.
        assert_eq!(
            sensitive_harm_flags(&stores, &make_msg("51", "2")),
            HARM_EXPLICIT | HARM_GORE
        );
        // NSFW guild channel skips explicit (16) while still blurring gore (64).
        assert_eq!(
            sensitive_harm_flags(&stores, &make_msg("52", "2")),
            HARM_GORE
        );
        // Friend and non-friend DMs read their redaction prefs.
        assert_eq!(
            sensitive_harm_flags(&stores, &make_msg("10", "2")),
            HARM_EXPLICIT | HARM_GORE
        );
        assert_eq!(
            sensitive_harm_flags(&stores, &make_msg("11", "3")),
            HARM_EXPLICIT | HARM_GORE
        );
    }
}
