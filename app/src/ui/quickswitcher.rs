//! The Quick Switcher (`quickswitcher_a1DYes` in a large modal), opened with Ctrl+K or "Find or start a conversation":
//! "Where would you like to go?", empty-query sections (`Previous Channels`, `Drafts`, `Mentions`, `Unread Channels`),
//! or matches for the query (`@` / `@@` people, `#` text channels & threads, `!` voice & stage channels, `*` servers,
//! group DMs, and in-app navigation), arrow keys / `Ctrl+P` / `Ctrl+N` to pick, `Enter` to go, `Escape` to clear query
//! then close (`174768`, `793322`, `997625`, `787578`, `989133`, `910839`, `320264`).

use super::actions::FRECENCY_KEY;
use super::{Root, avatar_source, channel_icon, color, editor_style, icon, status_dot_small};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::autocomplete::{fuzzy, skeleton, strip_marks};
use dc_state::frecency::{Frecency, Kind, Usage};
use dc_state::{
    Channel, ChannelKind, Guild, Id, PrivateChannel, Stores, Thread, User, permissions,
};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, Image, ImageFormat, IntoElement,
    ScrollHandle, SharedString, Subscription, Window, deferred, div, img, prelude::*, px,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const SINGLE_CATEGORY_LIMIT: usize = 100;
const GENERAL_CATEGORY_LIMIT: usize = 5;
const PREFIX_DEFAULT_LIMIT: usize = 50;
const TUTORIAL_STORAGE_KEY: &str = "seenQSTutorial";
const HELP_ARTICLE_URL: &str = "https://support.discord.com/hc/en-us/articles/115000070311";

const ANNOUNCEMENT_KIND: ChannelKind = ChannelKind::Other(5);
const STAGE_KIND: ChannelKind = ChannelKind::Other(13);
const FORUM_KIND: ChannelKind = ChannelKind::Other(15);
const MEDIA_KIND: ChannelKind = ChannelKind::Other(16);

const RELATIONSHIP_FRIEND: u64 = 1;
const RELATIONSHIP_BLOCKED: u64 = 2;

/// Prefix modes of module `793322` (`LV`) and `662090` (`rD`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryMode {
    User,
    UserGlobal,
    TextChannel,
    VoiceChannel,
    Guild,
}

/// Built-in destinations of module `997625` (`queryInAppNavigations`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavTarget {
    Shop,
    Nitro,
    Quests,
    Settings,
}

impl NavTarget {
    fn label(self) -> String {
        match self {
            Self::Shop => dc_intl::t!("pWG4ze").to_owned(),
            Self::Nitro => dc_intl::t!("Ipxkog").to_owned(),
            Self::Quests => dc_intl::t!("JALI2K").to_owned(),
            Self::Settings => dc_intl::t!("3D5yo/").to_owned(),
        }
    }

    fn icon_name(self) -> &'static str {
        match self {
            Self::Shop => "shop",
            Self::Nitro => "nitro_wheel",
            Self::Quests => "home_quests",
            Self::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    User(Id),
    GroupDm(Id),
    TextChannel { guild: Id, id: Id, is_thread: bool },
    VoiceChannel { guild: Id, id: Id },
    Guild(Id),
    InAppNavigation(NavTarget),
}

impl Destination {
    fn channel_id(self) -> Option<Id> {
        match self {
            Self::GroupDm(id) | Self::TextChannel { id, .. } | Self::VoiceChannel { id, .. } => {
                Some(id)
            }
            Self::User(_) | Self::Guild(_) | Self::InAppNavigation(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SwitcherRow {
    Header(String),
    Result {
        destination: Destination,
        comparator: Option<String>,
        score: f64,
        sortable: String,
    },
}

impl SwitcherRow {
    fn is_header(&self) -> bool {
        matches!(self, Self::Header(_))
    }

    fn destination(&self) -> Option<Destination> {
        match self {
            Self::Header(_) => None,
            Self::Result { destination, .. } => Some(*destination),
        }
    }
}

pub(crate) struct QuickSwitcher {
    pub(super) search: Entity<Editor>,
    _events: Subscription,
    _interceptor: Subscription,
    scroll: ScrollHandle,
    query: String,
    selected: Option<usize>,
    seen_tutorial: bool,
}

/// The protip's text around its symbols (`BGHbLb`): `0` before the first symbol, `1` after the last one, before
/// "Learn more".
fn protip_part(which: usize) -> String {
    let hooks = [
        "userSymbolHook",
        "textChannelSymbolHook",
        "voiceChannelSymbolHook",
        "guildSymbolHook",
        "gameProfileSymbolHook",
    ];
    let values: Vec<(&str, dc_intl::Arg)> = hooks.iter().map(|hook| (*hook, "@".into())).collect();
    let runs = dc_intl::format_runs("BGHbLb", &values);
    let plain: Vec<&str> = runs
        .iter()
        .filter(|(_, tag)| tag.is_none())
        .map(|(text, _)| text.trim())
        .filter(|text| !text.is_empty())
        .collect();
    let part = if which == 0 {
        plain.first()
    } else {
        plain.last()
    };
    part.copied().unwrap_or_default().to_owned()
}

/// Parses prefix modes `@`, `@@`, `#`, `!`, `*` (`793322:LV`).
fn parse_query_mode(raw: &str) -> (Option<QueryMode>, &str) {
    let trimmed_start = raw.trim_start();
    if let Some(rest) = trimmed_start.strip_prefix("@@") {
        (Some(QueryMode::UserGlobal), rest.trim())
    } else if let Some(rest) = trimmed_start.strip_prefix('@') {
        (Some(QueryMode::User), rest.trim())
    } else if let Some(rest) = trimmed_start.strip_prefix('#') {
        (Some(QueryMode::TextChannel), rest.trim())
    } else if let Some(rest) = trimmed_start.strip_prefix('!') {
        (Some(QueryMode::VoiceChannel), rest.trim())
    } else if let Some(rest) = trimmed_start.strip_prefix('*') {
        (Some(QueryMode::Guild), rest.trim())
    } else {
        (None, raw.trim())
    }
}

/// NFD / accent stripping + lowercase (`787578` / `240248:sS`).
fn fold_text(text: &str) -> String {
    strip_marks(text).to_lowercase()
}

/// Space, hyphen, and underscore stripping (`eb` in `997625`).
fn strip_separators(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, ' ' | '-' | '_'))
        .collect()
}

/// Official string scorer `eT` (`997625`):
/// `10` exact, `9` prefix, `8` stripped prefix, `7` substring, `6` stripped substring,
/// `5 * len_ratio` fuzzy, `4 * len_ratio` stripped fuzzy.
fn score_string(query: &str, candidate: &str) -> f64 {
    let q = fold_text(query);
    let c = fold_text(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if c == q {
        return 10.0;
    }
    if c.starts_with(&q) {
        return 9.0;
    }
    let q_s = strip_separators(&q);
    let c_s = strip_separators(&c);
    if !q_s.is_empty() && c_s.starts_with(&q_s) {
        return 8.0;
    }
    if c.contains(&q) {
        return 7.0;
    }
    if !q_s.is_empty() && c_s.contains(&q_s) {
        return 6.0;
    }
    let q_len = q.chars().count() as f64;
    let c_len = c.chars().count().max(1) as f64;
    if fuzzy(&q, &c) {
        return 5.0 * (q_len / c_len);
    }
    if !q_s.is_empty() && fuzzy(&q_s, &c_s) {
        let qs_len = q_s.chars().count() as f64;
        let cs_len = c_s.chars().count().max(1) as f64;
        return 4.0 * (qs_len / cs_len);
    }
    0.0
}

/// Scores a single user candidate string (`787578`): `10` prefix, `5` substring, `1` fuzzy / skeleton fuzzy.
fn score_user_candidate(
    query_folded: &str,
    query_stripped: &str,
    query_skel: &str,
    name: &str,
) -> f64 {
    if name.is_empty() || query_folded.is_empty() {
        return 0.0;
    }
    let c = fold_text(name);
    if c.is_empty() {
        return 0.0;
    }
    let c_s = strip_separators(&c);
    if c.starts_with(query_folded)
        || (!query_stripped.is_empty() && c_s.starts_with(query_stripped))
    {
        return 10.0;
    }
    if c.contains(query_folded) || (!query_stripped.is_empty() && c_s.contains(query_stripped)) {
        return 5.0;
    }
    if fuzzy(query_folded, &c)
        || (!query_stripped.is_empty() && fuzzy(query_stripped, &c_s))
        || (!query_skel.is_empty() && fuzzy(query_skel, &skeleton(&c)))
    {
        return 1.0;
    }
    0.0
}

/// Matches a user across `[username, friend_nickname, global_name, guild_nicknames...]` (`787578`), keeping
/// candidate priority on ties.
fn match_user_names<'a>(
    user: &'a User,
    friend_nickname: Option<&'a str>,
    guild_nicknames: &[&'a str],
    query: &str,
) -> Option<(&'a str, f64)> {
    let q_folded = fold_text(query);
    if q_folded.is_empty() {
        return None;
    }
    let q_stripped = strip_separators(&q_folded);
    let q_skel = skeleton(&q_folded);

    let mut best_name: Option<&'a str> = None;
    let mut best_score = 0.0;
    let base_candidates = [
        Some(user.username.as_str()),
        friend_nickname,
        user.global_name.as_deref(),
    ];
    for candidate in base_candidates
        .into_iter()
        .flatten()
        .chain(guild_nicknames.iter().copied())
    {
        let tier = score_user_candidate(&q_folded, &q_stripped, &q_skel, candidate);
        if tier > best_score {
            best_score = tier;
            best_name = Some(candidate);
        }
    }
    best_name.map(|name| (name, best_score))
}

#[cfg(test)]
fn matched_user_name<'a>(
    user: &'a User,
    nickname: Option<&'a str>,
    query: &str,
) -> Option<&'a str> {
    match_user_names(user, nickname, &[], query).map(|(name, _)| name)
}

/// Official `KJ.getDisplayNickname` (`320264`): shows the matched `comparator` when it was a guild nickname or
/// non-username alias, falling back to friend nickname, global display name, or username.
fn user_display_nickname(
    user: &User,
    friend_nickname: Option<&str>,
    comparator: Option<&str>,
) -> String {
    let id_str = user.id.to_string();
    if let Some(comp) = comparator.filter(|comp| !comp.is_empty())
        && !comp.eq_ignore_ascii_case(&user.username)
        && comp != id_str
    {
        return comp.to_owned();
    }
    friend_nickname
        .filter(|nick| !nick.is_empty())
        .or_else(|| user.global_name.as_deref().filter(|name| !name.is_empty()))
        .unwrap_or(user.username.as_str())
        .to_owned()
}

/// Comparator of module `989133`: descending `score`, then ascending lexicographical `sortable`.
fn compare_results(a: &SwitcherRow, b: &SwitcherRow) -> std::cmp::Ordering {
    match (a, b) {
        (
            SwitcherRow::Result {
                score: sa,
                sortable: na,
                ..
            },
            SwitcherRow::Result {
                score: sb,
                sortable: nb,
                ..
            },
        ) => sb.total_cmp(sa).then_with(|| na.cmp(nb)),
        _ => std::cmp::Ordering::Equal,
    }
}

fn first_selectable_index(rows: &[SwitcherRow]) -> Option<usize> {
    rows.iter().position(|row| !row.is_header())
}

/// Official `Vv` (`662090`): steps selection up (`-1`) or down (`+1`) with wrap-around, skipping `Header` rows.
fn step_selection(rows: &[SwitcherRow], current: Option<usize>, delta: i32) -> Option<usize> {
    let len = rows.len();
    if len == 0 || rows.iter().all(SwitcherRow::is_header) {
        return None;
    }
    let Some(start) = current.filter(|&idx| idx < len && !rows[idx].is_header()) else {
        return if delta < 0 {
            rows.iter().rposition(|row| !row.is_header())
        } else {
            first_selectable_index(rows)
        };
    };
    let mut idx = start as isize;
    for _ in 0..len {
        idx += if delta < 0 { -1 } else { 1 };
        if idx < 0 {
            idx = len as isize - 1;
        } else if idx >= len as isize {
            idx = 0;
        }
        let candidate = idx as usize;
        if !rows[candidate].is_header() {
            return Some(candidate);
        }
    }
    Some(start)
}

fn is_text_searchable_kind(kind: ChannelKind) -> bool {
    matches!(
        kind,
        ChannelKind::Text | ANNOUNCEMENT_KIND | FORUM_KIND | MEDIA_KIND
    )
}

fn is_vocal_kind(kind: ChannelKind) -> bool {
    matches!(kind, ChannelKind::Voice | STAGE_KIND)
}

fn can_view_guild_channel(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    if guild.role_permissions.is_empty() {
        return true;
    }
    let Some(me) = stores.me() else {
        return true;
    };
    permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
}

fn find_dm_for_user(stores: &Stores, user_id: Id) -> Option<&PrivateChannel> {
    stores
        .private_channels
        .iter()
        .find(|dm| dm.kind == 1 && dm.recipients.len() == 1 && dm.recipients[0] == user_id)
}

fn prefix_header(mode: QueryMode, stores: &Stores, current_guild: Option<Id>) -> String {
    match mode {
        QueryMode::User => {
            if let Some(gid) = current_guild
                && let Some(guild) = stores.guild(gid)
            {
                dc_intl::t!("FREzQs", name = guild.name.as_str())
            } else {
                dc_intl::t!("XFYW1o").to_owned()
            }
        }
        QueryMode::UserGlobal => dc_intl::t!("XFYW1o").to_owned(),
        QueryMode::TextChannel => dc_intl::t!("W26k4V").to_owned(),
        QueryMode::VoiceChannel => dc_intl::t!("zUoI5C").to_owned(),
        QueryMode::Guild => dc_intl::t!("olADPs").to_owned(),
    }
}

struct IndexedUser<'a> {
    user: &'a User,
    is_friend: bool,
    friend_nickname: Option<&'a str>,
    dm_channel_id: Option<Id>,
    guild_nicknames: Vec<(Id, &'a str)>,
    in_current_guild: bool,
}

/// Indexes all non-blocked, non-ignored users across `stores.users`, `stores.relationships`,
/// `stores.members`, and 1:1 private channels, excluding `current_user.id` (`450827` / `787578`).
fn collect_indexed_users(stores: &Stores, current_guild: Option<Id>) -> Vec<IndexedUser<'_>> {
    let me_id = stores.current_user.as_ref().map(|me| me.id);
    let mut member_users: HashMap<Id, &User> = HashMap::new();
    let mut guild_nicks: HashMap<Id, Vec<(Id, &str)>> = HashMap::new();
    let mut current_guild_members: HashSet<Id> = HashSet::new();

