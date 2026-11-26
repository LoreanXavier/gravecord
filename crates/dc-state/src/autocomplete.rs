//! The composer's autocomplete rules. Ports of the query helpers in module 997625 (`AutocompleteUtils`), the type
//! table in module 496533, the current-word rule in module 84046 and match-sorter in module 896170. No UI here.

use crate::confusables::SKELETON;
use crate::members::Member;
use crate::permissions::{self, ADMINISTRATOR, VIEW_CHANNEL};
use crate::{Channel, ChannelKind, Guild, Id, Thread, User};
use unicode_normalization::UnicodeNormalization;

/// How many member results one query keeps (`limit: 10`).
pub const LIMIT: usize = 10;
/// `MENTION_EVERYONE`.
pub const MENTION_EVERYONE: u64 = 1 << 17;

/// `sS`: the text without combining marks.
pub fn strip_marks(text: &str) -> String {
    text.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}

/// `S8`: the confusables skeleton of the text, in lower case.
pub fn skeleton(text: &str) -> String {
    let mapped: String = text
        .nfd()
        .map(|c| {
            SKELETON
                .binary_search_by(|(key, _)| key.cmp(&c))
                .map_or_else(|_| c.to_string(), |index| SKELETON[index].1.to_owned())
        })
        .collect();
    mapped.nfd().collect::<String>().to_lowercase()
}

/// Module 91871: every character of `query` appears in `text` in order.
pub fn fuzzy(query: &str, text: &str) -> bool {
    let (query, text): (Vec<char>, Vec<char>) = (query.chars().collect(), text.chars().collect());
    if query.len() > text.len() {
        return false;
    }
    if query.len() == text.len() {
        return query == text;
    }
    let mut at = 0;
    'outer: for wanted in &query {
        while at < text.len() {
            let found = text[at] == *wanted;
            at += 1;
            if found {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// `eT`: how well `text` matches an already lower-cased query: 10 exact, 7 prefix, 5 contained, 3 every word
/// contained, 1 fuzzy, 0 none.
pub fn text_score(text: &str, query_lower: &str, allow_fuzzy: bool) -> u32 {
    let lower = text.to_lowercase();
    if lower.starts_with(query_lower) {
        return if lower == query_lower { 10 } else { 7 };
    }
    if lower.contains(query_lower) {
        return 5;
    }
    if query_lower
        .split([',', ' '])
        .all(|part| part.is_empty() || lower.contains(part))
    {
        return 3;
    }
    if allow_fuzzy && fuzzy(query_lower, text) {
        return 1;
    }
    0
}

/// match-sorter's rank of `value` for `query` (`u` in module 896170), 0 when it does not match.
fn match_rank(value: &str, query: &str) -> f64 {
    let (value, query) = (strip_marks(value), strip_marks(query));
    if query.chars().count() > value.chars().count() {
        return 0.;
    }
    if value == query {
        return 7.;
    }
    let (value, query) = (value.to_lowercase(), query.to_lowercase());
    if value == query {
        return 6.;
    }
    if value.starts_with(&query) {
        return 5.;
    }
    if value.contains(&format!(" {query}")) {
        return 4.;
    }
    if value.contains(&query) {
        return 3.;
    }
    if query.chars().count() == 1 {
        return 0.;
    }
    let acronym: String = value
        .split(' ')
        .flat_map(|word| word.split('-'))
        .filter_map(|part| part.chars().next())
        .collect();
    if acronym.contains(&query) {
        return 2.;
    }
    let (value, query): (Vec<char>, Vec<char>) = (value.chars().collect(), query.chars().collect());
    let (mut hits, mut at, mut first) = (0, 0, None);
    for wanted in &query {
        match value[at..].iter().position(|c| c == wanted) {
            Some(offset) => {
                hits += 1;
                first.get_or_insert(at + offset);
                at += offset + 1;
            }
            None => return 0.,
        }
    }
    let spread = at - first.unwrap_or(0);
    1. + (1. / spread as f64) * (hits as f64 / query.len() as f64)
}

/// match-sorter over names: the indexes of those that match, best first, ties by name.
pub fn match_sort(names: &[&str], query: &str) -> Vec<usize> {
    let mut ranked: Vec<(f64, usize)> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (match_rank(name, query), index))
        .filter(|(rank, _)| *rank >= 1.)
        .collect();
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                names[a.1]
                    .to_lowercase()
                    .cmp(&names[b.1].to_lowercase())
                    .then_with(|| names[a.1].cmp(names[b.1]))
            })
    });
    ranked.into_iter().map(|(_, index)| index).collect()
}

