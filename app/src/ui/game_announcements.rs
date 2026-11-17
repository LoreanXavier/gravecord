//! The game profile's "Community Announcements" (module 581745 `nJ`): the game's latest announcement messages from
//! `GET /games/{id}/announcements?limit=8` (`Rf` of module 880405, `GAME_PROFILE_GET_ANNOUNCEMENTS_SUCCESS`), read as
//! `pp` of module 890060 reads them, as cards in the horizontal scroller of module 151869 (`R`, `scrollStep: "page"`,
//! 16px apart) under the heading and its "View All" button (`ty`). A card is a poll (`nK`), an embed's source (`nz`)
//! or the message itself with its picture (`nX`). "View All" and a card close the profile and open the announcements
//! channel, at the message for a card (`nD`); a server the account is not in is opened as a preview, as the
//! Communities cards do. Not here yet: the skeleton while the answer is out (`tE`, shown only when the game has a
//! website), the scroller's edge fade, the card's 4px lift on hover, smooth scrolling, and the way back to the
//! profile when the channel is left (`setGameProfilePendingReturn`).
use super::{Root, color};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

pub(super) const KEY_PREFIX: &str = "game-announcements:";
/// `{limit: 8}` of `nJ`.
const LIMIT: u32 = 8;
/// `calc(44% - var(--space-sm)/2)` of the 766.66px column.
const CARD: f32 = 766.66 * 0.44 - 6.;
/// `gap: 16` of the scroller and the 16px its page steps keep clear of the edges.
const GAP: f32 = 16.;
/// The scroller's width, the column's.
const CLIENT: f32 = 766.66;
/// `IS_COMPONENTS_V2` of the message flags.
const COMPONENTS_V2: u64 = 1 << 15;
/// `TEXT_DISPLAY` and `MEDIA_GALLERY` of the component types (`I5`).
const TEXT_DISPLAY: u64 = 10;
const MEDIA_GALLERY: u64 = 12;

/// A card's picture: its URL and size.
#[derive(Clone, Debug, PartialEq)]
struct Picture {
    url: String,
    width: Option<f64>,
    height: Option<f64>,
}

