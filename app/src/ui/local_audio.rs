//! Each person's volume and local mute in calls, and their soundboard mute, kept as the official client keeps them.
//!
//! Two places, as there. `MediaEngineStore` (module 754712) holds `localVolumes` (a percent, 100 is unity) and
//! `localMutes` of its `default` context in the client's storage, written on every change and read at start, so a
//! choice applies to the next call at once. The account holds them too, in `audio_context_settings` (field 16 of the
//! settings proto, module 386741): a volume is sent two seconds after the last move (`S`, a 2 s debounce), a mute or a
//! soundboard mute at once (`S.cancel(); C()`). When the account's settings arrive (`CONNECTION_OPEN`) or another
//! session changes them (`USER_SETTINGS_PROTO_UPDATE`), its entries replace the local ones (`nC`), except for people
//! whose change here is not sent yet (module 125325); the soundboard mutes come from the account only
//! (`SoundboardStore` `k`). Our own user is never in either.
//!
//! A stream's volume stays in `audio.stream_volumes` (`stream_volume.rs`): its percent is the same unit, but the official
//! stream default is 18 where ours is 100, so its entries are not synced.

use super::Root;
use crate::net::Request;
use crate::storage;
use dc_proto::settings::audio_context::{AudioContext, AudioContextSettings, Entry};
use dc_state::Id;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `MediaEngineStore`'s storage key, and the context the people's voices are in.
pub(super) const STORE: &str = "MediaEngineStore";
const DEFAULT_CONTEXT: &str = "default";
/// How long a volume waits for the slider to stop before it is sent (`r().debounce(() => C(), 2e3)`).
const VOLUME_DEBOUNCE: Duration = Duration::from_secs(2);

/// A person's mix as the menus read it: a volume factor (1.0 is 100%) and the local mute.
pub(super) type Mix = HashMap<Id, (f32, bool)>;

/// The stored `localVolumes` and `localMutes` of the `default` context, as factors and mutes.
pub(super) fn mix_from(store: &Value) -> Mix {
    let context = &store[DEFAULT_CONTEXT];
    let mut mix = Mix::new();
    for (id, volume) in context["localVolumes"].as_object().into_iter().flatten() {
        if let (Ok(id), Some(volume)) = (id.parse::<Id>(), volume.as_f64()) {
            mix.entry(id).or_insert((1., false)).0 = (volume as f32 / 100.).max(0.);
        }
    }
    for (id, muted) in context["localMutes"].as_object().into_iter().flatten() {
        if let (Ok(id), Some(true)) = (id.parse::<Id>(), muted.as_bool()) {
            mix.entry(id).or_insert((1., false)).1 = true;
        }
    }
    mix.retain(|_, (volume, muted)| *volume != 1. || *muted);
    mix
}

/// `store` with one person's entry as `AUDIO_SET_LOCAL_VOLUME` and `AUDIO_TOGGLE_LOCAL_MUTE` leave it: the volume is
/// dropped at 100, the mute when off. Every other key of the store is kept.
pub(super) fn with_person(store: Value, user: Id, volume: f32, muted: bool) -> Value {
    let mut store = match store {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let context = store.entry(DEFAULT_CONTEXT).or_insert_with(|| json!({}));
    if !context.is_object() {
        *context = json!({});
    }
    let percent = volume * 100.;
    let key = user.to_string();
    for (name, keep, value) in [
        ("localVolumes", percent != 100., json!(percent)),
        ("localMutes", muted, json!(true)),
    ] {
        let list = context
            .as_object_mut()
            .expect("an object")
            .entry(name)
            .or_insert_with(|| json!({}));
        if !list.is_object() {
            *list = json!({});
        }
        let list = list.as_object_mut().expect("an object");
        if keep {
            list.insert(key.clone(), value);
        } else {
            list.remove(&key);
        }
    }
    Value::Object(store)
}

/// The changes of one person made here and not sent yet (module 125325 `gq`): only what was changed, so an entry the
/// account did not have takes the other values at their defaults, as `C` does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Change {
    pub volume: Option<f32>,
    pub muted: Option<bool>,
    pub soundboard_muted: Option<bool>,
}