    for guild in &stores.guilds {
        for member in stores.members.of(guild.id) {
            let uid = member.user.id;
            member_users.entry(uid).or_insert(&member.user);
            if Some(guild.id) == current_guild {
                current_guild_members.insert(uid);
            }
            if let Some(nick) = member.nick.as_deref().filter(|nick| !nick.is_empty()) {
                let nicks = guild_nicks.entry(uid).or_default();
                if !nicks.iter().any(|(_, existing)| *existing == nick) {
                    nicks.push((guild.id, nick));
                }
            }
        }
    }

    let mut dm_by_user: HashMap<Id, Id> = HashMap::new();
    for dm in &stores.private_channels {
        if dm.kind == 1 && dm.recipients.len() == 1 {
            dm_by_user.entry(dm.recipients[0]).or_insert(dm.id);
        }
    }

    let mut all_ids: Vec<Id> = Vec::new();
    let mut seen: HashSet<Id> = HashSet::new();
    for &uid in stores
        .users
        .keys()
        .chain(stores.relationships.keys())
        .chain(member_users.keys())
        .chain(dm_by_user.keys())
    {
        if seen.insert(uid) {
            all_ids.push(uid);
        }
    }
    all_ids.sort_unstable();

    let mut indexed = Vec::new();
    for uid in all_ids {
        if Some(uid) == me_id {
            continue;
        }
        let rel = stores.relationships.get(&uid).copied();
        if rel == Some(RELATIONSHIP_BLOCKED) || stores.is_ignored(uid) {
            continue;
        }
        let Some(user) = stores
            .users
            .get(&uid)
            .or_else(|| member_users.get(&uid).copied())
        else {
            continue;
        };
        indexed.push(IndexedUser {
            user,
            is_friend: rel == Some(RELATIONSHIP_FRIEND),
            friend_nickname: stores
                .relationship_nicknames
                .get(&uid)
                .map(String::as_str)
                .filter(|nick| !nick.is_empty()),
            dm_channel_id: dm_by_user.get(&uid).copied(),
            guild_nicknames: guild_nicks.remove(&uid).unwrap_or_default(),
            in_current_guild: current_guild_members.contains(&uid),
        });
    }
    indexed
}

fn destination_for_channel_id(stores: &Stores, channel_id: Id) -> Option<(Destination, String)> {
    if let Some(dm) = stores
        .private_channels
        .iter()
        .find(|dm| dm.id == channel_id && !dm.is_request())
    {
        if dm.kind == 1 {
            if let Some(user) = stores.dm_recipient(dm) {
                let me_id = stores.current_user.as_ref().map(|me| me.id);
                let blocked = stores.relationships.get(&user.id) == Some(&RELATIONSHIP_BLOCKED);
                if Some(user.id) != me_id && !blocked && !stores.is_ignored(user.id) {
                    return Some((Destination::User(user.id), fold_text(&user.username)));
                }
            }
            return None;
        }
        if dm.kind == 3 {
            let label = dm.name.clone().unwrap_or_default();
            return Some((Destination::GroupDm(dm.id), fold_text(&label)));
        }
    }
    for guild in &stores.guilds {
        if let Some(ch) = guild.channels.iter().find(|ch| ch.id == channel_id) {
            if !can_view_guild_channel(stores, guild, ch) {
                return None;
            }
            if is_text_searchable_kind(ch.kind) {
                return Some((
                    Destination::TextChannel {
                        guild: guild.id,
                        id: ch.id,
                        is_thread: false,
                    },
                    fold_text(&ch.name),
                ));
            }
            if is_vocal_kind(ch.kind) {
                return Some((
                    Destination::VoiceChannel {
                        guild: guild.id,
                        id: ch.id,
                    },
                    fold_text(&ch.name),
                ));
            }
        }
        if let Some(thread) = guild
            .threads
            .iter()
            .find(|thread| thread.id == channel_id && !thread.archived)
        {
            return Some((
                Destination::TextChannel {
                    guild: guild.id,
                    id: thread.id,
                    is_thread: true,
                },
                fold_text(&thread.name),
            ));
        }
    }
    None
}

fn is_current_destination(dest: Destination, stores: &Stores, current_channel: Option<Id>) -> bool {
    let Some(open_ch) = current_channel else {
        return false;
    };
    match dest {
        Destination::User(uid) => find_dm_for_user(stores, uid).is_some_and(|dm| dm.id == open_ch),
        _ => dest.channel_id() == Some(open_ch),
    }
}

struct SwitcherContext<'a> {
    stores: &'a Stores,
    current_guild: Option<Id>,
    current_channel: Option<Id>,
    channel_history: &'a [Id],
    draft_channels: &'a [Id],
    frecency_scores: &'a HashMap<String, f64>,
    frequent_ids: &'a [Id],
    now: chrono::DateTime<chrono::Utc>,
}

fn frecency_boost(scores: &HashMap<String, f64>, max_frecency: f64, key: Id) -> f64 {
    if max_frecency <= 0.0 {
        return 0.0;
    }
    scores
        .get(&key.to_string())
        .copied()
        .map(|val| (val / max_frecency).clamp(0.0, 1.0) * 0.2)
        .unwrap_or(0.0)
}

