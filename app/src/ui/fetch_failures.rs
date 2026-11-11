//! Failed `Request::Fetch` bodies (`net::fetch_failure`): which keys retry, and when.
use super::Root;
use crate::net::is_fetch_failure;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A failed key may be asked for again this long after it failed, so a dead route is tried twice a minute at
/// most however often the view renders.
pub(super) const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Keys whose readers inspect the failure body themselves.
fn reads_own_failure(key: &str) -> bool {
    key.starts_with("invite:")
        || key.starts_with("oauth-token:")
        || key.starts_with(super::activity_shelf::SHELF_PREFIX)
        || key == "content-inventory:feed"
}

/// When `failures` holds `key` from at least `RETRY_AFTER` ago, forgets it and says the key may be fetched again.
pub(super) fn take_due(failures: &mut HashMap<String, Instant>, key: &str, now: Instant) -> bool {
    match failures.get(key) {
        Some(at) if now.duration_since(*at) >= RETRY_AFTER => {
            failures.remove(key);
            true
        }
        _ => false,
    }
}

impl Root {
    /// Takes a failed fetch: remembers when it failed and keeps it away from consumers that expect data.
    pub(super) fn fetch_failed(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !is_fetch_failure(body) || reads_own_failure(key) {
            return false;
        }
        self.fetch_failures.insert(key.to_owned(), Instant::now());
        // The steps that retry run when something changed: the wait ending is that change.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RETRY_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.data_revision += 1;
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// Whether a failed `key` is due another try; when it is, the request guards of its caller are cleared.
    pub(super) fn retry_failed_fetch(&mut self, key: &str) -> bool {
        if !take_due(&mut self.fetch_failures, key, Instant::now()) {
            return false;
        }
        self.requested.remove(key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_key_is_retried_once_after_the_wait_and_not_before() {
        let mut failures = HashMap::new();
        let start = Instant::now();
        failures.insert("k".to_owned(), start);
        assert!(!take_due(
            &mut failures,
            "k",
            start + Duration::from_secs(5)
        ));
        assert!(take_due(&mut failures, "k", start + RETRY_AFTER));
        assert!(!take_due(&mut failures, "k", start + RETRY_AFTER * 2));
        assert!(!take_due(&mut failures, "other", start + RETRY_AFTER));
    }

    #[test]
    fn keys_that_read_their_own_failure_are_not_taken() {
        assert!(reads_own_failure("invite:abc"));
        assert!(reads_own_failure("content-inventory:feed"));
        assert!(!reads_own_failure("guild-nav:events:1"));
    }
}
