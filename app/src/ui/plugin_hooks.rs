//! Where the built-in plugins (`crate::plugins`) act on the chat. Not part of the pinned client.

use super::Root;
use super::menu::{Action, Item};
use crate::plugins::State;
use dc_state::Id;
use gpui::{ClipboardItem, Context, Modifiers, Window};
use serde_json::Value;

/// What a double-click on a message does under MessageClickActions (Vencord `src/plugins/messageClickActions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClickAction {
    None,
    Edit,
    Reply,
    CopyContent,
    CopyLink,
    CopyId,
    CopyUserId,
}

impl ClickAction {
    fn parse(value: &str) -> Self {
        match value {
            "EDIT" => Self::Edit,
            "REPLY" => Self::Reply,
            "COPY_CONTENT" => Self::CopyContent,
            "COPY_LINK" => Self::CopyLink,
            "COPY_ID" => Self::CopyId,
            "COPY_USER_ID" => Self::CopyUserId,
            _ => Self::None,
        }
    }
}

/// `isModifierPressed`: "NONE" wants no modifier held, the others want theirs.
fn modifier_held(wanted: &str, modifiers: Modifiers) -> bool {
    match wanted {
        "SHIFT" => modifiers.shift,
        "CTRL" => modifiers.control,
        "ALT" => modifiers.alt,
        _ => !(modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform),
    }
}

/// The action a double-click runs, or `None`. `own`: the message is the account's; `dm`: the open channel is a DM or
/// group DM. Whether the message can be edited or answered is the message menu's call (`message_double_clicked`).
fn double_click_action_in(state: &State, own: bool, dm: bool, modifiers: Modifiers) -> ClickAction {
    const ID: &str = "MessageClickActions";
    if !state.is_on(ID) || (dm && state.value(ID, "disableInDms") == Value::Bool(true)) {
        return ClickAction::None;
    }
    if !modifier_held(
        state
            .value(ID, "doubleClickModifier")
            .as_str()
            .unwrap_or(""),
        modifiers,
    ) {
        return ClickAction::None;
    }
    let key = if own {
        "doubleClickAction"
    } else {
        "doubleClickOthersAction"
    };
    match ClickAction::parse(state.value(ID, key).as_str().unwrap_or("")) {
        // The original edits only your own messages, whatever the setting says.
        ClickAction::Edit if !own => ClickAction::None,
        other => other,
    }
}

/// VolumeBooster (Vencord `src/plugins/volumeBooster`): the top of the User Volume slider in percent, 200 (the
/// official `maxValue` on desktop) times the multiplier.
fn user_volume_max_in(state: &State) -> u32 {
    match state.value("VolumeBooster", "multiplier").as_f64() {
        Some(multiplier) if state.is_on("VolumeBooster") => (200. * multiplier).round() as u32,
        _ => 200,
    }
}

pub(super) fn user_volume_max() -> u32 {
    crate::plugins::with(user_volume_max_in)
}

/// The loudest factor one person may have now: the slider's top on the slider's curve (2 at 200%).
pub(super) fn user_volume_max_factor() -> f32 {
    super::menu_slider::perceptual_to_amplitude_raw(user_volume_max() as f32)
}

/// NoPendingCount (Vencord `src/plugins/noPendingCount`): whether a request count badge is hidden, by its setting.
fn hide_count_in(state: &State, setting: &str) -> bool {
    state.is_on("NoPendingCount") && state.value("NoPendingCount", setting) == Value::Bool(true)
}

pub(super) fn hide_count(setting: &str) -> bool {
    crate::plugins::with(|state| hide_count_in(state, setting))
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
    crate::plugins::with(max_accounts_in)
}

/// AlwaysTrust (Vencord `src/plugins/alwaysTrust`, its `domain` setting): `isTrustedDomain` answers yes for every web
/// address. Other protocols still ask.
fn trust_every_link_in(state: &State) -> bool {
    state.is_on("AlwaysTrust") && state.value("AlwaysTrust", "domain") == Value::Bool(true)
}

pub(super) fn trust_every_link() -> bool {
    crate::plugins::with(trust_every_link_in)
}

/// RevealAllSpoilers (Vencord `src/plugins/revealAllSpoilers`): a key in `Root::revealed` that opens every spoiler of a
/// message (`(message, u32::MAX, 0)`) or of a channel (`(channel, u32::MAX, 1)`), next to the per-spoiler keys
/// `(message, block, start)`.
const ALL_OF_MESSAGE: (u32, usize) = (u32::MAX, 0);
const ALL_OF_CHANNEL: (u32, usize) = (u32::MAX, 1);

/// The key a press on a spoiler adds besides its own: with Ctrl the message's, with Ctrl+Shift the channel's (the
/// original's `ctrlKey`/`metaKey` and `shiftKey`).
fn reveal_all_key_in(
    state: &State,
    message: Id,
    channel: Option<Id>,
    modifiers: Modifiers,
) -> Option<(Id, u32, usize)> {
    if !state.is_on("RevealAllSpoilers") || !(modifiers.control || modifiers.platform) {
        return None;
    }
    Some(match (modifiers.shift, channel) {
        (true, Some(channel)) => (channel, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1),
        _ => (message, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1),
    })
}

pub(super) fn reveal_all_spoilers(
    revealed: &mut std::collections::HashSet<(Id, u32, usize)>,
    message: Id,
    channel: Option<Id>,
    modifiers: Modifiers,
) {
    if let Some(key) =
        crate::plugins::with(|state| reveal_all_key_in(state, message, channel, modifiers))
    {
        revealed.insert(key);
    }
}