impl Change {
    fn merge(&mut self, other: Change) {
        self.volume = other.volume.or(self.volume);
        self.muted = other.muted.or(self.muted);
        self.soundboard_muted = other.soundboard_muted.or(self.soundboard_muted);
    }

    fn apply(&self, entry: &mut Entry) {
        if let Some(volume) = self.volume {
            entry.volume = volume;
        }
        if let Some(muted) = self.muted {
            entry.muted = muted;
        }
        if let Some(on) = self.soundboard_muted {
            entry.soundboard_muted = on;
        }
    }
}

/// `audio` with every pending change written into it, stamped `now_ms`.
pub(super) fn with_pending(
    mut audio: AudioContextSettings,
    pending: &HashMap<Id, Change>,
    now_ms: u64,
) -> AudioContextSettings {
    let mut people: Vec<_> = pending.iter().collect();
    people.sort_by_key(|(id, _)| **id);
    for (user, change) in people {
        audio.update(AudioContext::User, *user, now_ms, |entry| {
            change.apply(entry)
        });
    }
    audio
}

/// `nC`: the account's entries over the local mix. People with a change not sent yet keep theirs; with `prune`, people
/// the account has no entry for are back at 100% and unmuted.
pub(super) fn mix_with_account(
    local: &Mix,
    audio: &AudioContextSettings,
    pending: &HashSet<Id>,
    prune: bool,
) -> Mix {
    let mut mix = local.clone();
    for (user, entry) in audio.map(AudioContext::User) {
        if pending.contains(user) {
            continue;
        }
        let volume = if entry.volume != AudioContext::User.default_volume() {
            entry.volume / 100.
        } else {
            1.
        };
        if volume == 1. && !entry.muted {
            mix.remove(user);
        } else {
            mix.insert(*user, (volume.max(0.), entry.muted));
        }
    }
    if prune {
        mix.retain(|user, _| {
            pending.contains(user) || audio.get(AudioContext::User, *user).is_some()
        });
    }
    mix
}

/// `SoundboardStore` `k`: the people whose soundboard is muted are the account's.
pub(super) fn soundboard_muted_in(audio: &AudioContextSettings) -> HashSet<Id> {
    audio
        .map(AudioContext::User)
        .iter()
        .filter(|(_, entry)| entry.soundboard_muted)
        .map(|(id, _)| *id)
        .collect()
}

#[derive(Default)]
struct Outbox {
    /// The PATCH value to send, built from the account's settings with every pending change in.
    patch: Option<String>,
    pending: HashMap<Id, Change>,
    /// When the debounced write goes, and whether a thread is waiting for it.
    due: Option<Instant>,
    waiting: bool,
}

/// The writes of this module on their way to the account.
#[derive(Clone, Default)]
pub(super) struct AudioSync {
    outbox: Arc<Mutex<Outbox>>,
    /// The Soundboard Volume, sent a second after the last move (`r().debounce(e => updateSetting({volume: e}), 1e3)`).
    soundboard: Debounced,
}

/// One settings write that waits for its control to rest: each change replaces the PATCH value and moves the time it
/// goes; a single thread waits for it.
#[derive(Clone, Default)]
struct Debounced {
    shared: Arc<Mutex<DebouncedState>>,
}

#[derive(Default)]
struct DebouncedState {
    patch: Option<String>,
    due: Option<Instant>,
    waiting: bool,
}

impl Debounced {
    fn pending(&self) -> bool {
        self.shared.lock().is_ok_and(|state| state.patch.is_some())
    }

    fn schedule(&self, patch: String, delay: Duration, requests: &crate::net::Requests) {
        let Ok(mut state) = self.shared.lock() else {
            return;
        };
        state.patch = Some(patch);
        state.due = Some(Instant::now() + delay);
        if state.waiting {
            return;
        }
        state.waiting = true;
        drop(state);
        let shared = self.shared.clone();
        let requests = requests.clone();
        std::thread::spawn(move || {
            loop {
                let wait = {
                    let Ok(mut state) = shared.lock() else {
                        return;
                    };
                    match state.due {
                        Some(due) if due > Instant::now() => {
                            due.saturating_duration_since(Instant::now())
                        }
                        _ => {
                            state.due = None;
                            state.waiting = false;
                            if let Some(patch) = state.patch.take() {
                                send_patch(&requests, patch);
                            }
                            return;
                        }
                    }
                };
                std::thread::sleep(wait);
            }
        });
    }
}

