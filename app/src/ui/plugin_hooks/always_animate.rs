//! AlwaysAnimate (Vencord `src/plugins/alwaysAnimate`, GPL-3.0-or-later): what plays only under the pointer plays all
//! the time. `icons` forces `canAnimate` (avatars, server icons, profile banners), `nameplates` the nameplates' `animate`,
//! `roleGradients` every `animateGradient`; two of its patches carry no setting, so with the plugin on a gradient name in
//! chat and in the member list always slides. Not part of the pinned client.

use crate::plugins::State;
use serde_json::Value;

const ID: &str = "AlwaysAnimate";

/// What is asked about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Part {
    /// `canAnimate:!0`: avatars, server icons, banners.
    Icons,
    /// The nameplates' `animate:true,loop:true`.
    Nameplates,
    /// `animateGradient:!0` everywhere.
    RoleGradients,
    /// The two patches without a setting: gradient names in chat and in the member list.
    ChatAndMemberGradients,
}

fn plays(state: &State, part: Part) -> bool {
    state.is_on(ID)
        && match part {
            Part::Icons => state.value(ID, "icons") == Value::Bool(true),
            Part::Nameplates => state.value(ID, "nameplates") == Value::Bool(true),
            Part::RoleGradients => state.value(ID, "roleGradients") == Value::Bool(true),
            Part::ChatAndMemberGradients => true,
        }
}

/// Whether `part` plays without the pointer on it.
pub(in crate::ui) fn always_animate(part: Part) -> bool {
    !crate::plugins::bypassed() && crate::plugins::with(|state| plays(state, part))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_switch_plays_its_part_and_two_patches_need_none() {
        let mut state = State::default();
        for part in [
            Part::Icons,
            Part::Nameplates,
            Part::RoleGradients,
            Part::ChatAndMemberGradients,
        ] {
            assert!(!plays(&state, part));
        }
        state.set_enabled(ID, true);
        for part in [
            Part::Icons,
            Part::Nameplates,
            Part::RoleGradients,
            Part::ChatAndMemberGradients,
        ] {
            assert!(plays(&state, part), "{part:?} defaults on");
        }
        for (key, part) in [
            ("icons", Part::Icons),
            ("nameplates", Part::Nameplates),
            ("roleGradients", Part::RoleGradients),
        ] {
            state.set_value(ID, key, &json!(false));
            assert!(!plays(&state, part), "{key} off");
        }
        assert!(plays(&state, Part::ChatAndMemberGradients));
    }
}
