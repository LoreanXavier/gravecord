//! SuperReactionTweaks (Vencord `src/plugins/superReactionTweaks`, GPL-3.0-or-later): with `superReactByDefault` the
//! emoji picker opened to react starts with Super Reactions on (`isBurstReaction` initial state, intention REACTION),
//! for an account whose own Nitro is real (`premiumTypeActual`, not a plugin's override).
//!
//! GAP: `unlimitedSuperReactionPlaying` and `superReactionPlayingLimit` cap the super reaction effects that play at
//! once (`BURST_REACTION_EFFECT_PLAY`); the app plays no super reaction effects yet. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use serde_json::Value;

const ID: &str = "SuperReactionTweaks";

fn by_default(state: &State) -> bool {
    state.is_on(ID) && state.value(ID, "superReactByDefault") != Value::Bool(false)
}

impl Root {
    /// The reaction picker's Super Reactions toggle starts on.
    pub(in crate::ui) fn super_react_by_default(&self) -> bool {
        !crate::plugins::bypassed()
            && crate::plugins::with(by_default)
            && self.stores.account.premium_type.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn super_reactions_start_on_only_with_the_switch() {
        let mut state = State::default();
        assert!(!by_default(&state));
        state.set_enabled(ID, true);
        assert!(by_default(&state), "superReactByDefault defaults on");
        state.set_value(ID, "superReactByDefault", &json!(false));
        assert!(!by_default(&state));
    }
}
