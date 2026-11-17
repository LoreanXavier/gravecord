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

/// CopyUserURLs (Vencord `src/plugins/copyUserURLs`): "Copy User URL" at the end of a user's context menu.
pub(super) fn with_user_url_row(
    mut items: Vec<super::context_menu::Entry>,
    user: Option<Id>,
) -> Vec<super::context_menu::Entry> {
    if user.is_some() && !crate::plugins::bypassed() && crate::plugin_on!("CopyUserURLs") {
        items.push(super::context_menu::Entry::Separator);
        items.push(super::context_menu::Entry::Row(
            super::context_menu::Row::new(
                super::context_menu::Act::CopyLink,
                "Copy User URL".to_owned(),
            )
            .icon("menu_copy_message_link"),
        ));
    }
    items
}

/// The URL as the original copies it: in angle brackets, so a pasted link shows no embed.
pub(super) fn user_url(user: Id) -> String {
    format!("<https://discord.com/users/{user}>")
}

pub(super) fn copy_user_url(user: Id, cx: &mut gpui::App) {
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(user_url(user)));
}

/// QuickMention (Vencord `src/plugins/quickMention`): its hover bar button shows where you may write (any DM, a server
/// channel with Send Messages).
pub(super) fn quick_mention(can_send: bool) -> bool {
    can_send && !crate::plugins::bypassed() && crate::plugin_on!("QuickMention")
}

/// `insertTextIntoChatInputBox(`<@${msg.author.id}> `)`.
pub(super) fn mention_text(user: Id) -> String {
    format!("<@{user}> ")
}

/// BetterUploadButton (Vencord `src/plugins/betterUploadButton`): the composer's plus uploads on a click and opens its
/// menu on a right click.
pub(super) fn better_upload_button() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("BetterUploadButton")
}

/// ColorSighted (Vencord `src/plugins/colorSighted`): status dots keep their colour but lose the shapes cut into them
/// for colour blindness (`Masks.STATUS_ONLINE` for idle, Do Not Disturb, streaming and offline).
pub(super) fn color_sighted() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("ColorSighted")
}

/// ReverseImageSearch (Vencord `src/plugins/reverseImageSearch`): its engines, in its order, and the address each
/// opens for a picture (`engine + encodeURIComponent(src)`).
pub(super) const IMAGE_SEARCH_ENGINES: &[(&str, &str)] = &[
    ("Google", "https://lens.google.com/uploadbyurl?url="),
    (
        "Yandex",
        "https://yandex.com/images/search?rpt=imageview&url=",
    ),
    ("SauceNAO", "https://saucenao.com/search.php?url="),
    ("IQDB", "https://iqdb.org/?url="),
    (
        "Bing",
        "https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:",
    ),
    ("TinEye", "https://www.tineye.com/search?url="),
    ("ImgOps", "https://imgops.com/start?url="),
];

/// The last row of the submenu, which opens every engine.
pub(super) const IMAGE_SEARCH_ALL: &str = "All";

pub(super) fn reverse_image_search() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("ReverseImageSearch")
}

pub(super) fn image_search_url(engine: usize, src: &str) -> String {
    format!(
        "{}{}",
        IMAGE_SEARCH_ENGINES[engine].1,
        encode_uri_component(src)
    )
}

