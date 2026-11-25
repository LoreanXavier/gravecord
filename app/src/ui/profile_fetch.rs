//! When a user's profile is read again. Port of module 917445 `E` (`fetchProfile`): not while a read of it is under
//! way; not when the profile is in, with what the request asks for, and was read less than a minute ago; and a 404
//! or 429 answer is not asked again within that minute. A route stands for what a request asks for (its type, mutual
//! servers and friends, server).

use super::Root;
use crate::net::Request;
use dc_state::Id;
use std::collections::HashMap;

/// 917445: a profile read is stale a minute after it ended (`6e4`).
const STALE_MS: u64 = 60_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Read {
    user: Id,
    in_flight: bool,
    /// `fetchEndedAt`; 0 before the first answer.
    ended_ms: u64,
    /// The failed answer's status (`fetchError.status`), 0 when it succeeded.
    status: u16,
}

/// The profile reads by route.
#[derive(Debug, Default)]
pub(super) struct ProfileFetches {
    reads: HashMap<String, Read>,
}

impl ProfileFetches {
    /// Whether `route` (a read of `user`) goes out now, `have` saying the profile is already in. Marks it started.
    fn start(&mut self, user: Id, route: &str, have: bool, now_ms: u64) -> bool {
        let read = self.reads.get(route).copied();
        if read.is_some_and(|read| read.in_flight) {
            return false;
        }
        let ended = read.map_or(0, |read| read.ended_ms);
        let stale = now_ms.saturating_sub(ended) >= STALE_MS;
        if read.is_some_and(|read| matches!(read.status, 404 | 429)) && !stale {
            return false;
        }
        // In with what this route asks for (it was answered) and fresh: nothing to read.
        if have && read.is_some_and(|read| read.ended_ms > 0) && !stale {
            return false;
        }
        self.reads.insert(
            route.to_owned(),
            Read {
                user,
                in_flight: true,
                ended_ms: ended,
                status: read.map_or(0, |read| read.status),
            },
        );
        true
    }

    /// `USER_PROFILE_FETCH_SUCCESS` / `_FAILURE`: the reads of `user` under way have ended, with `status` (0 for a
    /// success).
    fn ended(&mut self, user: Id, status: u16, now_ms: u64) {
        for read in self.reads.values_mut() {
            if read.user == user && read.in_flight {
                read.in_flight = false;
                read.ended_ms = now_ms;
                read.status = status;
            }
        }
    }
}

impl Root {
    /// Reads `user`'s profile with `route` into `profile:<user>`, as `fetchProfile` decides.
    pub(super) fn fetch_user_profile(&mut self, user: Id, route: String) {
        let key = format!("profile:{user}");
        let have = self
            .fetched
            .get(&key)
            .is_some_and(|body| body["fetch_failed"] != true);
        if self.profile_fetches.start(user, &route, have, now_ms()) {
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    /// A `profile:<user>` answer came in.
    pub(super) fn profile_answered(&mut self, key: &str, body: &serde_json::Value) {
        let Some(user) = key
            .strip_prefix("profile:")
            .and_then(|user| user.parse::<Id>().ok())
        else {
            return;
        };
        let status = if body["fetch_failed"] == true {
            body["status"].as_u64().unwrap_or(0) as u16
        } else {
            0
        };
        self.profile_fetches.ended(user, status, now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = "/users/7/profile?type=sidebar";

    #[test]
    fn a_profile_is_read_once_a_minute_and_never_twice_at_once() {
        let mut reads = ProfileFetches::default();
        assert!(reads.start(7, ROUTE, false, 1_000));
        // Opening the DM again before the answer: no second read.
        assert!(!reads.start(7, ROUTE, false, 1_500));
        reads.ended(7, 0, 2_000);
        // Back and forth between channels within the minute: the profile is in.
        assert!(!reads.start(7, ROUTE, true, 5_000));
        assert!(!reads.start(7, ROUTE, true, 61_999));
        assert!(reads.start(7, ROUTE, true, 62_000));
    }

    #[test]
    fn another_route_asks_for_other_parts() {
        let mut reads = ProfileFetches::default();
        assert!(reads.start(7, ROUTE, false, 1_000));
        reads.ended(7, 0, 2_000);
        assert!(reads.start(7, "/users/7/profile?type=popout", true, 3_000));
    }

    #[test]
    fn a_missing_or_limited_profile_waits_a_minute_and_other_failures_retry() {
        let mut reads = ProfileFetches::default();
        assert!(reads.start(7, ROUTE, false, 1_000));
        reads.ended(7, 429, 2_000);
        assert!(!reads.start(7, ROUTE, false, 30_000));
        assert!(reads.start(7, ROUTE, false, 62_000));
        reads.ended(7, 500, 63_000);
        assert!(reads.start(7, ROUTE, false, 64_000));
    }
}
