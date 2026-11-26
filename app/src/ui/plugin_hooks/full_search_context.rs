//! FullSearchContext (Vencord `src/plugins/fullSearchContext`, GPL-3.0-or-later): a right press on a message search
//! result opens the chat's full message menu (`useMessageMenu`, module 761304) for that message instead of the
//! search's short one (module 847342), with Copy Author ID after Copy Message ID in developer mode
//! (`devmode-copy-id`, `COPY_ID_AUTHOR`). The menu acts on the result's own message and channel; Reply and Edit, which
//! write in that channel's composer and list, first open the channel at the message. Not part of the pinned client.

use super::super::Root;
use super::super::menu::{Action, Item};
use dc_state::Message;
use gpui::Context;

const ID: &str = "FullSearchContext";

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// Whether a search result's right press opens the full menu.
pub(in crate::ui) fn full_search_context() -> bool {
    on()
}

/// The full menu's items for a search result: Copy Author ID joins Copy Message ID's group.
pub(in crate::ui) fn full_search_context_items(items: &mut Vec<Item>) {
    if on() {
        add_author_id(items);
    }
}

fn add_author_id(items: &mut Vec<Item>) {
    if let Some(at) = items
        .iter()
        .position(|item| matches!(item, Item::Entry(_, _, Action::CopyId, _)))
    {
        items.insert(
            at + 1,
            Item::Entry("Copy Author ID", "menu_id", Action::CopyAuthorId, false),
        );
    }
}

impl Root {
    /// Reply or Edit on a result outside the open channel: the channel opens at the message first, so the reply bar
    /// and the edit box are that channel's.
    pub(in crate::ui) fn full_search_context_open(
        &mut self,
        message: &Message,
        cx: &mut Context<Self>,
    ) {
        if !on() || self.channel == Some(message.channel_id) {
            return;
        }
        if let Some(guild) = self.guild {
            self.select(guild, Some(message.channel_id));
        }
        self.jump_to_message(message, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_author_id_follows_copy_message_id() {
        let mut items = vec![
            Item::Entry("Delete Message", "", Action::Delete, false),
            Item::Separator,
            Item::Entry("Copy Message ID", "menu_id", Action::CopyId, false),
        ];
        add_author_id(&mut items);
        assert!(matches!(
            items[3],
            Item::Entry("Copy Author ID", _, Action::CopyAuthorId, _)
        ));
        // Without developer mode there is no Copy Message ID, and no author row either.
        let mut plain = vec![Item::Entry("Delete Message", "", Action::Delete, false)];
        add_author_id(&mut plain);
        assert_eq!(plain.len(), 1);
    }
}