/// JavaScript's `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` as UTF-8 percent escapes.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// AnonymiseFileNames (Vencord `src/plugins/anonymiseFileNames`): the name an upload is sent with. `choice` is the
/// card's own switch (`None` until it is pressed: `anonymiseByDefault`), `now_ms` the local time for the Timestamp
/// and Date methods, `random` a source of indices for Random. `None` keeps the name.
pub(super) fn anonymised_name_in(
    state: &State,
    name: &str,
    choice: Option<bool>,
    now: chrono::DateTime<chrono::Local>,
    mut random: impl FnMut(usize) -> usize,
) -> Option<String> {
    const ID: &str = "AnonymiseFileNames";
    if !state.is_on(ID) {
        return None;
    }
    let flag = |key| state.value(ID, key) == Value::Bool(true);
    if !choice.unwrap_or_else(|| flag("anonymiseByDefault")) {
        return None;
    }
    // The app marks a spoiler in the name; the original keeps it apart from the name, so it survives here too.
    let (spoiler, plain) = match name.strip_prefix(super::uploads::SPOILER_PREFIX) {
        Some(plain) => (true, plain),
        None => (false, name),
    };
    // `tarExtMatcher`, else the last dot.
    let tar = regex_tar(plain);
    let ext = match tar.or_else(|| plain.rfind('.')) {
        Some(at) => &plain[at..],
        None => "",
    };
    let text = |key| state.value(ID, key).as_str().unwrap_or("").to_owned();
    let stem = match state.value(ID, "method").as_i64().unwrap_or(0) {
        1 => text("consistent"),
        2 => now.timestamp_millis().to_string(),
        3 => {
            let format = text("dateFormat")
                .replace("YYYY", &now.format("%Y").to_string())
                .replace("MM", &now.format("%m").to_string())
                .replace("DD", &now.format("%d").to_string())
                .replace("HH", &now.format("%H").to_string())
                .replace("mm", &now.format("%M").to_string())
                .replace("ss", &now.format("%S").to_string())
                .replace("SSS", &format!("{:03}", now.timestamp_subsec_millis()));
            if format.is_empty() {
                now.timestamp_millis().to_string()
            } else {
                format
            }
        }
        _ => {
            const CHARS: &[u8] = b"0123456789bdfhjkmnpqrstvwxz";
            let length = state
                .value(ID, "randomisedLength")
                .as_f64()
                .unwrap_or(7.)
                .max(0.) as usize;
            (0..length)
                .map(|_| CHARS[random(CHARS.len()) % CHARS.len()] as char)
                .collect()
        }
    };
    let prefix = if spoiler || flag("spoilerMessages") {
        super::uploads::SPOILER_PREFIX
    } else {
        ""
    };
    Some(format!("{prefix}{stem}{ext}"))
}

