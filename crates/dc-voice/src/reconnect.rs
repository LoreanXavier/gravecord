//! `RTCConnection`'s reconnect (module 921747): what happens after a connection ended by itself.
//!
//! The official client never gives up on a call it did not leave. Every disconnect that is not its own `Force Close`
//! schedules `reconnect` after a jittered backoff of 1 to 10 s (`tK() = new Backoff(1e3, 1e4)`), on the same endpoint
//! and token, with a fresh identify (`e.close(); e.connect()` on the same socket). Refused or invalid DAVE messages
//! (4803, 4804, 4805) use a second backoff of the same shape (`_scheduleMLSFailureReconnect`). Nothing waits for a new
//! `VOICE_SERVER_UPDATE`: a new server only arrives that way, and replaces the endpoint when it does.
use crate::DisconnectReason;
use dc_proto::backoff::Backoff;
use dc_proto::voice_op::close;
use std::time::Duration;

/// What follows a connection that ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// The call is over: left here, or the service removed this session (4014).
    Stop,
    /// Connect again to the same endpoint with the same token after the delay.
    Retry(Duration),
}

#[derive(Debug)]
pub struct Reconnect {
    backoff: Backoff,
    mls: Backoff,
}

impl Default for Reconnect {
    fn default() -> Self {
        Self::new()
    }
}

fn tk() -> Backoff {
    Backoff::new(Duration::from_secs(1), Duration::from_secs(10))
}

impl Reconnect {
    pub fn new() -> Self {
        Self {
            backoff: tk(),
            mls: tk(),
        }
    }

    /// `_handleDisconnect`: the next step after `reason`.
    pub fn after(&mut self, reason: &DisconnectReason, rng: &mut impl rand::Rng) -> Next {
        match reason {
            DisconnectReason::Requested | DisconnectReason::Kicked => Next::Stop,
            DisconnectReason::Closed(
                close::REPEATED_MLS_INVALID_MESSAGES
                | close::DAVE_DOWNGRADE_REFUSED
                | close::DAVE_INVALID_MESSAGE,
            ) => {
                // `_scheduleMLSFailureReconnect`: the other backoff is cancelled, its fails kept.
                Next::Retry(self.mls.fail(rng))
            }
            _ => Next::Retry(self.backoff.fail(rng)),
        }
    }

    /// `_backoff.succeed()` once the media path selected its protocol ([`crate::Event::ProtocolSelected`]).
    pub fn routed(&mut self) {
        self.backoff.succeed();
    }

    /// `_handleMLSSuccess`: the end-to-end encrypted group works ([`crate::Event::Connected`]).
    pub fn secured(&mut self) {
        self.mls.succeed();
    }

    /// Failed attempts since the media path last worked.
    pub fn fails(&self) -> u32 {
        self.backoff.fails()
    }

    /// Op 32 goes only with the first NO_ROUTE of a cycle (`0 === this._backoff.fails`).
    pub fn report_no_route(&self) -> bool {
        self.backoff.fails() == 0
    }

    /// `resetBackoff` for a system event while a retry waits: the wait is cut short. `true` when one was waiting.
    pub fn reset(&mut self) -> bool {
        let waiting = self.backoff.fails() > 0 || self.mls.fails() > 0;
        self.backoff.succeed();
        self.mls.succeed();
        waiting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unrequested_end_retries_within_one_to_ten_seconds() {
        let mut rng = rand::rng();
        let mut reconnect = Reconnect::new();
        assert_eq!(
            reconnect.after(&DisconnectReason::Requested, &mut rng),
            Next::Stop
        );
        assert_eq!(
            reconnect.after(&DisconnectReason::Kicked, &mut rng),
            Next::Stop
        );
        assert!(reconnect.report_no_route());
        for reason in [
            DisconnectReason::Failed("Discord voice found no route to the server; reconnecting"),
            DisconnectReason::Closed(close::INVALID_SESSION),
            DisconnectReason::Closed(close::AUTHENTICATION_FAILED),
            DisconnectReason::Closed(close::UNRESUMABLE),
        ] {
            let Next::Retry(delay) = reconnect.after(&reason, &mut rng) else {
                panic!("{reason:?} retries");
            };
            assert!(delay >= Duration::from_secs(1) && delay <= Duration::from_secs(10));
        }
        assert!(!reconnect.report_no_route(), "op 32 once per cycle");
        for _ in 0..50 {
            let Next::Retry(delay) = reconnect.after(&DisconnectReason::Closed(4006), &mut rng)
            else {
                panic!("never gives up");
            };
            assert!(delay <= Duration::from_secs(10));
        }
        reconnect.routed();
        assert!(reconnect.report_no_route());
        assert_eq!(reconnect.fails(), 0);
    }

    #[test]
    fn dave_failures_count_on_their_own_backoff() {
        let mut rng = rand::rng();
        let mut reconnect = Reconnect::new();
        assert!(matches!(
            reconnect.after(
                &DisconnectReason::Closed(close::DAVE_DOWNGRADE_REFUSED),
                &mut rng
            ),
            Next::Retry(_)
        ));
        assert_eq!(reconnect.fails(), 0, "the connection backoff is untouched");
        assert!(reconnect.reset(), "a retry was waiting");
        assert!(!reconnect.reset());
        reconnect.secured();
    }
}