/// `S` of `pp`: where an embed came from.
#[derive(Clone, Debug, Default, PartialEq)]
struct Source {
    author: Option<String>,
    author_icon: Option<String>,
    provider: Option<String>,
    provider_icon: Option<String>,
    url: Option<String>,
    color: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
struct Poll {
    question: String,
    answers: Vec<String>,
    expiry: Option<String>,
}

/// One message as `pp` returns it.
#[derive(Clone, Debug, PartialEq)]
struct Announcement {
    id: Id,
    media: Option<Picture>,
    title: Option<String>,
    body: String,
    timestamp: String,
    reactions: u64,
    source: Option<Source>,
    poll: Option<Poll>,
}

fn picture(value: &Value) -> Option<Picture> {
    let url = value["proxy_url"]
        .as_str()
        .or_else(|| value["url"].as_str())?;
    Some(Picture {
        url: url.to_owned(),
        width: value["width"].as_f64(),
        height: value["height"].as_f64(),
    })
}

fn is_kind(content_type: &Value, kind: &str) -> bool {
    content_type
        .as_str()
        .is_some_and(|value| value.starts_with(&format!("{kind}/")))
}

/// `_` of `pp`: the gallery's first item, an image then a video attachment, a video embed's thumbnail, an embed's
/// image, an embed's thumbnail.
fn media(message: &Value, v2: bool) -> Option<Picture> {
    if v2 {
        let gallery = message["components"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|component| component["type"].as_u64() == Some(MEDIA_GALLERY));
        if let Some(found) = gallery.and_then(|gallery| picture(&gallery["items"][0]["media"])) {
            return Some(found);
        }
    }
    let attachments = message["attachments"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for kind in ["image", "video"] {
        if let Some(found) = attachments
            .iter()
            .find(|attachment| is_kind(&attachment["content_type"], kind))
            .and_then(picture)
        {
            return Some(found);
        }
    }
    let embeds = message["embeds"].as_array().cloned().unwrap_or_default();
    if let Some(found) = embeds
        .iter()
        .find(|embed| !embed["video"].is_null() && !embed["thumbnail"].is_null())
        .and_then(|embed| picture(&embed["thumbnail"]))
    {
        return Some(found);
    }
    for field in ["image", "thumbnail"] {
        if let Some(found) = embeds
            .iter()
            .find(|embed| !embed[field].is_null())
            .and_then(|embed| picture(&embed[field]))
        {
            return Some(found);
        }
    }
    None
}

/// `m` of `pp`: a components message's text displays; else the content, or, when it is empty or a bare link, the
/// first embed's title as a heading and its description.
fn text_of(message: &Value, v2: bool) -> String {
    if v2 {
        return message["components"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|component| component["type"].as_u64() == Some(TEXT_DISPLAY))
            .filter_map(|component| component["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
    }
    let content = message["content"].as_str().unwrap_or_default();
    let trimmed = content.trim();
    let bare_link = (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.contains(char::is_whitespace);
    if content.is_empty() || bare_link {
        let embed = &message["embeds"][0];
        let mut parts = Vec::new();
        if let Some(title) = embed["title"].as_str() {
            parts.push(format!("# {title}"));
        }
        if let Some(description) = embed["description"].as_str() {
            parts.push(description.to_owned());
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    content.to_owned()
}

/// `/^#{1,3}\s+(.+)$/` on the first line: its text is the title and the rest, trimmed at the start, the body.
fn split_title(text: &str) -> (Option<String>, String) {
    let (first, rest) = match text.find('\n') {
        Some(index) => (&text[..index], Some(&text[index + 1..])),
        None => (text, None),
    };
    let hashes = first.bytes().take_while(|byte| *byte == b'#').count();
    let after = &first[hashes..];
    let heading = (1..=3).contains(&hashes)
        && after.starts_with(char::is_whitespace)
        && !after.trim().is_empty();
    if heading {
        (
            Some(after.trim().to_owned()),
            rest.map(|rest| rest.trim_start().to_owned())
                .unwrap_or_default(),
        )
    } else {
        (None, text.to_owned())
    }
}

fn source_of(message: &Value) -> Option<Source> {
    let embed = &message["embeds"][0];
    if embed.is_null() {
        return None;
    }
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let source = Source {
        author: text(&embed["author"]["name"]),
        author_icon: text(&embed["author"]["proxy_icon_url"])
            .or_else(|| text(&embed["author"]["icon_url"])),
        provider: text(&embed["footer"]["text"]).or_else(|| text(&embed["provider"]["name"])),
        provider_icon: text(&embed["footer"]["proxy_icon_url"])
            .or_else(|| text(&embed["footer"]["icon_url"])),
        url: text(&embed["url"]),
        color: embed["color"].as_u64().map(|value| value as u32),
    };
    (source.author.is_some() || source.provider.is_some() || source.url.is_some()).then_some(source)
}

fn poll_of(message: &Value) -> Option<Poll> {
    let poll = &message["poll"];
    if poll.is_null() {
        return None;
    }
    Some(Poll {
        question: poll["question"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        answers: poll["answers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|answer| {
                answer["poll_media"]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect(),
        expiry: poll["expiry"].as_str().map(str::to_owned),
    })
}

/// `pp`.
fn announcement(message: &Value) -> Option<Announcement> {
    let id = message["id"].as_str()?.parse().ok()?;
    let v2 = message["flags"].as_u64().unwrap_or(0) & COMPONENTS_V2 != 0;
    let text = text_of(message, v2);
    let content = message["content"].as_str().unwrap_or_default();
    let (title, body) = split_title(&text);
    let reactions = message["reactions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reaction| reaction["count"].as_u64())
        .sum();
    Some(Announcement {
        id,
        media: media(message, v2),
        title,
        body,
        timestamp: message["timestamp"].as_str().unwrap_or_default().to_owned(),
        reactions,
        source: if text == content || v2 {
            None
        } else {
            source_of(message)
        },
        poll: poll_of(message),
    })
}

/// The answer's messages and where they were posted.
fn announcements_of(answer: &Value) -> (Vec<Announcement>, Option<Id>, Option<Id>) {
    let body = &answer["body"];
    let messages = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(announcement)
        .collect();
    let id = |value: &Value| value.as_str().and_then(|value| value.parse().ok());
    (messages, id(&body["channel_id"]), id(&body["guild_id"]))
}

/// `nB`: the picture at `kr(364)` wide (one device pixel per pixel) and as high as its aspect asks, as WebP (`VZ`).
fn card_picture(url: &str, aspect: f64) -> String {
    let width = super::game_profile::cdn_size(364);
    let height = (f64::from(width) / aspect).round() as u32;
    let join = if url.contains('?') { '&' } else { '?' };
    format!("{url}{join}format=webp&width={width}&height={height}")
}

fn aspect(media: &Picture) -> f64 {
    match (media.width, media.height) {
        (Some(width), Some(height)) if height > 0. => width / height,
        _ => 16. / 9.,
    }
}

/// `LL`: "November 14, 2026", in local time.
fn long_date(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%B %-d, %Y")
                .to_string()
        })
        .unwrap_or_default()
}

fn grouped(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `J` of module 708676: the time left, `None` once it has passed.
fn time_left(expiry: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let end = chrono::DateTime::parse_from_rfc3339(expiry)
        .ok()?
        .with_timezone(&chrono::Utc);
    if end <= now {
        return None;
    }
    let left = end - now;
    if left.num_days() > 1 {
        return Some(format!("{}d left", left.num_days()));
    }
    if left.num_hours() > 1 {
        return Some(format!("{}h left", left.num_hours()));
    }
    Some(match left.num_minutes() {
        0 => "< 1m left".to_owned(),
        minutes => format!("{minutes}m left"),
    })
}

/// `y` of module 151869: the first (`left`) or last (`right`) card that shows past the 16px margins, and whether it
/// is cut.
fn visible_card(left: bool, scroll: f32, count: usize) -> (usize, bool) {
    let from = scroll + GAP;
    let to = scroll + CLIENT - GAP;
    let bounds = |index: usize| {
        let start = index as f32 * (CARD + GAP);
        (start, start + CARD)
    };
    if left {
        for index in 0..count {
            let (start, end) = bounds(index);
            if end > from && start < to {
                return (index, start < from - 1.);
            }
        }
        (0, false)
    } else {
        for index in (0..count).rev() {
            let (start, end) = bounds(index);
            if end > from && start < to {
                return (index, end > to + 1.);
            }
        }
        (count.saturating_sub(1), false)
    }
}

fn max_scroll(count: usize) -> f32 {
    let width = count as f32 * CARD + count.saturating_sub(1) as f32 * GAP;
    (width - CLIENT).max(0.)
}

/// `F` with `scrollStep: "page"`: the next page to the left ends at the first visible card, to the right starts at
/// the last one (16px clear), or one card further when that is already where the scroller is.
fn page_step(left: bool, scroll: f32, count: usize) -> f32 {
    if count == 0 {
        return 0.;
    }
    let direction: isize = if left { -1 } else { 1 };
    let (index, cut) = visible_card(left, scroll, count);
    let clamp = |index: isize| index.clamp(0, count as isize - 1) as usize;
    let target = if cut {
        index
    } else {
        clamp(index as isize + direction)
    };
    let position = |index: usize| {
        let start = index as f32 * (CARD + GAP);
        let value = if left {
            start + CARD - CLIENT + GAP
        } else {
            start - GAP
        };
        value.clamp(0., max_scroll(count))
    };
    let first = position(target);
    if (first - scroll).abs() > 0.5 {
        return first;
    }
    let next = clamp(target as isize + direction);
    if next == target {
        scroll
    } else {
        position(next)
    }
}

impl Root {
    /// `Rf(game, {limit: 8})`, once per game.
    pub(super) fn game_announcements_fetch(&mut self, game: &str) {
        let key = format!("{KEY_PREFIX}{game}");
        if game.parse::<u64>().is_err()
            || self.fetched.contains_key(&key)
            || !self.requested.insert(key.clone())
        {
            return;
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(key),
            method: reqwest::Method::GET,
            route: format!("/games/{game}/announcements?limit={LIMIT}"),
            body: None,
        });
    }

    /// The answer: kept, and the cards' pictures asked for.
    pub(super) fn game_announcements_fetched(&mut self, key: &str, body: Value) -> bool {
        if !key.starts_with(KEY_PREFIX) {
            return false;
        }
        let (messages, channel, _) = announcements_of(&body);
        tracing::debug!(
            status = body["status"].as_u64(),
            messages = messages.len(),
            channel = channel.is_some(),
            "game announcements answer"
        );
        let mut emoji = Vec::new();
        for message in &messages {
            for text in message.title.iter().chain(std::iter::once(&message.body)) {
                for block in dc_markdown::parse(text) {
                    for span in super::block_spans(&block) {
                        match &span.kind {
                            dc_markdown::Kind::Emoji { id, .. } => emoji.push((
                                format!("custom:{id}"),
                                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=96"),
                            )),
                            dc_markdown::Kind::Unicode => {
                                if let Some(file) = dc_tokens::emoji_file(&span.text) {
                                    emoji.push((
                                        format!("emoji:{}", span.text),
                                        format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Some(media) = &message.media {
                let url = card_picture(&media.url, aspect(media));
                self.request_image(url.clone(), url);
            }
            if let Some(source) = &message.source {
                for url in [&source.author_icon, &source.provider_icon]
                    .into_iter()
                    .flatten()
                {
                    self.request_image(url.clone(), url.clone());
                }
            }
        }
        for (key, url) in emoji {
            if !self.images.contains_key(&key) {
                self.request_image(key, url);
            }
        }
        self.fetched.insert(key.to_owned(), body);
        true
    }

    /// `j` and `v` of `nJ` through `nD`: the profile closes and the announcements channel opens, at the message when a
    /// card was chosen.
    fn open_game_announcement(
        &mut self,
        guild: Id,
        channel: Id,
        message: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        self.game_profile = None;
        if self.stores.guild(guild).is_some() {
            match message {
                Some(message) => self.jump_to_message_id(channel, message, true, None),
                None => self.select(guild, Some(channel)),
            }
        } else {
            self.servers_join(guild, cx);
        }
        cx.notify();
    }

    fn markdown(
        &self,
        text: &str,
        tint: gpui::Hsla,
        size: f32,
        weight: f32,
        line: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let blocks = dc_markdown::parse(text);
        div()
            .flex()
            .flex_col()
            .children(blocks.iter().map(|block| {
                // `.description h1, h2, h3`: 15px, 4px under. Emoji are `1.375em` square.
                let heading = matches!(block, dc_markdown::Block::Heading { .. });
                let (size, weight) = if heading { (15., 700.) } else { (size, weight) };
                let spans = super::block_spans(block).to_vec();
                let pieces = spans
                    .iter()
                    .map(|span| self.piece(span, tint, size, weight, size * 1.375))
                    .collect();
                let links = super::link_runs(&spans);
                let root = cx.weak_entity();
                div().when(heading, |this| this.mb(px(4.))).child(
                    crate::inline::inline(pieces, px(line)).on_piece_click(move |piece, _, cx| {
                        if let Some(Some((url, label))) = links.get(piece).cloned() {
                            let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                        }
                    }),
                )
            }))
            .into_any_element()
    }

    /// `title_-weL8h`: one line of `heading-sm/semibold`.
    fn announcement_title(&self, title: &str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_none()
            .max_h(px(22.))
            .overflow_hidden()
            .child(self.markdown(title, color("--text-strong").into(), 16., 600., 20., cx))
            .into_any_element()
    }

    /// `description_-weL8h`: the muted body, cut where the card ends under a 12px fade.
    fn announcement_body(&self, body: &str, cx: &mut Context<Self>) -> AnyElement {
        let surface: gpui::Hsla = color("--background-surface-high").into();
        div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .child(self.markdown(body, color("--text-muted").into(), 14., 500., 18., cx))
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(12.))
                    .bg(gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(surface.alpha(0.), 0.),
                        gpui::linear_color_stop(surface, 0.8),
                    )),
            )
            .into_any_element()
    }

    fn announcement_meta(
        &self,
        leading: Vec<AnyElement>,
        date: String,
        reactions: u64,
    ) -> AnyElement {
        let muted = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(text)
        };
        div()
            .flex_none()
            .mt_auto()
            .flex()
            .items_center()
            .gap(px(8.))
            .children(leading)
            .child(muted(date))
            .when(reactions > 0, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(super::icon("reaction", 16., "--text-muted"))
                        .child(muted(grouped(reactions))),
                )
            })
            .into_any_element()
    }

    /// `nY` in its 16:9 (or the picture's own) box with 8px corners.
    fn announcement_picture(&self, media: &Picture, width: f32) -> AnyElement {
        let ratio = aspect(media);
        let picture = self.images.get(&card_picture(&media.url, ratio)).cloned();
        div()
            .flex_none()
            .w(px(width))
            .h(px(width / ratio as f32))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .children(
                picture.map(|picture| img(picture).size_full().object_fit(gpui::ObjectFit::Cover)),
            )
            .into_any_element()
    }

    fn announcement_card(
        &self,
        message: &Announcement,
        open: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let inner = CARD - 2. - 24.;
        let date = long_date(&message.timestamp);
        let mut body = div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .overflow_hidden();
        let mut card = div()
            .id(("game-announcement", message.id))
            .flex_none()
            .w(px(CARD))
            .my(px(8.))
            .max_h(px(328.))
            .p(px(12.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap(px(8.))
            .on_click(cx.listener(move |this, _, _, cx| open(this, cx)));
        if let Some(poll) = &message.poll {
            // `nK`: the question, three answers and "+N more options", the date and the time left.
            let shown = poll.answers.len().min(3);
            let more = poll.answers.len() - shown;
            let expiry = poll
                .expiry
                .as_deref()
                .and_then(|expiry| time_left(expiry, chrono::Utc::now()))
                .unwrap_or_else(|| dc_intl::tr("Poll closed").to_string());
            body = body
                .child(
                    div()
                        .flex_none()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(poll.question.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(poll.answers.iter().take(3).map(|answer| {
                            div()
                                .py(px(8.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lowest"))
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(answer.clone()),
                                )
                        }))
                        .when(more > 0, |this| {
                            this.child(
                                div()
                                    .px(px(12.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(if more == 1 {
                                        "+1 more option".to_owned()
                                    } else {
                                        format!("+{more} more options")
                                    }),
                            )
                        }),
                )
                .child(self.announcement_meta(Vec::new(), format!("{date} · {expiry}"), 0));
            return card.child(body).into_any_element();
        }
        if let Some(source) = &message.source {
            // `nz`: the link, then the embed's box with its colour down the left.
            if let Some(url) = &source.url {
                body = body.child(
                    div()
                        .flex_none()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-link"))
                        .child(url.clone()),
                );
            }
            let mut area = div()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .gap(px(4.))
                .pl(px(8.))
                .border_l_4()
                .rounded_l(px(4.))
                .border_color(match source.color {
                    Some(value) => gpui::rgb(value),
                    None => color("--border-subtle"),
                });
            if let Some(author) = &source.author {
                let icon = source
                    .author_icon
                    .as_ref()
                    .and_then(|url| self.images.get(url).cloned());
                area = area.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children(source.author_icon.as_ref().map(|_| {
                            div()
                                .size(px(20.))
                                .rounded_full()
                                .overflow_hidden()
                                .children(icon.map(|icon| img(icon).size(px(20.))))
                        }))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(author.clone()),
                        ),
                );
            }
            if let Some(media) = &message.media {
                area = area.child(self.announcement_picture(media, inner - 12.));
            }
            if let Some(title) = &message.title {
                area = area.child(self.announcement_title(title, cx));
            }
            if !message.body.is_empty() {
                area = area.child(self.announcement_body(&message.body, cx));
            }
            let provider_icon = source.provider_icon.as_ref().map(|url| {
                let icon = self.images.get(url).cloned();
                div()
                    .flex_none()
                    .size(px(16.))
                    .children(icon.map(|icon| img(icon).size(px(16.))))
                    .into_any_element()
            });
            let date = match &source.provider {
                Some(provider) => format!("{provider} · {date}"),
                None => date,
            };
            area = area.child(self.announcement_meta(
                provider_icon.into_iter().collect(),
                date,
                message.reactions,
            ));
            return card.child(body.child(area)).into_any_element();
        }
        // `nX`: the picture, then `nH`.
        if let Some(media) = &message.media {
            card = card.child(self.announcement_picture(media, inner));
        }
        if let Some(title) = &message.title {
            body = body.child(self.announcement_title(title, cx));
        }
        if !message.body.is_empty() {
            body = body.child(self.announcement_body(&message.body, cx));
        }
        body = body.child(self.announcement_meta(Vec::new(), date, message.reactions));
        card.child(body).into_any_element()
    }

    /// `overlay-secondary` at 40px with the large arrow; hidden (`disabled_s64Ydp`, opacity 0) when that way has
    /// nothing more.
    fn announcement_arrow(
        &self,
        left: bool,
        enabled: bool,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(if left {
                "game-announcements-left"
            } else {
                "game-announcements-right"
            })
            .size(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-overlay-secondary-border-default"))
            .bg(gpui::rgba(0x0000008f))
            .flex()
            .items_center()
            .justify_center()
            .when(!enabled, |this| this.opacity(0.))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.game_announcements_scroll =
                            page_step(left, this.game_announcements_scroll, count);
                        cx.notify();
                    }))
            })
            .child(super::icon(
                if left {
                    "arrow_large_left"
                } else {
                    "arrow_large_right"
                },
                20.,
                "--white",
            ))
            .into_any_element()
    }

    /// `nJ`: nothing until the answer has messages and a channel.
    pub(super) fn game_announcements_section(
        &self,
        game: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let answer = self.fetched.get(&format!("{KEY_PREFIX}{game}"))?;
        let (messages, channel, guild) = announcements_of(answer);
        let (channel, guild) = (channel?, guild?);
        if messages.is_empty() {
            return None;
        }
        let count = messages.len();
        let scroll = self.game_announcements_scroll.clamp(0., max_scroll(count));
        let can_left = scroll > 0.;
        let can_right = scroll < max_scroll(count) - 1.;
        let shows_actions = max_scroll(count) > 0.;
        let cards: Vec<AnyElement> = messages
            .iter()
            .map(|message| {
                let id = message.id;
                self.announcement_card(
                    message,
                    move |this, cx| this.open_game_announcement(guild, channel, Some(id), cx),
                    cx,
                )
            })
            .collect();
        // `ty`: the heading and the secondary "View All" with a chevron after it.
        let view_all = div()
            .id("game-announcements-view-all")
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_game_announcement(guild, channel, None, cx)
            }))
            .child(dc_intl::tr("View All"))
            .child(super::icon(
                "overlay_chevron_small_right",
                16.,
                "--control-secondary-text-default",
            ));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .min_h(px(32.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::tr("Community Announcements")),
                        )
                        .child(view_all),
                )
                .child(
                    div()
                        .relative()
                        .mx(px(-4.))
                        .my(px(-4.))
                        .p(px(4.))
                        .overflow_hidden()
                        .child(
                            div()
                                .id("game-announcements-cards")
                                .w(px(CLIENT))
                                .overflow_hidden()
                                // A sideways wheel (or shift and the wheel) moves the cards; an upright one
                                // scrolls the profile, as `overflow-x: scroll` does.
                                .on_scroll(cx.listener(
                                    move |this, event: &gpui::ScrollEvent, _, cx| {
                                        let delta = event.delta.pixel_delta(px(16.));
                                        let sideways = if event.modifiers.shift {
                                            delta.y
                                        } else {
                                            delta.x
                                        };
                                        if f32::from(sideways) != 0. {
                                            this.game_announcements_scroll = (this
                                                .game_announcements_scroll
                                                - f32::from(sideways))
                                            .clamp(0., max_scroll(count));
                                            cx.stop_propagation();
                                            cx.notify();
                                        }
                                    },
                                ))
                                .child(div().flex().gap(px(GAP)).ml(px(-scroll)).children(cards)),
                        )
                        .when(shows_actions, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .p(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(self.announcement_arrow(true, can_left, count, cx))
                                    .child(self.announcement_arrow(false, can_right, count, cx)),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_heading_line_is_the_title_and_the_rest_the_body() {
        assert_eq!(
            split_title("## Captain's Day\n\n  The Divas are here"),
            (
                Some("Captain's Day".to_owned()),
                "The Divas are here".to_owned()
            )
        );
        assert_eq!(
            split_title("No heading\nmore"),
            (None, "No heading\nmore".to_owned())
        );
        assert_eq!(split_title("#### four"), (None, "#### four".to_owned()));
    }

    #[test]
    fn a_bare_link_reads_its_embed_and_keeps_the_embed_source() {
        let message = json!({
            "id": "5", "content": "https://example.com/news", "timestamp": "2026-11-14T17:00:00+00:00",
            "embeds": [{"title": "Patch notes", "description": "All fixed", "url": "https://example.com/news",
                        "provider": {"name": "Example"}, "image": {"url": "https://a/b.png", "proxy_url": "https://p/b.png", "width": 1600, "height": 900}}],
            "reactions": [{"count": 3}, {"count": 4}]
        });
        let card = announcement(&message).unwrap();
        assert_eq!(card.title.as_deref(), Some("Patch notes"));
        assert_eq!(card.body, "All fixed");
        assert_eq!(card.reactions, 7);
        assert_eq!(
            card.source
                .as_ref()
                .and_then(|s| s.provider.clone())
                .as_deref(),
            Some("Example")
        );
        assert_eq!(
            card.media.as_ref().map(|m| m.url.as_str()),
            Some("https://p/b.png")
        );
        let plain =
            announcement(&json!({"id": "6", "content": "# Hi\nthere", "embeds": [{"url": "x"}]}))
                .unwrap();
        assert!(
            plain.source.is_none(),
            "a message with its own text shows no embed source"
        );
    }

    #[test]
    fn page_steps_move_to_the_cut_card_and_stop_at_the_ends() {
        assert_eq!(page_step(true, 0., 8), 0.);
        let first = page_step(false, 0., 8);
        // The third card is cut at the right edge: the next page starts 16px before it.
        assert!((first - (2. * (CARD + GAP) - GAP)).abs() < 0.01, "{first}");
        let end = max_scroll(8);
        assert!((page_step(false, end, 8) - end).abs() < 0.01);
        assert!(page_step(true, first, 8) < first);
    }

    #[test]
    fn the_time_left_and_the_card_picture() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-14T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            time_left("2026-11-17T00:00:00Z", now).as_deref(),
            Some("3d left")
        );
        assert_eq!(
            time_left("2026-11-14T05:00:00Z", now).as_deref(),
            Some("5h left")
        );
        assert_eq!(
            time_left("2026-11-14T00:00:30Z", now).as_deref(),
            Some("< 1m left")
        );
        assert_eq!(time_left("2026-11-13T00:00:00Z", now), None);
        assert_eq!(
            card_picture("https://media/x.png?ex=1", 16. / 9.),
            "https://media/x.png?ex=1&format=webp&width=480&height=270"
        );
        assert_eq!(grouped(4665), "4,665");
    }
}