/// What the word before the caret asks the list for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mention,
    Channel,
    /// `#!`: voice channels.
    VoiceChannel,
    Emoji,
    Command,
    /// `+:` at the start of the text: the emoji to react to the last message with (`REACTIONS`, module 496533).
    Reaction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    pub kind: Kind,
    /// The text after the sentinel, in lower case (`findMatchingAutocompleteType` lower-cases it).
    pub query: String,
    /// Where the word starts and the caret sits, as byte offsets: what an insertion replaces.
    pub start: usize,
    pub end: usize,
}

/// `getCurrentWord` and `findMatchingAutocompleteType`: the word ends at the caret and starts after the last space.
/// `private` is a DM, where channels and global mentions do not apply.
pub fn detect(text: &str, caret: usize, private: bool) -> Option<Trigger> {
    if caret > text.len() || !text.is_char_boundary(caret) || text.trim().is_empty() {
        return None;
    }
    let before = &text[..caret];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(index, c)| index + c.len_utf8());
    let word = &before[start..];
    let make = |kind, sentinel: usize| Trigger {
        kind,
        query: word[sentinel..].to_lowercase(),
        start,
        end: caret,
    };
    if let Some(rest) = word.strip_prefix(':')
        && rest.chars().count() > 1
    {
        return Some(make(Kind::Emoji, 1));
    }
    // `REACTIONS` (sentinel `+:`) matches only the text's first word (`currentWordIsAtStart`).
    if word.starts_with("+:") && before[..start].trim().is_empty() {
        return Some(make(Kind::Reaction, 2));
    }
    if word.starts_with('#') && !private {
        return Some(match word.strip_prefix("#!") {
            Some(_) => make(Kind::VoiceChannel, 2),
            None => make(Kind::Channel, 1),
        });
    }
    if word.starts_with('@') {
        return Some(make(Kind::Mention, 1));
    }
    if text.starts_with('/') {
        return Some(Trigger {
            kind: Kind::Command,
            query: text[1..caret.max(1)].to_lowercase(),
            start: 0,
            end: caret,
        });
    }
    None
}

/// A member the list can show: who, and the name to rank by.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub score: u32,
}

