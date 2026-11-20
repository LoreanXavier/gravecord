//! Pictures that failed to download are asked for again.
//!
//! The official client's loader (module 776231 `yt`) retries a failed picture with the backoff of module 158390
//! (500 ms first, at most ten times that, a random share of twice the current wait added each time) while it has
//! failed fewer than five times, waiting for the network first; after that it reports the failure and forgets the
//! picture, so the next element that shows it asks again. An `<img>` that failed is asked for again by the next
//! mount as well. This module keeps the same promises for pictures decoded from bytes:
//!
//! - a transient failure (no connection, a timeout, a 5xx, a 408 or 429, a body cut short) is retried with that
//!   backoff, five times;
//! - after that the picture rests: the next time something draws it (`Images::get` misses it) or asks for it
//!   (`Root::request_image`) it goes out again, once `REST_MS` has passed, so a view that is shown again recovers
//!   and a caller that asks every frame cannot turn it into a loop;
//! - a definite answer (another 4xx, as a 404) is not retried by itself, and a new demand sends it again only after
//!   `REST_PERMANENT_MS`;
//! - when the network comes back (the gateway connected again, or a picture arrived after failures) every picture
//!   resting after transient failures may go out at the next demand.
//!
//! Nothing here blocks: the bookkeeping is a few hash lookups, and the clock is an argument so it can be tested.

use gpui::Image;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

/// Module 158390's `min` as module 776231 constructs it: the first wait.
pub(super) const BACKOFF_MIN_MS: u64 = 500;
/// Module 158390's default `max`: ten times `min`.
pub(super) const BACKOFF_MAX_MS: u64 = 10 * BACKOFF_MIN_MS;
/// Module 776231 `yt`: retried while `backoff.fails < 5`.
pub(super) const MAX_RETRIES: u32 = 5;
/// How long a picture that gave up after transient failures rests before a new demand sends it again.
pub(super) const REST_MS: u64 = 10_000;
/// How long a picture the server refused (a 404) rests before a new demand sends it again.
pub(super) const REST_PERMANENT_MS: u64 = 300_000;

/// Why a download failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The network, the server's state or a body cut short: worth asking again.
    Transient,
    /// The server answered that the picture is not there (a 4xx other than 408 and 429).
    Permanent,
}

impl Failure {
    /// The class of an HTTP status (0 for a transport error or a body that does not decode).
    pub(crate) fn of_status(status: u16) -> Self {
        match status {
            408 | 429 => Self::Transient,
            400..=499 => Self::Permanent,
            _ => Self::Transient,
        }
    }
}

#[derive(Debug, PartialEq)]
enum State {
    /// A request is out.
    InFlight,
    /// A retry is scheduled and has not fired yet.
    Waiting,
    /// Nothing is scheduled; a demand after the rest sends it again. `at` is when it started resting.
    Resting { at: u64, permanent: bool },
}

#[derive(Debug)]
struct Entry {
    url: String,
    /// Failures since the last success or fresh demand (module 158390 `fails`).
    failures: u32,
    /// Module 158390 `_current`.
    current_ms: u64,
    state: State,
}

/// What a failure asks for: send the request again after `after_ms`.
#[derive(Debug, PartialEq)]
pub(super) struct Retry {
    pub after_ms: u64,
}

/// The answer to a new demand for a picture that was asked for before.
#[derive(Debug, PartialEq)]
pub(super) enum Demand {
    /// Send this url now; the entry is in flight again.
    Send(String),
    /// It rests until this time (ms on `now_ms`'s clock).
    Later(u64),
    /// In flight or a retry is scheduled: nothing to do.
    Busy,
}

/// The retry state of every picture asked for through `Root::request_image`.
#[derive(Default)]
pub(super) struct Retries {
    entries: HashMap<String, Entry>,
    /// The keys in `State::Resting`, for the draw-side check in `Images::get`.
    resting: HashSet<String>,
    /// The network came back at this time: transient rests started before it are over.
    online_since: u64,
    /// A transient failure happened since the last picture arrived.
    failing: bool,
}

