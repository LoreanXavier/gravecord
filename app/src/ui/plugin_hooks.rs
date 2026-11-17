//! Where the built-in plugins (`crate::plugins`) act on the chat. Not part of the pinned client.

use super::Root;
use super::menu::Action;
use dc_state::Id;
use gpui::{ClipboardItem, Context, Modifiers, Window};

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

/// The action a double-click runs, or `None`. `own`: the message is the account's; `editable`: a text message or a
/// reply (the kinds `canEditMessage` allows); `dm`: the open channel is a DM or group DM.
pub(super) fn double_click_action(
    own: bool,
    editable: bool,
    dm: bool,
    modifiers: Modifiers,
) -> ClickAction {
    const ID: &str = "MessageClickActions";
    if !crate::plugins::on(ID) {
        return ClickAction::None;
    }
    if dm && crate::plugins::flag(ID, "disableInDms") {
        return ClickAction::None;
    }
    if !modifier_held(&crate::plugins::text(ID, "doubleClickModifier"), modifiers) {
        return ClickAction::None;
    }
    let key = if own {
        "doubleClickAction"
    } else {
        "doubleClickOthersAction"
    };
    let action = ClickAction::parse(&crate::plugins::text(ID, key));
    match action {
        // Only your own text messages and replies can be edited; replies need a message that can be answered.
        ClickAction::Edit if !(own && editable) => ClickAction::None,
        ClickAction::Reply if !editable => ClickAction::None,
        other => other,
    }
}

/// VolumeBooster (Vencord `src/plugins/volumeBooster`): the top of the User Volume slider in percent, 200 (the
/// official `maxValue` on desktop) times the multiplier.
pub(super) fn user_volume_max() -> u32 {
    if crate::plugin_on!("VolumeBooster") {
        (200. * crate::plugins::number("VolumeBooster", "multiplier")).round() as u32
    } else {
        200
    }
}

/// NoPendingCount (Vencord `src/plugins/noPendingCount`): whether a request count badge is hidden, by its setting.
pub(super) fn hide_count(setting: &str) -> bool {
    crate::plugin_on!("NoPendingCount") && crate::plugins::flag("NoPendingCount", setting)
}

/// UnlimitedAccounts (Equicord `src/equicordplugins/unlimitedAccounts`): how many accounts the switcher keeps,
/// `getMaxAccounts` (0 is no limit), or the official five.
pub(super) fn max_accounts() -> usize {
    if !crate::plugin_on!("UnlimitedAccounts") {
        return super::account_switcher::MAX_ACCOUNTS;
    }
    match crate::plugins::number("UnlimitedAccounts", "maxAccounts") as usize {
        0 => usize::MAX,
        most => most,
    }
}

/// RevealAllSpoilers (Vencord `src/plugins/revealAllSpoilers`): a key in `Root::revealed` that opens every spoiler of a
/// message (`(message, u32::MAX, 0)`) or of a channel (`(channel, u32::MAX, 1)`), next to the per-spoiler keys
/// `(message, block, start)`.
const ALL_OF_MESSAGE: (u32, usize) = (u32::MAX, 0);
const ALL_OF_CHANNEL: (u32, usize) = (u32::MAX, 1);

/// A spoiler was opened by a press: with Ctrl, every spoiler of its message opens, with Ctrl+Shift every one in the
/// channel (the original's `ctrlKey`/`metaKey` and `shiftKey`).
pub(super) fn reveal_all_spoilers(
    revealed: &mut std::collections::HashSet<(Id, u32, usize)>,
    message: Id,
    channel: Option<Id>,
    modifiers: Modifiers,
) {
    if !crate::plugin_on!("RevealAllSpoilers") || !(modifiers.control || modifiers.platform) {
        return;
    }
    match (modifiers.shift, channel) {
        (true, Some(channel)) => {
            revealed.insert((channel, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1));
        }
        _ => {
            revealed.insert((message, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1));
        }
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
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.id == found.author.id);
        let editable = matches!(found.kind, 0 | 19);
        let dm = self.guild.is_none();
        match double_click_action(own, editable, dm, modifiers) {
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

    #[test]
    fn whole_message_and_whole_channel_keys_open_spoilers() {
        let mut revealed = std::collections::HashSet::new();
        assert!(!all_spoilers_revealed(&revealed, 7, Some(9)));
        revealed.insert((7, ALL_OF_MESSAGE.0, ALL_OF_MESSAGE.1));
        assert!(all_spoilers_revealed(&revealed, 7, Some(9)));
        assert!(!all_spoilers_revealed(&revealed, 8, Some(9)));
        revealed.insert((9, ALL_OF_CHANNEL.0, ALL_OF_CHANNEL.1));
        assert!(all_spoilers_revealed(&revealed, 8, Some(9)));
        assert!(!all_spoilers_revealed(&revealed, 8, Some(10)));
        // Off by default: a Ctrl press reveals nothing more.
        let mut untouched = std::collections::HashSet::new();
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        reveal_all_spoilers(&mut untouched, 7, Some(9), ctrl);
        assert!(untouched.is_empty());
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
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(modifier_held("NONE", none));
        assert!(!modifier_held("NONE", shift));
        assert!(modifier_held("SHIFT", shift));
        assert!(!modifier_held("CTRL", shift));
    }

    /// With the defaults (the plugin is on by default): your message is edited, someone else's is answered, and a
    /// message that cannot be edited or answered gets nothing.
    #[test]
    fn the_default_double_click_edits_your_messages_and_replies_to_others() {
        let none = Modifiers::default();
        assert_eq!(
            double_click_action(true, true, false, none),
            ClickAction::Edit
        );
        assert_eq!(
            double_click_action(false, true, false, none),
            ClickAction::Reply
        );
        assert_eq!(
            double_click_action(true, false, false, none),
            ClickAction::None
        );
        assert_eq!(
            double_click_action(false, false, true, none),
            ClickAction::None
        );
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(
            double_click_action(true, true, false, ctrl),
            ClickAction::None
        );
    }
}
