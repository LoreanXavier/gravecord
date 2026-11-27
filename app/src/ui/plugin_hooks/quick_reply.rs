//! QuickReply (Vencord `src/plugins/quickReply`, GPL-3.0-or-later): Ctrl+Up and Ctrl+Down (Cmd on a Mac; not with
//! Alt or the Windows key) reply to the previous or next message of the open channel, Ctrl+Shift+Up and
//! Ctrl+Shift+Down edit the previous or next of your own. The first press takes the newest message; each next one
//! steps from the message being replied to or edited (a reply or edit started some other way counts too), and
//! stepping past the newest cancels the reply or ends the edit. Messages that are deleted (MessageLogger keeps them),
//! not replyable, ephemeral or not sent yet are passed over, and with `ignoreBlockedAndIgnored` those of blocked or
//! ignored people too. A reply mentions by `shouldMention` (its "Follow NoReplyMention" choice mentions: that plugin
//! is not built in, as the original does without it); the composer takes focus for a reply, the edit box for an
//! edit, and the list scrolls only when the message is out of view, without a flash. In a server channel where you
//! cannot send messages the key does nothing. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use dc_state::{Id, MessageState};
use gpui::{Context, Keystroke, Window};
use serde_json::Value;

const ID: &str = "QuickReply";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Settings {
    mention: bool,
    skip_hidden: bool,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    Some(Settings {
        // `MentionOptions`: 0 is DISABLED, 1 ENABLED, 2 follows NoReplyMention (not built in, so it mentions).
        mention: state.value(ID, "shouldMention").as_i64() != Some(0),
        skip_hidden: state.value(ID, "ignoreBlockedAndIgnored") == Value::Bool(true),
    })
}

/// `onKeydown`: the arrow and whether it is the edit (Shift) form; `None` for any other key.
fn pressed(keystroke: &Keystroke) -> Option<(bool, bool)> {
    let up = match keystroke.key.as_str() {
        "up" => true,
        "down" => false,
        _ => return None,
    };
    let held = keystroke.modifiers;
    let mac = cfg!(target_os = "macos");
    // `isCtrl`: metaKey on a Mac, else ctrlKey; `isAltOrMeta`: Alt, or the Windows key off a Mac.
    let ctrl = if mac { held.platform } else { held.control };
    let alt_or_meta = held.alt || (!mac && held.platform);
    (ctrl && !alt_or_meta).then_some((up, held.shift))
}

/// `findNextNonDeleted` over the candidates, oldest first: the newest without a current one (or when it is gone),
/// else its neighbour, `None` past either end.
fn next(candidates: &[Id], current: Option<Id>, up: bool) -> Option<Id> {
    let newest = candidates.last().copied();
    let Some(at) = current.and_then(|current| candidates.iter().position(|id| *id == current))
    else {
        return newest;
    };
    if up {
        at.checked_sub(1).map(|before| candidates[before])
    } else {
        candidates.get(at + 1).copied()
    }
}

impl Root {
    /// The key layer's turn for QuickReply: true when the key was taken. `covered` is a modal or settings layer over
    /// the chat, where the key is left alone.
    pub(in crate::ui) fn quick_reply_key(
        &mut self,
        keystroke: &Keystroke,
        covered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((up, edit)) = pressed(keystroke) else {
            return false;
        };
        if covered || crate::plugins::bypassed() {
            return false;
        }
        let Some(settings) = crate::plugins::with(settings_of) else {
            return false;
        };
        let Some(channel) = self.channel else {
            return false;
        };
        // `currChannel.guild_id && !PermissionStore.can(SEND_MESSAGES)`: nothing, the key still taken.
        if !self.can_send_here() {
            return true;
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let candidates: Vec<Id> = self
            .stores
            .messages
            .get(&channel)
            .into_iter()
            .flatten()
            .filter(|message| {
                let (blocked, ignored) = self.hidden_for_message(message);
                message.state == MessageState::Sent
                    && !super::message_logger_deleted(message.id)
                    && (!edit || Some(message.author.id) == me)
                    && super::super::hover_bar::REPLYABLE.contains(&message.kind)
                    && message.flags & dc_state::components::FLAG_EPHEMERAL == 0
                    && !(settings.skip_hidden && (blocked || ignored))
            })
            .map(|message| message.id)
            .collect();
        let current = if edit {
            self.editing.as_ref().map(|(id, _)| *id)
        } else {
            self.replying.as_ref().map(|message| message.id)
        };
        let found = next(&candidates, current, up).and_then(|id| {
            self.stores
                .messages
                .get(&channel)?
                .iter()
                .position(|message| message.id == id)
        });
        let Some(index) = found else {
            // `DELETE_PENDING_REPLY` / `MESSAGE_END_EDIT`.
            if edit {
                if self.editing.is_some() {
                    self.end_edit(window, cx);
                }
            } else if self.replying.take().is_some() {
                cx.notify();
            }
            return true;
        };
        let message = self.stores.messages[&channel][index].clone();
        if edit {
            self.begin_edit(message.id, &message.content, window, cx);
        } else {
            self.begin_reply(message, window, cx);
            self.reply_ping = settings.mention;
        }
        // `jumpIfOffScreen`: the row comes into view, without a flash, only when it is out of it.
        let row = self.rows.iter().position(
            |row| matches!(row, super::super::Row::Message { index: at, .. } if *at == index),
        );
        if let Some(row) = row {
            self.list.scroll_to_reveal_item(row);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stroke(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    #[test]
    fn the_keys_follow_the_original() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert_eq!(pressed(&stroke("ctrl-up")), Some((true, false)));
        assert_eq!(pressed(&stroke("ctrl-down")), Some((false, false)));
        assert_eq!(pressed(&stroke("ctrl-shift-up")), Some((true, true)));
        assert_eq!(pressed(&stroke("ctrl-shift-down")), Some((false, true)));
        for other in [
            "up",
            "shift-up",
            "ctrl-alt-up",
            "ctrl-left",
            "alt-up",
            "ctrl-a",
        ] {
            assert_eq!(pressed(&stroke(other)), None, "{other}");
        }
    }

    #[test]
    fn steps_start_at_the_newest_and_end_past_either_side() {
        let ids = [1, 2, 3];
        assert_eq!(next(&ids, None, true), Some(3));
        assert_eq!(next(&ids, None, false), Some(3));
        assert_eq!(next(&ids, Some(3), true), Some(2));
        assert_eq!(next(&ids, Some(1), true), None);
        assert_eq!(next(&ids, Some(2), false), Some(3));
        assert_eq!(next(&ids, Some(3), false), None);
        // A current message no longer listed starts again at the newest.
        assert_eq!(next(&ids, Some(9), true), Some(3));
        assert_eq!(next(&[], None, true), None);
    }

    #[test]
    fn should_mention_follows_its_three_choices() {
        assert_eq!(settings_of(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(
            settings_of(&state),
            Some(Settings {
                mention: true,
                skip_hidden: true
            })
        );
        state.set_value(ID, "shouldMention", &json!(0));
        state.set_value(ID, "ignoreBlockedAndIgnored", &json!(false));
        assert_eq!(
            settings_of(&state),
            Some(Settings {
                mention: false,
                skip_hidden: false
            })
        );
        state.set_value(ID, "shouldMention", &json!(1));
        assert!(settings_of(&state).unwrap().mention);
    }
}