/// `/\.tar\.\w+$/`: where a `.tar.gz`-like extension starts.
fn regex_tar(name: &str) -> Option<usize> {
    let at = name.rfind(".tar.")?;
    let rest = &name[at + 5..];
    (!rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(at)
}

/// The name an upload is sent with now (see [`anonymised_name_in`]).
pub(super) fn anonymised_name(name: &str, choice: Option<bool>) -> Option<String> {
    if crate::plugins::bypassed() {
        return None;
    }
    let now = chrono::Local::now();
    crate::plugins::with(|state| {
        anonymised_name_in(state, name, choice, now, |count| {
            rand::random_range(0..count.max(1))
        })
    })
}

/// Whether an upload card shows the anonymise switch, and what it says while untouched (`anonymiseByDefault`).
pub(super) fn anonymise_default() -> Option<bool> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(|state| {
        state
            .is_on("AnonymiseFileNames")
            .then(|| state.value("AnonymiseFileNames", "anonymiseByDefault") == Value::Bool(true))
    })
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

    #[test]
    fn copy_user_urls_and_quick_mention_write_what_the_originals_write() {
        assert_eq!(user_url(42), "<https://discord.com/users/42>");
        assert_eq!(mention_text(42), "<@42> ");
        // Off (the unit tests' state), the menu is untouched and there is no button.
        let items = vec![super::super::context_menu::Entry::Separator];
        assert_eq!(with_user_url_row(items.clone(), Some(42)), items);
        assert!(!quick_mention(true));
        assert!(!better_upload_button());
        assert!(!color_sighted());
    }

    /// The QuickMention button sits where Vencord's popover buttons go: after the quick reactions, before Add
    /// Reaction, and it is there without the reaction buttons too.
    #[test]
    fn quick_mention_sits_before_add_reaction() {
        use super::super::hover_bar::{Button, Caps, buttons};
        let caps = Caps {
            can_react: true,
            can_reply: true,
            quick_mention: true,
            ..Caps::default()
        };
        assert_eq!(
            &buttons(caps)[..3],
            [Button::Quick, Button::QuickMention, Button::AddReaction]
        );
        let caps = Caps {
            can_reply: true,
            quick_mention: true,
            ..Caps::default()
        };
        assert_eq!(buttons(caps)[0], Button::QuickMention);
        let off = Caps {
            can_react: true,
            ..Caps::default()
        };
        assert!(!buttons(off).contains(&Button::QuickMention));
    }

    #[test]
    fn image_search_opens_each_engine_with_the_encoded_address() {
        assert_eq!(IMAGE_SEARCH_ENGINES.len(), 7);
        assert_eq!(
            image_search_url(0, "https://cdn.discordapp.com/a b.png?x=1&y=2"),
            "https://lens.google.com/uploadbyurl?url=https%3A%2F%2Fcdn.discordapp.com%2Fa%20b.png%3Fx%3D1%26y%3D2"
        );
        assert_eq!(encode_uri_component("a-_.!~*'()é"), "a-_.!~*'()%C3%A9");
        assert!(!reverse_image_search());
    }

    #[test]
    fn anonymised_names_follow_each_method() {
        let now = chrono::Local::now();
        let zero = |_| 0;
        let state = State::default();
        assert_eq!(anonymised_name_in(&state, "cat.png", None, now, zero), None);
        let mut state = with_on("AnonymiseFileNames");
        let name = |state: &State, file: &str, choice: Option<bool>, pick: fn(usize) -> usize| {
            anonymised_name_in(state, file, choice, now, pick)
        };
        assert_eq!(
            name(&state, "cat.png", None, zero).as_deref(),
            Some("0000000.png")
        );
        assert_eq!(name(&state, "cat.png", Some(false), zero), None);
        assert_eq!(
            name(&state, "a.tar.gz", None, |_| 26).as_deref(),
            Some("zzzzzzz.tar.gz")
        );
        assert_eq!(
            name(&state, "README", None, zero).as_deref(),
            Some("0000000")
        );
        // A spoiler stays one.
        assert_eq!(
            name(&state, "SPOILER_cat.png", None, zero).as_deref(),
            Some("SPOILER_0000000.png")
        );
        state.set_value("AnonymiseFileNames", "method", &json!(1));
        assert_eq!(
            name(&state, "cat.png", None, zero).as_deref(),
            Some("image.png")
        );
        state.set_value("AnonymiseFileNames", "spoilerMessages", &json!(true));
        assert_eq!(
            name(&state, "cat.png", None, zero).as_deref(),
            Some("SPOILER_image.png")
        );
        state.set_value("AnonymiseFileNames", "spoilerMessages", &json!(false));
        state.set_value("AnonymiseFileNames", "method", &json!(2));
        let millis = format!("{}.png", now.timestamp_millis());
        assert_eq!(name(&state, "cat.png", None, zero), Some(millis.clone()));
        state.set_value("AnonymiseFileNames", "method", &json!(3));
        state.set_value("AnonymiseFileNames", "dateFormat", &json!("YYYY/MM"));
        assert_eq!(
            name(&state, "cat.png", None, zero),
            Some(format!("{}.png", now.format("%Y/%m")))
        );
        state.set_value("AnonymiseFileNames", "dateFormat", &json!(""));
        assert_eq!(name(&state, "cat.png", None, zero), Some(millis));
        state.set_value("AnonymiseFileNames", "anonymiseByDefault", &json!(false));
        assert_eq!(name(&state, "cat.png", None, zero), None);
        assert!(name(&state, "cat.png", Some(true), zero).is_some());
    }
}
