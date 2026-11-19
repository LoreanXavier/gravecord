//! SpotifyCrack (Vencord `src/plugins/spotifyCrack`, GPL-3.0-or-later): a Spotify account reads as Premium whatever
//! its product (`SPOTIFY_PROFILE_UPDATE`'s `isPremium`), so listening along works without Premium; with
//! `noSpotifyAutoPause` speaking in a voice channel no longer pauses Spotify after 30 seconds. Not part of the pinned
//! client.

use serde_json::Value;

const ID: &str = "SpotifyCrack";

/// Whether a Spotify profile reads as Premium regardless of its product.
pub(crate) fn spotify_premium_forced() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// `noSpotifyAutoPause`: the auto-pause timer does not start.
pub(in crate::ui) fn spotify_no_auto_pause() -> bool {
    !crate::plugins::bypassed()
        && crate::plugins::with(|state| {
            let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
            state.is_on(ID) && flag("noSpotifyAutoPause")
        })
}

#[cfg(test)]
mod tests {
    use crate::plugins::State;
    use serde_json::Value;

    #[test]
    fn the_defaults_follow_the_original() {
        let mut state = State::default();
        state.set_enabled(super::ID, true);
        assert_eq!(
            state.value(super::ID, "noSpotifyAutoPause"),
            Value::Bool(true)
        );
        assert_eq!(
            state.value(super::ID, "keepSpotifyActivityOnIdle"),
            Value::Bool(false)
        );
    }
}
