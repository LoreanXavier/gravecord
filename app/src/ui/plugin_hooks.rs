//! Where the built-in plugins (`crate::plugins`) act on the chat. Not part of the pinned client.

mod message_clicks;
mod voice_stats;

pub(super) use message_clicks::follow as follow_message_clicks;
pub(in crate::ui) use voice_stats::import_totals as import_voice_totals;

use crate::plugins::State;
use dc_state::Id;
use gpui::Modifiers;
use serde_json::Value;

/// VolumeBooster (Vencord `src/plugins/volumeBooster`): the top of the User Volume slider in percent, 200 (the
/// official `maxValue` on desktop) times the multiplier.
fn user_volume_max_in(state: &State) -> u32 {
    match state.value("VolumeBooster", "multiplier").as_f64() {
        Some(multiplier) if state.is_on("VolumeBooster") => (200. * multiplier).round() as u32,
        _ => 200,
    }
}

pub(super) fn user_volume_max() -> u32 {
    if crate::plugins::bypassed() {
        return 200;
    }
    crate::plugins::with(user_volume_max_in)
}

/// The loudest factor one person may have while VolumeBooster is on: the slider's top on the slider's curve. `None`
/// while it is off, where the volume is the pinned client's (the voice engine's own limit, 2).
pub(super) fn user_volume_cap() -> Option<f32> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(user_volume_cap_in)
}

fn user_volume_cap_in(state: &State) -> Option<f32> {
    state
        .is_on("VolumeBooster")
        .then(|| super::menu_slider::perceptual_to_amplitude_raw(user_volume_max_in(state) as f32))
}

/// NoPendingCount (Vencord `src/plugins/noPendingCount`): whether a request count badge is hidden, by its setting.
fn hide_count_in(state: &State, setting: &str) -> bool {
    state.is_on("NoPendingCount") && state.value("NoPendingCount", setting) == Value::Bool(true)
}

/// NoPendingCount's `hideFriendRequestsCount`: `getPendingCount` answers 0.
pub(super) fn hide_friend_request_count() -> bool {
    hide_count("hideFriendRequestsCount")
}

/// NoPendingCount's `hideMessageRequestsCount`: the Message Requests badge is hidden, the row stays.
pub(super) fn hide_message_request_count() -> bool {
    hide_count("hideMessageRequestsCount")
}

fn hide_count(setting: &str) -> bool {
    !crate::plugins::bypassed() && crate::plugins::with(|state| hide_count_in(state, setting))
}

/// UnlimitedAccounts (Equicord `src/equicordplugins/unlimitedAccounts`): how many accounts the switcher keeps,
/// `getMaxAccounts` (0 is no limit), or the official five.
fn max_accounts_in(state: &State) -> usize {
    if !state.is_on("UnlimitedAccounts") {
        return super::account_switcher::MAX_ACCOUNTS;
    }
    match state
        .value("UnlimitedAccounts", "maxAccounts")
        .as_f64()
        .unwrap_or(0.) as usize
    {
        0 => usize::MAX,
        most => most,
    }
}

pub(super) fn max_accounts() -> usize {
    if crate::plugins::bypassed() {
        return super::account_switcher::MAX_ACCOUNTS;
    }
    crate::plugins::with(max_accounts_in)
}

/// AlwaysTrust (Vencord `src/plugins/alwaysTrust`, its `domain` setting): `isTrustedDomain` answers yes for every web
/// address. Other protocols still ask.
fn trust_every_link_in(state: &State) -> bool {
    state.is_on("AlwaysTrust") && state.value("AlwaysTrust", "domain") == Value::Bool(true)
}

pub(super) fn trust_every_link() -> bool {
    !crate::plugins::bypassed() && crate::plugins::with(trust_every_link_in)
}

/// AlwaysTrust's `noDeleteSafety` and `confirmModal`: how a server is deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeleteSafety {
    /// The pinned client's dialog: the server's name typed back.
    Official,
    /// The plugin's "Delete server?" question, no name to type.
    Confirm,
    /// Deleted at once.
    Skip,
}

