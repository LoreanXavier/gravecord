//! AltKrispSwitch (Equicord `src/equicordplugins/altKrispSwitch`, GPL-3.0-or-later): the voice panel's Noise
//! Suppression popout switch moves between None and the noise suppressor instead of the noise suppressor and Standard
//! (`shouldCancelSuppression` drops the popout's `AUDIO_SET_NOISE_SUPPRESSION` dispatch when it turns Krisp off). This
//! client's noise suppressor is DeepFilterNet in Krisp's place, so the switch goes between None and DeepFilterNet.
//! Only the popout's switch (`section: "Noise Cancellation Popout"`) changes; the settings page's select stays as it
//! is. Not part of the pinned client.

use crate::plugins::State;

const ID: &str = "AltKrispSwitch";

/// The voice settings' suppression index for "None" (`NOISE_STORED`: DeepFilterNet, Standard, None).
const NONE: usize = 2;

fn on(state: &State) -> bool {
    state.is_on(ID)
}

/// What the popout's switch sets when it turns the noise suppressor off: `None` for the pinned client's Standard,
/// `Some(index)` for this plugin's None.
pub(in crate::ui) fn noise_popout_off() -> Option<usize> {
    (!crate::plugins::bypassed() && crate::plugins::with(on)).then_some(NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_turns_to_none_only_while_on() {
        let mut state = State::default();
        assert!(!on(&state));
        state.set_enabled(ID, true);
        assert!(on(&state));
    }
}
