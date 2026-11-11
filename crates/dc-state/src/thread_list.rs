//! Which threads the channel list shows under a channel.
//!
//! Ports `ActiveJoinedThreadsStore` (module 863005: `isUnread` / `isRelevant` / `isTimedRelevant` in `k`, the
//! relevant and unread maps per parent), `lastActivity` and the auto-archive time (module 826767) and the row order
//! of `eT` with the choice between the two maps in `eA.computeState` (module 791513).
//!
//! A thread is listed only when it is joined and active and is "relevant": inside its auto-archive window since its
//! last activity, pinned, unread (and not muted), mentioned, or the voice channel this client is in. Threads outside
//! those are only reachable through the Threads popout.

use crate::Thread;
use chrono::{DateTime, Utc};
use serde_json::Value;

const MINUTE_MS: i64 = 60_000;
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// A timestamp field as Unix milliseconds: an ISO string (`new Date(...)`) or a number.
pub fn time_ms(value: &Value) -> Option<i64> {
    match value {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|time| time.timestamp_millis()),
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

/// What the read state and the voice state say about one thread (`k` in module 863005).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standing {
    /// `ReadStateStore.hasUnread(id)`.
    pub has_unread: bool,
    /// `ReadStateStore.getMentionCount(id)`.
    pub mentions: u64,
    /// `SelectedChannelStore.getVoiceChannelId() === id`.
    pub in_voice: bool,
}

/// Which of the store's two maps a parent row reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `getActiveJoinedRelevantThreadsForParent`.
    Relevant,
    /// `getActiveJoinedUnreadThreadsForParent`.
    Unread,
}

impl Thread {
    /// `lastActivity` (module 826767 `l`): the newest of the last message, the last non-message activity and the
    /// archive time, in milliseconds.
    pub fn last_activity_ms(&self) -> i64 {
        let message = self.last_message_id.unwrap_or(self.id);
        let sent = (message >> 22) as i64 + DISCORD_EPOCH_MS;
        sent.max(self.last_non_message_activity_ms.unwrap_or(0))
            .max(self.archive_timestamp_ms.unwrap_or(0))
    }

    /// Module 826767 `s`: when the thread archives by itself.
    pub fn auto_archive_at_ms(&self) -> i64 {
        self.last_activity_ms() + self.auto_archive_duration as i64 * MINUTE_MS
    }

    /// `isTimedRelevant`: active, and still inside its auto-archive window.
    pub fn is_timed_relevant(&self, now_ms: i64) -> bool {
        !self.archived && self.auto_archive_duration > 0 && self.auto_archive_at_ms() > now_ms
    }

    /// `hasUnread && !isMuted`.
    fn unread_and_not_muted(&self, standing: Standing, now: DateTime<Utc>) -> bool {
        standing.has_unread && !self.is_muted(now)
    }

    /// `isRelevant` of module 863005 `k`.
    pub fn is_relevant(&self, standing: Standing, now: DateTime<Utc>) -> bool {
        self.is_timed_relevant(now.timestamp_millis())
            || self.pinned()
            || self.unread_and_not_muted(standing, now)
            || standing.mentions > 0
            || standing.in_voice
    }

    /// `isUnread` of module 863005 `k`: an active or pinned thread with unread messages that is not muted, or any
    /// thread with a mention.
    pub fn is_unread_for_list(&self, standing: Standing, now: DateTime<Utc>) -> bool {
        ((!self.archived || self.pinned()) && self.unread_and_not_muted(standing, now))
            || standing.mentions > 0
    }
}

/// The rows under one channel (module 791513 `eA.computeState` choosing the map, `eT` ordering it).
pub struct Query<'a, F: Fn(&Thread) -> Standing> {
    pub threads: &'a [Thread],
    pub parent: u64,
    /// The relevant map when the channel itself (or its voice channel) is open, one of its threads is open, or its
    /// category is open and the channel not muted; otherwise only the unread map.
    pub source: Source,
    /// The open channel when it is a thread of `parent`.
    pub selected_thread: Option<u64>,
    /// The parent itself is the open channel (`eT`'s `a`).
    pub parent_selected: bool,
    /// `hideMutedChannels`: muted threads show only with a mention.
    pub hide_muted: bool,
    pub standing: F,
    pub now: DateTime<Utc>,
}