impl AudioSync {
    fn pending(&self) -> HashSet<Id> {
        self.outbox
            .lock()
            .map(|outbox| outbox.pending.keys().copied().collect())
            .unwrap_or_default()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn send_patch(requests: &crate::net::Requests, patch: String) {
    let _ = requests.send(Request::Rest {
        key: None,
        method: reqwest::Method::PATCH,
        route: "/users/@me/settings-proto/1".to_owned(),
        body: Some(json!({ "settings": patch })),
    });
}

impl super::context_menu::State {
    /// The state a start opens with: the people's mix as the last run left it.
    pub(super) fn restored() -> Self {
        Self {
            mix: mix_from(&storage::get(STORE).unwrap_or(Value::Null)),
            ..Self::default()
        }
    }
}

impl Root {
    fn audio_me(&self) -> Option<Id> {
        self.stores.current_user.as_ref().map(|user| user.id)
    }

    /// A person's volume or local mute changed here (`set_user_mix`): kept on this machine at once, and in the account
    /// at once for a mute, two seconds after the last move for a volume.
    pub(super) fn user_mix_changed(&mut self, user: Id, before: (f32, bool), after: (f32, bool)) {
        if Some(user) == self.audio_me() || before == after {
            return;
        }
        let stored = storage::get(STORE).unwrap_or(Value::Null);
        storage::set(STORE, with_person(stored, user, after.0, after.1));
        let mute = before.1 != after.1;
        let change = if mute {
            Change {
                muted: Some(after.1),
                ..Change::default()
            }
        } else {
            Change {
                volume: Some(after.0 * 100.),
                ..Change::default()
            }
        };
        self.queue_audio_change(user, change, mute);
    }

    /// Mute Soundboard of a person's menu (`AUDIO_TOGGLE_LOCAL_SOUNDBOARD_MUTE`): the account holds it, sent at once.
    pub(super) fn soundboard_mute_changed(&mut self, user: Id) {
        if Some(user) == self.audio_me() {
            return;
        }
        let on = self.context.soundboard_muted.contains(&user);
        let change = Change {
            soundboard_muted: Some(on),
            ..Change::default()
        };
        self.queue_audio_change(user, change, true);
    }

    fn queue_audio_change(&mut self, user: Id, change: Change, now: bool) {
        if crate::storage::settings_writes_blocked() {
            return;
        }
        let Ok(mut outbox) = self.context.audio_sync.outbox.lock() else {
            return;
        };
        outbox.pending.entry(user).or_default().merge(change);
        let audio = with_pending(self.stores.audio_context(), &outbox.pending, now_ms());
        let patch = self.stores.set_audio_context(&audio);
        if patch.is_empty() {
            // The account's settings have not arrived: the change waits for them (`rebuild_audio_patch`).
            return;
        }
        outbox.patch = Some(patch);
        if now {
            outbox.due = None;
            outbox.pending.clear();
            if let Some(patch) = outbox.patch.take() {
                send_patch(&self.requests, patch);
            }
            return;
        }
        outbox.due = Some(Instant::now() + VOLUME_DEBOUNCE);
        if outbox.waiting {
            return;
        }
        outbox.waiting = true;
        drop(outbox);
        let shared = self.context.audio_sync.outbox.clone();
        let requests = self.requests.clone();
        std::thread::spawn(move || {
            loop {
                let wait = {
                    let Ok(mut outbox) = shared.lock() else {
                        return;
                    };
                    match outbox.due {
                        Some(due) if due > Instant::now() => {
                            due.saturating_duration_since(Instant::now())
                        }
                        Some(_) => {
                            outbox.due = None;
                            outbox.waiting = false;
                            outbox.pending.clear();
                            if let Some(patch) = outbox.patch.take() {
                                send_patch(&requests, patch);
                            }
                            return;
                        }
                        None => {
                            outbox.waiting = false;
                            return;
                        }
                    }
                };
                std::thread::sleep(wait);
            }
        });
    }

    /// The account's settings were replaced (READY, or another session's change): the changes not sent yet go into the
    /// new settings, so the write that follows does not take back what arrived.
    fn rebuild_audio_patch(&mut self) {
        let Ok(mut outbox) = self.context.audio_sync.outbox.lock() else {
            return;
        };
        if outbox.pending.is_empty() {
            return;
        }
        let audio = with_pending(self.stores.audio_context(), &outbox.pending, now_ms());
        let patch = self.stores.set_audio_context(&audio);
        if patch.is_empty() {
            return;
        }
        outbox.patch = Some(patch);
        if outbox.due.is_none() && !outbox.waiting {
            // Changed before the settings arrived: sent now that they have.
            outbox.pending.clear();
            if let Some(patch) = outbox.patch.take() {
                send_patch(&self.requests, patch);
            }
        }
    }

    /// `nC` and `SoundboardStore` `k` after the account's settings arrived (`prune` false, `CONNECTION_OPEN`) or another
    /// session changed them (`prune` true): the account's entries are the mix, saved here and given to the call.
    pub(super) fn account_audio_changed(&mut self, prune: bool) {
        if !self.stores.settings_loaded() {
            return;
        }
        self.rebuild_audio_patch();
        let audio = self.stores.audio_context();
        let pending = self.context.audio_sync.pending();
        let me = self.audio_me();
        let mut mix = mix_with_account(&self.context.mix, &audio, &pending, prune);
        if let Some(me) = me {
            mix.remove(&me);
        }
        let mut soundboard = soundboard_muted_in(&audio);
        if let Some(me) = me {
            soundboard.remove(&me);
        }
        self.context.soundboard_muted = soundboard;
        // `SoundboardStore` reads its volume from the account (`?? 100`), unless a move here is on its way.
        if !self.context.audio_sync.soundboard.pending() {
            let volume = self.stores.soundboard_volume().unwrap_or(100.);
            if super::settings::soundboard_volume() != volume.clamp(0., 100.) {
                super::settings::set_soundboard_volume(volume);
            }
        }
        if mix == self.context.mix {
            return;
        }
        let changed: HashSet<Id> = mix
            .keys()
            .chain(self.context.mix.keys())
            .copied()
            .filter(|user| mix.get(user) != self.context.mix.get(user))
            .collect();
        let mut stored = storage::get(STORE).unwrap_or(Value::Null);
        for user in &changed {
            let (volume, muted) = mix.get(user).copied().unwrap_or((1., false));
            stored = with_person(stored, *user, volume, muted);
            if let Some(call) = &self.call {
                call.controls
                    .set_user_mix(*user, engine_volume(volume), muted);
            }
        }
        storage::set(STORE, stored);
        self.context.mix = mix;
    }

    /// A new call's connection takes every person's mix, as `MediaEngineStore` gives each connection `localVolumes`
    /// and `localMutes` when it opens.
    pub(super) fn give_call_the_mix(&self, controls: &dc_voice::Controls) {
        let me = self.audio_me();
        for (user, (volume, muted)) in &self.context.mix {
            if Some(*user) != me {
                controls.set_user_mix(*user, engine_volume(*volume), *muted);
            }
        }
    }

    /// The Soundboard Volume was moved (Voice & Video, or the soundboard's own slider): the account takes it a second
    /// after the last move. `audio.soundboard_volume` keeps it on this machine meanwhile.
    pub(in crate::ui) fn soundboard_volume_moved(&mut self) {
        if crate::storage::settings_writes_blocked() {
            return;
        }
        let volume = super::settings::soundboard_volume();
        if self.stores.soundboard_volume().unwrap_or(100.) == volume {
            return;
        }
        let patch = self.stores.set_soundboard_volume(volume);
        if !patch.is_empty() {
            self.context.audio_sync.soundboard.schedule(
                patch,
                Duration::from_secs(1),
                &self.requests,
            );
        }
    }

    /// The account's settings changed here or elsewhere: the start cache keeps them, so the next start shows these
    /// and not those READY brought (`UserSettingsProtoStore-Cache`).
    pub(super) fn keep_settings_for_start(&mut self) {
        if self.hydrated_from_cache || !super::start_cache::enabled() {
            return;
        }
        let Some(settings) = self.stores.settings_base64() else {
            return;
        };
        let Some(ready) = self.cached_ready.as_mut() else {
            return;
        };
        if ready["user_settings_proto"].as_str() == Some(settings.as_str()) {
            return;
        }
        ready["user_settings_proto"] = Value::String(settings);
        super::start_cache::save_trimmed_ready(ready.clone());
    }
}

/// The volume the voice engine takes: the VolumeBooster plugin's top, else 2 as the pinned client.
fn engine_volume(volume: f32) -> f32 {
    let cap = super::plugin_hooks::user_volume_cap();
    let volume = cap.map_or(volume, |cap| volume.min(cap));
    volume.min(cap.unwrap_or(2.))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_and_a_mute_survive_the_round_trip_through_storage() {
        let store = with_person(Value::Null, 7, 1.5, false);
        let store = with_person(store, 8, 1., true);
        assert_eq!(
            store,
            json!({"default": {"localVolumes": {"7": 150.0}, "localMutes": {"8": true}}})
        );
        let mix = mix_from(&store);
        assert_eq!(mix.get(&7), Some(&(1.5, false)));
        assert_eq!(mix.get(&8), Some(&(1., true)));
        // Back at 100% and unmuted: nothing is kept for them.
        let store = with_person(store, 7, 1., false);
        let store = with_person(store, 8, 1., false);
        assert!(mix_from(&store).is_empty());
        assert_eq!(
            store,
            json!({"default": {"localVolumes": {}, "localMutes": {}}})
        );
    }

    #[test]
    fn the_rest_of_the_store_is_kept() {
        let store = json!({"default": {"inputVolume": 80, "localPans": {"3": {"left": 1}}}, "stream": {"x": 1}});
        let store = with_person(store, 9, 0.5, false);
        assert_eq!(store["default"]["inputVolume"], 80);
        assert_eq!(store["default"]["localPans"]["3"]["left"], 1);
        assert_eq!(store["stream"]["x"], 1);
        assert_eq!(store["default"]["localVolumes"]["9"], 50.0);
    }

    fn account(entries: &[(Id, f32, bool, bool)]) -> AudioContextSettings {
        let mut audio = AudioContextSettings::default();
        for (id, volume, muted, soundboard) in entries {
            audio.update(AudioContext::User, *id, 1, |entry| {
                entry.volume = *volume;
                entry.muted = *muted;
                entry.soundboard_muted = *soundboard;
            });
        }
        audio
    }

    #[test]
    fn the_account_wins_except_for_a_change_not_sent_yet() {
        let local: Mix = [(1, (0.5, false)), (2, (1.2, false)), (3, (0.7, true))]
            .into_iter()
            .collect();
        let audio = account(&[(1, 150., false, false), (2, 30., true, false)]);
        let pending: HashSet<Id> = [2].into_iter().collect();
        // CONNECTION_OPEN: a person the account does not know keeps the local choice.
        let mix = mix_with_account(&local, &audio, &pending, false);
        assert_eq!(mix.get(&1), Some(&(1.5, false)));
        assert_eq!(mix.get(&2), Some(&(1.2, false)));
        assert_eq!(mix.get(&3), Some(&(0.7, true)));
        // Another session's change: the account's list is the whole list.
        let mix = mix_with_account(&local, &audio, &pending, true);
        assert_eq!(mix.get(&3), None);
        assert_eq!(mix.get(&2), Some(&(1.2, false)));
    }

    #[test]
    fn pending_changes_write_only_what_changed() {
        let audio = account(&[(5, 40., false, false)]);
        let pending: HashMap<Id, Change> = [
            (
                5,
                Change {
                    muted: Some(true),
                    ..Change::default()
                },
            ),
            (
                6,
                Change {
                    volume: Some(130.),
                    ..Change::default()
                },
            ),
        ]
        .into_iter()
        .collect();
        let written = with_pending(audio, &pending, 99);
        let five = written.get(AudioContext::User, 5).unwrap();
        assert_eq!((five.volume, five.muted, five.modified_at), (40., true, 99));
        let six = written.get(AudioContext::User, 6).unwrap();
        assert_eq!((six.volume, six.muted), (130., false));
        assert_eq!(
            soundboard_muted_in(&account(&[(7, 100., false, true), (8, 50., false, false)])),
            [7].into_iter().collect()
        );
    }
}