fn server_delete_safety_in(state: &State) -> DeleteSafety {
    const ID: &str = "AlwaysTrust";
    if !state.is_on(ID) || state.value(ID, "noDeleteSafety") != Value::Bool(true) {
        DeleteSafety::Official
    } else if state.value(ID, "confirmModal") == Value::Bool(true) {
        DeleteSafety::Confirm
    } else {
        DeleteSafety::Skip
    }
}

pub(super) fn server_delete_safety() -> DeleteSafety {
    if crate::plugins::bypassed() {
        return DeleteSafety::Official;
    }
    crate::plugins::with(server_delete_safety_in)
}

/// RevealAllSpoilers (Vencord `src/plugins/revealAllSpoilers`): a key in `Root::revealed` that opens every spoiler of a
/// message (`(message, u32::MAX, 0)`), next to the per-spoiler keys `(message, block, start)`.
const ALL_OF_MESSAGE: (u32, usize) = (u32::MAX, 0);

/// The messages whose spoilers a press on a hidden spoiler of `message` opens too: with Ctrl its own message's (the
/// spoiler's parent), with Ctrl+Shift every message the chat holds now (`messagesWrapper`; later messages stay
/// hidden). Ctrl is `metaKey` on a Mac (`IS_MAC`).
fn reveal_all_in(state: &State, message: Id, loaded: &[Id], modifiers: Modifiers) -> Vec<Id> {
    let held = if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    };
    if !held || !state.is_on("RevealAllSpoilers") {
        return Vec::new();
    }
    if modifiers.shift {
        loaded.to_vec()
    } else {
        vec![message]
    }
}

pub(super) fn reveal_all_spoilers(
    revealed: &mut std::collections::HashSet<(Id, u32, usize)>,
    message: Id,
    loaded: &[Id],
    modifiers: Modifiers,
) {
    if crate::plugins::bypassed() {
        return;
    }
    for message in crate::plugins::with(|state| reveal_all_in(state, message, loaded, modifiers)) {
        revealed.insert((message, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1));
    }
}

