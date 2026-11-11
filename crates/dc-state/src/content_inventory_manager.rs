//! Global-feed scheduling from ContentInventoryManager (819235).

use crate::content_inventory::Feed;

#[derive(Clone, Copy)]
pub struct Conditions {
    pub enabled: bool,
    pub connected: bool,
    pub focused: bool,
    pub hidden: bool,
    pub idle_ms: u64,
}

impl Conditions {
    fn allows(self, feed: Option<&Feed>) -> bool {
        self.enabled
            && self.connected
            && self.focused
            && !(self.hidden && feed.is_some())
            && self.idle_ms <= 15 * 60 * 1000
    }
}

#[derive(Default)]
pub struct ContentInventoryManager {
    pending: bool,
    failures: u32,
    stale_at: Option<i64>,
    next_fetch: Option<i64>,
}

impl ContentInventoryManager {
    pub fn next_fetch(&self) -> Option<i64> {
        self.next_fetch
    }

    pub fn disconnected(&mut self) {
        self.next_fetch = None;
    }

    pub fn schedule(&mut self, feed: Option<&Feed>, conditions: Conditions, now_ms: i64) {
        // The retry timer owns its deadline until a successful response resets the counter.
        if (1..=4).contains(&self.failures) {
            return;
        }
        self.next_fetch = None;
        if self.pending || !conditions.allows(feed) {
            return;
        }
        if feed.is_some_and(|f| f.refresh_stale_inbox_after_ms.is_some()) && self.stale_at.is_none()
        {
            return;
        }
        self.next_fetch = Some(
            now_ms
                .max(self.stale_at.unwrap_or(now_ms))
                .max(feed.and_then(|f| f.expires_at).unwrap_or(now_ms)),
        );
    }

    pub fn inbox_stale(&mut self, feed: Option<&Feed>, after_ms: Option<u64>, now_ms: i64) {
        if let Some(default) = feed.and_then(|f| f.refresh_stale_inbox_after_ms) {
            let after = i64::try_from(after_ms.unwrap_or(default)).unwrap_or(i64::MAX);
            // The source stores a UTCString, which drops milliseconds.
            self.stale_at = Some(now_ms.saturating_add(after).div_euclid(1000) * 1000);
        }
    }

    pub fn begin_fetch(
        &mut self,
        feed: Option<&Feed>,
        conditions: Conditions,
        now_ms: i64,
    ) -> bool {
        if !self.next_fetch.is_some_and(|at| at <= now_ms) {
            return false;
        }
        self.next_fetch = None;
        if self.pending || !conditions.allows(feed) {
            return false;
        }
        self.pending = true;
        true
    }

    pub fn succeeded(&mut self) {
        self.pending = false;
        self.failures = 0;
        self.stale_at = None;
    }

