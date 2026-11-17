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
