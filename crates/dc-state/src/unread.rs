//! ReadStateStore mention importance and GuildReadStateStore counters (218461/458294/543465).
use crate::read_state::Importance;
use crate::{Channel, ChannelKind, Guild, Id, Stores, guild_settings, notifications, permissions};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub mentions: u64,
    pub high_mentions: u64,
    pub unread: bool,
}

/// One cached `guild_counts` answer and what it was computed for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CountsEntry {
    now_ms: i64,
    channels: usize,
    threads: usize,
    stamp: u64,
    counts: Counts,
}

/// A fingerprint of what the counts read from stores that callers change directly (the guild's settings entry, the
/// read states of its channels, the channels' own flags), so such a change shows without waiting for an event.
fn stamp(stores: &Stores, guild: &Guild) -> u64 {
    use std::hash::{Hash, Hasher};
    let hash = |value: &dyn Fn(&mut std::collections::hash_map::DefaultHasher)| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value(&mut hasher);
        hasher.finish()
    };
    let mut total = hash(&|h| {
        stores.notification_flags.hash(h);
        stores.account.nsfw_allowed.hash(h);
        guild.nsfw_level.hash(h);
        guild.my_roles.hash(h);
        guild.joined_at.hash(h);
    });
    if let Some(settings) = stores.guild_settings.guilds.get(&guild.id) {
        total = total.wrapping_add(hash(&|h| {
            (
                settings.flags,
                settings.message_notifications,
                settings.muted,
            )
                .hash(h);
            (
                settings.mute_end,
                settings.suppress_everyone,
                settings.suppress_roles,
                settings.mute_scheduled_events,
            )
                .hash(h);
        }));
        for (id, entry) in &settings.channels {
            total = total.wrapping_add(hash(&|h| {
                (
                    id,
                    entry.flags,
                    entry.message_notifications,
                    entry.muted,
                    entry.mute_end,
                )
                    .hash(h);
            }));
        }
    }
    for channel in &guild.channels {
        total = total.wrapping_add(hash(&|h| {
            (channel.id, channel.nsfw, channel.flags, channel.parent_id).hash(h);
            (
                channel.overwrites.len(),
                channel.kind == ChannelKind::Category,
            )
                .hash(h);
        }));
    }
    let ids = guild
        .channels
        .iter()
        .map(|channel| channel.id)
        .chain(guild.threads.iter().map(|thread| thread.id));
    for id in ids {
        let state = stores.read_states.states.get(&id);
        let latest = stores.read_states.latest.get(&id);
        total = total.wrapping_add(hash(&|h| {
            (
                id,
                state.map(|s| (s.acked, stores.read_states.mentions(id), s.low_importance)),
                latest,
            )
                .hash(h);
        }));
    }
    for thread in &guild.threads {
        total = total.wrapping_add(hash(&|h| {
            (
                thread.id,
                thread.joined,
                thread.archived,
                thread.member_flags,
            )
                .hash(h);
            (thread.muted, thread.mute_end).hash(h);
        }));
    }
    let events = stores.read_states.typed.get(&(guild.id, 1));
    total = total.wrapping_add(hash(&|h| {
        events.map(|s| (s.acked, s.last, s.mention_count)).hash(h);
    }));
    total
}

pub(crate) fn muted(
    stores: &Stores,
    guild: Id,
    channel: Id,
    parent: Option<Id>,
    now: DateTime<Utc>,
) -> bool {
    let settings = stores.guild_settings.guilds.get(&guild);
    settings.is_some_and(|s| guild_settings::mute_active(s.muted, s.mute_end, now))
        || [Some(channel), parent].into_iter().flatten().any(|id| {
            settings
                .and_then(|s| s.channels.get(&id))
                .is_some_and(|s| guild_settings::mute_active(s.muted, s.mute_end, now))
        })
}

pub(crate) fn message_level(stores: &Stores, guild: &Guild, channel: &Channel) -> u64 {
    let settings = stores.guild_settings.guilds.get(&guild.id);
    [Some(channel.id), channel.parent_id]
        .into_iter()
        .flatten()
        .filter_map(|id| settings?.channels.get(&id).map(|s| s.message_notifications))
        .chain(settings.map(|s| s.message_notifications))
        .find(|level| *level != guild_settings::INHERIT)
        .unwrap_or(guild.default_message_notifications)
}

fn all_unreads(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    if stores.notification_flags & 16 == 0 {
        return true;
    }
    let settings = stores.guild_settings.guilds.get(&guild.id);
    for id in [Some(channel.id), channel.parent_id].into_iter().flatten() {
        let flags = settings
            .and_then(|s| s.channels.get(&id))
            .map_or(0, |s| s.flags);
        if flags & 1024 != 0 {
            return true;
        }
        if flags & 512 != 0 {
            return false;
        }
    }
    let flags = settings.map_or(0, |s| s.flags);
    if flags & 2048 != 0 {
        return true;
    }
    if flags & 4096 != 0 {
        return false;
    }
    message_level(stores, guild, channel) == guild_settings::ALL_MESSAGES
}