/// `eN`: members whose username, nickname or global name starts with the query come first (10), then those whose
/// skeleton does or that match loosely (1), each group in the order given; at most `limit`.
pub fn rank_members(query: &str, members: &[(&User, Option<&str>)], limit: usize) -> Vec<Ranked> {
    let wanted = query.to_lowercase();
    let wanted_skeleton = skeleton(&wanted);
    let starts =
        |text: &str, wanted: &str| text.chars().take(wanted.chars().count()).eq(wanted.chars());
    let (mut exact, mut loose, mut loose_count) = (Vec::new(), Vec::new(), 0);
    for (index, (user, nick)) in members.iter().enumerate() {
        let nick = nick.map(str::to_lowercase);
        let global = user.global_name.as_ref().map(|name| name.to_lowercase());
        let name = user.username.to_lowercase();
        let name_plain = strip_marks(&name);
        let name_skeleton = skeleton(&name_plain);
        let nick_forms = nick.as_deref().map(|text| {
            let plain = strip_marks(text);
            let skel = skeleton(&plain);
            (plain, skel)
        });
        let global_forms = global.as_deref().map(|text| {
            let plain = strip_marks(text);
            let skel = skeleton(&plain);
            (plain, skel)
        });
        let hit = |plain_or_skeleton: bool| {
            let pick = |forms: &Option<(String, String)>| {
                forms.as_ref().map(|(plain, skel)| {
                    if plain_or_skeleton {
                        plain.clone()
                    } else {
                        skel.clone()
                    }
                })
            };
            let target = if plain_or_skeleton {
                &wanted
            } else {
                &wanted_skeleton
            };
            let own = if plain_or_skeleton {
                [Some(name.clone()), Some(name_plain.clone())]
            } else {
                [Some(name_skeleton.clone()), None]
            };
            own.into_iter()
                .flatten()
                .chain(pick(&nick_forms))
                .chain(pick(&global_forms))
                .chain(if plain_or_skeleton {
                    nick.clone().into_iter().chain(global.clone()).collect()
                } else {
                    Vec::new()
                })
                .any(|text| starts(&text, target))
        };
        let score = if query == user.id.to_string() || hit(true) {
            Some(true)
        } else if hit(false) {
            Some(false)
        } else if loose_count < 50
            && (fuzzy(&wanted, &name_plain)
                || fuzzy(&wanted_skeleton, &name_skeleton)
                || nick_forms.as_ref().is_some_and(|(plain, skel)| {
                    fuzzy(&wanted, plain) || fuzzy(&wanted_skeleton, skel)
                })
                || global_forms.as_ref().is_some_and(|(plain, skel)| {
                    fuzzy(&wanted, plain) || fuzzy(&wanted_skeleton, skel)
                }))
        {
            loose_count += 1;
            None
        } else {
            continue;
        };
        match score {
            Some(true) => exact.push(Ranked {
                index,
                score: 10_000,
            }),
            Some(false) => exact.push(Ranked {
                index,
                score: 1_000,
            }),
            None => loose.push(Ranked {
                index,
                score: 1_000,
            }),
        }
    }
    // Equal scores keep their order: the official comparator never separates them.
    exact.sort_by_key(|a| std::cmp::Reverse(a.score));
    if exact.len() < limit {
        loose.sort_by_key(|a| std::cmp::Reverse(a.score));
        exact.extend(loose.into_iter().take(limit - exact.len()));
    }
    exact.truncate(limit);
    exact
}

/// A mention the list offers besides people and roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    Everyone,
    Here,
    /// `MENTION_GAME` (module 966382), offered while "Show games in autocomplete results" is on.
    Game,
    /// `MENTION_TIMESTAMP` (module 966382), always offered.
    Time,
}

impl Global {
    pub fn text(self) -> &'static str {
        match self {
            Self::Everyone => "@everyone",
            Self::Here => "@here",
            Self::Game => "@game",
            Self::Time => "@time",
        }
    }

    /// The word the query is matched against.
    fn word(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Here => "here",
            Self::Game => "game",
            Self::Time => "time",
        }
    }

    /// The row's description: `MENTION_GAME` and `MENTION_TIMESTAMP` carry the strings `1kR88y` and `V6L3TV`.
    pub fn description(self) -> &'static str {
        match self {
            Self::Everyone => "Notify everyone who has permission to view this channel.",
            Self::Here => "Notify everyone online who has permission to view this channel.",
            Self::Game => "Mention a game",
            Self::Time => "Refer to a time dynamically in the viewer's time zone",
        }
    }
}

/// The `@game` and `@time` entries that follow `@everyone` and `@here` (`queryGuildMentionResults` of module 997625,
/// helper `S`): each shows when the query matches its word and the list is not full (an empty query always shows
/// them), and `@game` only while the games setting is on. `total` is how many rows the list already holds.
pub fn extra_globals(query: &str, total: usize, games_enabled: bool) -> Vec<Global> {
    let lower = query.to_lowercase();
    let mut out = Vec::new();
    let mut total = total;
    for global in [Global::Game, Global::Time] {
        if global == Global::Game && !games_enabled {
            continue;
        }
        if ((total < LIMIT || lower.is_empty()) && fuzzy(&lower, global.word()))
            || lower == global.word()
        {
            out.push(global);
            total += 1;
        }
    }
    out
}

/// What the account may mention in this channel (`getOptions` for the normal composer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentionRules {
    /// `MENTION_EVERYONE` in the channel: @everyone, @here and every role, mentionable or not.
    pub everyone: bool,
    pub private: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mentions {
    /// Indexes into the candidate members given to `mention_results`.
    pub users: Vec<usize>,
    pub globals: Vec<Global>,
    /// Role ids, best match first.
    pub roles: Vec<Id>,
}