impl Retries {
    /// The first request for `key` went out.
    pub(super) fn sent(&mut self, key: &str, url: &str) {
        self.resting.remove(key);
        self.entries.insert(
            key.to_owned(),
            Entry {
                url: url.to_owned(),
                failures: 0,
                current_ms: BACKOFF_MIN_MS,
                state: State::InFlight,
            },
        );
    }

    /// A new demand for `key` at `now_ms` (`url`, when the caller has one, replaces the remembered one).
    pub(super) fn demand(&mut self, key: &str, url: Option<&str>, now_ms: u64) -> Demand {
        let online_since = self.online_since;
        let Some(entry) = self.entries.get_mut(key) else {
            return Demand::Busy;
        };
        let State::Resting { at, permanent } = entry.state else {
            return Demand::Busy;
        };
        let due = if permanent {
            at + REST_PERMANENT_MS
        } else if online_since > at {
            // The network came back after it gave up: no need to wait out the rest.
            now_ms.min(at + REST_MS)
        } else {
            at + REST_MS
        };
        if now_ms < due {
            return Demand::Later(due);
        }
        if let Some(url) = url {
            url.clone_into(&mut entry.url);
        }
        entry.failures = 0;
        entry.current_ms = BACKOFF_MIN_MS;
        entry.state = State::InFlight;
        self.resting.remove(key);
        Demand::Send(entry.url.clone())
    }

