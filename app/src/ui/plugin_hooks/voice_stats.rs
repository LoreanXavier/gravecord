//! VoiceStats (Equicord `src/equicordplugins/voiceStats/index.tsx`, GPL-3.0-or-later): how long you have spent in voice
//! with each person, shown as "Voice Time" in their profile. Not part of the pinned client.
//!
//! The counting is the original's: while you are in a voice channel, everyone else in it has a session that started
//! when you joined or they did; a session ends (its whole seconds added to the person's total) when they leave, when
//! you leave, or when you move (a stop and a new start). Every 30 seconds the running sessions are added and the
//! totals saved. The totals live in `storage.json` under `plugin_voice_stats`; an Equicord backup brings the
//! original's own totals (`VoiceStats_totals` in its data store).

use super::super::{Root, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, ParentElement, Styled, div, px, svg,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const ID: &str = "VoiceStats";
/// `storageKey` of the original lives in its data store; ours is a key of `storage.json`.
const STORAGE_KEY: &str = "plugin_voice_stats";
/// `saveIntervalMs`.
const SAVE_EVERY_MS: u64 = 30_000;

/// The original's module state: `totalsByUser`, `sessionStarts` and `trackedChannelId`.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Stats {
    /// Seconds with each person.
    totals: BTreeMap<Id, u64>,
    /// When each running session started (or was last added), in ms.
    starts: BTreeMap<Id, u64>,
    /// The voice channel tracked, as (guild, channel).
    tracked: Option<(Id, Id)>,
    loaded: bool,
    last_save: u64,
}

impl Stats {
    /// `flushActiveSessions`: adds the whole seconds of every running session and restarts them now.
    fn flush(&mut self, now: u64) {
        for (user, started) in self.starts.iter_mut() {
            let accrued = now.saturating_sub(*started) / 1000;
            *self.totals.entry(*user).or_default() += accrued;
            *started = now;
        }
    }

    /// `startTrackingChannel`.
    fn start(&mut self, channel: (Id, Id), others: &BTreeSet<Id>, now: u64) {
        self.tracked = Some(channel);
        for user in others {
            self.starts.insert(*user, now);
        }
        self.last_save = now;
    }

    /// `stopTrackingChannel`: whether the totals must be saved.
    fn stop(&mut self, now: u64) -> bool {
        if self.tracked.is_none() {
            return false;
        }
        self.flush(now);
        self.starts.clear();
        self.tracked = None;
        true
    }

    /// The `VOICE_STATE_UPDATES` handler, as a comparison of who is in your channel now with who was: `mine` is your
    /// voice channel, `others` the people in it besides you. Returns whether the totals must be saved.
    pub(super) fn observe(
        &mut self,
        mine: Option<(Id, Id)>,
        others: &BTreeSet<Id>,
        now: u64,
    ) -> bool {
        if mine != self.tracked {
            let saved = self.stop(now);
            if let Some(channel) = mine {
                self.start(channel, others, now);
            }
            return saved;
        }
        if self.tracked.is_none() {
            return false;
        }
        let mut save = false;
        // Someone left your channel: their session ends.
        let left: Vec<Id> = self
            .starts
            .keys()
            .filter(|user| !others.contains(user))
            .copied()
            .collect();
        for user in left {
            if let Some(started) = self.starts.remove(&user) {
                *self.totals.entry(user).or_default() += now.saturating_sub(started) / 1000;
                save = true;
            }
        }
        // Someone joined it: theirs starts.
        for user in others {
            self.starts.entry(*user).or_insert(now);
        }
        // The 30-second save of the running sessions.
        if now.saturating_sub(self.last_save) >= SAVE_EVERY_MS {
            self.flush(now);
            self.last_save = now;
            save = true;
        }
        save
    }