/// Builds the empty-query sections (`174768`):
/// `Previous Channels` (`80lOZ1`), `Drafts` (`4B63jZ`), `Mentions` (`61Df13`), `Unread Channels` (`ieCAhD`).
fn build_empty_query_rows(ctx: &SwitcherContext<'_>) -> Vec<SwitcherRow> {
    let mut drafts: Vec<(Destination, String)> = Vec::new();
    for &ch_id in ctx.draft_channels {
        if let Some((dest, sortable)) = destination_for_channel_id(ctx.stores, ch_id)
            && !is_current_destination(dest, ctx.stores, ctx.current_channel)
            && !drafts.iter().any(|(existing, _)| *existing == dest)
        {
            drafts.push((dest, sortable));
        }
    }

    let mut mentions: Vec<(Destination, String)> = Vec::new();
    for dm in ctx.stores.pickable_private_channels() {
        if ctx.stores.read_states.mentions(dm.id) > 0
            && let Some((dest, sortable)) = destination_for_channel_id(ctx.stores, dm.id)
            && !is_current_destination(dest, ctx.stores, ctx.current_channel)
            && !mentions.iter().any(|(existing, _)| *existing == dest)
        {
            mentions.push((dest, sortable));
        }
    }
    for guild in &ctx.stores.guilds {
        for ch in guild.sidebar_rows() {
            if is_text_searchable_kind(ch.kind)
                && ctx.stores.read_states.mentions(ch.id) > 0
                && can_view_guild_channel(ctx.stores, guild, ch)
            {
                let dest = Destination::TextChannel {
                    guild: guild.id,
                    id: ch.id,
                    is_thread: false,
                };
                if !is_current_destination(dest, ctx.stores, ctx.current_channel)
                    && !mentions.iter().any(|(existing, _)| *existing == dest)
                {
                    mentions.push((dest, fold_text(&ch.name)));
                }
            }
        }
        for thread in &guild.threads {
            if !thread.archived && ctx.stores.read_states.mentions(thread.id) > 0 {
                let dest = Destination::TextChannel {
                    guild: guild.id,
                    id: thread.id,
                    is_thread: true,
                };
                if !is_current_destination(dest, ctx.stores, ctx.current_channel)
                    && !mentions.iter().any(|(existing, _)| *existing == dest)
                {
                    mentions.push((dest, fold_text(&thread.name)));
                }
            }
        }
    }

    let mut unreads: Vec<(Destination, String)> = Vec::new();
    if let Some(gid) = ctx.current_guild
        && let Some(guild) = ctx.stores.guild(gid)
    {
        for ch in guild.sidebar_rows() {
            if !is_text_searchable_kind(ch.kind)
                || !can_view_guild_channel(ctx.stores, guild, ch)
                || ctx.stores.guild_settings.channel_muted(gid, ch.id, ctx.now)
                || !ctx.stores.is_unread(ch.id)
            {
                continue;
            }
            let dest = Destination::TextChannel {
                guild: gid,
                id: ch.id,
                is_thread: false,
            };
            if !is_current_destination(dest, ctx.stores, ctx.current_channel)
                && !mentions.iter().any(|(existing, _)| *existing == dest)
                && !unreads.iter().any(|(existing, _)| *existing == dest)
            {
                unreads.push((dest, fold_text(&ch.name)));
            }
        }
        for thread in &guild.threads {
            let thread_muted =
                dc_state::guild_settings::mute_active(thread.muted, thread.mute_end, ctx.now);
            if !thread.archived && thread.joined && !thread_muted && ctx.stores.is_unread(thread.id)
            {
                let dest = Destination::TextChannel {
                    guild: gid,
                    id: thread.id,
                    is_thread: true,
                };
                if !is_current_destination(dest, ctx.stores, ctx.current_channel)
                    && !mentions.iter().any(|(existing, _)| *existing == dest)
                    && !unreads.iter().any(|(existing, _)| *existing == dest)
                {
                    unreads.push((dest, fold_text(&thread.name)));
                }
            }
        }
    }

    let has_other_sections = !drafts.is_empty() || !mentions.is_empty() || !unreads.is_empty();
    let prev_limit = if has_other_sections { 3 } else { 8 };
    let mut previous: Vec<(Destination, String)> = Vec::new();
    for &ch_id in ctx.channel_history.iter().chain(ctx.frequent_ids.iter()) {
        if previous.len() >= prev_limit {
            break;
        }
        if let Some((dest, sortable)) = destination_for_channel_id(ctx.stores, ch_id)
            && !is_current_destination(dest, ctx.stores, ctx.current_channel)
            && !previous.iter().any(|(existing, _)| *existing == dest)
        {
            previous.push((dest, sortable));
        }
    }

    let mut rows = Vec::new();
    let mut append_section = |title: String, items: Vec<(Destination, String)>| {
        if items.is_empty() {
            return;
        }
        rows.push(SwitcherRow::Header(title));
        for (destination, sortable) in items {
            rows.push(SwitcherRow::Result {
                destination,
                comparator: None,
                score: 0.0,
                sortable,
            });
        }
    };
    append_section(dc_intl::t!("80lOZ1").to_owned(), previous);
    append_section(dc_intl::t!("4B63jZ").to_owned(), drafts);
    append_section(dc_intl::t!("61Df13").to_owned(), mentions);
    append_section(dc_intl::t!("ieCAhD").to_owned(), unreads);
    rows
}

/// Mode-appropriate defaults when a prefix symbol (`@`, `@@`, `#`, `!`, `*`) is entered with an empty query (`174768:q`).
fn build_prefix_default_rows(ctx: &SwitcherContext<'_>, mode: QueryMode) -> Vec<SwitcherRow> {
    let mut rows = vec![SwitcherRow::Header(prefix_header(
        mode,
        ctx.stores,
        ctx.current_guild,
    ))];
    match mode {
        QueryMode::User | QueryMode::UserGlobal => {
            let strict_guild = mode == QueryMode::User && ctx.current_guild.is_some();
            let indexed = collect_indexed_users(ctx.stores, ctx.current_guild);
            let allowed: HashMap<Id, &IndexedUser<'_>> = indexed
                .iter()
                .filter(|u| !strict_guild || u.is_friend || u.in_current_guild)
                .map(|u| (u.user.id, u))
                .collect();

            let mut pushed: HashSet<Id> = HashSet::new();
            // 1. Recent 1:1 DMs (`getRecentlyTalked`) in `pickable_private_channels` order.
            for dm in ctx.stores.pickable_private_channels() {
                if dm.kind == 1
                    && Some(dm.id) != ctx.current_channel
                    && let Some(recipient) = ctx.stores.dm_recipient(dm)
                    && allowed.contains_key(&recipient.id)
                    && pushed.insert(recipient.id)
                {
                    rows.push(SwitcherRow::Result {
                        destination: Destination::User(recipient.id),
                        comparator: None,
                        score: 0.0,
                        sortable: fold_text(&recipient.username),
                    });
                    if pushed.len() >= PREFIX_DEFAULT_LIMIT {
                        return rows;
                    }
                }
            }
            // 2. Friends, then remaining allowed users sorted alphabetically.
            let mut rest: Vec<&IndexedUser<'_>> = allowed
                .values()
                .copied()
                .filter(|u| !pushed.contains(&u.user.id))
                .collect();
            rest.sort_by(|a, b| {
                b.is_friend
                    .cmp(&a.is_friend)
                    .then_with(|| fold_text(&a.user.username).cmp(&fold_text(&b.user.username)))
            });
            for entry in rest {
                if pushed.len() >= PREFIX_DEFAULT_LIMIT {
                    break;
                }
                if pushed.insert(entry.user.id) {
                    rows.push(SwitcherRow::Result {
                        destination: Destination::User(entry.user.id),
                        comparator: None,
                        score: 0.0,
                        sortable: fold_text(&entry.user.username),
                    });
                }
            }
        }
        QueryMode::TextChannel => {
            let mut ordered_guilds: Vec<&Guild> = Vec::new();
            if let Some(gid) = ctx.current_guild
                && let Some(current) = ctx.stores.guild(gid)
            {
                ordered_guilds.push(current);
            }
            for guild in &ctx.stores.guilds {
                if Some(guild.id) != ctx.current_guild {
                    ordered_guilds.push(guild);
                }
            }
            let mut count = 0;
            for guild in ordered_guilds {
                for ch in guild.sidebar_rows() {
                    if count >= PREFIX_DEFAULT_LIMIT {
                        return rows;
                    }
                    if is_text_searchable_kind(ch.kind)
                        && Some(ch.id) != ctx.current_channel
                        && can_view_guild_channel(ctx.stores, guild, ch)
                    {
                        rows.push(SwitcherRow::Result {
                            destination: Destination::TextChannel {
                                guild: guild.id,
                                id: ch.id,
                                is_thread: false,
                            },
                            comparator: None,
                            score: 0.0,
                            sortable: fold_text(&ch.name),
                        });
                        count += 1;
                    }
                }
                for thread in &guild.threads {
                    if count >= PREFIX_DEFAULT_LIMIT {
                        return rows;
                    }
                    if !thread.archived && Some(thread.id) != ctx.current_channel {
                        rows.push(SwitcherRow::Result {
                            destination: Destination::TextChannel {
                                guild: guild.id,
                                id: thread.id,
                                is_thread: true,
                            },
                            comparator: None,
                            score: 0.0,
                            sortable: fold_text(&thread.name),
                        });
                        count += 1;
                    }
                }
            }
        }
        QueryMode::VoiceChannel => {
            let mut ordered_guilds: Vec<&Guild> = Vec::new();
            if let Some(gid) = ctx.current_guild
                && let Some(current) = ctx.stores.guild(gid)
            {
                ordered_guilds.push(current);
            }
            for guild in &ctx.stores.guilds {
                if Some(guild.id) != ctx.current_guild {
                    ordered_guilds.push(guild);
                }
            }
            let mut count = 0;
            for guild in ordered_guilds {
                for ch in guild.sidebar_rows() {
                    if count >= PREFIX_DEFAULT_LIMIT {
                        return rows;
                    }
                    if is_vocal_kind(ch.kind) && can_view_guild_channel(ctx.stores, guild, ch) {
                        rows.push(SwitcherRow::Result {
                            destination: Destination::VoiceChannel {
                                guild: guild.id,
                                id: ch.id,
                            },
                            comparator: None,
                            score: 0.0,
                            sortable: fold_text(&ch.name),
                        });
                        count += 1;
                    }
                }
            }
        }
        QueryMode::Guild => {
            for guild in &ctx.stores.guilds {
                if Some(guild.id) != ctx.current_guild {
                    rows.push(SwitcherRow::Result {
                        destination: Destination::Guild(guild.id),
                        comparator: None,
                        score: 0.0,
                        sortable: fold_text(&guild.name),
                    });
                }
            }
        }
    }
    rows
}