    /// The download of `key` failed at `now_ms`. `Some` schedules a retry after the backoff (`jitter` in 0..1 is
    /// module 158390's `Math.random()`); `None` when the key is unknown, was already handled, or now rests.
    pub(super) fn failed(
        &mut self,
        key: &str,
        failure: Failure,
        now_ms: u64,
        jitter: f64,
    ) -> Option<Retry> {
        let entry = self.entries.get_mut(key)?;
        if entry.state != State::InFlight {
            // A second failure report for the same attempt.
            return None;
        }
        if failure == Failure::Transient {
            self.failing = true;
        }
        if failure == Failure::Permanent || entry.failures >= MAX_RETRIES {
            entry.state = State::Resting {
                at: now_ms,
                permanent: failure == Failure::Permanent,
            };
            self.resting.insert(key.to_owned());
            return None;
        }
        // Module 158390 `fail`: `n = 2 * current * random`, `current = min(current + n, max)`.
        entry.failures += 1;
        let grow = (2. * entry.current_ms as f64 * jitter.clamp(0., 1.)) as u64;
        entry.current_ms = (entry.current_ms + grow).min(BACKOFF_MAX_MS);
        entry.state = State::Waiting;
        Some(Retry {
            after_ms: entry.current_ms,
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

    /// The picture arrived: nothing more to keep. True when pictures were failing before it, which says the network
    /// works again (the caller then lets the resting ones go out at their next demand).
    pub(super) fn arrived(&mut self, key: &str) -> bool {
        self.resting.remove(key);
        self.entries.remove(key);
        std::mem::take(&mut self.failing)
    }

    /// The network is back at `now_ms` (the gateway connected, or a picture arrived after failures): the keys resting
    /// after transient failures, which their next demand now sends at once.
    pub(super) fn online(&mut self, now_ms: u64) -> Vec<String> {
        self.online_since = now_ms.max(1);
        self.entries
            .iter()
            .filter(|(_, entry)| {
                matches!(
                    entry.state,
                    State::Resting {
                        permanent: false,
                        ..
                    }
                )
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Whether `key` rests after failing (so a draw that misses it is a demand).
    pub(super) fn is_resting<Q>(&self, key: &Q) -> bool
    where
        String: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        !self.resting.is_empty() && self.resting.contains(key)
    }

    /// Failures recorded for `key` (0 when unknown): the harness's state line, the full profile's activity cards
    /// (UnknownGameIcon, module 263577), and the "Image failed to load." state of an attachment (module 251843).
    pub(super) fn failures(&self, key: &str) -> u32 {
        self.entries.get(key).map_or(0, |entry| entry.failures)
    }

    /// The keys with retry state that start with `prefix`, for the harness's state line.
    #[cfg(feature = "capture")]
    pub(super) fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut keys: Vec<String> = self
            .entries
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort_unstable();
        keys
    }

    /// A short word for the state of `key`, for the harness's state line.
    #[cfg(feature = "capture")]
    pub(super) fn state_word(&self, key: &str) -> &'static str {
        match self.entries.get(key).map(|entry| &entry.state) {
            None => "none",
            Some(State::InFlight) => "in-flight",
            Some(State::Waiting) => "waiting",
            Some(State::Resting {
                permanent: true, ..
            }) => "resting-404",
            Some(State::Resting { .. }) => "resting",
        }
    }
}

/// The downloaded pictures by key, with their retry state. It reads as the map it wraps; `get` also notes a draw
/// that misses a picture which failed and rests, so that `Root::image_demands` can ask for it again: a picture the
/// view shows again is retried without every draw site asking for it itself.
#[derive(Default)]
pub(super) struct Images {
    map: HashMap<String, Arc<Image>>,
    pub(super) retries: Retries,
    misses: RefCell<Vec<String>>,
    wake: Wake,
}

impl Images {
    /// The picture of `key`, when it arrived. A miss of a resting picture is remembered for the next frame.
    pub(super) fn get<Q>(&self, key: &Q) -> Option<&Arc<Image>>
    where
        String: Borrow<Q>,
        Q: Hash + Eq + ?Sized + AsRef<str>,
    {
        let found = self.map.get(key);
        if found.is_none() && self.retries.is_resting(key) {
            self.misses.borrow_mut().push(key.as_ref().to_owned());
        }
        found
    }

    /// The resting pictures the views missed since the last call, each once.
    pub(super) fn take_misses(&self) -> Vec<String> {
        let mut misses = std::mem::take(&mut *self.misses.borrow_mut());
        misses.sort_unstable();
        misses.dedup();
        misses
    }

    /// A shown picture rests until `due`: the view is drawn again then.
    pub(super) fn wake_at(&mut self, due: u64) {
        self.wake.wanted = Some(self.wake.wanted.map_or(due, |at| at.min(due)));
    }

    /// The time to set a timer for, when the earliest wanted wake is before the one already set (or none is set).
    pub(super) fn wake_to_schedule(&mut self) -> Option<u64> {
        let wanted = self.wake.wanted.take()?;
        if self.wake.scheduled.is_some_and(|at| at <= wanted) {
            return None;
        }
        self.wake.scheduled = Some(wanted);
        Some(wanted)
    }

    /// The timer set for `due` fired.
    pub(super) fn woke(&mut self, due: u64) {
        if self.wake.scheduled == Some(due) {
            self.wake.scheduled = None;
        }
    }
}

/// The redraw that lets a resting picture on a still view go out once its rest is over.
#[derive(Default)]
struct Wake {
    wanted: Option<u64>,
    scheduled: Option<u64>,
}

impl std::ops::Deref for Images {
    type Target = HashMap<String, Arc<Image>>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl std::ops::DerefMut for Images {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

impl<'a> IntoIterator for &'a Images {
    type Item = (&'a String, &'a Arc<Image>);
    type IntoIter = std::collections::hash_map::Iter<'a, String, Arc<Image>>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

/// Module 158390's `Math.random()`: a share in 0..1 that differs between calls.
pub(super) fn jitter() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(now_ms());
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Milliseconds on a clock that only moves forward, from the first call.
pub(super) fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// Whether a downloaded body is a whole picture: its format's end marker is there (a PNG's `IEND`, a JPEG's `EOI`,
/// a GIF's trailer, a RIFF's declared length). A body cut short would be stored and fail to decode for good; it
/// counts as a transient failure instead. Formats it does not know (SVG, Lottie, MP4) pass.
pub(crate) fn complete(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // The IEND chunk: length 0, type, CRC; some encoders add bytes after it.
        return bytes
            .windows(8)
            .rev()
            .take(64)
            .any(|window| window == b"IEND\xaeB`\x82");
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        // EOI, allowing for padding after it.
        return bytes
            .windows(2)
            .rev()
            .take(64)
            .any(|window| window == [0xff, 0xd9]);
    }
    if bytes.starts_with(b"GIF8") {
        return bytes.iter().rev().take(64).any(|byte| *byte == 0x3b);
    }
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        return bytes.len() >= declared.saturating_add(8);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_in_flight(retries: &mut Retries, key: &str, now: u64, jitter: f64) -> Option<Retry> {
        retries.failed(key, Failure::Transient, now, jitter)
    }

    #[test]
    fn the_backoff_is_module_158390() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        // Without jitter the wait stays at the minimum; with the most jitter it triples up to the cap.
        let mut waits = Vec::new();
        for _ in 0..MAX_RETRIES {
            waits.push(failed_in_flight(&mut retries, "k", 0, 1.).unwrap().after_ms);
            assert_eq!(retries.resend("k"), Some("u".to_owned()));
        }
        assert_eq!(waits, [1_500, 4_500, 5_000, 5_000, 5_000]);
        let mut calm = Retries::default();
        calm.sent("k", "u");
        assert_eq!(
            calm.failed("k", Failure::Transient, 0, 0.),
            Some(Retry { after_ms: 500 })
        );
    }

    #[test]
    fn a_failed_picture_is_retried_five_times_then_rests() {
        let mut retries = Retries::default();
        retries.sent("badge:a", "https://cdn/a.png");
        for round in 0..MAX_RETRIES {
            assert!(
                failed_in_flight(&mut retries, "badge:a", 0, 0.5).is_some(),
                "{round}"
            );
            assert_eq!(
                retries.resend("badge:a"),
                Some("https://cdn/a.png".to_owned())
            );
        }
        assert_eq!(failed_in_flight(&mut retries, "badge:a", 1_000, 0.5), None);
        assert!(retries.is_resting("badge:a"));
        // A demand during the rest waits; after it the picture goes out with a clean count.
        assert_eq!(
            retries.demand("badge:a", None, 1_000 + REST_MS - 1),
            Demand::Later(1_000 + REST_MS)
        );
        assert_eq!(
            retries.demand("badge:a", None, 1_000 + REST_MS),
            Demand::Send("https://cdn/a.png".to_owned())
        );
        assert!(!retries.is_resting("badge:a"));
        assert_eq!(retries.failures("badge:a"), 0);
        assert_eq!(
            failed_in_flight(&mut retries, "badge:a", 20_000, 0.),
            Some(Retry { after_ms: 500 })
        );
    }

    #[test]
    fn a_404_is_not_retried_by_itself_and_rests_longer() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        assert_eq!(retries.failed("k", Failure::Permanent, 100, 0.5), None);
        assert!(retries.is_resting("k"));
        // Even every frame asking for it sends nothing until the long rest is over.
        for now in (100..100 + REST_PERMANENT_MS).step_by(16_000) {
            assert!(matches!(retries.demand("k", None, now), Demand::Later(_)));
        }
        // Nor does the network coming back.
        retries.online(200);
        assert!(matches!(retries.demand("k", None, 300), Demand::Later(_)));
        assert_eq!(
            retries.demand("k", Some("u2"), 100 + REST_PERMANENT_MS),
            Demand::Send("u2".to_owned())
        );
    }

    #[test]
    fn the_network_coming_back_ends_the_rest_of_transient_failures() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        retries.sent("gone", "u404");
        for _ in 0..MAX_RETRIES {
            failed_in_flight(&mut retries, "k", 0, 0.);
            retries.resend("k");
        }
        assert_eq!(failed_in_flight(&mut retries, "k", 1_000, 0.), None);
        retries.failed("gone", Failure::Permanent, 1_000, 0.);
        assert!(matches!(retries.demand("k", None, 2_000), Demand::Later(_)));
        assert_eq!(retries.online(2_500), vec!["k".to_owned()]);
        assert_eq!(
            retries.demand("k", None, 2_600),
            Demand::Send("u".to_owned())
        );
        assert!(matches!(
            retries.demand("gone", None, 2_600),
            Demand::Later(_)
        ));
    }

    #[test]
    fn an_arrival_after_failures_says_the_network_works() {
        let mut retries = Retries::default();
        retries.sent("a", "u");
        retries.sent("b", "v");
        assert!(!retries.arrived("b"), "nothing failed before");
        retries.sent("b", "v");
        failed_in_flight(&mut retries, "a", 0, 0.);
        assert!(retries.arrived("b"));
        assert!(!retries.arrived("c"), "said once per failing spell");
    }

    #[test]
    fn a_picture_that_arrived_is_not_sent_again() {
        let mut retries = Retries::default();
        retries.sent("badge:a", "https://cdn/a.png");
        assert!(failed_in_flight(&mut retries, "badge:a", 0, 0.).is_some());
        retries.arrived("badge:a");
        assert_eq!(retries.resend("badge:a"), None);
        assert_eq!(failed_in_flight(&mut retries, "badge:a", 5, 0.), None);
        assert_eq!(retries.demand("badge:a", None, u64::MAX), Demand::Busy);
    }

    #[test]
    fn a_duplicate_failure_report_does_not_count_twice() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        assert!(failed_in_flight(&mut retries, "k", 0, 0.).is_some());
        assert_eq!(failed_in_flight(&mut retries, "k", 1, 0.), None);
        assert_eq!(retries.failures("k"), 1);
    }

    #[test]
    fn demand_for_a_picture_in_flight_or_waiting_does_not_send_twice() {
        let mut retries = Retries::default();
        retries.sent("k", "u");
        assert_eq!(retries.demand("k", Some("u"), u64::MAX), Demand::Busy);
        assert!(failed_in_flight(&mut retries, "k", 0, 0.).is_some());
        assert_eq!(retries.demand("k", Some("u"), u64::MAX), Demand::Busy);
        assert_eq!(retries.demand("unknown", Some("u"), u64::MAX), Demand::Busy);
    }

    #[test]
    fn statuses_are_classed() {
        assert_eq!(Failure::of_status(0), Failure::Transient);
        assert_eq!(Failure::of_status(404), Failure::Permanent);
        assert_eq!(Failure::of_status(403), Failure::Permanent);
        assert_eq!(Failure::of_status(408), Failure::Transient);
        assert_eq!(Failure::of_status(429), Failure::Transient);
        assert_eq!(Failure::of_status(500), Failure::Transient);
        assert_eq!(Failure::of_status(503), Failure::Transient);
    }

    #[test]
    fn a_draw_that_misses_a_resting_picture_is_noted_once() {
        let mut images = Images::default();
        images.retries.sent("rest", "u");
        images.retries.failed("rest", Failure::Permanent, 0, 0.);
        images.retries.sent("loading", "v");
        assert!(images.get("rest").is_none());
        assert!(images.get(&"rest".to_owned()).is_none());
        assert!(images.get("loading").is_none());
        assert!(images.get("never").is_none());
        assert_eq!(images.take_misses(), vec!["rest".to_owned()]);
        assert!(images.take_misses().is_empty());
    }

    #[test]
    fn a_still_view_is_woken_once_for_the_earliest_rest() {
        let mut images = Images::default();
        assert_eq!(images.wake_to_schedule(), None);
        images.wake_at(30);
        images.wake_at(20);
        assert_eq!(images.wake_to_schedule(), Some(20));
        // Every frame asking again for the same or a later time sets no second timer.
        images.wake_at(20);
        images.wake_at(25);
        assert_eq!(images.wake_to_schedule(), None);
        // An earlier one does.
        images.wake_at(10);
        assert_eq!(images.wake_to_schedule(), Some(10));
        images.woke(20);
        images.woke(10);
        images.wake_at(40);
        assert_eq!(images.wake_to_schedule(), Some(40));
    }

    #[test]
    fn bodies_cut_short_are_not_whole_pictures() {
        let png = {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            bytes.extend_from_slice(&[0; 20]);
            bytes.extend_from_slice(b"\0\0\0\0IEND\xaeB`\x82");
            bytes
        };
        assert!(complete(&png));
        assert!(!complete(&png[..png.len() - 3]));
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 0xff, 0xd9];
        assert!(complete(&jpeg));
        assert!(!complete(&jpeg[..6]));
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&8u32.to_le_bytes());
        webp.extend_from_slice(b"WEBPVP8 ");
        assert!(complete(&webp));
        assert!(!complete(&webp[..14]));
        assert!(complete(b"GIF89a....;"));
        assert!(!complete(b"GIF89a...."));
        assert!(complete(b"<svg/>"));
        assert!(!complete(b""));
    }
}
