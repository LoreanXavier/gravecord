//! Pictures that failed to download are asked for again.
//!
//! `Root::request_image` sends each key once; before this a download that failed (a connection reset, a pooled
//! connection the server had closed, a stalled request that timed out) was never repeated, so the picture stayed
//! missing for the rest of the session: an empty badge slot, a blank avatar. The official client's image loader
//! (`<img>` in Chromium) leaves a failed request to the next layout, which asks again; this keeps the same promise
//! for pictures that are decoded from bytes: a failure is retried with a doubling delay, and a key that gave up is
//! asked again by the next demand once a cooldown has passed.
//!
//! The bookkeeping takes the clock as an argument so it can be tested.

use std::collections::HashMap;

/// Delay before the first retry; each further failure doubles it.
pub(super) const FIRST_DELAY_MS: u64 = 1_000;
/// The longest delay between two attempts.
pub(super) const MAX_DELAY_MS: u64 = 30_000;
/// Failures in a row after which a key stops being retried by itself.
pub(super) const MAX_FAILURES: u32 = 6;
/// How long after giving up a new demand for the same picture may start downloading again.
pub(super) const COOLDOWN_MS: u64 = 60_000;

#[derive(Debug, PartialEq)]
enum State {
    /// A request is out.
    InFlight,
    /// A retry is scheduled and has not fired yet.
    Waiting,
    /// Failed `MAX_FAILURES` times; nothing is scheduled. `at` is when it gave up.
    GaveUp { at: u64 },
}

#[derive(Debug)]
struct Entry {
    url: String,
    failures: u32,
    state: State,
}

/// What a failure asks for: send the request again after `after_ms`.
#[derive(Debug, PartialEq)]
pub(super) struct Retry {
    pub after_ms: u64,
}

/// The retry state of every picture asked for through `Root::request_image`.
#[derive(Default)]
pub(super) struct Retries {
    entries: HashMap<String, Entry>,
}

impl Retries {
    /// The first request for `key` went out.
    pub(super) fn sent(&mut self, key: &str, url: &str) {
        self.entries.insert(
            key.to_owned(),
            Entry {
                url: url.to_owned(),
                failures: 0,
                state: State::InFlight,
            },
        );
    }

    /// A new demand for a picture that was asked for before: true when it gave up long enough ago that its request
    /// should go out again now (the entry is then in flight with a clean count).
    pub(super) fn again(&mut self, key: &str, url: &str, now_ms: u64) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        match entry.state {
            State::GaveUp { at } if now_ms.saturating_sub(at) >= COOLDOWN_MS => {
                entry.url = url.to_owned();
                entry.failures = 0;
                entry.state = State::InFlight;
                true
            }
            _ => false,
        }
    }

    /// The download of `key` failed at `now_ms`. `Some` schedules a retry; `None` when the key is unknown or has
    /// used up its attempts.
    pub(super) fn failed(&mut self, key: &str, now_ms: u64) -> Option<Retry> {
        let entry = self.entries.get_mut(key)?;
        if entry.state == State::Waiting {
            // A second failure report for the same attempt.
            return None;
        }
        entry.failures += 1;
        if entry.failures >= MAX_FAILURES {
            entry.state = State::GaveUp { at: now_ms };
            return None;
        }
        entry.state = State::Waiting;
        Some(Retry {
            after_ms: delay_ms(entry.failures),
        })
    }

    /// The scheduled retry of `key` is due: the url to request again, with the entry back in flight. `None` when the
    /// picture arrived or was dropped meanwhile.
    pub(super) fn resend(&mut self, key: &str) -> Option<String> {
        let entry = self.entries.get_mut(key)?;
        if entry.state != State::Waiting {
            return None;
        }
        entry.state = State::InFlight;
        Some(entry.url.clone())
    }

    /// The picture arrived: nothing more to keep.
    pub(super) fn arrived(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Failures recorded for `key` (0 when unknown), for the harness's state line.
    #[cfg(any(test, feature = "capture"))]
    pub(super) fn failures(&self, key: &str) -> u32 {
        self.entries.get(key).map_or(0, |entry| entry.failures)
    }
}

/// The wait after the `failures`-th failure in a row: 1 s, 2 s, 4 s, 8 s, 16 s, then 30 s at most.
pub(super) fn delay_ms(failures: u32) -> u64 {
    FIRST_DELAY_MS
        .saturating_mul(1u64 << failures.saturating_sub(1).min(16))
        .min(MAX_DELAY_MS)
}

/// Milliseconds on a clock that only moves forward, from the first call.
pub(super) fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_up_to_the_cap() {
        let all: Vec<u64> = (1..=7).map(delay_ms).collect();
        assert_eq!(all, [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]);
    }

    #[test]
    fn a_failed_picture_is_scheduled_and_sent_again() {
        let mut retries = Retries::default();
        retries.sent("badge:a", "https://cdn/a.png");
        assert_eq!(
            retries.failed("badge:a", 10),
            Some(Retry { after_ms: 1_000 })
        );
        assert_eq!(
            retries.resend("badge:a"),
            Some("https://cdn/a.png".to_owned())
        );
        // The second attempt fails too: a longer wait.
        assert_eq!(
            retries.failed("badge:a", 2_000),
            Some(Retry { after_ms: 2_000 })
        );
        assert_eq!(retries.failures("badge:a"), 2);
    }

    #[test]
    fn a_picture_that_arrived_is_not_sent_again() {
        let mut retries = Retries::default();
        retries.sent("badge:a", "https://cdn/a.png");
        assert!(retries.failed("badge:a", 0).is_some());
        retries.arrived("badge:a");
        assert_eq!(retries.resend("badge:a"), None);
        assert_eq!(retries.failed("badge:a", 5), None);
    }

    #[test]
    fn a_duplicate_failure_report_does_not_count_twice() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        assert!(retries.failed("k", 0).is_some());
        assert_eq!(retries.failed("k", 1), None);
        assert_eq!(retries.failures("k"), 1);
    }

    #[test]
    fn it_gives_up_after_the_attempts_and_a_later_demand_starts_again() {
        let mut retries = Retries::default();
        retries.sent("k", "u1");
        for round in 1..MAX_FAILURES {
            assert!(retries.failed("k", 0).is_some(), "failure {round}");
            assert_eq!(retries.resend("k"), Some("u1".to_owned()));
        }
        assert_eq!(
            retries.failed("k", 100_000),
            None,
            "the last failure gives up"
        );
        // Demand during the cooldown does nothing; after it the picture is asked for again with a clean count.
        assert!(!retries.again("k", "u2", 100_000 + COOLDOWN_MS - 1));
        assert!(retries.again("k", "u2", 100_000 + COOLDOWN_MS));
        assert_eq!(retries.failures("k"), 0);
        assert_eq!(
            retries.failed("k", 200_000),
            Some(Retry { after_ms: 1_000 })
        );
        assert_eq!(retries.resend("k"), Some("u2".to_owned()));
    }

    #[test]
    fn demand_for_a_picture_in_flight_or_waiting_does_not_send_twice() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        assert!(!retries.again("k", "u", u64::MAX));
        assert!(retries.failed("k", 0).is_some());
        assert!(!retries.again("k", "u", u64::MAX));
        assert!(!retries.again("unknown", "u", u64::MAX));
    }
}