pub(super) fn all_spoilers_revealed(
    revealed: &std::collections::HashSet<(Id, u32, usize)>,
    message: Id,
    channel: Option<Id>,
) -> bool {
    revealed.contains(&(message, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1))
        || channel.is_some_and(|channel| {
            revealed.contains(&(channel, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1))
        })
}

impl Root {
    /// A double press on a message of the open channel.
    pub(super) fn message_double_clicked(
        &mut self,
        message: Id,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(found) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|known| known.id == message))
            .cloned()
        else {
            return;
        };
        // A message still sending, or seen only by you (an ephemeral notice), takes no click action.
        if self.send_state_of(message).is_some()
            || found.flags & dc_state::components::FLAG_EPHEMERAL != 0
        {
            return;
        }
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.id == found.author.id);
        let dm = self.guild.is_none();
        let action =
            crate::plugins::with(|state| double_click_action_in(state, own, dm, modifiers));
        // Edit and Reply run only where the message menu offers them (a text message of yours, a channel you can
        // write in, not a poll or a failed send).
        let offered = |wanted: Action| {
            super::menu::message_items_saved(own, self.saved_flags(message))
                .iter()
                .any(|item| matches!(item, Item::Entry(_, _, action, _) if *action == wanted))
        };
        let editable = matches!(found.kind, 0 | 19);
        match action {
            ClickAction::Edit if !(editable && offered(Action::Edit)) => return,
            ClickAction::Reply if !offered(Action::Reply) => return,
            _ => {}
        }
        match action {
            ClickAction::None => return,
            ClickAction::Edit => self.run_action(Action::Edit, message, window, cx),
            ClickAction::Reply => self.run_action(Action::Reply, message, window, cx),
            ClickAction::CopyContent => self.run_action(Action::CopyText, message, window, cx),
            ClickAction::CopyLink => self.run_action(Action::CopyLink, message, window, cx),
            ClickAction::CopyId => self.run_action(Action::CopyId, message, window, cx),
            ClickAction::CopyUserId => {
                cx.write_to_clipboard(ClipboardItem::new_string(found.author.id.to_string()));
            }
        }
        // The double press also selected a word under the pointer; the action replaces that.
        cx.stop_propagation();
        cx.notify();
    }
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

    #[test]
    fn actions_parse_from_the_original_values() {
        assert_eq!(ClickAction::parse("EDIT"), ClickAction::Edit);
        assert_eq!(ClickAction::parse("COPY_USER_ID"), ClickAction::CopyUserId);
        // Actions of the original that are not ported do nothing.
        assert_eq!(ClickAction::parse("DELETE"), ClickAction::None);
    }

    #[test]
    fn the_modifier_setting_wants_exactly_its_key() {
        assert!(modifier_held("NONE", keys(false, false)));
        assert!(!modifier_held("NONE", keys(false, true)));
        assert!(modifier_held("SHIFT", keys(false, true)));
        assert!(!modifier_held("CTRL", keys(false, true)));
    }

    /// Defaults (the plugin is on by default): your message is edited, someone else's is answered.
    #[test]
    fn double_click_follows_its_settings() {
        let none = keys(false, false);
        let mut state = State::default();
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::Edit
        );
        assert_eq!(
            double_click_action_in(&state, false, false, none),
            ClickAction::Reply
        );
        assert_eq!(
            double_click_action_in(&state, true, false, keys(true, false)),
            ClickAction::None
        );
        state.set_value("MessageClickActions", "doubleClickModifier", &json!("CTRL"));
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::None
        );
        assert_eq!(
            double_click_action_in(&state, true, false, keys(true, false)),
            ClickAction::Edit
        );
        state.set_value("MessageClickActions", "doubleClickModifier", &json!("NONE"));
        state.set_value("MessageClickActions", "disableInDms", &json!(true));
        assert_eq!(
            double_click_action_in(&state, true, true, none),
            ClickAction::None
        );
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::Edit
        );
        state.set_value(
            "MessageClickActions",
            "doubleClickAction",
            &json!("COPY_LINK"),
        );
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::CopyLink
        );
        state.set_enabled("MessageClickActions", false);
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::None
        );
        state.set_enabled("MessageClickActions", true);
        state.master = false;
        assert_eq!(
            double_click_action_in(&state, true, false, none),
            ClickAction::None
        );
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
    fn ctrl_reveals_the_message_and_ctrl_shift_the_channel() {
        let state = with_on("RevealAllSpoilers");
        assert_eq!(
            reveal_all_key_in(&state, 7, Some(9), keys(false, false)),
            None
        );
        assert_eq!(
            reveal_all_key_in(&state, 7, Some(9), keys(true, false)),
            Some((7, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1))
        );
        assert_eq!(
            reveal_all_key_in(&state, 7, Some(9), keys(true, true)),
            Some((9, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1))
        );
        assert_eq!(
            reveal_all_key_in(&State::default(), 7, Some(9), keys(true, false)),
            None
        );
        let mut revealed = std::collections::HashSet::new();
        assert!(!all_spoilers_revealed(&revealed, 7, Some(9)));
        revealed.insert((7, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1));
        assert!(all_spoilers_revealed(&revealed, 7, Some(9)));
        assert!(!all_spoilers_revealed(&revealed, 8, Some(9)));
        revealed.insert((9, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1));
        assert!(all_spoilers_revealed(&revealed, 8, Some(9)));
        assert!(!all_spoilers_revealed(&revealed, 8, Some(10)));
    }
}
