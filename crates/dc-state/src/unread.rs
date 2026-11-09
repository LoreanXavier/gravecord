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

fn muted(stores: &Stores, guild: Id, channel: Id, parent: Option<Id>, now: DateTime<Utc>) -> bool {
    let settings = stores.guild_settings.guilds.get(&guild);
    settings.is_some_and(|s| guild_settings::mute_active(s.muted, s.mute_end, now))
        || [Some(channel), parent].into_iter().flatten().any(|id| {
            settings
                .and_then(|s| s.channels.get(&id))
                .is_some_and(|s| guild_settings::mute_active(s.muted, s.mute_end, now))
        })
}

fn message_level(stores: &Stores, guild: &Guild, channel: &Channel) -> u64 {
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

fn opted_out(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    let settings = stores.guild_settings.guilds.get(&guild.id);
    settings.is_some_and(|s| s.flags & 16384 != 0)
        && ![Some(channel.id), channel.parent_id]
            .into_iter()
            .flatten()
            .any(|id| {
                settings
                    .and_then(|s| s.channels.get(&id))
                    .is_some_and(|s| s.flags & 4096 != 0)
            })
}

fn allowed_age(stores: &Stores, guild: &Guild, channel: &Channel) -> bool {
    stores.account.nsfw_allowed || (!channel.nsfw && !matches!(guild.nsfw_level, 1 | 3))
}

/// Badge mentions differ from desktop notifications: muted channels can still contain direct pings.
pub fn message_mention(stores: &Stores, message: &Value, now: DateTime<Utc>) -> Option<Importance> {
    let me = stores.current_user.as_ref()?.id;
    let channel = crate::id(&message["channel_id"])?;
    let author = crate::id(&message["author"]["id"])?;
    if stores.relationships.get(&author) == Some(&2) || stores.ignored.contains(&author) {
        return None;
    }
    let guild_channel = stores.guilds.iter().find_map(|guild| {
        let target = guild
            .channels
            .iter()
            .find(|c| c.id == channel)
            .or_else(|| {
                let parent = guild.threads.iter().find(|t| t.id == channel)?.parent_id;
                guild.channels.iter().find(|c| c.id == parent)
            })?;
        Some((guild, target))
    });
    if let Some((guild, target)) = guild_channel {
        if !allowed_age(stores, guild, target) {
            return None;
        }
        let settings = stores.guild_settings.guilds.get(&guild.id);
        if notifications::mentions_user(
            message,
            me,
            &guild.my_roles,
            settings.is_some_and(|s| s.suppress_everyone),
            settings.is_some_and(|s| s.suppress_roles),
        ) {
            return Some(Importance::High);
        }
        if channel == target.id
            && stores.notification_flags & 32 != 0
            && !matches!(target.kind, ChannelKind::Voice | ChannelKind::Other(13))
            && !muted(stores, guild.id, channel, target.parent_id, now)
            && message_level(stores, guild, target) == guild_settings::ALL_MESSAGES
        {
            return Some(Importance::Low);
        }
    } else {
        let private = stores.private_channels.iter().find(|c| c.id == channel);
        if private.is_some_and(|c| c.kind == 3) && message["type"].as_u64() == Some(2) {
            return None;
        }
        if notifications::mentions_user(message, me, &[], false, false)
            || (private.is_some() && !muted(stores, 0, channel, None, now))
        {
            return Some(Importance::High);
        }
    }
    None
}

fn add_mentions(counts: &mut Counts, stores: &Stores, channel: Id) {
    if let Some(state) = stores.read_states.states.get(&channel) {
        counts.mentions = counts.mentions.saturating_add(state.mention_count);
        if !state.low_importance {
            counts.high_mentions = counts.high_mentions.saturating_add(state.mention_count);
        }
    }
}

pub fn guild_counts(stores: &Stores, guild: &Guild, now: DateTime<Utc>) -> Counts {
    let mut counts = Counts::default();
    let Some(me) = stores.me() else {
        return counts;
    };
    for channel in &guild.channels {
        if channel.kind == ChannelKind::Category
            || channel.flags & 128 != 0
            || !allowed_age(stores, guild, channel)
            || !permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
        {
            continue;
        }
        let mentions = stores.read_states.mentions(channel.id);
        if mentions == 0
            && (matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13))
                || opted_out(stores, guild, channel))
        {
            continue;
        }
        add_mentions(&mut counts, stores, channel.id);
        if !muted(stores, guild.id, channel.id, channel.parent_id, now)
            && (mentions > 0 || all_unreads(stores, guild, channel))
            && stores
                .read_states
                .is_unread(channel.id, guild.joined_snowflake())
        {
            counts.unread = true;
        }
    }
    for thread in guild.threads.iter().filter(|t| t.joined && !t.archived) {
        let Some(parent) = guild.channels.iter().find(|c| c.id == thread.parent_id) else {
            continue;
        };
        if allowed_age(stores, guild, parent)
            && permissions::can_in_channel(guild, parent, me, permissions::VIEW_CHANNEL)
        {
            add_mentions(&mut counts, stores, thread.id);
        }
    }
    counts
}

pub fn incoming_requests(stores: &Stores) -> u64 {
    stores
        .relationships
        .values()
        .filter(|kind| **kind == 3)
        .count() as u64
}

/// 596339: disabling the unread marker leaves explicit mention/request counts on the taskbar.
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
}