/// Builds the full Quick Switcher row list (`174768`, `997625`, `787578`, `989133`).
fn build_switcher_rows(ctx: &SwitcherContext<'_>, raw_query: &str) -> Vec<SwitcherRow> {
    let (mode, query) = parse_query_mode(raw_query);
    if mode.is_none() && query.is_empty() {
        return build_empty_query_rows(ctx);
    }
    if let Some(prefix_mode) = mode
        && query.is_empty()
    {
        return build_prefix_default_rows(ctx, prefix_mode);
    }

    let max_frecency = ctx
        .frecency_scores
        .values()
        .copied()
        .fold(0.0_f64, f64::max);

    let mut user_matches: Vec<SwitcherRow> = Vec::new();
    let mut group_dm_matches: Vec<SwitcherRow> = Vec::new();
    let mut channel_matches: Vec<SwitcherRow> = Vec::new();
    let mut guild_matches: Vec<SwitcherRow> = Vec::new();
    let mut nav_matches: Vec<SwitcherRow> = Vec::new();

    // 1. Users (`787578` + `450827`).
    if matches!(mode, None | Some(QueryMode::User | QueryMode::UserGlobal)) {
        let strict_guild = mode == Some(QueryMode::User) && ctx.current_guild.is_some();
        for entry in collect_indexed_users(ctx.stores, ctx.current_guild) {
            if strict_guild && !entry.is_friend && !entry.in_current_guild {
                continue;
            }
            let uid = entry.user.id;
            let mut matched: Option<(String, f64)> = None;
            if query == uid.to_string() {
                matched = Some((uid.to_string(), 10.0));
            } else {
                let guild_nicks: Vec<&str> = if strict_guild {
                    entry
                        .guild_nicknames
                        .iter()
                        .filter(|(gid, _)| Some(*gid) == ctx.current_guild)
                        .map(|(_, nick)| *nick)
                        .collect()
                } else {
                    entry
                        .guild_nicknames
                        .iter()
                        .map(|(_, nick)| *nick)
                        .collect()
                };
                if let Some((comp, tier)) =
                    match_user_names(entry.user, entry.friend_nickname, &guild_nicks, query)
                {
                    matched = Some((comp.to_owned(), tier));
                }
            }
            if let Some((comparator, tier)) = matched {
                let dm_frecency = entry
                    .dm_channel_id
                    .map(|dm_id| frecency_boost(ctx.frecency_scores, max_frecency, dm_id))
                    .unwrap_or(0.0);
                let user_frecency = frecency_boost(ctx.frecency_scores, max_frecency, uid);
                let mut booster = 1.0 + dm_frecency.max(user_frecency);
                if entry.is_friend {
                    booster += 0.2;
                }
                if entry.dm_channel_id.is_some() {
                    booster += 0.1;
                }
                let sortable = fold_text(&comparator);
                user_matches.push(SwitcherRow::Result {
                    destination: Destination::User(uid),
                    comparator: Some(comparator),
                    score: tier * booster,
                    sortable,
                });
            }
        }
        user_matches.sort_by(compare_results);
    }

    // 2. Group DMs (`997625:594-632`): explicit group name `0..=10`, recipient names capped at `5.0`.
    if mode.is_none() {
        for dm in ctx.stores.pickable_private_channels() {
            if dm.kind != 3 {
                continue;
            }
            let explicit_tier = dm
                .name
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(|name| score_string(query, name))
                .unwrap_or(0.0);
            let mut recipient_tier = 0.0_f64;
            for &uid in &dm.recipients {
                if let Some(user) = ctx.stores.users.get(&uid) {
                    let friend_nick = ctx
                        .stores
                        .relationship_nicknames
                        .get(&uid)
                        .map(String::as_str);
                    for name in [
                        Some(user.username.as_str()),
                        friend_nick,
                        user.global_name.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        recipient_tier = recipient_tier.max(score_string(query, name).min(5.0));
                    }
                }
            }
            let tier = explicit_tier.max(recipient_tier);
            if tier > 0.0 {
                let boost = frecency_boost(ctx.frecency_scores, max_frecency, dm.id);
                let fallback_name = dm.name.clone().unwrap_or_else(|| {
                    dm.recipients
                        .iter()
                        .filter_map(|id| ctx.stores.users.get(id).map(|u| u.display_name()))
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                group_dm_matches.push(SwitcherRow::Result {
                    destination: Destination::GroupDm(dm.id),
                    comparator: Some(fallback_name.clone()),
                    score: tier + boost,
                    sortable: fold_text(&fallback_name),
                });
            }
        }
        group_dm_matches.sort_by(compare_results);
    }

    // 3. Guild Channels & Active Threads (`997625:queryChannels`).
    if matches!(
        mode,
        None | Some(QueryMode::TextChannel | QueryMode::VoiceChannel)
    ) {
        let voice_only = mode == Some(QueryMode::VoiceChannel);
        for guild in &ctx.stores.guilds {
            let in_current = Some(guild.id) == ctx.current_guild;
            for channel in &guild.channels {
                let text_kind = is_text_searchable_kind(channel.kind);
                let vocal_kind = is_vocal_kind(channel.kind);
                let include = if voice_only {
                    vocal_kind
                } else {
                    text_kind || vocal_kind
                };
                if !include || !can_view_guild_channel(ctx.stores, guild, channel) {
                    continue;
                }
                let mut tier = score_string(query, &channel.name);
                if tier <= 0.0 {
                    continue;
                }
                if vocal_kind && !voice_only {
                    tier = (tier - 1.0).max(0.5);
                }
                if in_current {
                    tier += 0.5;
                }
                let boost = frecency_boost(ctx.frecency_scores, max_frecency, channel.id);
                let destination = if vocal_kind {
                    Destination::VoiceChannel {
                        guild: guild.id,
                        id: channel.id,
                    }
                } else {
                    Destination::TextChannel {
                        guild: guild.id,
                        id: channel.id,
                        is_thread: false,
                    }
                };
                channel_matches.push(SwitcherRow::Result {
                    destination,
                    comparator: Some(channel.name.clone()),
                    score: tier + boost,
                    sortable: fold_text(&channel.name),
                });
            }

            if !voice_only {
                for thread in &guild.threads {
                    if thread.archived {
                        continue;
                    }
                    let mut tier = score_string(query, &thread.name);
                    if tier <= 0.0 {
                        continue;
                    }
                    if in_current {
                        tier += 0.5;
                    }
                    let boost = frecency_boost(ctx.frecency_scores, max_frecency, thread.id);
                    channel_matches.push(SwitcherRow::Result {
                        destination: Destination::TextChannel {
                            guild: guild.id,
                            id: thread.id,
                            is_thread: true,
                        },
                        comparator: Some(thread.name.clone()),
                        score: tier + boost,
                        sortable: fold_text(&thread.name),
                    });
                }
            }
        }
        channel_matches.sort_by(compare_results);
    }

    // 4. Guilds (`997625:queryGuilds`), excluding current guild when viewing one.
    if matches!(mode, None | Some(QueryMode::Guild)) {
        let q_folded = fold_text(query);
        for guild in &ctx.stores.guilds {
            if Some(guild.id) == ctx.current_guild {
                continue;
            }
            let name_tier = score_string(query, &guild.name);
            let acronym = fold_text(&guild.acronym());
            let acronym_tier =
                if !q_folded.is_empty() && !acronym.is_empty() && acronym.starts_with(&q_folded) {
                    7.0
                } else {
                    0.0
                };
            let tier = name_tier.max(acronym_tier);
            if tier > 0.0 {
                let boost = frecency_boost(ctx.frecency_scores, max_frecency, guild.id);
                guild_matches.push(SwitcherRow::Result {
                    destination: Destination::Guild(guild.id),
                    comparator: Some(guild.name.clone()),
                    score: tier + boost,
                    sortable: fold_text(&guild.name),
                });
            }
        }
        guild_matches.sort_by(compare_results);
    }

    // 5. In-App Navigation (`997625:652-696`) in general mode.
    if mode.is_none() {
        for target in [
            NavTarget::Shop,
            NavTarget::Nitro,
            NavTarget::Quests,
            NavTarget::Settings,
        ] {
            let label = target.label();
            let tier = score_string(query, &label);
            if tier > 0.0 {
                nav_matches.push(SwitcherRow::Result {
                    destination: Destination::InAppNavigation(target),
                    comparator: Some(label.clone()),
                    score: tier,
                    sortable: fold_text(&label),
                });
            }
        }
        nav_matches.sort_by(compare_results);
    }

    let non_empty_categories = [
        !user_matches.is_empty(),
        !group_dm_matches.is_empty(),
        !channel_matches.is_empty(),
        !guild_matches.is_empty(),
        !nav_matches.is_empty(),
    ]
    .into_iter()
    .filter(|&present| present)
    .count();

    let per_category_limit = if mode.is_some() || non_empty_categories <= 1 {
        SINGLE_CATEGORY_LIMIT
    } else {
        GENERAL_CATEGORY_LIMIT
    };

    user_matches.truncate(per_category_limit);
    group_dm_matches.truncate(per_category_limit);
    channel_matches.truncate(per_category_limit);
    guild_matches.truncate(per_category_limit);
    nav_matches.truncate(per_category_limit);

    let mut combined = Vec::new();
    combined.extend(user_matches);
    combined.extend(group_dm_matches);
    combined.extend(channel_matches);
    combined.extend(guild_matches);
    combined.extend(nav_matches);
    combined.sort_by(compare_results);
    combined.truncate(SINGLE_CATEGORY_LIMIT);

    if let Some(prefix_mode) = mode
        && !combined.is_empty()
    {
        let mut with_header = Vec::with_capacity(combined.len() + 1);
        with_header.push(SwitcherRow::Header(prefix_header(
            prefix_mode,
            ctx.stores,
            ctx.current_guild,
        )));
        with_header.extend(combined);
        return with_header;
    }
    combined
}

/// The tutorial arrows: asset, rotation in degrees, its laid-out box (x, y, width, height) relative to the
/// switcher's left and the tutorial's top, measured from the capture.
const ARROWS: [(&str, f32, [f32; 4]); 3] = [
    ("a8546d03debd645f", 0., [0., 118., 43., 18.]),
    ("43a0c71f5cc76261", 40., [-4.8, 24., 51.6, 48.5]),
    ("6c67a101e81dc0ad", 70., [65.3, 0., 34.4, 47.6]),
];

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// The SVG of an arrow turned by `degrees` about its centre (and mirrored for the right side, which the official
/// layout flips with `scaleX(-1)`), sized to the turned box.
pub(super) fn rotated_svg(
    source: &str,
    degrees: f32,
    mirror: bool,
    width: f32,
    height: f32,
) -> Option<String> {
    let start = source.find("<svg")?;
    let open_end = start + source[start..].find('>')?;
    let inner = source[open_end + 1..].trim_end().strip_suffix("</svg>")?;
    let attributes = &source[start..open_end];
    let number = |name: &str| -> Option<f32> {
        let at = attributes.find(&format!(" {name}=\""))? + name.len() + 3;
        attributes[at..]
            .split('"')
            .next()?
            .trim_end_matches("px")
            .parse()
            .ok()
    };
    let (own_width, own_height) = (number("width")?, number("height")?);
    let view_box = attributes
        .find("viewBox=\"")
        .and_then(|at| attributes[at + 9..].split('"').next())
        .unwrap_or("")
        .to_owned();
    let view_box = if view_box.is_empty() {
        format!("0 0 {own_width} {own_height}")
    } else {
        view_box
    };
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><g transform="translate({cx} {cy}) scale({flip} 1) rotate({degrees}) translate({ox} {oy})"><svg width="{own_width}" height="{own_height}" viewBox="{view_box}">{inner}</svg></g></svg>"#,
        flip = if mirror { -1 } else { 1 },
        cx = width / 2.,
        cy = height / 2.,
        ox = -own_width / 2.,
        oy = -own_height / 2.,
    ))
}