    /// `getLiveSeconds`.
    pub(super) fn live_seconds(&self, user: Id, now: u64) -> u64 {
        let stored = self.totals.get(&user).copied().unwrap_or(0);
        self.starts.get(&user).map_or(stored, |started| {
            stored + now.saturating_sub(*started) / 1000
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "totals": self
                .totals
                .iter()
                .map(|(user, seconds)| (user.to_string(), json!(seconds)))
                .collect::<serde_json::Map<_, _>>()
        })
    }

    /// The totals of `{"user id": seconds}` (ours under `totals`, or the original's `VoiceStats_totals` as it is).
    fn totals_of(value: &Value) -> BTreeMap<Id, u64> {
        value
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(user, seconds)| {
                        Some((
                            user.parse().ok()?,
                            seconds.as_f64().filter(|s| *s >= 0.)? as u64,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `formatDuration`.
pub(super) fn format_duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

thread_local! {
    static STATS: RefCell<Stats> = RefCell::new(Stats::default());
    /// The one-second refresh while a channel is tracked (`useTimer`), which also drives the 30-second save.
    static TICKER: RefCell<Option<gpui::Task<()>>> = const { RefCell::new(None) };
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::with(|state: &State| state.is_on(ID))
}

fn save(stats: &Stats) {
    crate::plugins::note("storage:voice_stats");
    crate::storage::set(STORAGE_KEY, stats.to_value());
}

/// `loadStoredTotals`, once.
fn load(stats: &mut Stats) {
    if stats.loaded {
        return;
    }
    stats.loaded = true;
    if cfg!(test) {
        return;
    }
    if let Some(saved) = crate::storage::get(STORAGE_KEY) {
        stats.totals = Stats::totals_of(&saved["totals"]);
    }
}

/// An Equicord backup's `VoiceStats_totals`: replaces the totals (a second import does not add them again).
pub(in crate::ui) fn import_totals(totals: &Value) -> usize {
    let totals = Stats::totals_of(totals);
    let count = totals.len();
    STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.loaded = true;
        stats.totals = totals;
        save(&stats);
    });
    count
}

impl Root {
    /// The voice channel this account is in (any of its sessions, as `VOICE_STATE_UPDATES` sees the account's own
    /// state) and the other people in it.
    fn voice_company(&self) -> Option<((Id, Id), BTreeSet<Id>)> {
        let me = self.stores.current_user.as_ref()?.id;
        let (guild, mine) = self
            .stores
            .voice_states
            .iter()
            .find_map(|(guild, states)| {
                states
                    .iter()
                    .find(|state| state.user_id == me && state.channel_id != 0)
                    .map(|state| (*guild, state.channel_id))
            })?;
        let others = self
            .stores
            .voice_members(guild, mine)
            .into_iter()
            .map(|state| state.user_id)
            .filter(|user| *user != me)
            .collect();
        Some(((guild, mine), others))
    }

    /// Follows the voice states (after each `VOICE_STATE_UPDATE`, and each frame for the plugin's own switch: `start`
    /// and `stop`). Off, it ends a tracked channel once and then does nothing.
    pub(in crate::ui) fn voice_stats_observe(&mut self, cx: &mut Context<Self>) {
        let active = on();
        let tracking = STATS.with(|stats| stats.borrow().tracked.is_some());
        if !active && !tracking {
            return;
        }
        let company = if active { self.voice_company() } else { None };
        let (mine, others) = match company {
            Some((channel, others)) => (Some(channel), others),
            None => (None, BTreeSet::new()),
        };
        let now = now_ms();
        let tracked = STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            load(&mut stats);
            if stats.observe(mine, &others, now) {
                save(&stats);
            }
            stats.tracked.is_some()
        });
        let ticking = TICKER.with(|ticker| ticker.borrow().is_some());
        if tracked && !ticking {
            let task = cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this
                        .update(cx, |this, cx| {
                            this.voice_stats_observe(cx);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            TICKER.with(|ticker| *ticker.borrow_mut() = Some(task));
        } else if !tracked && ticking {
            TICKER.with(|ticker| ticker.borrow_mut().take());
        }
    }

    /// The "Voice Time" section of a profile (`VoiceStatsSection`), after Member Since: in the DM sidebar
    /// (`isSideBar`) a plain heading and text, in the full profile the text beside a microphone. None while the
    /// plugin is off or the time is zero.
    pub(in crate::ui) fn voice_time_section(
        &self,
        user: Id,
        sidebar: bool,
        strong: Hsla,
        text: Hsla,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        let seconds = STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            load(&mut stats);
            stats.live_seconds(user, now_ms())
        });
        if seconds == 0 {
            return None;
        }
        let duration = format_duration(seconds);
        let heading = div()
            .h(px(16.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(if sidebar { 600. } else { 500. }))
            .text_color(if sidebar { strong } else { text })
            .child("Voice Time");
        let value = div()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(text);
        let body = if sidebar {
            value.child(duration).into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    svg()
                        .path("icons/plugin_mic.svg")
                        .size(px(16.))
                        .text_color(color("--interactive-icon-default")),
                )
                .child(value.child(duration))
                .into_any_element()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(if sidebar { 8. } else { 4. }))
                .child(heading)
                .child(body)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[Id]) -> BTreeSet<Id> {
        ids.iter().copied().collect()
    }

    #[test]
    fn durations_read_as_the_original() {
        assert_eq!(format_duration(5), "5s");
        assert_eq!(format_duration(65), "1m 5s");
        assert_eq!(format_duration(3725), "1h 2m 5s");
    }

    /// Joining counts everyone already there; someone who leaves gets their whole seconds.
    #[test]
    fn people_in_your_channel_are_counted_until_they_leave() {
        let mut stats = Stats::default();
        let room = Some((1, 10));
        assert!(!stats.observe(room, &set(&[7, 8]), 0));
        assert!(stats.observe(room, &set(&[8]), 12_900), "7 left: saved");
        assert_eq!(stats.live_seconds(7, 50_000), 12);
        assert_eq!(stats.live_seconds(8, 20_000), 20, "8 still running");
        // 9 joins later and is counted from then.
        assert!(!stats.observe(room, &set(&[8, 9]), 15_000));
        assert_eq!(stats.live_seconds(9, 16_000), 1);
    }

    /// Leaving or moving ends every session; nobody is counted outside your channel.
    #[test]
    fn leaving_or_moving_ends_the_sessions() {
        let mut stats = Stats::default();
        stats.observe(Some((1, 10)), &set(&[7]), 0);
        assert!(
            stats.observe(Some((1, 11)), &set(&[8]), 4_000),
            "a move stops and saves"
        );
        assert_eq!(stats.live_seconds(7, 60_000), 4);
        assert_eq!(stats.live_seconds(8, 6_000), 2);
        assert!(stats.observe(None, &set(&[]), 9_000));
        assert_eq!(stats.live_seconds(8, 99_000), 5);
        assert_eq!(stats.tracked, None);
        assert!(
            !stats.observe(None, &set(&[]), 10_000),
            "nothing tracked, nothing saved"
        );
    }

    /// The 30-second save adds the running sessions without ending them (`flushActiveSessions`).
    #[test]
    fn running_sessions_are_saved_every_thirty_seconds() {
        let mut stats = Stats::default();
        stats.observe(Some((1, 10)), &set(&[7]), 0);
        assert!(!stats.observe(Some((1, 10)), &set(&[7]), 29_000));
        assert!(stats.observe(Some((1, 10)), &set(&[7]), 30_500));
        assert_eq!(stats.totals.get(&7), Some(&30));
        assert_eq!(stats.live_seconds(7, 31_500), 31);
    }

    #[test]
    fn totals_read_from_ours_and_the_originals() {
        let mut stats = Stats::default();
        stats.totals.insert(7, 42);
        let ours = stats.to_value();
        assert_eq!(Stats::totals_of(&ours["totals"]), stats.totals);
        let original = json!({"7": 12, "8": 3600.0, "x": 1, "9": -1});
        assert_eq!(
            Stats::totals_of(&original),
            BTreeMap::from([(7, 12), (8, 3600)])
        );
    }
}