/// 395504 `WW`: opt-in is on for the guild, and the guild is a community (or the account is staff).
fn opt_in_enabled(stores: &Stores, guild: &Guild) -> bool {
    stores
        .guild_settings
        .guilds
        .get(&guild.id)
        .is_some_and(|s| s.flags & 16384 != 0)
        && (guild.has_feature("COMMUNITY")
            || stores
                .current_user
                .as_ref()
                .is_some_and(|user| user.has_flag(crate::USER_FLAG_STAFF)))
}

/// 543465 `isChannelRecordOrParentOptedIn`.
fn opted_in(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    let settings = stores.guild_settings.guilds.get(&guild.id);
    [Some(channel.id), channel.parent_id]
        .into_iter()
        .flatten()
        .any(|id| {
            settings
                .and_then(|s| s.channels.get(&id))
                .is_some_and(|s| s.flags & 4096 != 0)
        })
}

const DAY_MS: i64 = 86_400_000;

/// 218461 `getGuildChannelUnreadState`, first clause: with opt-in on, a channel whose newest message is over seven
/// days old, or that is not opted in, not visited and read within three days and has no mention, counts for nothing.
/// `getGuildRecentsDismissedAt` lives in the app and is not known here: it is taken as never dismissed.
fn opt_in_zeroes(stores: &Stores, guild: &Guild, channel: &Channel, now_ms: i64) -> bool {
    let timestamp = |id: Id| crate::read_state::snowflake_ms(id) as i64;
    let latest = stores
        .read_states
        .latest
        .get(&channel.id)
        .map_or(0, |id| timestamp(*id));
    if latest < now_ms - 7 * DAY_MS {
        return true;
    }
    let recently_visited = latest > 0
        && stores
            .read_states
            .states
            .get(&channel.id)
            .is_some_and(|state| state.acked != 0 && timestamp(state.acked) > now_ms - 3 * DAY_MS);
    !opted_in(stores, guild, channel)
        && !recently_visited
        && stores.read_states.mentions(channel.id) == 0
}

fn allowed_age(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    stores.account.nsfw_allowed || (!channel.nsfw && !matches!(guild.nsfw_level, 1 | 3))
}

fn mention_with(
    stores: &Stores,
    channel: Id,
    kind: u64,
    blocked_or_ignored: bool,
    now: DateTime<Utc>,
    mentions_me: impl FnOnce(&[Id], bool, bool) -> bool,
) -> Option<Importance> {
    if blocked_or_ignored {
        return None;
    }
    if let Some(guild) = stores.guild_of_channel(channel) {
        let thread = guild.threads.iter().find(|t| t.id == channel);
        let parent = thread.map_or(channel, |t| t.parent_id);
        let target = guild.channels.iter().find(|c| c.id == parent)?;
        if !allowed_age(stores, guild, target) {
            return None;
        }
        let settings = stores.guild_settings.guilds.get(&guild.id);
        if mentions_me(
            &guild.my_roles,
            settings.is_some_and(|s| s.suppress_everyone),
            settings.is_some_and(|s| s.suppress_roles),
        ) {
            return Some(Importance::High);
        }
        if stores.notification_flags & 32 != 0 {
            // 218461 MESSAGE_CREATE: a thread uses its own member level, anything else the channel's.
            let low = match thread {
                Some(thread) => {
                    notifications::thread_level(stores, guild, thread, now)
                        == crate::THREAD_ALL_MESSAGES
                }
                None => {
                    !matches!(target.kind, ChannelKind::Voice | ChannelKind::Other(13))
                        && !muted(stores, guild.id, channel, target.parent_id, now)
                        && message_level(stores, guild, target) == guild_settings::ALL_MESSAGES
                }
            };
            if low {
                return Some(Importance::Low);
            }
        }
    } else {
        let private = stores.private_channels.iter().find(|c| c.id == channel);
        if private.is_some_and(|c| c.kind == 3) && kind == 2 {
            return None;
        }
        if mentions_me(&[], false, false)
            || (private.is_some() && !muted(stores, 0, channel, None, now))
        {
            return Some(Importance::High);
        }
    }
    None
}

/// Badge mentions differ from desktop notifications: muted channels can still contain direct pings.
pub fn message_mention(stores: &Stores, message: &Value, now: DateTime<Utc>) -> Option<Importance> {
    let me = stores.current_user.as_ref()?.id;
    let channel = crate::id(&message["channel_id"])?;
    crate::id(&message["author"]["id"])?;
    let kind = message["type"].as_u64().unwrap_or(0);
    let blocked = notifications::blocked_or_ignored_for_message(stores, message);
    mention_with(stores, channel, kind, blocked, now, |roles, se, sr| {
        notifications::mentions_user(message, me, roles, se, sr)
    })
}