/// `queryMentionResults` after the people are chosen: roles the account may mention while room is left, then
/// `@everyone` and `@here`. `users` are the people already listed (at most ten).
pub fn mention_results(
    query: &str,
    users: Vec<usize>,
    guild: Option<&Guild>,
    rules: MentionRules,
) -> Mentions {
    let mut total = users.len();
    let lower = query.to_lowercase();
    let mut roles = Vec::new();
    if total < LIMIT
        && let Some(guild) = guild
    {
        let mut listed: Vec<(Id, &str, i64)> = guild
            .roles
            .iter()
            .filter(|(id, _)| **id != guild.id)
            .filter(|(id, _)| {
                let info = guild.role_info.get(id).copied().unwrap_or_default();
                (info.mentionable || rules.everyone) && !is_managed_everyone(guild, **id)
            })
            .filter(|(_, name)| fuzzy(&lower, &name.to_lowercase()))
            .map(|(id, name)| {
                let position = guild.role_info.get(id).map_or(0, |info| info.position);
                (*id, name.as_str(), position)
            })
            .collect();
        // `getSortedRoles`: the highest role first.
        listed.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        let names: Vec<&str> = listed.iter().map(|(_, name, _)| *name).collect();
        roles = match_sort(&names, query)
            .into_iter()
            .take(LIMIT - total)
            .map(|index| listed[index].0)
            .collect();
        total += roles.len();
    }
    let mut globals = Vec::new();
    if !rules.private && rules.everyone {
        for global in [Global::Everyone, Global::Here] {
            if total < LIMIT && fuzzy(&lower, global.word()) {
                globals.push(global);
                total += 1;
            }
        }
    }
    Mentions {
        users,
        globals,
        roles,
    }
}

fn is_managed_everyone(guild: &Guild, role: Id) -> bool {
    role == guild.id
}

/// Whether a member can see the channel (`VIEW_CHANNEL` for them), the filter `queryChannelUsers` applies.
pub fn member_can_view(guild: &Guild, channel: &Channel, member: &Member) -> bool {
    permissions::member_channel_permissions(guild, channel, member.user.id, &member.roles)
        & (VIEW_CHANNEL | ADMINISTRATOR)
        != 0
}

/// The channels `#` lists, scored as `queryChannels` does.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelHit {
    pub id: Id,
    pub name: String,
    pub kind: HitKind,
    pub parent: Option<Id>,
    pub score: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Text,
    Voice,
    Thread,
    Other,
}

