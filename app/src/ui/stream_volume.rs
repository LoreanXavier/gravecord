//! What this client remembers about a watched stream's sound, per stream owner: the level of the Stream Volume slider
//! (`getLocalVolume(owner, STREAM)`, a percent where 100 is unity and the slider runs to 200) and the Mute row
//! (`isLocalMute(owner, STREAM)`). Both live in storage.json (`audio.stream_volumes`, `audio.stream_muted`), keyed by
//! the owner's id, and reach the mix through `VideoHub::set_stream_volume` when a stream is watched or the control moves.

use crate::storage;
use serde_json::{Value, json};

const VOLUMES: &str = "audio.stream_volumes";
const MUTED: &str = "audio.stream_muted";
/// The slider's top, as `maxValue` is 200 on the desktop client (module 972432).
pub(super) const MAX_PERCENT: f32 = 200.;

/// The stored percent of `owner`; unity when none was chosen.
pub(super) fn percent_in(store: &Value, owner: u64) -> f32 {
    store[owner.to_string()]
        .as_f64()
        .map_or(100., |percent| (percent as f32).clamp(0., MAX_PERCENT))
}

pub(super) fn muted_in(store: &Value, owner: u64) -> bool {
    store
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&owner.to_string())))
}

/// The linear gain the mix takes: zero while muted.
pub(super) fn gain_of(volumes: &Value, muted: &Value, owner: u64) -> f32 {
    if muted_in(muted, owner) {
        0.
    } else {
        percent_in(volumes, owner) / 100.
    }
}

pub(super) fn percent(owner: u64) -> f32 {
    percent_in(&storage::get(VOLUMES).unwrap_or(Value::Null), owner)
}

pub(super) fn muted(owner: u64) -> bool {
    muted_in(&storage::get(MUTED).unwrap_or(Value::Null), owner)
}

pub(super) fn gain(owner: u64) -> f32 {
    gain_of(
        &storage::get(VOLUMES).unwrap_or(Value::Null),
        &storage::get(MUTED).unwrap_or(Value::Null),
        owner,
    )
}

pub(super) fn set_percent(owner: u64, percent: f32) {
    let mut store = storage::get(VOLUMES).unwrap_or_else(|| json!({}));
    if !store.is_object() {
        store = json!({});
    }
    store[owner.to_string()] = json!(percent.clamp(0., MAX_PERCENT));
    storage::set(VOLUMES, store);
}

pub(super) fn set_muted(owner: u64, on: bool) {
    let mut ids: Vec<Value> = storage::get(MUTED)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let id = json!(owner.to_string());
    ids.retain(|existing| existing != &id);
    if on {
        ids.push(id);
    }
    storage::set(MUTED, Value::Array(ids));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_without_a_choice_plays_at_unity() {
        assert_eq!(percent_in(&Value::Null, 7), 100.);
        assert_eq!(gain_of(&Value::Null, &Value::Null, 7), 1.);
    }

    #[test]
    fn the_level_is_kept_per_owner_and_capped_at_the_slider_top() {
        let volumes = json!({"7": 150.0, "8": 900.0});
        assert_eq!(percent_in(&volumes, 7), 150.);
        assert_eq!(percent_in(&volumes, 8), 200.);
        assert_eq!(percent_in(&volumes, 9), 100.);
    }

    #[test]
    fn mute_wins_over_the_level_without_forgetting_it() {
        let volumes = json!({"7": 50.0});
        let muted = json!(["7"]);
        assert!(muted_in(&muted, 7) && !muted_in(&muted, 8));
        assert_eq!(gain_of(&volumes, &muted, 7), 0.);
        assert_eq!(gain_of(&volumes, &json!([]), 7), 0.5);
    }
}