/// Same mention rule evaluated against a stored [`crate::Message`] on delete.
pub(crate) fn stored_message_mention(
    stores: &Stores,
    message: &crate::Message,
    now: DateTime<Utc>,
) -> Option<Importance> {
    let me = stores.current_user.as_ref()?.id;
    if message.author.id == me && message.kind != 46 {
        return None;
    }
    let blocked = message.blocked
        || message.ignored
        || stores.relationships.get(&message.author.id) == Some(&2)
        || stores.is_ignored(message.author.id);
    mention_with(
        stores,
        message.channel_id,
        message.kind,
        blocked,
        now,
        |roles, se, sr| message.mentions_me(me, roles, se, sr),
    )
}

fn add_mentions(counts: &mut Counts, stores: &Stores, channel: Id) {
    if let Some(state) = stores.read_states.states.get(&channel) {
        // The badge as shown: a hidden channel's count is filtered (`ReadStates::hidden`).
        let shown = stores.read_states.mentions(channel);
        counts.mentions = counts.mentions.saturating_add(shown);
        if !state.low_importance {
            counts.high_mentions = counts.high_mentions.saturating_add(shown);
        }
    }
}

/// How long a cached answer holds against the clock: mutes end by time alone. What callers change directly is caught
/// by `stamp`; what only events change (roles, the member, the guild) by the cache being cleared on them.
const COUNTS_TTL_MS: i64 = 250;

/// The guild's counts, cached per guild. `Stores::dispatch` clears the cache on every event that can change them.
pub fn guild_counts(stores: &Stores, guild: &Guild, now: DateTime<Utc>) -> Counts {
    let now_ms = now.timestamp_millis();
    let stamp = stamp(stores, guild);
    let cache = stores
        .counts_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(entry) = cache.get(&guild.id)
        && entry.channels == guild.channels.len()
        && entry.threads == guild.threads.len()
        && (now_ms - entry.now_ms).abs() < COUNTS_TTL_MS
        && entry.stamp == stamp
    {
        return entry.counts;
    }
    drop(cache);
    let counts = guild_counts_uncached(stores, guild, now);
    stores
        .counts_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            guild.id,
            CountsEntry {
                now_ms,
                channels: guild.channels.len(),
                threads: guild.threads.len(),
                stamp,
                counts,
            },
        );
    counts
}

/// 458294 `W` for one guild.
pub fn guild_counts_uncached(stores: &Stores, guild: &Guild, now: DateTime<Utc>) -> Counts {
    let mut counts = Counts::default();
    let Some(me) = stores.me() else {
        return counts;
    };
    let now_ms = now.timestamp_millis();
    let opt_in = opt_in_enabled(stores, guild);
    let joined = guild.joined_snowflake();
    for channel in &guild.channels {
        if matches!(
            channel.kind,
            ChannelKind::Category | ChannelKind::Other(6 | 14)
        ) || channel.flags & 128 != 0
            || !allowed_age(stores, guild, channel)
            || !permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
        {
            continue;
        }
        let mentions = stores.read_states.mentions(channel.id);
        // getGuildChannelUnreadState: everything is zero, so nothing is added or marked.
        if opt_in && opt_in_zeroes(stores, guild, channel, now_ms) {
            continue;
        }
        if mentions == 0
            && (matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13))
                // `U`: opt-in on, not opted in, and no mention (threads are exempt, `fT`).
                || (opt_in && !opted_in(stores, guild, channel)))
        {
            continue;
        }
        add_mentions(&mut counts, stores, channel.id);
        if !counts.unread
            && !muted(stores, guild.id, channel.id, channel.parent_id, now)
            && (mentions > 0 || all_unreads(stores, guild, channel))
            && stores.read_states.is_unread(channel.id, joined)
        {
            counts.unread = true;
        }
    }
    // `W`'s thread loop: active joined threads mark the guild unread unless the thread or the guild is muted, and
    // add their mentions; no permission or NSFW check is made.
    let guild_muted = stores
        .guild_settings
        .guilds
        .get(&guild.id)
        .is_some_and(|s| guild_settings::mute_active(s.muted, s.mute_end, now));
    for thread in guild.threads.iter().filter(|t| t.joined && !t.archived) {
        if !counts.unread
            && stores.read_states.is_unread(thread.id, joined)
            && !thread.is_muted(now)
            && !guild_muted
        {
            counts.unread = true;
        }
        add_mentions(&mut counts, stores, thread.id);
    }
    let mute_events = stores
        .guild_settings
        .guilds
        .get(&guild.id)
        .is_some_and(|s| s.mute_scheduled_events);
    if !counts.unread
        && !guild_muted
        && !mute_events
        && stores
            .read_states
            .typed
            .get(&(guild.id, 1))
            .is_some_and(|state| state.last > state.acked || state.mention_count > 0)
    {
        counts.unread = true;
    }
    counts
}

pub fn incoming_requests(stores: &Stores) -> u64 {
    let incoming = stores
        .relationships
        .values()
        .filter(|kind| **kind == 3)
        .count();
    // 994500 subtracts both counters and clamps; they are not a union of filtered ids.
    incoming
        .saturating_sub(stores.spam_relationships.len())
        .saturating_sub(stores.ignored_pending_relationships.len()) as u64
}