/// Mention badge (`320264:x`, `.wrapper_t2U4kv` in `10071.css`):
/// `padding: 3px 6px`, `rounded(px(3.))`, `12px/12px` weight `500`, `--white` on `--background-feedback-notification`.
fn mention_badge(count: u64) -> gpui::Div {
    let label = if count < 1000 {
        count.to_string()
    } else {
        format!("{}K+", (count / 1000).min(9))
    };
    div()
        .ml(px(6.))
        .flex_none()
        .px(px(6.))
        .py(px(3.))
        .rounded(px(3.))
        .bg(color("--background-feedback-notification"))
        .text_size(px(12.))
        .line_height(px(12.))
        .font_weight(FontWeight(500.))
        .text_color(color("--white"))
        .child(label)
}

impl Root {
    fn switcher_draft_channels(&self) -> Vec<Id> {
        let mut out: Vec<Id> = Vec::new();
        let mut push = |ch_id: Id| {
            if !out.contains(&ch_id) {
                out.push(ch_id);
            }
        };
        for (&ch_id, drafts) in &self.slowmode_rejected {
            if drafts.iter().any(|draft| !draft.content.trim().is_empty()) {
                push(ch_id);
            }
        }
        for key in ["drafts", "DraftStore"] {
            if let Some(map) = crate::storage::get(key).and_then(|val| val.as_object().cloned()) {
                for (id_str, entry) in map {
                    let non_empty = entry.as_str().is_some_and(|s| !s.trim().is_empty())
                        || entry["draft"]
                            .as_str()
                            .is_some_and(|s| !s.trim().is_empty());
                    if non_empty && let Ok(ch_id) = id_str.parse::<Id>() {
                        push(ch_id);
                    }
                }
            }
        }
        out
    }

