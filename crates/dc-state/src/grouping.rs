//! Message grouping. Port of `u` in module 857069 with the date helpers `ro` and `v0` from module 5377.
//! Inputs the M0 message model does not carry yet (forum posts, lobbies, additional names) are left to their rows.

use crate::Message;
use chrono::{DateTime, Datelike, Local, Utc};

const WINDOW_MS: i64 = 7 * 60 * 1000;
const DAY_MS: i64 = 86_400_000;

const MESSAGE_DEFAULT: u64 = 0;
const MESSAGE_REPLY: u64 = 19;
const FLAG_HAS_THREAD: u64 = 1 << 5;
const FLAG_EPHEMERAL: u64 = 1 << 6;
const FLAG_SUPPRESS_NOTIFICATIONS: u64 = 1 << 12;
const COMMAND_TYPES: [u64; 2] = [20, 23];

/// `ro`: at most a day apart and on the same day of the month, in local time.
fn same_day(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    let (local_a, local_b) = (a.with_timezone(&Local), b.with_timezone(&Local));
    (a - b).num_milliseconds().abs() <= DAY_MS && local_a.day() == local_b.day()
}

/// `v0`: strictly closer than the window.
fn within_window(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    (a - b).num_milliseconds().abs() < WINDOW_MS
}

/// `l.A`: whether a message renders as a user message for grouping.
fn is_user_message(message: &Message) -> bool {
    !matches!(message.kind, MESSAGE_DEFAULT | MESSAGE_REPLY)
}

fn has(message: &Message, flag: u64) -> bool {
    message.flags & flag != 0
}

/// True when `next` starts a new group after `previous`.
pub fn starts_group(previous: &Message, next: &Message) -> bool {
    if has(next, FLAG_HAS_THREAD) || COMMAND_TYPES.contains(&next.kind) {
        return true;
    }
    if previous.blocked != next.blocked || previous.ignored != next.ignored {
        return true;
    }
    if next.kind > MESSAGE_DEFAULT {
        return !is_user_message(previous) || next.kind == MESSAGE_REPLY;
    }
    is_user_message(previous)
        || previous.author.id != next.author.id
        || has(previous, FLAG_EPHEMERAL) != has(next, FLAG_EPHEMERAL)
        || (next.webhook_id.is_some() && previous.author.username != next.author.username)
        || !same_day(previous.timestamp, next.timestamp)
        || !within_window(previous.timestamp, next.timestamp)
        || (has(next, FLAG_SUPPRESS_NOTIFICATIONS) && !has(previous, FLAG_SUPPRESS_NOTIFICATIONS))
        || (has(previous, FLAG_SUPPRESS_NOTIFICATIONS)
            && !has(next, FLAG_SUPPRESS_NOTIFICATIONS)
            && (next.mentions > 0 || next.mention_roles > 0 || next.mention_everyone))
        || next.application_id != previous.application_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::User;
    use chrono::{Duration, TimeZone};

    fn base() -> Message {
        Message {
            id: 1,
            channel_id: 1,
            author: User {
                id: 7,
                username: "a".into(),
                ..User::default()
            },
            content: String::new(),
            edited: false,
            referenced: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            timestamp: Local
                .with_ymd_and_hms(2026, 11, 7, 12, 0, 0)
                .unwrap()
                .with_timezone(&Utc),
            kind: 0,
            flags: 0,
            webhook_id: None,
            application_id: None,
            mentions: 0,
            mention_roles: 0,
            mention_everyone: false,
            blocked: false,
            ignored: false,
        }
    }

    fn after(milliseconds: i64) -> Message {
        let mut next = base();
        next.timestamp += Duration::milliseconds(milliseconds);
        next
    }

    #[test]
    fn same_author_inside_the_window_groups() {
        assert!(!starts_group(&base(), &after(60_000)));
    }

    #[test]
    fn the_window_is_strict() {
        assert!(!starts_group(&base(), &after(WINDOW_MS - 1)));
        assert!(starts_group(&base(), &after(WINDOW_MS)));
    }

    #[test]
    fn a_new_day_breaks_the_group() {
        let mut late = base();
        late.timestamp = Local
            .with_ymd_and_hms(2026, 11, 7, 23, 59, 0)
            .unwrap()
            .with_timezone(&Utc);
        let mut next = late.clone();
        next.timestamp += Duration::minutes(2);
        assert!(starts_group(&late, &next));
    }

    #[test]
    fn a_different_author_breaks_the_group() {
        let mut next = after(1000);
        next.author.id = 8;
        assert!(starts_group(&base(), &next));
    }

    #[test]
    fn replies_always_start_a_group() {
        let mut next = after(1000);
        next.kind = MESSAGE_REPLY;
        assert!(starts_group(&base(), &next));
    }

    #[test]
    fn threads_and_commands_start_a_group() {
        let mut next = after(1000);
        next.flags = FLAG_HAS_THREAD;
        assert!(starts_group(&base(), &next));
        let mut command = after(1000);
        command.kind = 20;
        assert!(starts_group(&base(), &command));
    }

    #[test]
    fn ephemeral_change_breaks_the_group() {
        let mut next = after(1000);
        next.flags = FLAG_EPHEMERAL;
        assert!(starts_group(&base(), &next));
    }

    #[test]
    fn suppressed_notifications_follow_the_original_conditions() {
        let mut suppressed = after(1000);
        suppressed.flags = FLAG_SUPPRESS_NOTIFICATIONS;
        assert!(starts_group(&base(), &suppressed));

        let mut previous = base();
        previous.flags = FLAG_SUPPRESS_NOTIFICATIONS;
        assert!(!starts_group(&previous, &after(1000)));
        let mut mention = after(1000);
        mention.mentions = 1;
        assert!(starts_group(&previous, &mention));
    }

    #[test]
    fn webhooks_group_by_username() {
        let mut next = after(1000);
        next.webhook_id = Some(1);
        assert!(!starts_group(&base(), &next));
        next.author.username = "other".into();
        assert!(starts_group(&base(), &next));
    }
}