/// 912309 `dH`: RelationshipStore `getPendingCount` plus GameRelationshipStore `getPendingIncomingCount`. Both the
/// desktop badge and the tray's unread state add it to the mention count.
pub fn desktop_requests(stores: &Stores) -> u64 {
    incoming_requests(stores).saturating_add(stores.game_relationships.pending_incoming_count())
}

/// 596339 SystemTrayManager `NV`: the tray shows its unread icon when the badge setting is on and anything is unread
/// in a server, a high-importance mention is pending (`getTotalMentionCount`, private channels included) or a friend
/// request waits. Low-importance mentions alone do not count.
pub fn tray_unread(high_mentions: u64, requests: u64, unread: bool, disable_unread: bool) -> bool {
    !disable_unread && (unread || high_mentions > 0 || requests > 0)
}

/// 596339 `Ni`: `getTotalMentionCount()` (high importance) plus the request count; with nothing counted, any unread
/// server gives -1 (the dot) unless "Enable Unread Message Badge" is off. Disabling the unread marker leaves explicit
/// mention/request counts on the taskbar. There is no cap here: desktop core 6820 caps the picture at "9+".
pub fn badge_count(mentions: u64, requests: u64, unread: bool, disable_unread: bool) -> i64 {
    let count = mentions.saturating_add(requests).min(i64::MAX as u64) as i64;
    if count == 0 && unread && !disable_unread {
        -1
    } else {
        count
    }
}