    pub(super) fn switcher_frecency(&self) -> (HashMap<String, f64>, Vec<Id>) {
        let mut frecency = Frecency::new(Kind::Channel);
        let stored = self
            .fetched
            .get(FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(|settings| {
                dc_proto::frecency::id_map(settings, dc_proto::frecency::GUILD_AND_CHANNEL_FRECENCY)
            })
            .unwrap_or_default();
        frecency.overwrite(
            stored
                .into_iter()
                .map(|(key, item)| {
                    (
                        key.to_string(),
                        Usage {
                            total_uses: item.total_uses,
                            recent_uses: item.recent_uses,
                        },
                    )
                })
                .collect(),
        );
        let stamp = now_ms();
        let scores = frecency.scores(stamp);
        let frequent_ids = frecency
            .frequently(stamp, |key| {
                key.parse::<Id>()
                    .ok()
                    .and_then(|id| destination_for_channel_id(&self.stores, id))
                    .is_some()
            })
            .into_iter()
            .filter_map(|key| key.parse::<Id>().ok())
            .collect();
        (scores, frequent_ids)
    }

    pub(super) fn compute_switcher_rows(&self, raw_query: &str) -> Vec<SwitcherRow> {
        let drafts = self.switcher_draft_channels();
        let (frecency_scores, frequent_ids) = self.switcher_frecency();
        let ctx = SwitcherContext {
            stores: &self.stores,
            current_guild: self.guild,
            current_channel: self.channel,
            channel_history: &self.channel_history,
            draft_channels: &drafts,
            frecency_scores: &frecency_scores,
            frequent_ids: &frequent_ids,
            now: chrono::Utc::now(),
        };
        build_switcher_rows(&ctx, raw_query)
    }

    fn request_switcher_images(&mut self, rows: &[SwitcherRow]) {
        let mut requests: Vec<(String, String)> = Vec::new();
        for row in rows {
            let Some(dest) = row.destination() else {
                continue;
            };
            match dest {
                Destination::User(uid) => {
                    if let Some(user) = self.stores.users.get(&uid) {
                        requests.push(avatar_source(user));
                    } else {
                        for guild in &self.stores.guilds {
                            if let Some(member) = self.stores.members.get(guild.id, uid) {
                                requests.push(avatar_source(&member.user));
                                break;
                            }
                        }
                    }
                }
                Destination::GroupDm(id) => {
                    if let Some(dm) = self.stores.private_channels.iter().find(|dm| dm.id == id) {
                        let (key, url) = super::unread_dms::group_icon(dm);
                        if !url.starts_with("data:") {
                            requests.push((key, url));
                        }
                    }
                }
                Destination::VoiceChannel { guild, id } => {
                    for state in self.stores.voice_members(guild, id).into_iter().take(4) {
                        if let Some(user) = self.stores.users.get(&state.user_id) {
                            requests.push(avatar_source(user));
                        }
                    }
                }
                Destination::Guild(gid) => {
                    if let Some(guild) = self.stores.guild(gid)
                        && let Some(hash) = guild.icon.as_deref()
                    {
                        requests.push((
                            format!("guild:{gid}"),
                            format!("https://cdn.discordapp.com/icons/{gid}/{hash}.webp?size=64"),
                        ));
                    }
                }
                Destination::TextChannel { .. } | Destination::InAppNavigation(_) => {}
            }
        }
        for (key, url) in requests {
            self.request_image(key, url);
        }
    }

    fn sync_switcher_query(&mut self, new_query: String) {
        let rows = self.compute_switcher_rows(&new_query);
        let first = first_selectable_index(&rows);
        if let Some(switcher) = self.quick_switcher.as_mut() {
            switcher.query = new_query;
            switcher.selected = first;
            switcher.scroll.scroll_to_item(0);
        }
        self.request_switcher_images(&rows);
    }

    /// `910839` Escape handling: clears a non-empty query first and closes the modal only when the query is empty.
    fn handle_switcher_escape(&mut self, cx: &mut Context<Self>) {
        let non_empty = self
            .quick_switcher
            .as_ref()
            .is_some_and(|switcher| !switcher.query.is_empty());
        if non_empty {
            if let Some(editor) = self.quick_switcher.as_ref().map(|s| s.search.clone()) {
                editor.update(cx, |ed, cx| {
                    ed.set_text("");
                    cx.notify();
                });
            }
            self.sync_switcher_query(String::new());
        } else {
            self.quick_switcher = None;
        }
        cx.notify();
    }

    pub(super) fn open_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_switcher.is_some() {
            self.quick_switcher = None;
            cx.notify();
            return;
        }
        let seen_tutorial = crate::storage::get(TUTORIAL_STORAGE_KEY)
            .and_then(|val| val.as_bool())
            .unwrap_or(false);
        if !seen_tutorial {
            for (asset, _, _) in ARROWS {
                self.request_image(
                    format!("text:qs-arrow-{asset}"),
                    format!("https://discordapp.com/assets/{asset}.svg"),
                );
            }
        }
        let search = cx.new(|cx| {
            // `input_a1DYes`: 22px text, centred in the 70px box.
            let style = crate::editor::EditorStyle {
                size: 22.,
                line_height: 28.,
                ..editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_placeholder_message(dc_intl::t!("VtvewW"));
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let next_query = editor.read(cx).text().to_owned();
                    this.sync_switcher_query(next_query);
                }
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.choose_switcher_result(None, cx);
                    return;
                }
                EditorEvent::Cancel => {
                    this.handle_switcher_escape(cx);
                    return;
                }
            }
            cx.notify();
        });

        let weak = cx.entity().downgrade();
        let interceptor = cx.intercept_keystrokes(move |event, window, cx| {
            let ks = &event.keystroke;
            let is_escape = ks.key == "escape" && !ks.modifiers.modified();
            let is_ctrl_p = ks.modifiers.control
                && !ks.modifiers.alt
                && !ks.modifiers.shift
                && !ks.modifiers.platform
                && ks.key.eq_ignore_ascii_case("p");
            let is_ctrl_n = ks.modifiers.control
                && !ks.modifiers.alt
                && !ks.modifiers.shift
                && !ks.modifiers.platform
                && ks.key.eq_ignore_ascii_case("n");
            let is_open_tab = (ks.modifiers.control || ks.modifiers.platform)
                && !ks.modifiers.alt
                && !ks.modifiers.shift
                && ks.key.eq_ignore_ascii_case("t");
            if !is_escape && !is_ctrl_p && !is_ctrl_n && !is_open_tab {
                return;
            }
            let handled = weak
                .update(cx, |this, cx| {
                    if this.quick_switcher.is_none() {
                        return false;
                    }
                    if is_escape {
                        this.handle_switcher_escape(cx);
                        return true;
                    }
                    if is_open_tab {
                        this.quick_switcher = None;
                        cx.notify();
                        return true;
                    }
                    if is_ctrl_p {
                        return this.switcher_key("up", cx);
                    }
                    if is_ctrl_n {
                        return this.switcher_key("down", cx);
                    }
                    false
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
                window.refresh();
            }
        });

        let initial_rows = self.compute_switcher_rows("");
        let initial_selected = first_selectable_index(&initial_rows);
        self.request_switcher_images(&initial_rows);

        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.quick_switcher = Some(QuickSwitcher {
            search,
            _events: events,
            _interceptor: interceptor,
            scroll: ScrollHandle::new(),
            query: String::new(),
            selected: initial_selected,
            seen_tutorial,
        });
        cx.notify();
    }

    /// Up and Down (plus `Ctrl+P` / `Ctrl+N`) move the highlight, wrapping around and skipping `HEADER` rows;
    /// true when the key was taken.
    pub(super) fn switcher_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let delta = match key {
            "up" => -1,
            "down" => 1,
            _ => return false,
        };
        let Some(query) = self.quick_switcher.as_ref().map(|s| s.query.clone()) else {
            return false;
        };
        let rows = self.compute_switcher_rows(&query);
        let Some(switcher) = self.quick_switcher.as_mut() else {
            return false;
        };
        let Some(next) = step_selection(&rows, switcher.selected, delta) else {
            return false;
        };
        switcher.selected = Some(next);
        switcher.scroll.scroll_to_item(next);
        cx.notify();
        true
    }

    fn choose_switcher_result(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some((query, current_selected)) = self
            .quick_switcher
            .as_ref()
            .map(|s| (s.query.clone(), s.selected))
        else {
            return;
        };
        let (mode, _) = parse_query_mode(&query);
        let rows = self.compute_switcher_rows(&query);
        let Some(chosen_idx) = index.or(current_selected) else {
            return;
        };
        let Some(destination) = rows.get(chosen_idx).and_then(SwitcherRow::destination) else {
            return;
        };

        // Module `174768:286` (`QUICKSWITCHER_SWITCH_TO`): persist `seenQSTutorial` once a result is activated.
        crate::storage::set(TUTORIAL_STORAGE_KEY, serde_json::Value::Bool(true));
        self.quick_switcher = None;

        match destination {
            Destination::User(uid) => {
                self.open_dm_with(uid, cx);
            }
            Destination::GroupDm(channel_id) => {
                self.select_home(Some(channel_id));
                self.request_images();
            }
            Destination::TextChannel { guild, id, .. } => {
                self.select(guild, Some(id));
                self.request_images();
            }
            Destination::VoiceChannel { guild, id } => {
                // Module `793322:39`: in `#` (`TEXT_CHANNEL`) mode, activating a vocal channel opens its
                // text-in-voice chat instead of joining voice.
                if mode == Some(QueryMode::TextChannel) {
                    self.vv.chat_open.insert(id);
                    self.select(guild, Some(id));
                    self.request_images();
                } else {
                    self.request_join_voice(guild, id, cx);
                }
            }
            Destination::Guild(guild) => {
                self.go_guild(guild, cx);
            }
            Destination::InAppNavigation(target) => {
                match target {
                    NavTarget::Shop => self.open_shop(cx),
                    NavTarget::Nitro => self.open_nitro(cx),
                    NavTarget::Quests => self.open_quests(cx),
                    NavTarget::Settings => self.open_settings(cx),
                }
                self.request_images();
            }
        }
        cx.notify();
    }

    fn resolve_user_for_row(&self, uid: Id) -> Option<&User> {
        if let Some(user) = self.stores.users.get(&uid) {
            return Some(user);
        }
        for guild in &self.stores.guilds {
            if let Some(member) = self.stores.members.get(guild.id, uid) {
                return Some(&member.user);
            }
        }
        None
    }

    fn voice_summary_avatars(&self, guild_id: Id, channel_id: Id) -> Option<AnyElement> {
        let states = self.stores.voice_members(guild_id, channel_id);
        if states.is_empty() {
            return None;
        }
        let max_slots = 4usize;
        let visible_users = if states.len() > max_slots {
            max_slots - 1
        } else {
            states.len()
        };
        let overflow = states.len().saturating_sub(visible_users).min(99);
        let mut items: Vec<AnyElement> = Vec::new();
        for (idx, state) in states.iter().take(visible_users).enumerate() {
            let pic = self
                .stores
                .users
                .get(&state.user_id)
                .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
            let is_last = idx + 1 == visible_users && overflow == 0;
            items.push(
                div()
                    .size(px(20.))
                    .when(!is_last, |el| el.mr(px(-4.)))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(color("--background-mod-subtle"))
                    .children(pic.map(|image| img(image).size(px(20.)).rounded_full()))
                    .into_any_element(),
            );
        }
        if overflow > 0 {
            items.push(
                div()
                    .h(px(20.))
                    .pl(px(6.))
                    .pr(px(8.))
                    .rounded(px(10.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-muted"))
                    .child(format!("+{overflow}"))
                    .into_any_element(),
            );
        }
        Some(
            div()
                .ml(px(6.))
                .flex_none()
                .flex()
                .items_center()
                .children(items)
                .into_any_element(),
        )
    }

    fn switcher_header_element(&self, index: usize, text: &str) -> AnyElement {
        div()
            .id(SharedString::from(format!("switcher-header-{index}")))
            .h(px(34.))
            .flex_none()
            .px(px(10.))
            .pt(px(4.))
            .text_size(px(14.))
            .line_height(px(30.))
            .font_weight(FontWeight(500.))
            .text_color(color("--interactive-text-default"))
            .child(text.to_owned())
            .into_any_element()
    }

    fn switcher_result_row(
        &self,
        index: usize,
        first_selectable: Option<usize>,
        destination: Destination,
        comparator: Option<&str>,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = chrono::Utc::now();
        let ring = if selected {
            "--interactive-background-selected"
        } else {
            "--background-surface-high"
        };

        let mut unread = false;
        let mut mentions: u64 = 0;
        let mut voice_summary: Option<AnyElement> = None;

        let (leading, name, note, misc): (AnyElement, String, Option<String>, Option<String>) =
            match destination {
                Destination::User(uid) => {
                    let user = self.resolve_user_for_row(uid);
                    let dm = find_dm_for_user(&self.stores, uid);
                    mentions = dm.map_or(0, |ch| self.stores.read_states.mentions(ch.id));
                    unread = mentions > 0;
                    let picture = user.and_then(|u| self.images.get(&avatar_source(u).0).cloned());
                    let status = self.stores.status(uid);
                    let friend_nick = self.nickname_for(uid);
                    let display = user
                        .map(|u| user_display_nickname(u, friend_nick, comparator))
                        .unwrap_or_else(|| uid.to_string());
                    let handle = user.map(|u| u.username.clone());
                    let avatar_el = div()
                        .relative()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .bg(color("--background-mod-subtle"))
                        .children(picture.map(|pic| img(pic).size(px(20.)).rounded_full()))
                        .child(status_dot_small(status, ring))
                        .into_any_element();
                    (avatar_el, display, None, handle)
                }
                Destination::GroupDm(ch_id) => {
                    let dm = self
                        .stores
                        .private_channels
                        .iter()
                        .find(|dm| dm.id == ch_id);
                    mentions = self.stores.read_states.mentions(ch_id);
                    unread = mentions > 0;
                    let picture = dm.and_then(|ch| {
                        let (key, _) = super::unread_dms::group_icon(ch);
                        self.images.get(&key).cloned()
                    });
                    let title = dm
                        .map(|ch| self.private_channel_name(ch))
                        .unwrap_or_default();
                    let icon_el = div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .overflow_hidden()
                        .bg(color("--background-mod-subtle"))
                        .children(picture.map(|pic| img(pic).size(px(20.)).rounded_full()))
                        .into_any_element();
                    (icon_el, title, None, None)
                }
                Destination::TextChannel {
                    guild: guild_id,
                    id,
                    is_thread,
                } => {
                    let guild = self.stores.guild(guild_id);
                    mentions = self.stores.read_states.mentions(id);
                    if is_thread {
                        let thread: Option<&Thread> =
                            guild.and_then(|g| g.threads.iter().find(|t| t.id == id));
                        let thread_muted = thread.is_some_and(|t| {
                            dc_state::guild_settings::mute_active(t.muted, t.mute_end, now)
                        });
                        unread = mentions > 0 || (!thread_muted && self.stores.is_unread(id));
                        let parent_name = thread
                            .and_then(|t| guild?.channels.iter().find(|c| c.id == t.parent_id))
                            .map(|parent| parent.name.clone());
                        (
                            div()
                                .w(px(20.))
                                .flex_none()
                                .flex()
                                .justify_center()
                                .child(icon("threads", 16., "--interactive-text-default"))
                                .into_any_element(),
                            thread.map(|t| t.name.clone()).unwrap_or_default(),
                            parent_name,
                            guild.map(|g| g.name.clone()),
                        )
                    } else {
                        let known = guild.and_then(|g| g.channels.iter().find(|c| c.id == id));
                        let muted = self.stores.guild_settings.channel_muted(guild_id, id, now);
                        unread = mentions > 0 || (!muted && self.stores.is_unread(id));
                        let category = known
                            .and_then(|c| c.parent_id)
                            .and_then(|parent| guild?.channels.iter().find(|c| c.id == parent))
                            .map(|cat| cat.name.clone());
                        let glyph = known
                            .map(|ch| {
                                let rules = guild
                                    .and_then(|g| g.rules_channel_id)
                                    .is_some_and(|rid| rid == ch.id);
                                let facts = channel_icon::Facts {
                                    kind: ch.kind,
                                    rules,
                                    nsfw: ch.nsfw,
                                    private: guild.is_some_and(|g| permissions::is_private(g, ch)),
                                    pending: guild.is_some_and(permissions::pending_gate),
                                    locked: false,
                                    active_threads: false,
                                };
                                channel_icon::name(&facts)
                            })
                            .unwrap_or("text_channel");
                        (
                            div()
                                .w(px(20.))
                                .flex_none()
                                .flex()
                                .justify_center()
                                .child(icon(glyph, 16., "--interactive-text-default"))
                                .into_any_element(),
                            known.map(|c| c.name.clone()).unwrap_or_default(),
                            category,
                            guild.map(|g| g.name.clone()),
                        )
                    }
                }
                Destination::VoiceChannel {
                    guild: guild_id,
                    id,
                } => {
                    // Module `320264`: voice channels are NEVER styled unread and never render mention badges.
                    let guild = self.stores.guild(guild_id);
                    let known = guild.and_then(|g| g.channels.iter().find(|c| c.id == id));
                    voice_summary = self.voice_summary_avatars(guild_id, id);
                    let glyph = known
                        .map(|ch| {
                            if ch.kind == STAGE_KIND {
                                "stage_channel"
                            } else {
                                "voice_channel"
                            }
                        })
                        .unwrap_or("voice_channel");
                    (
                        div()
                            .w(px(20.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(icon(glyph, 16., "--interactive-text-default"))
                            .into_any_element(),
                        known.map(|c| c.name.clone()).unwrap_or_default(),
                        None,
                        guild.map(|g| g.name.clone()),
                    )
                }
                Destination::Guild(guild_id) => {
                    let known = self.stores.guild(guild_id);
                    unread = known.is_some_and(|g| self.stores.guild_unread(g));
                    let picture = self.images.get(&format!("guild:{guild_id}")).cloned();
                    let acronym = known.map(Guild::acronym).unwrap_or_default();
                    let leading_el = div()
                        .size(px(20.))
                        .flex_none()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(color("--background-mod-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .when(picture.is_none() && !acronym.is_empty(), |el| {
                            el.child(acronym.chars().take(3).collect::<String>())
                        })
                        .children(picture.map(|pic| img(pic).size(px(20.))))
                        .into_any_element();
                    (
                        leading_el,
                        known.map(|g| g.name.clone()).unwrap_or_default(),
                        Some(dc_intl::t!("5qyruI").to_owned()),
                        None,
                    )
                }
                Destination::InAppNavigation(target) => (
                    div()
                        .w(px(20.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(icon(target.icon_name(), 16., "--interactive-text-default"))
                        .into_any_element(),
                    target.label(),
                    None,
                    None,
                ),
            };

        div()
            .when(Some(index) == first_selectable, |row| {
                row.parity("switcher-first-result")
            })
            .id(SharedString::from(format!("switcher-row-{index}")))
            .h(px(34.))
            .flex_none()
            .px(px(10.))
            .rounded(px(3.))
            .cursor_pointer()
            .when(selected, |this| {
                this.bg(color("--interactive-background-selected"))
            })
            .flex()
            .items_center()
            .text_color(if unread {
                color("--text-strong")
            } else {
                color("--interactive-text-default")
            })
            .font_weight(FontWeight(500.))
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside && let Some(switcher) = this.quick_switcher.as_mut() {
                    switcher.selected = Some(index);
                    cx.notify();
                }
            }))
            .on_click(
                cx.listener(move |this, _, _, cx| this.choose_switcher_result(Some(index), cx)),
            )
            .child(leading)
            .child(
                div()
                    .ml(px(5.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .line_height(px(34.))
                    .child(
                        div()
                            .truncate()
                            .text_color(color("--interactive-text-default"))
                            .when(unread, |this| this.text_color(color("--text-strong")))
                            .child(name),
                    )
                    .children(note.map(|note| {
                        div()
                            .flex_none()
                            .mt(px(1.))
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(note)
                    })),
            )
            .children(voice_summary)
            .children(misc.map(|misc| {
                div()
                    .ml(px(4.))
                    .flex_none()
                    .max_w(px(140.))
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(400.))
                    .opacity(0.6)
                    .child(misc)
            }))
            .children((mentions > 0).then(|| mention_badge(mentions)))
            .into_any_element()
    }

    fn arrow_image(
        &self,
        asset: &str,
        degrees: f32,
        mirror: bool,
        size: (f32, f32),
    ) -> Option<Arc<Image>> {
        let source = self.texts.get(&format!("text:qs-arrow-{asset}"))?;
        let svg = rotated_svg(source, degrees, mirror, size.0, size.1)?;
        Some(Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            svg.into_bytes(),
        )))
    }

    pub(super) fn quick_switcher_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let switcher = self.quick_switcher.as_ref()?;
        let rows_data = self.compute_switcher_rows(&switcher.query);
        let first_selectable = first_selectable_index(&rows_data);
        let selected_idx = switcher
            .selected
            .filter(|&idx| idx < rows_data.len() && !rows_data[idx].is_header())
            .or(first_selectable);

        let empty_query = switcher.query.trim().is_empty();
        let has_selectable = first_selectable.is_some();
        let show_empty_state = !empty_query && !has_selectable;
        let has_query_results = !empty_query && has_selectable;

        let viewport = window.viewport_size();
        let height = 398.42188_f32;
        let top = ((f32::from(viewport.height) - height) / 2.).max(8.);

        let row_elements: Vec<AnyElement> = rows_data
            .iter()
            .enumerate()
            .map(|(index, row)| match row {
                SwitcherRow::Header(title) => self.switcher_header_element(index, title),
                SwitcherRow::Result {
                    destination,
                    comparator,
                    ..
                } => self.switcher_result_row(
                    index,
                    first_selectable,
                    *destination,
                    comparator.as_deref(),
                    Some(index) == selected_idx,
                    cx,
                ),
            })
            .collect();

        let symbol = |text: &'static str| {
            div()
                .mx(px(1.5))
                .p(px(2.))
                .rounded(px(4.))
                .bg(color("--background-base-lowest"))
                .text_size(px(12.))
                .line_height(px(16.))
                .child(text)
        };

        let show_tutorial = !switcher.seen_tutorial;
        let arrows: Vec<AnyElement> = if show_tutorial {
            ARROWS
                .iter()
                .filter(|(asset, _, _)| !has_query_results || *asset == ARROWS[0].0)
                .flat_map(|(asset, degrees, [x, y, width, height])| {
                    [false, true].into_iter().filter_map(move |right| {
                        let picture =
                            self.arrow_image(asset, *degrees, right, (*width, *height))?;
                        let base_x = if right {
                            678. + 60. - x - width
                        } else {
                            -60. + x
                        };
                        Some(
                            div()
                                .absolute()
                                .left(px(base_x))
                                .top(px(9. - 1. - 80.
                                    + y
                                    + if has_query_results {
                                        // 910839 moves the horizontal arrow nine pixels below the selected row's top.
                                        (69. + selected_idx.unwrap_or(0) as f32 * 34.).min(250.)
                                    } else {
                                        0.
                                    }))
                                .child(img(picture).w(px(*width)).h(px(*height)))
                                .into_any_element(),
                        )
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let caption = show_tutorial.then(|| {
            div()
                .absolute()
                .left(px(-61.))
                .right(px(-61.))
                .top(px(9. - 1. - 80. + 40.))
                .h(px(24.))
                .flex()
                .justify_center()
                .text_size(px(18.))
                .line_height(px(24.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(if !has_query_results {
                    dc_intl::tr("Search for servers, channels or DMs")
                } else {
                    dc_intl::tr("Select a result and press ENTER to jump to it")
                })
        });

        // Results area (`910839`): either the `262px` centered empty state (`910839:X`) or the `236px` scroller
        // (`16px` top margin + `236px` scroller + `10px` bottom margin = `262px`).
        let results_area: AnyElement = if show_empty_state {
            div()
                .id("switcher-empty-state")
                .h(px(262.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .text_center()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t!("bpbATE")),
                )
                .child(
                    div()
                        .id("switcher-empty-learn-more")
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-link"))
                        .on_click(|_, _, cx| cx.open_url(HELP_ARTICLE_URL))
                        .child(dc_intl::t!("4iPfEO")),
                )
                .into_any_element()
        } else {
            div()
                .id("switcher-results")
                .track_scroll(&switcher.scroll)
                .h(px(236.))
                .mr(px(-17.))
                .pr(px(16.))
                .mt(px(16.))
                .mb(px(10.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .children(row_elements)
                .into_any_element()
        };

        let pose = super::motion::modal_pose("quick-switcher");
        let modal_w = 680_f32.min((self.viewport.0 - 48.).max(320.));
        let left = ((self.viewport.0 - modal_w) / 2.).max(24.);
        let top = top.clamp(56., (self.viewport.1 - 410.).max(56.));
        let modal = div()
            .parity_with_border("quick-switcher", 1.)
            .id("quick-switcher")
            .posed(pose)
            .occlude()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(modal_w))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![super::shadow_high()])
            .pt(px(8.))
            .pb(px(16.))
            .children(caption)
            .children(arrows)
            .child(
                div()
                    .pt(px(12.))
                    .px(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .parity_with_border("switcher-input", 1.)
                            .h(px(70.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .text_size(px(22.))
                            .child(div().flex_1().child(switcher.search.clone())),
                    )
                    .child(results_area)
                    .child(
                        div()
                            .h(px(28.421875))
                            .pt(px(10.))
                            .when(!empty_query, |this| {
                                this.border_t_1().border_color(color("--border-subtle"))
                            })
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-strong"))
                            .child(
                                div()
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-feedback-positive"))
                                    .child(dc_intl::tr("Protip:")),
                            )
                            .child(div().ml(px(3.)).child(protip_part(0)))
                            .child(symbol("@"))
                            .child(symbol("#"))
                            .child(symbol("!"))
                            .child(symbol("*"))
                            .child(symbol("$"))
                            .child(div().ml(px(2.)).child(protip_part(1)))
                            .child(
                                div()
                                    .id("switcher-learn-more")
                                    .ml(px(3.))
                                    .cursor_pointer()
                                    .text_color(color("--text-link"))
                                    .on_click(|_, _, cx| cx.open_url(HELP_ARTICLE_URL))
                                    .child(dc_intl::tr("Learn more")),
                            ),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("quick-switcher-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quick_switcher = None;
                        cx.notify();
                    }))
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Destination, NavTarget, QueryMode, SwitcherContext, SwitcherRow, build_switcher_rows,
        matched_user_name, parse_query_mode, rotated_svg, score_string, step_selection,
        user_display_nickname,
    };
    use dc_state::{Stores, User};
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_stores() -> Stores {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": { "id": "999", "username": "me_self" },
                "users": [
                    { "id": "1", "username": "alice_w", "global_name": "Alice Wonder" },
                    { "id": "2", "username": "bob_other", "global_name": "Bob Builder" },
                    { "id": "3", "username": "blocked_guy", "global_name": "Blocked Guy" },
                    { "id": "4", "username": "ignored_gal", "global_name": "Ignored Gal" },
                    { "id": "5", "username": "outside_user", "global_name": "Outside Person" }
                ],
                "relationships": [
                    { "id": "1", "type": 1, "nickname": "Ally" },
                    { "id": "3", "type": 2 },
                    { "id": "4", "type": 3, "user_ignored": true }
                ],
                "private_channels": [
                    { "id": "5001", "type": 1, "recipient_ids": ["1"], "last_message_id": "9001" },
                    { "id": "5002", "type": 3, "name": "Weekend Squad", "recipient_ids": ["1", "2"], "last_message_id": "9002" }
                ],
                "guilds": [
                    {
                        "id": "100",
                        "name": "Rust Programming Language",
                        "joined_at": "2024-01-01T00:00:00Z",
                        "channels": [
                            { "id": "101", "type": 0, "name": "general-chat", "position": 0 },
                            { "id": "102", "type": 5, "name": "news-feed", "position": 1 },
                            { "id": "103", "type": 15, "name": "help-forum", "position": 2 },
                            { "id": "104", "type": 16, "name": "clips-media", "position": 3 },
                            { "id": "105", "type": 2, "name": "general-voice", "position": 4 },
                            { "id": "106", "type": 13, "name": "town-hall-stage", "position": 5 }
                        ],
                        "threads": [
                            {
                                "id": "107",
                                "parent_id": "101",
                                "type": 11,
                                "name": "async-traits-thread",
                                "member": { "flags": 2 },
                                "thread_metadata": { "archived": false }
                            }
                        ],
                        "members": [
                            {
                                "user": { "id": "2", "username": "bob_other", "global_name": "Bob Builder" },
                                "nick": "ServerCaptain",
                                "roles": []
                            }
                        ]
                    },
                    {
                        "id": "200",
                        "name": "Second Server",
                        "joined_at": "2024-01-01T00:00:00Z",
                        "channels": [
                            { "id": "201", "type": 0, "name": "general-chat", "position": 0 }
                        ],
                        "members": [
                            {
                                "user": { "id": "5", "username": "outside_user", "global_name": "Outside Person" },
                                "nick": "RemoteNick",
                                "roles": []
                            }
                        ]
                    }
                ],
                "read_state": {
                    "entries": [
                        { "id": "102", "mention_count": 3, "last_message_id": "1" },
                        { "id": "103", "mention_count": 0, "last_message_id": "1" }
                    ]
                }
            }),
        );
        // Populate guild members via GUILD_MEMBERS_CHUNK (`stores.members`).
        stores.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({
                "guild_id": "100",
                "members": [
                    {
                        "user": { "id": "2", "username": "bob_other", "global_name": "Bob Builder" },
                        "nick": "ServerCaptain",
                        "roles": []
                    }
                ]
            }),
        );
        stores.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({
                "guild_id": "200",
                "members": [
                    {
                        "user": { "id": "5", "username": "outside_user", "global_name": "Outside Person" },
                        "nick": "RemoteNick",
                        "roles": []
                    }
                ]
            }),
        );
        // Mark 103 unread via a newer message.
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({
                "id": "999999999999999999",
                "channel_id": "103",
                "guild_id": "100",
                "content": "new forum post",
                "timestamp": "2026-01-01T00:00:00Z",
                "author": { "id": "2", "username": "bob_other" }
            }),
        );
        stores
    }

    #[test]
    fn user_search_keeps_the_best_name_and_stable_ties() {
        let user = User {
            username: "an_alice".into(),
            global_name: Some("Alice Example".into()),
            ..Default::default()
        };
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "alice"),
            Some("Alice Friend")
        );
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "example"),
            Some("Alice Example")
        );
        assert_eq!(
            matched_user_name(&user, Some("an_alias"), "an_"),
            Some("an_alice")
        );
        assert_eq!(
            matched_user_name(&user, Some("Alice Friend"), "friend"),
            Some("Alice Friend")
        );
        assert_eq!(matched_user_name(&user, None, "friend"), None);
        assert_eq!(matched_user_name(&user, Some(""), "friend"), None);
    }

    #[test]
    fn string_scorer_covers_exact_prefix_stripped_substring_and_fuzzy_tiers() {
        assert_eq!(score_string("general-chat", "general-chat"), 10.0);
        assert_eq!(score_string("general", "general-chat"), 9.0);
        assert_eq!(score_string("generalchat", "general-chat"), 8.0);
        assert_eq!(score_string("ral-ch", "general-chat"), 7.0);
        assert_eq!(score_string("ralch", "general-chat"), 6.0);
        let fuzzy_score = score_string("gncht", "general-chat");
        assert!(fuzzy_score > 0.0 && fuzzy_score < 5.0);
        // Accent folding (`strip_marks`).
        assert_eq!(score_string("cafe", "café-lounge"), 9.0);
    }

    #[test]
    fn prefix_modes_filter_users_channels_threads_voice_and_guilds() {
        let stores = sample_stores();
        let frecency = HashMap::new();
        let ctx = SwitcherContext {
            stores: &stores,
            current_guild: Some(100),
            current_channel: Some(101),
            channel_history: &[5001],
            draft_channels: &[104],
            frecency_scores: &frecency,
            frequent_ids: &[],
            now: chrono::Utc::now(),
        };

        assert_eq!(
            parse_query_mode("@@rem"),
            (Some(QueryMode::UserGlobal), "rem")
        );
        assert_eq!(parse_query_mode("@cap"), (Some(QueryMode::User), "cap"));

        // `@` inside guild 100 restricts to friends + members of guild 100 (excludes user 5 in guild 200),
        // and excludes blocked (3), ignored (4), and self (999).
        let local_remote = build_switcher_rows(&ctx, "@RemoteNick");
        assert!(local_remote.is_empty());

        // `@@` searches all indexed users across all guilds and finds user 5 by guild 200 nickname!
        let global_remote = build_switcher_rows(&ctx, "@@RemoteNick");
        assert!(matches!(
            &global_remote[0],
            SwitcherRow::Header(h) if h == "Searching All Users"
        ));
        assert!(matches!(
            &global_remote[1],
            SwitcherRow::Result {
                destination: Destination::User(5),
                comparator: Some(comp),
                ..
            } if comp == "RemoteNick"
        ));
        let u5 = stores.users.get(&5).unwrap();
        assert_eq!(
            user_display_nickname(u5, None, Some("RemoteNick")),
            "RemoteNick"
        );

        // `@` inside guild 100 prepends `"Search friends and users in: Rust Programming Language"` and matches guild nick.
        let local_captain = build_switcher_rows(&ctx, "@Captain");
        assert!(matches!(
            &local_captain[0],
            SwitcherRow::Header(h) if h == "Search friends and users in: Rust Programming Language"
        ));
        assert_eq!(local_captain[1].destination(), Some(Destination::User(2)));

        // `#` searches text, announcement (5), forum (15), media (16), and active threads!
        let thread_hit = build_switcher_rows(&ctx, "#async-traits");
        assert!(matches!(
            &thread_hit[0],
            SwitcherRow::Header(h) if h == "Searching Text Channels"
        ));
        assert_eq!(
            thread_hit[1].destination(),
            Some(Destination::TextChannel {
                guild: 100,
                id: 107,
                is_thread: true,
            })
        );

        // `!` searches voice (2) + stage (13).
        let stage_hit = build_switcher_rows(&ctx, "!town-hall");
        assert!(matches!(
            &stage_hit[0],
            SwitcherRow::Header(h) if h == "Searching Voice Channels"
        ));
        assert_eq!(
            stage_hit[1].destination(),
            Some(Destination::VoiceChannel {
                guild: 100,
                id: 106,
            })
        );

        // `*` excludes current guild (100) and finds guild 200; acronym match works when viewing Home!
        let star_rows = build_switcher_rows(&ctx, "*");
        assert!(
            !star_rows
                .iter()
                .any(|r| r.destination() == Some(Destination::Guild(100)))
        );
        assert!(
            star_rows
                .iter()
                .any(|r| r.destination() == Some(Destination::Guild(200)))
        );

        let home_ctx = SwitcherContext {
            current_guild: None,
            ..ctx
        };
        let acronym_rows = build_switcher_rows(&home_ctx, "*rpl");
        assert_eq!(acronym_rows[1].destination(), Some(Destination::Guild(100)));
    }

    #[test]
    fn general_mode_ranks_current_guild_boost_group_dms_and_in_app_nav() {
        let stores = sample_stores();
        let frecency = HashMap::new();
        let ctx = SwitcherContext {
            stores: &stores,
            current_guild: Some(100),
            current_channel: None,
            channel_history: &[],
            draft_channels: &[],
            frecency_scores: &frecency,
            frequent_ids: &[],
            now: chrono::Utc::now(),
        };

        // Current-guild `#general-chat` (101) beats other-guild `#general-chat` (201) via +0.5 boost,
        // and both beat vocal `general-voice` (105) due to general-mode vocal penalty!
        let general_rows = build_switcher_rows(&ctx, "general");
        let pos_101 = general_rows
            .iter()
            .position(|r| {
                r.destination()
                    == Some(Destination::TextChannel {
                        guild: 100,
                        id: 101,
                        is_thread: false,
                    })
            })
            .unwrap();
        let pos_201 = general_rows
            .iter()
            .position(|r| {
                r.destination()
                    == Some(Destination::TextChannel {
                        guild: 200,
                        id: 201,
                        is_thread: false,
                    })
            })
            .unwrap();
        let pos_105 = general_rows
            .iter()
            .position(|r| {
                r.destination()
                    == Some(Destination::VoiceChannel {
                        guild: 100,
                        id: 105,
                    })
            })
            .unwrap();
        assert!(pos_101 < pos_201);
        assert!(pos_201 < pos_105);

        // Group DM matches explicit group name and recipient names (capped at 5).
        let squad = build_switcher_rows(&ctx, "Weekend Squad");
        assert!(
            squad
                .iter()
                .any(|r| r.destination() == Some(Destination::GroupDm(5002)))
        );

        // In-app navigation matches Shop / Nitro / Quests / Settings in general mode.
        let nitro = build_switcher_rows(&ctx, "Nitro");
        assert!(
            nitro
                .iter()
                .any(|r| r.destination() == Some(Destination::InAppNavigation(NavTarget::Nitro)))
        );
    }

    #[test]
    fn empty_query_builds_previous_drafts_mentions_and_unread_sections_and_skips_headers_on_step() {
        let stores = sample_stores();
        let frecency = HashMap::new();
        let ctx = SwitcherContext {
            stores: &stores,
            current_guild: Some(100),
            current_channel: Some(101),
            channel_history: &[5001],
            draft_channels: &[104],
            frecency_scores: &frecency,
            frequent_ids: &[],
            now: chrono::Utc::now(),
        };
        let rows = build_switcher_rows(&ctx, "");
        let headers: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                SwitcherRow::Header(h) => Some(h.as_str()),
                SwitcherRow::Result { .. } => None,
            })
            .collect();
        assert_eq!(
            headers,
            vec!["Previous Channels", "Drafts", "Mentions", "Unread Channels"]
        );

        // Wrap-around keyboard stepping skips every `Header` row.
        let first = step_selection(&rows, None, 1).unwrap();
        assert!(!rows[first].is_header());
        let wrapped_up = step_selection(&rows, Some(first), -1).unwrap();
        assert!(!rows[wrapped_up].is_header());
        assert!(wrapped_up > first);
        let wrapped_down = step_selection(&rows, Some(wrapped_up), 1).unwrap();
        assert_eq!(wrapped_down, first);
    }

    #[test]
    fn rotates_inside_a_box_of_its_own() {
        let source = r#"<svg width="43" height="18" viewBox="0 0 43 18" xmlns="http://www.w3.org/2000/svg"><path d="M0 0h1"/></svg>"#;
        let out = rotated_svg(source, 40., false, 51.6, 48.5).unwrap();
        assert!(out.contains("rotate(40)"));
        assert!(out.contains(r#"<path d="M0 0h1"/>"#));
        assert!(out.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="51.6""#));
    }
}