pub(super) fn all_spoilers_revealed(
    revealed: &std::collections::HashSet<(Id, u32, usize)>,
    message: Id,
) -> bool {
    !crate::plugins::bypassed() && revealed.contains(&(message, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_on(id: &str) -> State {
        let mut state = State::default();
        state.set_enabled(id, true);
        state
    }

    fn keys(control: bool, shift: bool) -> Modifiers {
        Modifiers {
            control,
            shift,
            ..Modifiers::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    /// The two ways the plugins are off: the master switch off with every plugin on and every setting changed from
    /// its default, and the master on with every plugin turned off.
    fn off_states() -> [State; 2] {
        let mut everything = State::default();
        for def in crate::plugins::DEFS {
            everything.set_enabled(def.id, true);
            for setting in def.settings {
                assert!(
                    everything.set_value(def.id, setting.key, &setting.scrambled()),
                    "{}.{} takes its scrambled value",
                    def.id,
                    setting.key
                );
            }
        }
        everything.master = false;
        let mut each_off = State::default();
        for def in crate::plugins::DEFS {
            each_off.set_enabled(def.id, false);
        }
        [everything, each_off]
    }

    /// While the plugins are off every hook gives the pinned client's answer, and none of them counts an effect.
    #[test]
    fn every_hook_answers_as_the_pinned_client_while_the_plugins_are_off() {
        for state in off_states() {
            crate::plugins::reset_effects();
            let held = [
                keys(false, false),
                keys(true, false),
                keys(false, true),
                keys(true, true),
                alt(),
            ];
            // MessageClickActions reads no setting and so follows no key or press.
            assert!(message_clicks::Settings::read(&state).is_none());
            for modifiers in held {
                assert!(reveal_all_in(&state, 7, &[5, 7], modifiers).is_empty());
            }
            assert_eq!(user_volume_max_in(&state), 200);
            assert_eq!(user_volume_cap_in(&state), None);
            assert_eq!(
                max_accounts_in(&state),
                super::super::account_switcher::MAX_ACCOUNTS
            );
            assert!(!trust_every_link_in(&state));
            assert_eq!(server_delete_safety_in(&state), DeleteSafety::Official);
            assert!(!hide_count_in(&state, "hideFriendRequestsCount"));
            assert!(!hide_count_in(&state, "hideMessageRequestsCount"));
            for def in crate::plugins::DEFS {
                assert!(!state.is_on(def.id), "{} is off", def.id);
            }
            assert!(
                crate::plugins::effects().is_empty(),
                "{:?}",
                crate::plugins::effects()
            );
        }
    }

    /// The counter does see a plugin that acts, so the test above can fail.
    #[test]
    fn an_acting_plugin_is_counted() {
        crate::plugins::reset_effects();
        let state = with_on("VolumeBooster");
        assert_eq!(user_volume_max_in(&state), 400);
        assert_eq!(crate::plugins::effects().get("VolumeBooster"), Some(&1));
    }

    /// The source files of the app, for the static checks below.
    fn sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let name = path.to_string_lossy().replace('\\', "/");
                    out.push((name, std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        assert!(out.len() > 50);
        out
    }

    /// The plugin code never sends a request or touches the network: a hook only reads the plugin state and changes
    /// what the app does locally. A port that needs the network has to change this test on purpose.
    #[test]
    fn plugin_code_sends_no_requests() {
        for (name, text) in sources() {
            if !(name.contains("/src/plugins") || name.ends_with("/plugin_hooks.rs")) {
                continue;
            }
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for banned in [
                "Request::",
                "requests.send",
                "reqwest",
                "dc_http",
                "http_client",
                "std::net",
            ] {
                assert!(!code.contains(banned), "{name} uses {banned}");
            }
        }
    }

    /// Every plugin is read by the code outside its definition, and so is every setting, unless `IDLE_SETTINGS`
    /// says why it has nothing to act on yet; a listed setting must really be unread, so the list cannot go stale.
    #[test]
    fn every_plugin_and_setting_is_read_outside_the_registry() {
        let code: String = sources()
            .into_iter()
            .filter(|(name, _)| !name.ends_with("/plugins/registry.rs"))
            .map(|(_, text)| text.split("#[cfg(test)]").next().unwrap_or("").to_owned())
            .collect();
        // Settings are read in the hooks only (`plugin_hooks.rs` and its modules), where a key's name cannot be
        // some other string of the app.
        let hooks: String = sources()
            .into_iter()
            .filter(|(name, _)| name.contains("/plugin_hooks"))
            .map(|(_, text)| text.split("#[cfg(test)]").next().unwrap_or("").to_owned())
            .collect();
        for def in crate::plugins::DEFS {
            assert!(
                code.contains(&format!("\"{}\"", def.id)),
                "nothing reads {}",
                def.id
            );
            for setting in def.settings {
                let read = hooks.contains(&format!("\"{}\"", setting.key));
                let idle = crate::plugins::IDLE_SETTINGS
                    .iter()
                    .any(|(id, key, _)| *id == def.id && *key == setting.key);
                assert!(
                    read != idle,
                    "{}.{}: read {read}, listed idle {idle}",
                    def.id,
                    setting.key
                );
            }
        }
        for (id, key, why) in crate::plugins::IDLE_SETTINGS {
            assert!(
                crate::plugins::def(id)
                    .and_then(|def| def.setting(key))
                    .is_some(),
                "{id}.{key} is not a setting"
            );
            assert!(why.len() > 20, "{id}.{key} needs its reason");
        }
    }

    #[test]
    fn volume_booster_raises_the_slider_top_by_its_multiplier() {
        assert_eq!(user_volume_max_in(&State::default()), 200);
        let mut state = with_on("VolumeBooster");
        assert_eq!(user_volume_max_in(&state), 400);
        state.set_value("VolumeBooster", "multiplier", &json!(3.5));
        assert_eq!(user_volume_max_in(&state), 700);
        state.set_enabled("VolumeBooster", false);
        assert_eq!(user_volume_max_in(&state), 200);
        // 200% is the official factor 2 on the slider curve.
        assert!(
            (super::super::menu_slider::perceptual_to_amplitude_raw(200.) - 1.995).abs() < 0.01
        );
    }

    #[test]
    fn no_pending_count_hides_each_badge_by_its_setting() {
        assert!(!hide_count_in(&State::default(), "hideFriendRequestsCount"));
        let mut state = with_on("NoPendingCount");
        assert!(hide_count_in(&state, "hideFriendRequestsCount"));
        assert!(hide_count_in(&state, "hideMessageRequestsCount"));
        state.set_value("NoPendingCount", "hideMessageRequestsCount", &json!(false));
        assert!(!hide_count_in(&state, "hideMessageRequestsCount"));
        assert!(hide_count_in(&state, "hideFriendRequestsCount"));
    }

    #[test]
    fn unlimited_accounts_lifts_the_five_account_limit() {
        assert_eq!(max_accounts_in(&State::default()), 5);
        let mut state = with_on("UnlimitedAccounts");
        assert_eq!(max_accounts_in(&state), usize::MAX);
        state.set_value("UnlimitedAccounts", "maxAccounts", &json!(8));
        assert_eq!(max_accounts_in(&state), 8);
    }

    #[test]
    fn always_trust_needs_its_domain_setting() {
        assert!(!trust_every_link_in(&State::default()));
        let mut state = with_on("AlwaysTrust");
        assert!(trust_every_link_in(&state));
        state.set_value("AlwaysTrust", "domain", &json!(false));
        assert!(!trust_every_link_in(&state));
    }

    /// GifPaste, DisableCallIdle and NoTypingAnimation have no settings: their sites ask `plugin_on!` alone.
    #[test]
    fn switch_only_plugins_follow_their_switch_and_the_master() {
        for id in ["GifPaste", "DisableCallIdle", "NoTypingAnimation"] {
            assert!(!State::default().is_on(id), "{id} is off by default");
            let mut state = with_on(id);
            assert!(state.is_on(id));
            state.master = false;
            assert!(!state.is_on(id));
        }
    }

    #[test]
    fn ctrl_reveals_the_message_and_ctrl_shift_every_loaded_message() {
        let state = with_on("RevealAllSpoilers");
        let loaded = [5, 6, 7];
        assert!(reveal_all_in(&state, 7, &loaded, keys(false, false)).is_empty());
        assert_eq!(reveal_all_in(&state, 7, &loaded, keys(true, false)), [7]);
        assert_eq!(
            reveal_all_in(&state, 7, &loaded, keys(true, true)),
            [5, 6, 7]
        );
        // Shift alone, or the Windows key, is not the original's Ctrl.
        assert!(reveal_all_in(&state, 7, &loaded, keys(false, true)).is_empty());
        let win = Modifiers {
            platform: true,
            ..Modifiers::default()
        };
        assert!(reveal_all_in(&state, 7, &loaded, win).is_empty());
        assert!(reveal_all_in(&State::default(), 7, &loaded, keys(true, false)).is_empty());
        let mut revealed = std::collections::HashSet::new();
        assert!(!all_spoilers_revealed(&revealed, 7));
        revealed.insert((7, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1));
        assert!(all_spoilers_revealed(&revealed, 7));
        assert!(!all_spoilers_revealed(&revealed, 8));
    }

    #[test]
    fn always_trust_deletes_servers_by_its_two_settings() {
        assert_eq!(
            server_delete_safety_in(&State::default()),
            DeleteSafety::Official
        );
        let mut state = with_on("AlwaysTrust");
        assert_eq!(server_delete_safety_in(&state), DeleteSafety::Confirm);
        state.set_value("AlwaysTrust", "confirmModal", &json!(false));
        assert_eq!(server_delete_safety_in(&state), DeleteSafety::Skip);
        state.set_value("AlwaysTrust", "noDeleteSafety", &json!(false));
        assert_eq!(server_delete_safety_in(&state), DeleteSafety::Official);
    }
}
