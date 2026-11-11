//! The composer's suggestion popup for `@`, `#`, `#!` and `:`. The rules (what is offered, in which order, what
//! goes into the text) live in `dc_state::autocomplete`, ported from modules 84046, 31498, 997625 and 927659; the
//! rows and sizes are from `design/dom/channel/autocomplete-*-darker-cozy.json`.

use super::{Root, avatar_source, color, icon, picker, status_dot_sized};
use crate::net::Request;
use dc_state::autocomplete::{self as ac, ChannelHit, Global, HitKind, Kind, Trigger};
use dc_state::{Channel, Id, User, permissions};
use dc_tokens::emoji_data::EMOJIS;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, ScrollHandle, Window, div, img,
    prelude::*, px, relative,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `useRequestMembers`: the wait before asking the gateway, and how long an answer stays good.
const REQUEST_DELAY: Duration = Duration::from_millis(200);
const REQUEST_TTL: Duration = Duration::from_secs(60);
/// Emoji the list shows at most (`MAX_EMOJI_RESULTS` when stickers are off).
const EMOJI_LIMIT: usize = 50;

#[derive(Clone, Debug)]
pub(super) enum Entry {
    User {
        user: User,
        name: String,
    },
    Role {
        id: Id,
        name: String,
        color: u32,
    },
    Global(Global),
    Channel {
        hit: ChannelHit,
        secondary: String,
    },
    Emoji {
        item: picker::Item,
        name: String,
        insert: String,
    },
    /// A sticker the `:` list offers while "Show stickers in autocomplete results" is on (module 496533). The row's
    /// right side names the server or pack it belongs to.
    Sticker {
        sticker: dc_state::stickers::Sticker,
        source: String,
    },
}

impl Entry {
    /// What accepting the row puts in the text.
    fn markup(&self) -> String {
        match self {
            Self::User { user, .. } => ac::user_markup(user.id),
            Self::Role { id, .. } => ac::role_markup(*id),
            Self::Global(global) => global.text().to_owned(),
            Self::Channel { hit, .. } => ac::channel_markup(hit.id),
            Self::Emoji { insert, .. } => insert.clone(),
            // Choosing a sticker sends it; the text it replaces is removed.
            Self::Sticker { .. } => String::new(),
        }
    }
}

pub(super) struct Open {
    pub trigger: Trigger,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: ScrollHandle,
}

#[derive(Default)]
pub(super) struct State {
    pub open: Option<Open>,
    /// Escape hides the list until the text changes: the text it was hidden at.
    pub(super) hidden: Option<String>,
    /// The text and caret the list was last built for.
    signature: Option<(String, usize)>,
    /// `guild:query` (or `ids:...`) to when op 8 was asked, so a repeat inside a minute is skipped.
    searches: HashMap<String, Instant>,
    scheduled: Option<String>,
    /// The mention query the list is built for now.
    query: Option<String>,
}

/// One line of the popup: a title, a rule or a row (an index into `entries`).
enum Line {
    Heading(String),
    Divider,
    Row(usize),
}

fn lines(open: &Open) -> Vec<Line> {
    let query = &open.trigger.query;
    let matching = |title: &str, sentinel: &str| {
        if query.is_empty() {
            title.to_owned()
        } else {
            format!(
                "{} matching {sentinel}{query}",
                title.replace("Channels", "channels")
            )
        }
    };
    let mut out = Vec::new();
    let mut last_group = None;
    for (index, entry) in open.entries.iter().enumerate() {
        let group = match entry {
            Entry::User { .. } => 0,
            Entry::Role { .. } => 1,
            Entry::Global(_) => 2,
            Entry::Channel { hit, .. } if hit.kind == HitKind::Voice => 3,
            Entry::Channel { .. } => 4,
            Entry::Emoji { .. } => 5,
            Entry::Sticker { .. } => 6,
        };
        if last_group != Some(group) {
            if last_group.is_some() && matches!(group, 1 | 2 | 6) {
                out.push(Line::Divider);
            }
            let heading = match group {
                0 => Some(matching("Members", "@")),
                1 => Some(matching("Roles", "@")),
                3 => Some(matching("Voice Channels", "#!")),
                4 => Some(matching("Text Channels", "#")),
                5 => Some(format!("Emoji matching :{query}")),
                6 => Some(format!("Stickers matching :{query}")),
                _ => None,
            };
            out.extend(heading.map(Line::Heading));
            last_group = Some(group);
        }
        out.push(Line::Row(index));
    }
    out
}