/// Ids of the threads to list under `query.parent`, in order: newest joined first (`-joinTimestamp`), and the open
/// thread put on top when it is not in the list.
pub fn sidebar_threads<F: Fn(&Thread) -> Standing>(query: &Query<'_, F>) -> Vec<u64> {
    let mut listed: Vec<&Thread> = query
        .threads
        .iter()
        .filter(|thread| thread.parent_id == query.parent && thread.joined && !thread.archived)
        .filter(|thread| {
            let standing = (query.standing)(thread);
            match query.source {
                Source::Relevant => thread.is_relevant(standing, query.now),
                Source::Unread => thread.is_unread_for_list(standing, query.now),
            }
        })
        .collect();
    listed.sort_by_key(|thread| {
        std::cmp::Reverse(
            thread
                .join_timestamp
                .as_deref()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.timestamp_millis())
                .unwrap_or(0),
        )
    });
    let mut ids: Vec<u64> = listed.iter().map(|thread| thread.id).collect();
    if query.parent_selected {
        return ids;
    }
    if let Some(open) = query.selected_thread {
        if !ids.contains(&open) {
            ids.insert(0, open);
        }
    } else if query.hide_muted {
        ids.retain(|id| {
            query
                .threads
                .iter()
                .find(|thread| thread.id == *id)
                .is_none_or(|thread| {
                    !thread.is_muted(query.now) || (query.standing)(thread).mentions > 0
                })
        });
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 11, 12, 0, 0).unwrap()
    }

    fn snowflake(ms: i64) -> u64 {
        ((ms - DISCORD_EPOCH_MS) as u64) << 22
    }

    /// A joined thread whose last message was `age_ms` ago, with an auto-archive window of `minutes`.
    fn thread(id: u64, age_ms: i64, minutes: u64) -> Thread {
        let sent = snowflake(now().timestamp_millis() - age_ms);
        Thread {
            id,
            parent_id: 1,
            joined: true,
            auto_archive_duration: minutes,
            last_message_id: Some(sent),
            join_timestamp: Some("2026-11-01T00:00:00+00:00".into()),
            ..Thread::default()
        }
    }

    fn list(
        threads: &[Thread],
        source: Source,
        standing: impl Fn(&Thread) -> Standing,
    ) -> Vec<u64> {
        sidebar_threads(&Query {
            threads,
            parent: 1,
            source,
            selected_thread: None,
            parent_selected: false,
            hide_muted: false,
            standing,
            now: now(),
        })
    }

    const DAY: i64 = 86_400_000;

    #[test]
    fn only_threads_inside_their_auto_archive_window_are_listed() {
        // Active an hour ago with a day's window: listed. Active ten days ago with a day's window: not listed.
        let fresh = thread(1, 3_600_000, 1440);
        let stale = thread(2, 10 * DAY, 1440);
        assert!(fresh.is_timed_relevant(now().timestamp_millis()));
        assert!(!stale.is_timed_relevant(now().timestamp_millis()));
        let ids = list(&[fresh, stale], Source::Relevant, |_| Standing::default());
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn a_stale_thread_is_listed_when_unread_pinned_mentioned_or_in_voice() {
        let mut stale = thread(2, 10 * DAY, 1440);
        assert!(!stale.is_relevant(Standing::default(), now()));
        let unread = Standing {
            has_unread: true,
            ..Standing::default()
        };
        assert!(stale.is_relevant(unread, now()));
        // A muted thread's unread messages do not count, a mention does.
        stale.muted = true;
        assert!(!stale.is_relevant(unread, now()));
        let mention = Standing {
            mentions: 1,
            ..Standing::default()
        };
        assert!(stale.is_relevant(mention, now()));
        let voice = Standing {
            in_voice: true,
            ..Standing::default()
        };
        assert!(stale.is_relevant(voice, now()));
        stale.muted = false;
        stale.flags = 2;
        assert!(stale.is_relevant(Standing::default(), now()));
    }

    #[test]
    fn a_thread_with_a_later_non_message_activity_stays_inside_its_window() {
        let mut stale = thread(2, 10 * DAY, 1440);
        stale.last_non_message_activity_ms = Some(now().timestamp_millis() - 3_600_000);
        assert!(stale.is_timed_relevant(now().timestamp_millis()));
    }

    #[test]
    fn unjoined_and_archived_threads_are_never_listed() {
        let mut unjoined = thread(1, 1000, 1440);
        unjoined.joined = false;
        let mut archived = thread(2, 1000, 1440);
        archived.archived = true;
        assert!(
            list(&[unjoined, archived], Source::Relevant, |_| {
                Standing::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn a_collapsed_or_muted_parent_lists_only_unread_threads() {
        let fresh = thread(1, 1000, 1440);
        let unread = thread(2, 10 * DAY, 1440);
        let ids = list(&[fresh, unread], Source::Unread, |thread| Standing {
            has_unread: thread.id == 2,
            ..Standing::default()
        });
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn the_open_thread_is_put_on_top_and_the_newest_joined_comes_first() {
        let older = thread(1, 1000, 1440);
        let mut newer = thread(2, 1000, 1440);
        newer.join_timestamp = Some("2026-11-05T00:00:00+00:00".into());
        let stale = thread(3, 10 * DAY, 1440);
        let threads = [older, newer, stale];
        let ids = list(&threads, Source::Relevant, |_| Standing::default());
        assert_eq!(ids, vec![2, 1]);
        let open = sidebar_threads(&Query {
            threads: &threads,
            parent: 1,
            source: Source::Relevant,
            selected_thread: Some(3),
            parent_selected: false,
            hide_muted: false,
            standing: |_: &Thread| Standing::default(),
            now: now(),
        });
        assert_eq!(open, vec![3, 2, 1]);
    }
}
