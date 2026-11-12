//! The idle rule the account's presence follows. `IdleStore` (module 885576) keeps the time of the last activity and
//! on the desktop takes the system's input age every ten seconds; it is idle after ten minutes without activity, and
//! `AFK` (push notifications go to the phone) after the account's `afk_timeout` or ten minutes, whichever is less.
//! `SelfPresenceStore` (module 894768) turns an online account that is idle into `idle`, and op 3 carries
//! `since` (the moment the idle began) and `afk`.

/// Module 885576 `N`: `Date.now() - h > 6e5`, the idle limit, and the cap of the AFK limit (`Math.min(e * 1e3, 6e5)`).
pub const IDLE_AFTER_MS: u64 = 600_000;

/// The account's `afk_timeout` until it sets one (`mobile_notification_delay`: ten minutes).
pub const DEFAULT_AFK_SECONDS: u64 = 600;

/// What `IdleStore` says now: when the idle began (`getIdleSince`, none while active) and whether the account is AFK.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Idle {
    pub since_ms: Option<u64>,
    pub afk: bool,
}

impl Idle {
    /// One reading. `last_input_age_ms` is the system's input age (`getSystemIdleTimeMs`); `app_activity_ms` is the
    /// last moment the app saw the account act (a channel opened, a word spoken), so the last activity `h` is the later
    /// of the two. An `afk_seconds` of zero is AFK at once (`0 === e` in `N`).
    pub fn sample(
        now_ms: u64,
        last_input_age_ms: u64,
        app_activity_ms: u64,
        afk_seconds: u64,
    ) -> Self {
        let last = now_ms
            .saturating_sub(last_input_age_ms)
            .max(app_activity_ms.min(now_ms));
        let quiet = now_ms - last;
        Self {
            since_ms: (quiet > IDLE_AFTER_MS).then_some(last),
            afk: afk_seconds == 0 || quiet > afk_seconds.saturating_mul(1000).min(IDLE_AFTER_MS),
        }
    }

    /// `since` of op 3 (`b` in module 894768): zero while active.
    pub fn since(&self) -> u64 {
        self.since_ms.unwrap_or(0)
    }
}

/// The status op 3 carries (`H` in module 894768): the saved one, and an online account that is idle is `idle`.
pub fn wire_status(status: &str, idle: &Idle) -> &'static str {
    match crate::games::presence_status(status) {
        "online" if idle.since() > 0 => "idle",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 10_000_000;

    #[test]
    fn active_input_is_neither_idle_nor_afk() {
        let idle = Idle::sample(NOW, 30_000, 0, 600);
        assert_eq!(idle, Idle::default());
        assert_eq!(idle.since(), 0);
    }

    #[test]
    fn eleven_minutes_without_input_is_idle_since_the_last_input() {
        let age = 11 * 60_000;
        let idle = Idle::sample(NOW, age, 0, 600);
        assert_eq!(idle.since_ms, Some(NOW - age));
        assert!(idle.afk);
    }

    #[test]
    fn the_afk_limit_is_the_timeout_capped_at_ten_minutes() {
        // Five minutes without input, a two-minute timeout: AFK but not idle.
        let idle = Idle::sample(NOW, 300_000, 0, 120);
        assert!(idle.afk && idle.since_ms.is_none());
        // The same age with the default ten minutes: still active.
        assert!(!Idle::sample(NOW, 300_000, 0, 600).afk);
        // A longer timeout is capped at ten minutes.
        assert!(Idle::sample(NOW, 601_000, 0, 3600).afk);
        // Zero is AFK at once.
        assert!(Idle::sample(NOW, 0, 0, 0).afk);
    }

    #[test]
    fn what_the_app_saw_counts_as_activity() {
        let age = 30 * 60_000;
        let idle = Idle::sample(NOW, age, NOW - 1_000, 600);
        assert_eq!(idle, Idle::default());
    }

    #[test]
    fn only_an_online_account_turns_idle() {
        let idle = Idle {
            since_ms: Some(5),
            afk: true,
        };
        assert_eq!(wire_status("online", &idle), "idle");
        assert_eq!(wire_status("", &idle), "idle");
        assert_eq!(wire_status("dnd", &idle), "dnd");
        assert_eq!(wire_status("invisible", &idle), "invisible");
        assert_eq!(wire_status("online", &Idle::default()), "online");
    }
}