    /// Returns true when the fifth failed request must clear the feed. `random` supplies Math.random().
    pub fn failed(&mut self, now_ms: i64, random: f64) -> bool {
        self.pending = false;
        if self.failures < 4 {
            let delay = 60_000 * 2_i64.pow(self.failures);
            let jitter = (random.clamp(0., 1.) * f64::from(self.failures + 1) * 120_000.) as i64;
            self.next_fetch = Some(now_ms.saturating_add(delay).saturating_add(jitter));
            self.failures += 1;
            false
        } else {
            self.next_fetch = None;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_inventory::{ContentInventoryStore, GLOBAL_FEED};
    use serde_json::json;

    fn active() -> Conditions {
        Conditions {
            enabled: true,
            connected: true,
            focused: true,
            hidden: false,
            idle_ms: 0,
        }
    }

    #[test]
    fn initial_fetch_requires_focus_connection_and_recent_input_but_not_visible_cards() {
        let mut manager = ContentInventoryManager::default();
        for conditions in [
            Conditions {
                enabled: false,
                ..active()
            },
            Conditions {
                connected: false,
                ..active()
            },
            Conditions {
                focused: false,
                ..active()
            },
            Conditions {
                idle_ms: 900_001,
                ..active()
            },
        ] {
            manager.schedule(None, conditions, 100);
            assert_eq!(manager.next_fetch(), None);
        }
        let hidden = Conditions {
            hidden: true,
            idle_ms: 900_000,
            ..active()
        };
        manager.schedule(None, hidden, 100);
        assert!(manager.begin_fetch(None, hidden, 100));
        manager.schedule(None, active(), 101);
        assert_eq!(
            manager.next_fetch(),
            None,
            "only one request may be pending"
        );
    }

    #[test]
    fn push_refresh_waits_for_both_expiry_and_stale_deadline_and_rechecks_visibility() {
        let mut store = ContentInventoryStore::default();
        store.set_feed(GLOBAL_FEED, &json!({"entries":[],"wait_ms_until_next_fetch":10000,"refresh_stale_inbox_after_ms":5000}), 1000);
        let feed = store.get_feed(GLOBAL_FEED);
        let mut manager = ContentInventoryManager::default();
        manager.schedule(feed, active(), 1000);
        assert_eq!(
            manager.next_fetch(),
            None,
            "push-driven feeds do not poll at expiry"
        );
        manager.inbox_stale(feed, None, 2345);
        manager.schedule(feed, active(), 2345);
        assert_eq!(manager.next_fetch(), Some(11000));
        manager.inbox_stale(feed, Some(20000), 2345);
        manager.schedule(feed, active(), 2345);
        assert_eq!(manager.next_fetch(), Some(22000));
        assert!(!manager.begin_fetch(feed, active(), 21999));
        assert!(!manager.begin_fetch(
            feed,
            Conditions {
                hidden: true,
                ..active()
            },
            22000
        ));
        manager.schedule(feed, active(), 23000);
        assert!(manager.begin_fetch(feed, active(), 23000));
        manager.succeeded();
        manager.schedule(feed, active(), 24000);
        assert_eq!(
            manager.next_fetch(),
            None,
            "success consumes the stale notification"
        );
    }

    #[test]
    fn polling_resumes_after_focus_and_disconnect_and_hidden_feed_suppresses_it() {
        let mut store = ContentInventoryStore::default();
        store.set_feed(
            GLOBAL_FEED,
            &json!({"entries":[],"wait_ms_until_next_fetch":10000}),
            0,
        );
        let feed = store.get_feed(GLOBAL_FEED);
        let mut manager = ContentInventoryManager::default();
        manager.schedule(feed, active(), 0);
        assert_eq!(manager.next_fetch(), Some(10000));
        manager.schedule(
            feed,
            Conditions {
                focused: false,
                ..active()
            },
            10,
        );
        assert_eq!(manager.next_fetch(), None);
        manager.schedule(feed, active(), 20000);
        assert_eq!(manager.next_fetch(), Some(20000));
        manager.disconnected();
        assert_eq!(manager.next_fetch(), None);
        manager.schedule(
            feed,
            Conditions {
                hidden: true,
                ..active()
            },
            21000,
        );
        assert_eq!(manager.next_fetch(), None);
        manager.schedule(feed, active(), 22000);
        assert!(manager.begin_fetch(feed, active(), 22000));
    }

    #[test]
    fn four_exponential_retries_then_clear_and_success_resets_backoff() {
        let mut manager = ContentInventoryManager::default();
        manager.schedule(None, active(), 0);
        assert!(manager.begin_fetch(None, active(), 0));
        let mut now = 0;
        for delay in [120000, 240000, 420000, 720000] {
            assert!(!manager.failed(now, 0.5));
            assert_eq!(manager.next_fetch(), Some(now + delay));
            manager.schedule(None, active(), now + 1);
            assert_eq!(
                manager.next_fetch(),
                Some(now + delay),
                "focus must not shorten backoff"
            );
            now += delay;
            assert!(manager.begin_fetch(None, active(), now));
        }
        assert!(manager.failed(now, 0.5));
        manager.schedule(None, active(), now + 1);
        assert_eq!(manager.next_fetch(), None);
        manager.succeeded();
        manager.schedule(None, active(), now + 2);
        assert!(manager.begin_fetch(None, active(), now + 2));
        assert!(!manager.failed(now + 2, 0.));
        assert_eq!(manager.next_fetch(), Some(now + 60002));
    }

    /// The New Player badge reads a live first-session entry of the feed: four failed refreshes keep the feed (and
    /// the badge), the fifth clears it, and a success in between starts the count again.
    #[test]
    fn failed_refreshes_keep_the_feed_entry_until_the_fifth() {
        let mut store = ContentInventoryStore::default();
        let body = json!({"entries": [{"content": {
            "id": "30", "author_id": "7", "author_type": 1, "content_type": 1,
            "traits": [{"type": 3, "is_live": true}, {"type": 1, "first_time": true}],
            "extra": {"application_id": "9"},
        }}]});
        let activity = json!({"type": 0, "application_id": "9", "name": "G"});
        store.set_feed(GLOBAL_FEED, &body, 0);
        let badge = |store: &ContentInventoryStore| {
            store
                .matching_inbox_entry(GLOBAL_FEED, 7, &activity)
                .is_some_and(|entry| entry.first_time())
        };
        let mut manager = ContentInventoryManager::default();
        assert!(badge(&store));
        for round in 0..2 {
            for _ in 0..4 {
                if manager.failed(0, 0.5) {
                    store.clear_feed(GLOBAL_FEED);
                }
                assert!(badge(&store), "a failed refresh must not drop the badge");
            }
            if round == 0 {
                manager.succeeded();
                store.set_feed(GLOBAL_FEED, &body, 1);
            }
        }
        if manager.failed(0, 0.5) {
            store.clear_feed(GLOBAL_FEED);
        }
        assert!(!badge(&store), "the fifth failure clears the feed");
    }
}