impl Root {
    /// Rebuilds the list for the word before the caret. Called whenever the composer changes.
    pub(super) fn refresh_autocomplete(&mut self, force: bool, cx: &mut Context<Self>) {
        let (text, caret) = {
            let composer = self.composer.read(cx);
            (composer.text().to_owned(), composer.collapsed_caret())
        };
        // The sticker rows need the pack list, which is otherwise fetched when the picker's tab opens.
        if self.pref_on("chat_stickers_autocomplete", false)
            && !self.stores.stickers.has_loaded_packs()
            && self.requested.insert(super::stickers::PACKS_KEY.to_owned())
        {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: super::stickers::PACKS_KEY.to_owned(),
                route: super::stickers::PACKS_ROUTE.to_owned(),
            });
        }
        let signature = caret.map(|caret| (text.clone(), caret));
        if !force && signature == self.autocomplete.signature {
            return;
        }
        self.autocomplete.signature = signature;
        if self.slash_convert(&text, cx) {
            return;
        }
        if self.autocomplete.hidden.as_ref() != Some(&text) {
            self.autocomplete.hidden = None;
        }
        let trigger = match (caret, &self.autocomplete.hidden) {
            (Some(caret), None) => ac::detect(&text, caret, self.guild.is_none()),
            _ => None,
        };
        // `/` has its own list, with its own rail and rows.
        let slash_query = trigger
            .as_ref()
            .filter(|trigger| trigger.kind == Kind::Command)
            .map(|trigger| trigger.query.clone());
        let trigger = trigger.filter(|trigger| trigger.kind != Kind::Command);
        self.refresh_slash(slash_query.as_deref(), cx);
        let previous = self.autocomplete.open.take();
        self.autocomplete.query = trigger
            .as_ref()
            .filter(|trigger| trigger.kind == Kind::Mention)
            .map(|trigger| trigger.query.clone());
        let Some(trigger) = trigger else {
            if previous.is_some() {
                cx.notify();
            }
            return;
        };
        let entries = match trigger.kind {
            Kind::Mention => self.mention_entries(&trigger.query),
            Kind::Channel => self.channel_entries(&trigger.query, false),
            Kind::VoiceChannel => self.channel_entries(&trigger.query, true),
            Kind::Emoji => self.emoji_entries(&trigger.query),
            Kind::Command => Vec::new(),
        };
        self.request_autocomplete_images(&entries);
        if trigger.kind == Kind::Mention {
            self.schedule_member_search(&trigger.query, cx);
        }
        if entries.is_empty() {
            if previous.is_some() {
                cx.notify();
            }
            return;
        }
        let (selected, scroll) = match previous {
            Some(open)
                if open.trigger.kind == trigger.kind && open.trigger.query == trigger.query =>
            {
                (open.selected.min(entries.len() - 1), open.scroll)
            }
            _ => (0, ScrollHandle::new()),
        };
        self.autocomplete.open = Some(Open {
            trigger,
            entries,
            selected,
            scroll,
        });
        cx.notify();
    }

    /// The channel `@` and `#` are about: the current one, or a thread's parent.
    fn autocomplete_channel(&self) -> Option<Channel> {
        if let Some(channel) = self.current_channel() {
            return Some(channel.clone());
        }
        let thread = self.current_thread()?;
        self.current_guild()?
            .channels
            .iter()
            .find(|channel| channel.id == thread.parent_id)
            .cloned()
    }

    /// The people `@` can offer, with the name each is ranked and shown by.
    pub(super) fn mention_candidates(&self) -> Vec<(User, Option<String>)> {
        let mut out: Vec<(User, Option<String>)> = Vec::new();
        if let Some(guild) = self.current_guild() {
            let channel = self.autocomplete_channel();
            for member in self.stores.members.of(guild.id) {
                if member.pending {
                    continue;
                }
                if let Some(channel) = &channel
                    && !ac::member_can_view(guild, channel, member)
                {
                    continue;
                }
                out.push((member.user.clone(), member.nick.clone()));
            }
        } else if let Some(dm) = self.current_dm() {
            for id in &dm.recipients {
                if let Some(user) = self.stores.users.get(id) {
                    out.push((user.clone(), None));
                }
            }
        }
        if let Some(me) = &self.stores.current_user
            && !out.iter().any(|(user, _)| user.id == me.id)
        {
            out.push((me.clone(), None));
        }
        out
    }

    fn mention_entries(&self, query: &str) -> Vec<Entry> {
        let candidates = self.mention_candidates();
        let users: Vec<usize> = if query.is_empty() {
            // `getRecentlyTalkedUsers`: authors of the channel's messages, newest first.
            let mut picked: Vec<usize> = Vec::new();
            if let Some(messages) = self
                .channel
                .and_then(|channel| self.stores.messages.get(&channel))
            {
                for message in messages.iter().rev() {
                    if picked.len() >= ac::LIMIT {
                        break;
                    }
                    if message.author.bot || message.webhook_id.is_some() {
                        continue;
                    }
                    if let Some(index) = candidates
                        .iter()
                        .position(|(user, _)| user.id == message.author.id)
                        && !picked.contains(&index)
                    {
                        picked.push(index);
                    }
                }
            }
            picked
        } else {
            let refs: Vec<(&User, Option<&str>)> = candidates
                .iter()
                .map(|(user, nick)| (user, nick.as_deref()))
                .collect();
            ac::rank_members(query, &refs, ac::LIMIT)
                .into_iter()
                .map(|ranked| ranked.index)
                .collect()
        };
        let everyone = self
            .autocomplete_channel()
            .is_some_and(|channel| self.can_in_channel(&channel, ac::MENTION_EVERYONE));
        let rules = ac::MentionRules {
            everyone,
            private: self.guild.is_none(),
        };
        let found = ac::mention_results(query, users, self.current_guild(), rules);
        let mut entries = Vec::new();
        for index in found.users {
            let (user, nick) = &candidates[index];
            let name = nick
                .clone()
                .or_else(|| user.global_name.clone())
                .unwrap_or_else(|| user.username.clone());
            entries.push(Entry::User {
                user: user.clone(),
                name,
            });
        }
        if let Some(guild) = self.current_guild() {
            for id in found.roles {
                entries.push(Entry::Role {
                    id,
                    name: guild.roles.get(&id).cloned().unwrap_or_default(),
                    color: guild.role_info.get(&id).map_or(0, |info| info.color),
                });
            }
        }
        let listed = entries.len() + found.globals.len();
        entries.extend(found.globals.into_iter().map(Entry::Global));
        // `@game` (while the games setting is on) and `@time` follow `@everyone` and `@here`.
        entries.extend(
            ac::extra_globals(
                query,
                listed,
                self.pref_on("chat_game_mentions_autocomplete", true),
            )
            .into_iter()
            .map(Entry::Global),
        );
        entries
    }

    fn channel_entries(&self, query: &str, voice: bool) -> Vec<Entry> {
        let Some(guild) = self.current_guild() else {
            return Vec::new();
        };
        let hits = ac::channel_results(
            guild,
            query,
            voice,
            self.stores.current_user.as_ref().map(|user| user.id),
            |channel| self.can_in_channel(channel, permissions::VIEW_CHANNEL),
        );
        hits.into_iter()
            .take(ac::LIMIT)
            .map(|hit| {
                let secondary = hit
                    .parent
                    .and_then(|parent| guild.channels.iter().find(|channel| channel.id == parent))
                    .map(|parent| parent.name.clone())
                    .unwrap_or_default();
                Entry::Channel { hit, secondary }
            })
            .collect()
    }

    fn emoji_entries(&self, query: &str) -> Vec<Entry> {
        let now = picker::now_ms();
        let scores = self.emoji_frecency().scores(now);
        let tone = self.skin_tone;
        let mut found: Vec<(f64, String, Entry)> = Vec::new();
        for (index, emoji) in EMOJIS.iter().enumerate() {
            // Skin tone variants are the picker's; the list offers the base.
            if !emoji.diversity.is_empty() {
                continue;
            }
            let Some(score) = emoji
                .names
                .iter()
                .filter(|name| ac::emoji_matches(name, query))
                .map(|name| ac::emoji_score(name, query, scores.get(emoji.names[0]).copied()))
                .reduce(f64::max)
            else {
                continue;
            };
            let shown = picker::toned(emoji, tone);
            found.push((
                score,
                emoji.names[0].to_owned(),
                Entry::Emoji {
                    item: picker::Item::Unicode(index as u16),
                    name: emoji.names[0].to_owned(),
                    insert: ac::unicode_markup(shown.surrogates),
                },
            ));
        }
        // Custom emoji: this server's, and every other server's with Nitro.
        let premium = dc_state::premium::PremiumType::from_wire(self.stores.account.premium_type)
            .has(dc_state::premium::Feature::EmojisEverywhere);
        for guild in &self.stores.guilds {
            if Some(guild.id) != self.guild && !premium {
                continue;
            }
            for emoji in guild.emojis.iter().filter(|emoji| emoji.available) {
                if !ac::emoji_matches(&emoji.name, query) {
                    continue;
                }
                let score = ac::emoji_score(
                    &emoji.name,
                    query,
                    scores.get(&emoji.id.to_string()).copied(),
                );
                found.push((
                    score,
                    emoji.name.clone(),
                    Entry::Emoji {
                        item: picker::Item::Custom {
                            id: emoji.id,
                            name: emoji.name.clone(),
                            animated: emoji.animated,
                        },
                        name: emoji.name.clone(),
                        insert: ac::custom_emoji_markup(&emoji.name, emoji.id, emoji.animated),
                    },
                ));
            }
        }
        found.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        // `queryResults` of module 496533: with stickers on, at most 10 emoji are searched and the list keeps
        // `max(6, 10 - stickers)` of them after `max(4, 8 - emoji)` stickers; off, 50 emoji.
        let stickers_on =
            self.pref_on("chat_stickers_autocomplete", false) && query.chars().count() > 1;
        let limit = if stickers_on { 10 } else { EMOJI_LIMIT };
        let mut emoji: Vec<Entry> = found
            .into_iter()
            .take(limit)
            .map(|(_, _, entry)| entry)
            .collect();
        if !stickers_on {
            return emoji;
        }
        let stickers = self.sticker_entries(query, 4.max(8usize.saturating_sub(emoji.len())));
        emoji.truncate(6.max(limit.saturating_sub(stickers.len())));
        emoji.extend(stickers);
        emoji
    }

    /// `queryStickers([query], true, [channel, sendable])`: the stickers this account can send here.
    fn sticker_entries(&self, query: &str, take: usize) -> Vec<Entry> {
        let now = picker::now_ms();
        self.stores
            .stickers
            .search(
                query,
                now,
                |tags| {
                    tags.split(',')
                        .filter_map(|tag| {
                            dc_tokens::emoji_by_name(tag.trim()).map(|e| e.surrogates.to_owned())
                        })
                        .collect()
                },
                |guild| self.stores.guild(guild).map(|guild| guild.name.clone()),
                |sticker| self.sticker_is_sendable(sticker),
            )
            .into_iter()
            .take(take)
            .map(|hit| {
                let source = match (hit.sticker.guild_id, hit.sticker.pack_id) {
                    (Some(guild), _) => self
                        .stores
                        .guild(guild)
                        .map(|guild| guild.name.clone())
                        .unwrap_or_default(),
                    (None, Some(pack)) => self
                        .stores
                        .stickers
                        .pack(pack)
                        .map(|pack| pack.name.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                Entry::Sticker {
                    sticker: hit.sticker,
                    source,
                }
            })
            .collect()
    }

    fn request_autocomplete_images(&mut self, entries: &[Entry]) {
        for entry in entries {
            match entry {
                Entry::User { user, .. } => {
                    let (key, url) = avatar_source(user);
                    self.request_image(key, url);
                }
                Entry::Sticker { sticker, .. } => self.request_sticker_art(sticker),
                Entry::Emoji { item, .. } => match item {
                    picker::Item::Custom { id, animated, .. } => {
                        let ext = if *animated { "gif" } else { "webp" };
                        self.request_image(
                            format!("custom:{id}"),
                            format!("https://cdn.discordapp.com/emojis/{id}.{ext}?size=48"),
                        );
                    }
                    picker::Item::Unicode(index) => {
                        let emoji = &EMOJIS[usize::from(*index)];
                        if emoji.sprite.is_none() {
                            self.request_emoji_image(emoji.surrogates);
                        } else if self.requested.insert("emoji-sheet".to_owned()) {
                            let _ = self.requests.send(Request::EmojiSheet);
                        }
                    }
                },
                _ => {}
            }
        }
    }

    /// `useRequestMembers`: a query the loaded members may not cover goes to the gateway after 200 ms, once a minute
    /// per query; an empty query asks for the people who wrote in the channel instead. Answers arrive as
    /// `GUILD_MEMBERS_CHUNK` and refresh the list.
    fn schedule_member_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        let (key, user_ids) = if query.is_empty() {
            let ids = self.unknown_authors(guild);
            if ids.is_empty() {
                return;
            }
            (
                format!("{guild}:ids:{}", self.channel.unwrap_or_default()),
                ids,
            )
        } else {
            (format!("{guild}:{query}"), Vec::new())
        };
        let fresh = self
            .autocomplete
            .searches
            .get(&key)
            .is_some_and(|asked| asked.elapsed() < REQUEST_TTL);
        if fresh || self.autocomplete.scheduled.as_deref() == Some(key.as_str()) {
            return;
        }
        self.autocomplete.scheduled = Some(key.clone());
        let query = query.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REQUEST_DELAY).await;
            let _ = this.update(cx, |this, _| {
                this.autocomplete.scheduled = None;
                // Only when the word is still the one that was asked for.
                if this.autocomplete.query.as_deref() != Some(query.as_str()) {
                    return;
                }
                this.autocomplete.searches.insert(key, Instant::now());
                let _ = this.requests.send(Request::GuildMembers {
                    guilds: vec![guild],
                    query,
                    limit: ac::LIMIT as u64,
                    user_ids,
                });
            });
        })
        .detach();
    }

    /// Authors of the channel's loaded messages whose member this session has not seen.
    fn unknown_authors(&self, guild: Id) -> Vec<u64> {
        let mut ids: Vec<u64> = Vec::new();
        for message in self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
        {
            if message.webhook_id.is_none()
                && self.stores.members.get(guild, message.author.id).is_none()
                && !ids.contains(&message.author.id)
            {
                ids.push(message.author.id);
            }
        }
        ids
    }

    /// Up, Down, Tab, Enter and Escape while the list is open. True when the key was used.
    pub(super) fn autocomplete_key(
        &mut self,
        keys: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(open) = self.autocomplete.open.as_mut() else {
            return false;
        };
        let plain = !(keys.modifiers.control || keys.modifiers.alt || keys.modifiers.platform);
        let count = open.entries.len();
        match keys.key.as_str() {
            "up" if plain && !keys.modifiers.shift => {
                open.selected = (open.selected + count - 1) % count;
                self.scroll_autocomplete_to_selection();
            }
            "down" if plain && !keys.modifiers.shift => {
                open.selected = (open.selected + 1) % count;
                self.scroll_autocomplete_to_selection();
            }
            "tab" | "enter" if plain && !keys.modifiers.shift => {
                let selected = open.selected;
                self.accept_autocomplete(selected, window, cx);
                return true;
            }
            "escape" if plain => {
                self.autocomplete.hidden = Some(self.composer.read(cx).text().to_owned());
                self.autocomplete.open = None;
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn scroll_autocomplete_to_selection(&self) {
        let Some(open) = &self.autocomplete.open else {
            return;
        };
        let at = lines(open)
            .iter()
            .position(|line| matches!(line, Line::Row(index) if *index == open.selected));
        if let Some(at) = at {
            open.scroll.scroll_to_item(at);
        }
    }

    /// Puts the row's markup where the word was, with a space after it, and keeps typing in the composer.
    pub(super) fn accept_autocomplete(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.autocomplete.open.take() else {
            return;
        };
        let Some(entry) = open.entries.get(index) else {
            return;
        };
        if let Entry::Sticker { sticker, .. } = entry {
            // `sendSticker` after `insertText("")`: the typed `:word` goes, the sticker is sent or staged.
            let id = sticker.id;
            let Trigger { start, end, .. } = open.trigger;
            self.composer.update(cx, |composer, cx| {
                composer.replace_range(start..end, "", cx);
            });
            self.choose_sticker(id, false, cx);
            let focus = gpui::Focusable::focus_handle(self.composer.read(cx), cx);
            window.focus(&focus, cx);
            cx.notify();
            return;
        }
        let insert = format!("{} ", entry.markup());
        let Trigger { start, end, .. } = open.trigger;
        self.composer.update(cx, |composer, cx| {
            composer.replace_range(start..end, &insert, cx);
        });
        let focus = gpui::Focusable::focus_handle(self.composer.read(cx), cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn autocomplete_row(
        &self,
        index: usize,
        entry: &Entry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (height_pad, content): (f32, AnyElement) = match entry {
            Entry::User { user, name } => {
                let (key, _) = avatar_source(user);
                // The account's own presence is its settings status; others come from presences and member chunks.
                let status = if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user.id)
                {
                    self.stores.status.clone()
                } else {
                    self.stores.status(user.id).to_owned()
                };
                let avatar = div()
                    .relative()
                    .size(px(24.))
                    .flex_none()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(
                                self.images
                                    .get(&key)
                                    .map(|image| img(image.clone()).size(px(24.)).rounded_full()),
                            ),
                    )
                    .child(status_dot_sized(
                        &status,
                        "--background-surface-high",
                        8.,
                        3.,
                    ));
                (
                    8.,
                    self.row_content(
                        Some(avatar.into_any_element()),
                        name,
                        None,
                        user.username.clone(),
                        selected,
                    ),
                )
            }
            Entry::Role {
                name, color: tint, ..
            } => (
                8.,
                self.row_content(
                    None,
                    &format!("@{name}"),
                    (*tint != 0).then_some(*tint),
                    "Notify users with this role who have permission to view this channel."
                        .to_owned(),
                    selected,
                ),
            ),
            Entry::Global(global) => (
                8.,
                self.row_content(
                    None,
                    global.text(),
                    None,
                    global.description().to_owned(),
                    selected,
                ),
            ),
            Entry::Channel { hit, secondary } => {
                let name = match hit.kind {
                    HitKind::Voice => "channel_type_voice",
                    HitKind::Thread => "system_thread",
                    _ => "text_channel",
                };
                (
                    8.,
                    self.row_content(
                        Some(icon(name, 16., "--interactive-text-default").into_any_element()),
                        &hit.name,
                        None,
                        secondary.clone(),
                        selected,
                    ),
                )
            }
            Entry::Sticker { sticker, source } => (
                4.,
                self.row_content(
                    Some(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .children(self.sticker_picture(sticker.id, 40.))
                            .into_any_element(),
                    ),
                    &sticker.name,
                    None,
                    source.clone(),
                    selected,
                ),
            ),
            Entry::Emoji { item, name, .. } => {
                let picture = match item {
                    picker::Item::Unicode(at) => {
                        self.images.get(&item.image_key(0)).or_else(|| {
                            self.images
                                .get(&format!("emoji:{}", EMOJIS[usize::from(*at)].surrogates))
                        })
                    }
                    picker::Item::Custom { .. } => self.images.get(&item.image_key(0)),
                };
                let image = div()
                    .size(px(20.))
                    .flex_none()
                    .children(picture.map(|image| img(image.clone()).size(px(20.))));
                (
                    4.,
                    self.row_content(
                        Some(image.into_any_element()),
                        &format!(":{name}:"),
                        None,
                        String::new(),
                        selected,
                    ),
                )
            }
        };
        div()
            .id(("autocomplete-row", index))
            .px(px(8.))
            .cursor_pointer()
            .child(
                div()
                    .px(px(8.))
                    .py(px(height_pad))
                    .rounded(px(3.))
                    .when(selected, |this| {
                        this.bg(color("--interactive-background-hover"))
                    })
                    .child(content),
            )
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.autocomplete.open.as_mut()
                    && open.selected != index
                {
                    open.selected = index;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.accept_autocomplete(index, window, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn row_content(
        &self,
        lead: Option<AnyElement>,
        primary: &str,
        tint: Option<u32>,
        secondary: String,
        _selected: bool,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .min_h(px(if secondary.is_empty() { 23. } else { 0. }))
            .gap(px(8.))
            .children(lead)
            .child(
                div()
                    .flex_none()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(400.))
                    .text_color(match tint {
                        Some(rgb) => gpui::rgb(rgb).into(),
                        None => gpui::Hsla::from(color("--interactive-text-active")),
                    })
                    .child(primary.to_owned()),
            )
            .when(!secondary.is_empty(), |this| {
                this.child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .ml(px(8.))
                        .truncate()
                        .text_right()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(400.))
                        .text_color(color("--interactive-text-default"))
                        .child(secondary),
                )
            })
            .into_any_element()
    }

    /// The popup, placed 8px above the composer and spanning it.
    pub(super) fn autocomplete_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.autocomplete.open.as_ref()?;
        let children: Vec<AnyElement> = lines(open)
            .into_iter()
            .map(|line| match line {
                Line::Heading(text) => div()
                    .px(px(8.))
                    .pt(px(8.))
                    .pb(px(8.))
                    .child(
                        div()
                            .py(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.6667))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-default"))
                            .child(text),
                    )
                    .into_any_element(),
                Line::Divider => div()
                    .px(px(16.))
                    .h(px(17.))
                    .pt(px(8.))
                    .child(div().h(px(1.)).bg(color("--border-subtle")))
                    .into_any_element(),
                Line::Row(index) => {
                    self.autocomplete_row(index, &open.entries[index], index == open.selected, cx)
                }
            })
            .collect();
        Some(
            div()
                .id("autocomplete")
                .absolute()
                .left(px(8.))
                .right(px(16.))
                .bottom(relative(1.))
                .mb(px(8.))
                .occlude()
                .rounded(px(5.))
                .bg(color("--background-surface-high"))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 1., 0.08),
                        offset: gpui::point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: false,
                    },
                    gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                ])
                .pb(px(8.))
                .pr(px(8.))
                .child(
                    div()
                        .id("autocomplete-scroll")
                        .max_h(px(490.))
                        .overflow_y_scroll()
                        .track_scroll(&open.scroll)
                        .children(children),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(entries: Vec<Entry>, query: &str) -> Open {
        Open {
            trigger: Trigger {
                kind: Kind::Mention,
                query: query.to_owned(),
                start: 0,
                end: 1,
            },
            entries,
            selected: 0,
            scroll: ScrollHandle::new(),
        }
    }

    fn user(id: Id) -> Entry {
        Entry::User {
            user: User {
                id,
                ..User::default()
            },
            name: String::new(),
        }
    }

    #[test]
    fn members_then_a_rule_then_the_global_mentions_with_titles_where_the_official_has_them() {
        let list = lines(&open(
            vec![
                user(1),
                user(2),
                Entry::Global(Global::Everyone),
                Entry::Global(Global::Here),
            ],
            "",
        ));
        let shape: Vec<&str> = list
            .iter()
            .map(|line| match line {
                Line::Heading(_) => "title",
                Line::Divider => "rule",
                Line::Row(_) => "row",
            })
            .collect();
        assert_eq!(shape, ["title", "row", "row", "rule", "row", "row"]);
        assert!(matches!(&list[0], Line::Heading(text) if text == "Members"));
    }

    #[test]
    fn a_query_is_named_in_the_title() {
        let list = lines(&open(vec![user(1)], "ab"));
        assert!(matches!(&list[0], Line::Heading(text) if text == "Members matching @ab"));
    }

    #[test]
    fn rows_put_their_official_markup_into_the_text() {
        assert_eq!(user(7).markup(), "<@7>");
        assert_eq!(
            Entry::Role {
                id: 9,
                name: String::new(),
                color: 0
            }
            .markup(),
            "<@&9>"
        );
        assert_eq!(Entry::Global(Global::Here).markup(), "@here");
        let channel = Entry::Channel {
            hit: ChannelHit {
                id: 5,
                name: "x".to_owned(),
                kind: HitKind::Text,
                parent: None,
                score: 1.,
            },
            secondary: String::new(),
        };
        assert_eq!(channel.markup(), "<#5>");
    }
}
