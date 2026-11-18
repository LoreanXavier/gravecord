//! UnlockedAvatarZoom (Vencord `src/plugins/unlockedAvatarZoom`, GPL-3.0-or-later): the image crop tool's zoom slider
//! (`maxValue: 2` of module 142630, every picture it edits) goes up to `zoomMultiplier` (2 to 16, 4 by default). Not
//! part of the pinned client.

use crate::plugins::State;
use serde_json::Value;

const ID: &str = "UnlockedAvatarZoom";
/// The pinned slider's `maxValue`.
const PINNED_MAX: f64 = 2.;

fn max_of(state: &State) -> Option<f64> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    Some(match key("zoomMultiplier") {
        Value::Number(number) => number.as_f64().unwrap_or(4.),
        _ => 4.,
    })
}

/// The crop slider's top: the plugin's multiplier, else the pinned 2.
pub(in crate::ui) fn crop_max_zoom() -> f64 {
    if crate::plugins::bypassed() {
        return PINNED_MAX;
    }
    crate::plugins::with(max_of).unwrap_or(PINNED_MAX).max(1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_reaches_the_multiplier() {
        let mut state = State::default();
        assert_eq!(max_of(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(max_of(&state), Some(4.));
        state.set_value(ID, "zoomMultiplier", &Value::from(16));
        assert_eq!(max_of(&state), Some(16.));
    }
}