/// `queryChannelResults` for the text list (`voice` false) or the `#!` voice list, best score first and the sidebar
/// order among equals.
pub fn channel_results(
    guild: &Guild,
    query: &str,
    voice: bool,
    me: Option<Id>,
    visible: impl Fn(&Channel) -> bool,
) -> Vec<ChannelHit> {
    let lower = query.to_lowercase();
    let parts: Vec<&str> = query.split(' ').collect();
    let single = parts.len() == 1;
    let mut hits = Vec::new();
    let mut ordered: Vec<&Channel> = guild.channels.iter().collect();
    ordered.sort_by_key(|channel| channel_order(guild, channel));
    let score_of = |name: &str| -> f64 {
        let name_lower = name.to_lowercase();
        if single || parts.iter().filter(|part| !part.is_empty()).count() <= 1 {
            return f64::from(text_score(name, &lower, true));
        }
        // Several words: each must match; `a b` also matches `a-b`.
        let joined_words = lower.replace(' ', "-");
        if name_lower.contains(&joined_words) {
            return 7.;
        }
        let each: Vec<u32> = parts
            .iter()
            .filter(|part| !part.is_empty())
            .map(|part| text_score(name, &part.to_lowercase(), true))
            .collect();
        if each.iter().all(|score| *score > 0) {
            f64::from(*each.iter().min().unwrap_or(&0))
        } else {
            0.
        }
    };
    for channel in ordered {
        let kind = match channel.kind {
            ChannelKind::Text => HitKind::Text,
            ChannelKind::Voice => HitKind::Voice,
            ChannelKind::Other(5 | 15 | 16) => HitKind::Text,
            ChannelKind::Other(13) => HitKind::Voice,
            _ => continue,
        };
        if (kind == HitKind::Voice) != voice || !visible(channel) {
            continue;
        }
        let mut score = score_of(&channel.name);
        if score == 0. {
            continue;
        }
        if !voice && kind == HitKind::Voice {
            score = (score - 1.).max(0.5);
        }
        hits.push(ChannelHit {
            id: channel.id,
            name: channel.name.clone(),
            kind,
            parent: channel.parent_id,
            score,
        });
    }
    if !voice {
        // `computeAllActiveJoinedThreads`: only threads the account is in and that are not archived.
        let mut threads: Vec<&Thread> = guild
            .threads
            .iter()
            // A thread you started is one you are in, whether or not READY listed the membership.
            .filter(|thread| {
                (thread.joined || (thread.owner_id.is_some() && thread.owner_id == me))
                    && !thread.archived
            })
            .collect();
        threads.sort_by_key(|thread| thread.id);
        for thread in threads {
            let parent_visible = guild
                .channels
                .iter()
                .find(|channel| channel.id == thread.parent_id)
                .is_none_or(&visible);
            let score = score_of(&thread.name);
            if score == 0. || !parent_visible {
                continue;
            }
            hits.push(ChannelHit {
                id: thread.id,
                name: thread.name.clone(),
                kind: HitKind::Thread,
                parent: Some(thread.parent_id),
                score,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits
}

/// The sidebar order of channels: by parent category's position, then voice after text, then position.
fn channel_order(guild: &Guild, channel: &Channel) -> (i64, i64, i64, Id) {
    match channel.parent_id {
        None if channel.kind == ChannelKind::Category => {
            ((channel.position + 1) * 1000, 0, 0, channel.id)
        }
        None => (channel.position, 0, 0, channel.id),
        Some(parent) => {
            let parent_position = guild
                .channels
                .iter()
                .find(|candidate| candidate.id == parent)
                .map_or(0, |parent| parent.position);
            let voice = i64::from(matches!(
                channel.kind,
                ChannelKind::Voice | ChannelKind::Other(13)
            ));
            (
                (parent_position + 1) * 1000,
                voice,
                channel.position,
                channel.id,
            )
        }
    }
}

/// `getSearchResultsOrder`'s score of an emoji name for a query: 1, +4 for an exact name, +2 for a whole word, +1
/// for a prefix, times the usage score when it is above 100.
pub fn emoji_score(name: &str, query: &str, frecency: Option<f64>) -> f64 {
    let wanted = query.to_lowercase();
    let lower = name.to_lowercase();
    let mut score = 1.;
    if lower == wanted {
        score += 4.;
    }
    if word_match(&lower, &wanted) || word_match(name, &wanted) {
        score += 2.;
    }
    if lower.starts_with(&wanted) {
        score += 1.;
    }
    if let Some(used) = frecency {
        score *= (used / 100.).max(1.);
    }
    score
}

/// `(^|_|[A-Z])query s?([A-Z]|_|$)` tested on the name.
fn word_match(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    let chars: Vec<char> = name.chars().collect();
    let wanted: Vec<char> = query.chars().collect();
    (0..chars.len()).any(|at| {
        if chars[at..].len() < wanted.len() || chars[at..at + wanted.len()] != wanted[..] {
            return false;
        }
        let opens = at == 0 || chars[at - 1] == '_' || chars[at - 1].is_ascii_uppercase();
        let after = at + wanted.len();
        let closes = |index: usize| {
            index == chars.len() || chars[index] == '_' || chars[index].is_ascii_uppercase()
        };
        opens && (closes(after) || (chars.get(after) == Some(&'s') && closes(after + 1)))
    })
}

/// Whether an emoji name matches the query the way the list filters: spaces and underscores dropped, then contained.
pub fn emoji_matches(name: &str, query: &str) -> bool {
    let wanted: String = query
        .to_lowercase()
        .chars()
        .filter(|c| *c != ' ' && *c != '_')
        .collect();
    name.to_lowercase().replace('_', "").contains(&wanted)
}

/// The markup of a user mention.
pub fn user_markup(user: Id) -> String {
    format!("<@{user}>")
}

pub fn role_markup(role: Id) -> String {
    format!("<@&{role}>")
}

pub fn channel_markup(channel: Id) -> String {
    format!("<#{channel}>")
}

/// `<:name:id>` or `<a:name:id>`; a unicode emoji goes in as itself.
pub fn custom_emoji_markup(name: &str, id: Id, animated: bool) -> String {
    format!("<{}:{name}:{id}>", if animated { "a" } else { "" })
}

/// The text of a pasted or typed `:name:` that a unicode emoji list entry stands for.
pub fn unicode_markup(surrogates: &str) -> String {
    surrogates.to_owned()
}

/// Replaces `start..end` of `text` by `insert` and a space after it (the official adds one unless told not to), and
/// the caret's new place.
pub fn insert_at(text: &str, start: usize, end: usize, insert: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len() + insert.len() + 1);
    out.push_str(&text[..start]);
    out.push_str(insert);
    out.push(' ');
    let caret = out.len();
    out.push_str(&text[end..]);
    (out, caret)
}

#[cfg(test)]
mod tests {
    #[test]
    fn game_and_time_rows_follow_the_query_and_the_setting() {
        assert_eq!(extra_globals("", 0, true), [Global::Game, Global::Time]);
        assert_eq!(extra_globals("", 0, false), [Global::Time]);
        assert_eq!(extra_globals("gam", 0, true), [Global::Game]);
        assert!(extra_globals("zzz", 0, true).is_empty());
        // A full list only takes an exact word.
        assert!(extra_globals("gam", LIMIT, true).is_empty());
        assert_eq!(extra_globals("game", LIMIT, true), [Global::Game]);
    }

    use super::*;

    fn user(id: Id, name: &str, global: Option<&str>) -> User {
        User {
            id,
            username: name.to_owned(),
            global_name: global.map(str::to_owned),
            ..User::default()
        }
    }

    #[test]
    fn the_word_before_the_caret_picks_the_list() {
        let found = detect("hi @Lor", 7, false).unwrap();
        assert_eq!(
            (found.kind, found.query.as_str(), found.start),
            (Kind::Mention, "lor", 3)
        );
        assert_eq!(detect("see #gen", 8, false).unwrap().kind, Kind::Channel);
        assert_eq!(detect("see #gen", 8, true), None);
        assert_eq!(detect("#!vo", 4, false).unwrap().kind, Kind::VoiceChannel);
        assert_eq!(detect(":s", 2, false), None, "emoji need two characters");
        assert_eq!(detect("x :sm", 5, false).unwrap().query, "sm");
        assert_eq!(
            detect("a:sm", 4, false),
            None,
            "the sentinel starts the word"
        );
        assert_eq!(detect("@a b", 4, false), None, "a space ends the word");
        assert_eq!(
            detect("@abc", 2, false).unwrap().query,
            "a",
            "only what is before the caret"
        );
    }

    #[test]
    fn a_slash_at_the_start_of_the_message_is_a_command() {
        assert_eq!(detect("/shr", 4, false).unwrap().kind, Kind::Command);
        assert_eq!(detect("/shr", 4, false).unwrap().query, "shr");
        assert_eq!(detect("/me hi @lo", 10, false).unwrap().kind, Kind::Mention);
        assert_eq!(detect("/me hi", 6, false).unwrap().query, "me hi");
        assert_eq!(detect("a /shr", 6, false), None);
        let react = detect("+:thu", 5, false).unwrap();
        assert_eq!(
            (react.kind, react.query.as_str(), react.start),
            (Kind::Reaction, "thu", 0)
        );
        assert_eq!(detect("+:", 2, true).unwrap().kind, Kind::Reaction);
        assert_eq!(
            detect("hi +:thu", 8, false),
            None,
            "only the first word reacts"
        );
        assert_eq!(detect("++:thu", 6, false), None);
    }

    #[test]
    fn fuzzy_needs_the_characters_in_order() {
        assert!(fuzzy("lrn", "lorean"));
        assert!(!fuzzy("nrl", "lorean"));
        assert!(fuzzy("", "x"));
        assert!(!fuzzy("abcd", "abc"));
        assert!(!fuzzy("abc", "abd"), "equal lengths must be equal");
    }

    #[test]
    fn text_scores_are_ten_seven_five_three_one() {
        assert_eq!(text_score("General", "general", true), 10);
        assert_eq!(text_score("General Chat", "gen", true), 7);
        assert_eq!(text_score("the general", "gen", true), 5);
        assert_eq!(text_score("gen eral", "eral gen", true), 3);
        assert_eq!(text_score("gxexn", "gen", true), 1);
        assert_eq!(text_score("gxexn", "gen", false), 0);
    }

    #[test]
    fn members_rank_prefix_matches_before_loose_ones_and_keep_their_order() {
        let people = [
            user(1, "zed", Some("Lorean")),
            user(2, "xyz", None),
            user(3, "lorean", None),
            user(4, "alore_n", None),
        ];
        let refs: Vec<(&User, Option<&str>)> = people.iter().map(|user| (user, None)).collect();
        let ranked = rank_members("lor", &refs, 10);
        let order: Vec<usize> = ranked.iter().map(|r| r.index).collect();
        assert_eq!(
            order,
            vec![0, 2, 3],
            "prefix first in member order, then the fuzzy one"
        );
        assert_eq!(ranked[0].score, 10_000);
        assert_eq!(ranked[2].score, 1_000);
        assert_eq!(rank_members("lor", &refs, 1).len(), 1);
    }

    #[test]
    fn a_nickname_and_an_id_count_as_prefixes() {
        let people = [user(1234, "a", None), user(2, "b", None)];
        let refs = [(&people[0], None), (&people[1], Some("Zorro"))];
        assert_eq!(rank_members("zor", &refs, 10)[0].index, 1);
        assert_eq!(rank_members("1234", &refs, 10)[0].index, 0);
    }

    #[test]
    fn accents_and_lookalikes_match_their_plain_letters() {
        let people = [user(1, "Émile", None), user(2, "b0b", None)];
        let refs: Vec<(&User, Option<&str>)> = people.iter().map(|user| (user, None)).collect();
        assert_eq!(rank_members("emi", &refs, 10)[0].index, 0);
        assert_eq!(rank_members("bob", &refs, 10)[0].index, 1);
    }

    #[test]
    fn match_sort_ranks_equal_then_prefix_then_word_then_contains() {
        let names = ["my mods", "mods", "modern", "Moderators", "zzz"];
        let order = match_sort(&names, "mod");
        assert_eq!(
            order,
            vec![3, 2, 1, 0],
            "starts-with by name, then the word-start"
        );
        assert_eq!(match_sort(&names, "mods")[0], 1);
        assert!(match_sort(&names, "q").is_empty());
    }

    #[test]
    fn emoji_scores_follow_exact_word_and_prefix() {
        assert_eq!(emoji_score("smile", "smi", None), 2.);
        assert_eq!(emoji_score("smile", "smile", None), 1. + 4. + 2. + 1.);
        assert_eq!(emoji_score("big_smile", "smile", None), 1. + 2.);
        assert_eq!(emoji_score("kissing_smiling_eyes", "smi", None), 1.);
        assert_eq!(emoji_score("smile", "smi", Some(300.)), 6.);
        assert!(emoji_matches("kissing_smiling_eyes", "smi"));
        assert!(emoji_matches("smile_cat", "smile cat"));
        assert!(!emoji_matches("grin", "smi"));
    }

    #[test]
    fn insertion_replaces_the_word_and_adds_a_space() {
        let (text, caret) = insert_at("hi @lo there", 3, 6, "<@5>");
        assert_eq!(text, "hi <@5>  there");
        assert_eq!(caret, 8);
        let (text, caret) = insert_at(":smi", 0, 4, "😄");
        assert_eq!(text, "😄 ");
        assert_eq!(caret, text.len());
    }

    #[test]
    fn markup_is_the_official_one() {
        assert_eq!(user_markup(7), "<@7>");
        assert_eq!(role_markup(7), "<@&7>");
        assert_eq!(channel_markup(7), "<#7>");
        assert_eq!(custom_emoji_markup("wave", 9, false), "<:wave:9>");
        assert_eq!(custom_emoji_markup("wave", 9, true), "<a:wave:9>");
    }

    fn guild() -> Guild {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "READY",
            &serde_json::json!({
                "user": {"id": "5", "username": "me"},
                "guilds": [{
                    "id": "100",
                    "properties": {"name": "g", "owner_id": "5"},
                    "roles": [
                        {"id": "100", "name": "@everyone", "permissions": "1024", "position": 0},
                        {"id": "200", "name": "mods", "permissions": "0", "position": 2, "mentionable": true},
                        {"id": "300", "name": "hidden", "permissions": "0", "position": 1, "mentionable": false}
                    ],
                    "channels": [
                        {"id": "1", "type": 4, "name": "Text Channels", "position": 0},
                        {"id": "2", "type": 0, "name": "general", "position": 0, "parent_id": "1"},
                        {"id": "3", "type": 2, "name": "General", "position": 1, "parent_id": "1"},
                        {"id": "4", "type": 0, "name": "random", "position": 1, "parent_id": "1"}
                    ],
                    "threads": [
                        {"id": "20", "type": 11, "parent_id": "2", "name": "old thread", "thread_metadata": {"archived": true}},
                        {"id": "10", "type": 11, "parent_id": "2", "name": "new thread", "thread_metadata": {"archived": false}, "member": {}}
                    ]
                }]
            }),
        );
        stores.guilds.remove(0)
    }

    #[test]
    fn roles_show_when_mentionable_or_when_the_account_can_mention_everyone() {
        let guild = guild();
        let rules = |everyone| MentionRules {
            everyone,
            private: false,
        };
        let none = mention_results("", Vec::new(), Some(&guild), rules(false));
        assert_eq!(none.roles, vec![200], "only the mentionable role");
        assert!(
            none.globals.is_empty(),
            "no @everyone without the permission"
        );
        let all = mention_results("", Vec::new(), Some(&guild), rules(true));
        assert_eq!(all.roles, vec![300, 200], "alphabetical on an empty query");
        assert_eq!(all.globals, vec![Global::Everyone, Global::Here]);
        let here = mention_results("he", Vec::new(), Some(&guild), rules(true));
        assert_eq!(here.globals, vec![Global::Here]);
        assert_eq!(here.roles, vec![300]);
        let dm = mention_results(
            "",
            Vec::new(),
            None,
            MentionRules {
                everyone: true,
                private: true,
            },
        );
        assert!(dm.globals.is_empty() && dm.roles.is_empty());
    }

    #[test]
    fn a_full_list_of_people_leaves_no_room_for_roles() {
        let guild = guild();
        let people: Vec<User> = (0..12).map(|n| user(n + 1, "ann", None)).collect();
        let refs: Vec<(&User, Option<&str>)> = people.iter().map(|user| (user, None)).collect();
        let users = rank_members("", &refs, LIMIT)
            .into_iter()
            .map(|r| r.index)
            .collect();
        let found = mention_results(
            "",
            users,
            Some(&guild),
            MentionRules {
                everyone: true,
                private: false,
            },
        );
        assert_eq!(found.users.len(), 10);
        assert!(found.roles.is_empty() && found.globals.is_empty());
    }

    #[test]
    fn channels_list_text_then_threads_in_sidebar_order() {
        let guild = guild();
        let found = channel_results(&guild, "", false, None, |_| true);
        let names: Vec<&str> = found.iter().map(|hit| hit.name.as_str()).collect();
        assert_eq!(names, vec!["general", "random", "new thread"]);
        let voice = channel_results(&guild, "", true, None, |_| true);
        assert_eq!(voice.len(), 1);
        assert_eq!(voice[0].name, "General");
        let filtered = channel_results(&guild, "ran", false, None, |_| true);
        assert_eq!(filtered[0].name, "random");
        let hidden = channel_results(&guild, "", false, None, |channel| channel.id != 2);
        assert!(
            hidden
                .iter()
                .all(|hit| hit.name != "general" && hit.parent != Some(2))
        );
    }
}
