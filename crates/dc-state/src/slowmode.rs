//! SlowmodeStore, module 101392. Message and thread cooldowns are independent and gain a 100ms grace period.
use crate::Id;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    SendMessage,
    CreateThread,
}

#[derive(Clone, Debug)]
struct Cooldown {
    rate: u64,
    until: Instant,
    next_tick: Instant,
    remaining: u64,
}

#[derive(Default, Debug)]
pub struct SlowmodeStore {
    cooldowns: HashMap<(Id, Kind), Cooldown>,
}

impl SlowmodeStore {
    pub fn set(
        &mut self,
        channel: Id,
        kind: Kind,
        rate: u64,
        bypass: bool,
        millis: u64,
        now: Instant,
    ) {
        self.cooldowns.remove(&(channel, kind));
        if rate == 0 || bypass || millis == 0 {
            return;
        }
        if let Some(until) = now.checked_add(Duration::from_millis(millis.saturating_add(100))) {
            self.cooldowns.insert(
                (channel, kind),
                Cooldown {
                    rate,
                    until,
                    next_tick: now + Duration::from_secs(1),
                    remaining: millis.saturating_add(100),
                },
            );
        }
    }

    pub fn reset(&mut self, channel: Id, kind: Kind, rate: u64, bypass: bool, now: Instant) {
        self.set(channel, kind, rate, bypass, rate.saturating_mul(1000), now);
    }

    pub fn get_slowmode_cooldown_guess(&self, channel: Id, kind: Kind) -> u64 {
        self.cooldowns
            .get(&(channel, kind))
            .map_or(0, |cooldown| cooldown.remaining)
    }

    pub fn is_channel_on_cooldown(&self, channel: Id, kind: Kind, rate: u64, bypass: bool) -> bool {
        rate > 0 && !bypass && self.get_slowmode_cooldown_guess(channel, kind) > 0
    }

    pub fn channel_updated(&mut self, channel: Id, rate: u64, bypass: bool, now: Instant) {
        for kind in [Kind::SendMessage, Kind::CreateThread] {
            if self
                .cooldowns
                .get(&(channel, kind))
                .is_some_and(|cooldown| cooldown.rate != rate)
            {
                // CHANNEL_UPDATES clamps the remaining time without adding another grace period.
                let millis = self
                    .get_slowmode_cooldown_guess(channel, kind)
                    .min(rate.saturating_mul(1000));
                self.cooldowns.remove(&(channel, kind));
                if millis > 0
                    && !bypass
                    && let Some(until) = now.checked_add(Duration::from_millis(millis))
                {
                    self.cooldowns.insert(
                        (channel, kind),
                        Cooldown {
                            rate,
                            until,
                            next_tick: now + Duration::from_secs(1),
                            remaining: millis,
                        },
                    );
                }
            }
        }
    }

    pub fn next_tick_delay(&self, now: Instant) -> Option<Duration> {
        self.cooldowns
            .values()
            .map(|cooldown| cooldown.next_tick.saturating_duration_since(now))
            .min()
    }

    pub fn tick(&mut self, now: Instant) {
        self.cooldowns.retain(|_, cooldown| {
            if now < cooldown.next_tick {
                return true;
            }
            let remaining = cooldown
                .until
                .saturating_duration_since(now)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            if remaining == 0 {
                return false;
            }
            // The timer dispatches SET, which adds 100ms again (101392 E and SLOWMODE_SET_COOLDOWN).
            cooldown.remaining = remaining.saturating_add(100);
            let Some(until) = now.checked_add(Duration::from_millis(cooldown.remaining)) else {
                return false;
            };
            cooldown.until = until;
            cooldown.next_tick = now + Duration::from_secs(1);
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn message_and_thread_cooldowns_expire_independently() {
        let now = Instant::now();
        let mut store = SlowmodeStore::default();
        store.reset(1, Kind::SendMessage, 5, false, now);
        assert!(store.is_channel_on_cooldown(1, Kind::SendMessage, 5, false));
        assert!(!store.is_channel_on_cooldown(1, Kind::SendMessage, 5, true));
        assert!(!store.is_channel_on_cooldown(1, Kind::SendMessage, 0, false));
        assert!(!store.is_channel_on_cooldown(2, Kind::SendMessage, 5, false));
        store.set(1, Kind::CreateThread, 5, false, 12000, now);
        assert_eq!(
            store.get_slowmode_cooldown_guess(1, Kind::SendMessage),
            5100
        );
        assert_eq!(
            store.get_slowmode_cooldown_guess(1, Kind::CreateThread),
            12100
        );
        store.tick(now + Duration::from_millis(500));
        assert_eq!(
            store.get_slowmode_cooldown_guess(1, Kind::SendMessage),
            5100
        );
        for second in 1..=5 {
            store.tick(now + Duration::from_secs(second));
        }
        assert_eq!(store.get_slowmode_cooldown_guess(1, Kind::SendMessage), 600);
        store.tick(now + Duration::from_secs(6));
        assert_eq!(store.get_slowmode_cooldown_guess(1, Kind::SendMessage), 0);
        assert!(
            store
                .next_tick_delay(now + Duration::from_secs(6))
                .is_some()
        );
        store.tick(now + Duration::from_secs(20));
        assert!(store.next_tick_delay(now).is_none());
    }
    #[test]
    fn permission_and_channel_changes_clear_or_shorten_cooldowns() {
        let now = Instant::now();
        let mut store = SlowmodeStore::default();
        store.reset(1, Kind::SendMessage, 60, false, now);
        store.channel_updated(1, 5, false, now);
        assert_eq!(
            store.get_slowmode_cooldown_guess(1, Kind::SendMessage),
            5000
        );
        store.tick(now + Duration::from_secs(1));
        store.channel_updated(1, 120, false, now + Duration::from_secs(1));
        assert_eq!(
            store.get_slowmode_cooldown_guess(1, Kind::SendMessage),
            4100
        );
        store.channel_updated(1, 0, false, now);
        assert!(store.next_tick_delay(now).is_none());
        store.reset(1, Kind::SendMessage, 5, true, now);
        assert!(store.next_tick_delay(now).is_none());
        store.reset(1, Kind::SendMessage, 5, false, now);
        store.set(1, Kind::SendMessage, 5, false, 0, now);
        assert!(store.next_tick_delay(now).is_none());
    }
}