pub fn total_counts(stores: &Stores, now: DateTime<Utc>) -> Counts {
    let mut counts = Counts::default();
    for guild in &stores.guilds {
        let current = guild_counts(stores, guild, now);
        counts.mentions = counts.mentions.saturating_add(current.mentions);
        counts.high_mentions = counts.high_mentions.saturating_add(current.high_mentions);
        counts.unread |= current.unread;
    }
    for channel in &stores.private_channels {
        // 218461 `canHaveMentions`: a message request or spam channel has no mentions.
        if channel.is_request() {
            continue;
        }
        // GuildReadStateStore treats private-channel mentions as high importance.
        let mentions = stores.read_states.mentions(channel.id);
        counts.mentions = counts.mentions.saturating_add(mentions);
        counts.high_mentions = counts.high_mentions.saturating_add(mentions);
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pending_request_counts_follow_spam_ignore_and_normalized_gateway_flags() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"me"},
            "relationships":[
                {"id":"10","type":3},
                {"id":"11","type":3,"is_spam_request":true},
                {"id":"12","type":3,"user_ignored":true},
                {"id":"13","type":1,"user_ignored":true}
            ]}),
        );
        assert_eq!(incoming_requests(&stores), 1);
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"14","type":3,"is_spam_request":true}),
        );
        assert_eq!(incoming_requests(&stores), 1);
        stores.dispatch("RELATIONSHIP_UPDATE", &json!({"id":"11","type":3}));
        assert_eq!(incoming_requests(&stores), 2);
        stores.dispatch("RELATIONSHIP_UPDATE", &json!({"id":"12","type":3}));
        assert!(!stores.is_ignored(12));
        assert_eq!(incoming_requests(&stores), 3);
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id":"14","type":3}));
        assert_eq!(incoming_requests(&stores), 3);
        stores.set_ignored(10, true);
        assert_eq!(incoming_requests(&stores), 2);
        stores.dispatch(
            "RELATIONSHIP_REMOVE",
            &json!({"id":"10","type":3,"user_ignored":true}),
        );
        assert!(stores.is_ignored(10));
        assert_eq!(incoming_requests(&stores), 2);
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id":"13","type":1}));
        assert!(!stores.is_ignored(13));
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"me"},"relationships":[]}),
        );
        assert_eq!(incoming_requests(&stores), 0);
        assert!(stores.spam_relationships.is_empty());
        assert!(stores.ignored_pending_relationships.is_empty());
    }
    #[test]
    fn pending_request_counter_clamps_overlapping_filter_flags_like_the_source() {
        let mut stores = Stores::default();
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"10","type":3,
            "is_spam_request":true,"user_ignored":true}),
        );
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"11","type":3}));
        assert_eq!(incoming_requests(&stores), 0);
        stores.set_ignored(10, false);
        assert_eq!(incoming_requests(&stores), 1);
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"10","type":1}));
        assert_eq!(incoming_requests(&stores), 1);
    }
    fn setup() -> Stores {
        let mut stores = Stores::default();
        stores.dispatch("READY", &json!({
            "user":{"id":"1","username":"me"},
            "guilds":[{"id":"10","properties":{"name":"fixture","default_message_notifications":0},
                "roles":[{"id":"10","permissions":"1024"}],
                "channels":[{"id":"20","name":"chat","type":0,"parent_id":"30"}]}],
            "private_channels":[{"id":"40","type":1,"recipient_ids":["2"]}]
        }));
        stores.guilds[0].my_roles = vec![50];
        stores
    }
    fn message(stores: &mut Stores, channel: &str, mentions: Value) {
        let next = stores
            .read_states
            .latest
            .values()
            .copied()
            .max()
            .unwrap_or(100)
            + 1;
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({"id":next.to_string(),"channel_id":channel,
            "timestamp":"2026-11-09T12:00:00Z",
            "author":{"id":"2","username":"sender"},"content":"fixture", "mention_roles":mentions}),
        );
    }
    #[test]
    fn roles_and_private_messages_count_without_a_desktop_alert() {
        let mut stores = setup();
        stores.status = "dnd".into();
        message(&mut stores, "20", json!(["50"]));
        message(&mut stores, "40", json!([]));
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 2);
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","suppress_roles":true}));
        message(&mut stores, "20", json!(["50"]));
        assert_eq!(stores.read_states.mentions(20), 1);
        stores.relationships.insert(2, 2);
        message(&mut stores, "40", json!([]));
        assert_eq!(stores.read_states.mentions(40), 1);
    }
    #[test]
    fn inherited_mutes_stop_unread_dots_but_preserve_mentions() {
        let mut stores = setup();
        message(&mut stores, "20", json!([]));
        assert!(stores.guild_unread(&stores.guilds[0]));
        stores.guild_settings.update(
            &json!({"guild_id":"10","channel_overrides":[{"channel_id":"30","muted":true}]}),
        );
        message(&mut stores, "20", json!(["50"]));
        let counts = guild_counts(&stores, &stores.guilds[0], Utc::now());
        assert!(!counts.unread);
        assert_eq!(counts.high_mentions, 1);
        stores.guilds[0].channels[0].nsfw = true;
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 0);
        stores.account.nsfw_allowed = true;
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 1);
    }
    #[test]
    fn low_importance_mentions_promote_on_a_direct_ping() {
        let mut stores = setup();
        stores.notification_flags = 32;
        message(&mut stores, "20", json!([]));
        assert_eq!(total_counts(&stores, Utc::now()).mentions, 1);
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 0);
        message(&mut stores, "20", json!(["50"]));
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 2);
        message(&mut stores, "20", json!([]));
        assert_eq!(total_counts(&stores, Utc::now()).high_mentions, 3);
    }
    #[test]
    fn unread_settings_and_incoming_relationships_drive_the_badge() {
        let mut stores = setup();
        stores.notification_flags = 16;
        stores.guilds[0].default_message_notifications = 1;
        message(&mut stores, "20", json!([]));
        assert!(!stores.guild_unread(&stores.guilds[0]));
        stores.guild_settings.update(
            &json!({"guild_id":"10","channel_overrides":[{"channel_id":"30","flags":1024}]}),
        );
        assert!(stores.guild_unread(&stores.guilds[0]));
        for (id, kind) in [(3, 3), (4, 4), (5, 1)] {
            stores.dispatch(
                "RELATIONSHIP_ADD",
                &json!({"id":id.to_string(),"type":kind}),
            );
        }
        assert_eq!(incoming_requests(&stores), 1);
        assert_eq!(badge_count(0, 0, true, false), -1);
        assert_eq!(badge_count(0, 0, true, true), 0);
        assert_eq!(badge_count(2, incoming_requests(&stores), true, true), 3);
    }

    /// The taskbar number and the tray state from live stores, as 596339 `Ni` and `NV` compute them.
    fn desktop(stores: &Stores, disable_unread: bool) -> (i64, bool) {
        let counts = total_counts(stores, Utc::now());
        let requests = desktop_requests(stores);
        (
            badge_count(
                counts.high_mentions,
                requests,
                counts.unread,
                disable_unread,
            ),
            tray_unread(
                counts.high_mentions,
                requests,
                counts.unread,
                disable_unread,
            ),
        )
    }

    #[test]
    fn desktop_badge_and_tray_follow_the_official_count_rule() {
        let mut stores = setup();
        assert_eq!(desktop(&stores, false), (0, false));
        // Unreads only: the dot (-1) and the unread tray, both gone with "Enable Unread Message Badge" off.
        message(&mut stores, "20", json!([]));
        assert_eq!(desktop(&stores, false), (-1, true));
        assert_eq!(desktop(&stores, true), (0, false));
        // A direct message is a high-importance mention: a number, kept even with the setting off.
        message(&mut stores, "40", json!([]));
        assert_eq!(desktop(&stores, false), (1, true));
        assert_eq!(desktop(&stores, true), (1, false));
        // A role mention in the server adds to it.
        message(&mut stores, "20", json!(["50"]));
        assert_eq!(desktop(&stores, false), (2, true));
        // A muted server gives no unread mark but keeps its mentions.
        stores.dispatch(
            "USER_GUILD_SETTINGS_UPDATE",
            &json!({"guild_id":"10","muted":true,"message_notifications":0,"channel_overrides":[]}),
        );
        assert_eq!(desktop(&stores, false), (2, true));
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id":"40","message_id":"999","mention_count":0}),
        );
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id":"20","message_id":"999","mention_count":0}),
        );
        assert_eq!(desktop(&stores, false), (0, false));
        // Friend requests and game friend requests count like mentions.
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"7","type":3}));
        stores.dispatch(
            "GAME_RELATIONSHIP_ADD",
            &json!({"id":"8","application_id":"100","type":3}),
        );
        assert_eq!(desktop_requests(&stores), 2);
        assert_eq!(desktop(&stores, true), (2, false));
        assert_eq!(desktop(&stores, false), (2, true));
        // No cap in the count: the picture caps at "9+".
        assert_eq!(badge_count(250, 0, false, false), 250);
        // Low-importance mentions alone do not light the tray (getTotalMentionCount is high importance only).
        assert!(!tray_unread(0, 0, false, false));
        assert!(tray_unread(0, 0, true, false));
        assert!(!tray_unread(5, 5, true, true));
    }

    fn thread(stores: &mut Stores, flags: u64, muted: bool) {
        stores.dispatch(
            "THREAD_CREATE",
            &json!({"id":"60","guild_id":"10","parent_id":"20","type":11,"name":"talk","owner_id":"2",
                "last_message_id":"90","message_count":3,"thread_metadata":{"archived":false,"locked":false},
                "member":{"id":"60","user_id":"1","flags":flags,"muted":muted,"mute_config":null,
                    "join_timestamp":"2026-11-09T10:00:00.000000+00:00"}}),
        );
    }

    fn thread_message(stores: &mut Stores, id: u64, mention: bool) {
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({"id":id.to_string(),"channel_id":"60","type":0,"timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"2","username":"sender"},"content":"in thread",
                "mentions": if mention { json!([{"id":"1","username":"me"}]) } else { json!([]) },
                "mention_roles":[],"mention_everyone":false}),
        );
    }

    #[test]
    fn joined_threads_mark_the_guild_unread_unless_the_thread_or_guild_is_muted() {
        let mut stores = setup();
        thread(&mut stores, 2, false);
        // The thread's newest message (90) was never read.
        let now = Utc::now();
        assert!(guild_counts(&stores, &stores.guilds[0], now).unread);
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id":"60","message_id":"90","mention_count":0}),
        );
        assert!(!guild_counts_uncached(&stores, &stores.guilds[0], now).unread);
        thread_message(&mut stores, 100, false);
        assert!(guild_counts(&stores, &stores.guilds[0], now).unread);
        thread(&mut stores, 2, true);
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id":"60","message_id":"90","mention_count":0}),
        );
        thread_message(&mut stores, 101, false);
        assert!(!guild_counts(&stores, &stores.guilds[0], now).unread);
        thread(&mut stores, 2, false);
        stores.dispatch(
            "USER_GUILD_SETTINGS_UPDATE",
            &json!({"guild_id":"10","muted":true,
            "message_notifications":0,"channel_overrides":[]}),
        );
        assert!(!guild_counts(&stores, &stores.guilds[0], now).unread);
    }

    #[test]
    fn thread_mentions_count_without_the_parents_permission() {
        let mut stores = setup();
        stores.dispatch(
            "GUILD_ROLE_UPDATE",
            &json!({"guild_id":"10",
            "role":{"id":"10","name":"@everyone","permissions":"0"}}),
        );
        thread(&mut stores, 4, false);
        thread_message(&mut stores, 100, true);
        assert_eq!(stores.read_states.mentions(60), 1);
        let counts = guild_counts(&stores, &stores.guilds[0], Utc::now());
        assert_eq!((counts.mentions, counts.high_mentions), (1, 1));
        // An unjoined (archived) thread adds nothing.
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"id":"60","guild_id":"10","parent_id":"20","type":11,
            "name":"talk","thread_metadata":{"archived":true}}),
        );
        assert_eq!(
            guild_counts(&stores, &stores.guilds[0], Utc::now()).mentions,
            0
        );
    }

    #[test]
    fn thread_messages_are_low_importance_mentions_only_at_the_all_messages_level() {
        let mut stores = setup();
        stores.notification_flags = 32;
        thread(&mut stores, 2, false);
        thread_message(&mut stores, 100, false);
        assert_eq!(stores.read_states.mentions(60), 1);
        assert!(stores.read_states.states[&60].low_importance);
        thread(&mut stores, 4, false);
        thread_message(&mut stores, 101, false);
        assert_eq!(stores.read_states.mentions(60), 1);
        thread_message(&mut stores, 102, true);
        assert_eq!(stores.read_states.mentions(60), 2);
        assert!(!stores.read_states.states[&60].low_importance);
    }

    fn at_ms(ms: i64) -> Id {
        crate::read_state::snowflake_at(ms as u64)
    }

    fn channel_message(stores: &mut Stores, channel: &str, id: Id) {
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({"id":id.to_string(),"channel_id":channel,"timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"2","username":"sender"},"content":"x","mentions":[]}),
        );
    }

    #[test]
    fn opt_in_zeroes_old_and_unopted_channels_only_in_communities() {
        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        let fresh = at_ms(now_ms - 3_600_000);
        let old = at_ms(now_ms - 10 * 86_400_000);
        let guild = |features: Value| {
            let mut stores = Stores::default();
            stores.dispatch("READY", &json!({"user":{"id":"1","username":"me"},
                "guilds":[{"id":"10","joined_at":"2020-01-01T00:00:00+00:00",
                    "properties":{"name":"g","default_message_notifications":0,"features":features},
                    "roles":[{"id":"10","permissions":"1024"}],
                    "channels":[{"id":"20","name":"chat","type":0},{"id":"21","name":"old","type":0}]}]}));
            stores
        };
        let opt_in = json!({"guild_id":"10","flags":16384,"message_notifications":0});
        // Not a community: the opt-in flag alone changes nothing.
        let mut stores = guild(json!([]));
        stores.guild_settings.update(&opt_in);
        channel_message(&mut stores, "20", fresh);
        assert!(guild_counts(&stores, &stores.guilds[0], now).unread);
        // A community with opt-in on: a channel not opted in counts for nothing.
        let mut stores = guild(json!(["COMMUNITY"]));
        stores.guild_settings.update(&opt_in);
        channel_message(&mut stores, "20", fresh);
        assert!(!guild_counts(&stores, &stores.guilds[0], now).unread);
        // Opted in: it counts again, but a channel quiet for over seven days still does not.
        stores.guild_settings.update(&json!({"guild_id":"10","flags":16384,"message_notifications":0,
            "channel_overrides":[{"channel_id":"20","flags":4096},{"channel_id":"21","flags":4096}]}));
        stores.invalidate_counts();
        assert!(guild_counts(&stores, &stores.guilds[0], now).unread);
        let mut stores = guild(json!(["COMMUNITY"]));
        stores.guild_settings.update(
            &json!({"guild_id":"10","flags":16384,"message_notifications":0,
            "channel_overrides":[{"channel_id":"21","flags":4096}]}),
        );
        channel_message(&mut stores, "21", old);
        assert!(!guild_counts(&stores, &stores.guilds[0], now).unread);
        // Mentions in an old channel are zeroed too (218461), while a fresh opted-out one keeps its direct ping.
        stores.read_states.states.insert(
            21,
            crate::read_state::ReadState {
                acked: 1,
                mention_count: 2,
                ..Default::default()
            },
        );
        let counts = guild_counts(&stores, &stores.guilds[0], now);
        assert_eq!(counts.mentions, 0);
        stores.read_states.states.insert(
            20,
            crate::read_state::ReadState {
                acked: 1,
                mention_count: 1,
                ..Default::default()
            },
        );
        channel_message(&mut stores, "20", fresh);
        stores.read_states.states.insert(
            20,
            crate::read_state::ReadState {
                acked: 1,
                mention_count: 1,
                ..Default::default()
            },
        );
        assert_eq!(guild_counts(&stores, &stores.guilds[0], now).mentions, 1);
    }

    /// Every kind of event that can change a guild's counts leaves the cached answer equal to a fresh one, and the
    /// ones that change the answer change the cached answer too.
    #[test]
    fn the_counts_cache_agrees_with_a_fresh_count_after_every_kind_of_event() {
        let mut stores = setup();
        let now = Utc::now();
        let check = |stores: &Stores| {
            let guild = &stores.guilds[0];
            let cached = guild_counts(stores, guild, now);
            assert_eq!(cached, guild_counts_uncached(stores, guild, now));
            cached
        };
        let base = check(&stores);
        let events: Vec<(&str, Value)> = vec![
            (
                "MESSAGE_CREATE",
                json!({"id":"500","channel_id":"20","timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"2","username":"sender"},"content":"x","mentions":[{"id":"1"}]}),
            ),
            (
                "MESSAGE_ACK",
                json!({"channel_id":"20","message_id":"500","mention_count":0}),
            ),
            (
                "MESSAGE_CREATE",
                json!({"id":"501","channel_id":"20","timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"2","username":"sender"},"content":"x","mentions":[]}),
            ),
            (
                "USER_GUILD_SETTINGS_UPDATE",
                json!({"guild_id":"10","message_notifications":1,
                "channel_overrides":[{"channel_id":"30","muted":true}]}),
            ),
            (
                "USER_GUILD_SETTINGS_UPDATE",
                json!({"guild_id":"10","message_notifications":0,"channel_overrides":[]}),
            ),
            (
                "MESSAGE_CREATE",
                json!({"id":"502","channel_id":"20","timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"2","username":"sender"},"content":"x","mentions":[{"id":"1"}]}),
            ),
            (
                "GUILD_ROLE_UPDATE",
                json!({"guild_id":"10","role":{"id":"10","name":"@everyone","permissions":"0"}}),
            ),
            (
                "GUILD_ROLE_UPDATE",
                json!({"guild_id":"10","role":{"id":"10","name":"@everyone","permissions":"1024"}}),
            ),
            (
                "GUILD_MEMBER_UPDATE",
                json!({"guild_id":"10","user":{"id":"1","username":"me"},"roles":["50"]}),
            ),
            (
                "CHANNEL_UPDATE",
                json!({"id":"20","guild_id":"10","type":0,"name":"chat","parent_id":"30","nsfw":true}),
            ),
            (
                "CHANNEL_UPDATE",
                json!({"id":"20","guild_id":"10","type":0,"name":"chat","parent_id":"30","nsfw":false}),
            ),
            ("NOTIFICATION_SETTINGS_UPDATE", json!({"flags":16})),
            (
                "THREAD_CREATE",
                json!({"id":"60","guild_id":"10","parent_id":"20","type":11,"name":"t",
                "last_message_id":"900","thread_metadata":{"archived":false},"member":{"id":"60","user_id":"1","flags":2}}),
            ),
            (
                "THREAD_MEMBER_UPDATE",
                json!({"id":"60","guild_id":"10","user_id":"1","flags":2,"muted":true}),
            ),
            (
                "CHANNEL_CREATE",
                json!({"id":"22","guild_id":"10","type":0,"name":"new"}),
            ),
            (
                "CHANNEL_DELETE",
                json!({"id":"22","guild_id":"10","type":0}),
            ),
            (
                "THREAD_DELETE",
                json!({"id":"60","guild_id":"10","parent_id":"20","type":11}),
            ),
        ];
        let mut changes = 0;
        let mut last = base;
        for (name, data) in &events {
            stores.dispatch(name, data);
            let now_counts = check(&stores);
            changes += usize::from(now_counts != last);
            last = now_counts;
        }
        // Role and member events are not seen by the stamp, so the events alone had to refresh the cache.
        assert!(changes >= 6, "{changes}");
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({"id":"600","channel_id":"20","timestamp":"2026-11-09T12:00:00Z",
            "author":{"id":"2","username":"sender"},"content":"x","mentions":[{"id":"1"}]}),
        );
        let with_mention = check(&stores);
        stores.dispatch(
            "GUILD_ROLE_UPDATE",
            &json!({"guild_id":"10","role":{"id":"10","name":"@everyone","permissions":"0"}}),
        );
        let hidden = check(&stores);
        assert!(with_mention.mentions > 0);
        assert_eq!(hidden.mentions, 0);
    }

    #[test]
    fn channel_index_follows_creates_deletes_threads_and_guild_removal() {
        let mut stores = setup();
        assert_eq!(stores.guild_of_channel(20).map(|guild| guild.id), Some(10));
        assert!(stores.guild_of_channel(40).is_none());
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id":"23","guild_id":"10","type":0,"name":"fresh"}),
        );
        assert_eq!(stores.guild_of_channel(23).map(|guild| guild.id), Some(10));
        thread(&mut stores, 2, false);
        assert_eq!(stores.guild_of_channel(60).map(|guild| guild.id), Some(10));
        stores.dispatch(
            "THREAD_DELETE",
            &json!({"id":"60","guild_id":"10","parent_id":"20","type":11}),
        );
        assert!(stores.guild_of_channel(60).is_none());
        stores.dispatch(
            "CHANNEL_DELETE",
            &json!({"id":"23","guild_id":"10","type":0}),
        );
        assert!(stores.guild_of_channel(23).is_none());
        // A guild changed directly still resolves by the checked fallback.
        stores.guilds[0].channels.retain(|channel| channel.id != 20);
        assert!(stores.guild_of_channel(20).is_none());
        stores.dispatch("GUILD_DELETE", &json!({"id":"10"}));
        assert!(stores.guilds.is_empty());
        // An outage keeps the guild.
        let mut stores = setup();
        stores.dispatch("GUILD_DELETE", &json!({"id":"10","unavailable":true}));
        assert_eq!(stores.guilds.len(), 1);
    }

    #[test]
    fn store_and_directory_channels_skip_while_unmuted_events_mark_unread() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "guilds": [{
                    "id": "10",
                    "properties": {"name": "g", "default_message_notifications": 0},
                    "roles": [{"id": "10", "permissions": "1024"}],
                    "channels": [
                        {"id": "20", "name": "store", "type": 6, "last_message_id": "500"},
                        {"id": "21", "name": "dir", "type": 14, "last_message_id": "500"}
                    ]
                }],
                "read_state": {
                    "entries": [
                        {"id": "10", "read_state_type": 1, "last_acked_id": "100", "badge_count": 0}
                    ]
                }
            }),
        );
        assert!(!guild_counts(&stores, &stores.guilds[0], Utc::now()).unread);

        stores.dispatch(
            "GUILD_SCHEDULED_EVENT_CREATE",
            &json!({"id": "200", "guild_id": "10", "creator_id": "2", "name": "Ev"}),
        );
        assert!(guild_counts(&stores, &stores.guilds[0], Utc::now()).unread);

        stores.guild_settings.update(
            &json!({"guild_id": "10", "mute_scheduled_events": true}),
        );
        assert!(!guild_counts(&stores, &stores.guilds[0], Utc::now()).unread);

        stores.guild_settings.update(
            &json!({"guild_id": "10", "mute_scheduled_events": false}),
        );
        stores.dispatch(
            "GUILD_FEATURE_ACK",
            &json!({"id": "10", "ack_type": 1, "acked_id": "200"}),
        );
        assert!(!guild_counts(&stores, &stores.guilds[0], Utc::now()).unread);
    }
}
