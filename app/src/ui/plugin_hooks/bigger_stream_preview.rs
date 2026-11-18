//! BiggerStreamPreview (Vencord `src/plugins/biggerStreamPreview`, GPL-3.0-or-later): the user menu of someone who
//! is streaming ends with a separator and "View Stream Preview" (with a screen icon); it opens the stream's preview
//! picture (`ApplicationStreamPreviewStore.getPreviewURL`) at 1280 by 720 in the picture viewer. A preview not
//! fetched yet is asked for (`GET /streams/<key>/preview`) and opens when it arrives.
//!
//! GAP: the same row on a stream tile's own menu (`stream-context`). Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use dc_state::Id;
use gpui::Context;
use std::cell::RefCell;

const ID: &str = "BiggerStreamPreview";

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

thread_local! {
    /// The stream whose preview was asked for from the menu and is still on its way.
    static WAITING: RefCell<Option<String>> = const { RefCell::new(None) };
}

impl Root {
    /// `getAnyStreamForUser`: the (guild, channel) where `user` is streaming (0 for a call).
    fn stream_of(&self, user: Id) -> Option<(Id, Id)> {
        self.stores.voice_states.iter().find_map(|(guild, states)| {
            states
                .iter()
                .find(|state| state.user_id == user && state.self_stream)
                .map(|state| (*guild, state.channel_id))
        })
    }

    /// The user menu with the plugin's row at its end; as it is with the plugin off or nobody streaming.
    pub(in crate::ui) fn with_stream_preview_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        let Target::User { user, .. } = target else {
            return items;
        };
        if !on() || self.stream_of(user).is_none() {
            return items;
        }
        items.push(Entry::Separator);
        items.push(Entry::Row(
            Row::new(Act::PluginViewStreamPreview, "View Stream Preview").icon("golive_tab_screen"),
        ));
        items
    }

    /// The row ran: the preview in the viewer now, or once it is fetched.
    pub(in crate::ui) fn stream_preview_view(&mut self, user: Id, cx: &mut Context<Self>) {
        let Some((guild, channel)) = self.stream_of(user) else {
            return;
        };
        let key = super::super::stream_previews::key_of(guild, channel, user);
        let name = key.to_string();
        if let Some(url) = self.stream_previews.url(&name).map(str::to_owned) {
            self.open_plain_picture(url.clone(), url, (1280, 720), cx);
            return;
        }
        WAITING.with(|waiting| *waiting.borrow_mut() = Some(name));
        self.stream_preview_want(key);
    }

    /// A preview arrived: the one the menu asked for opens.
    pub(in crate::ui) fn stream_preview_arrived(&mut self, key: &str, cx: &mut Context<Self>) {
        let waited = WAITING.with(|waiting| {
            let mut waiting = waiting.borrow_mut();
            (waiting.as_deref() == Some(key)).then(|| waiting.take())
        });
        if waited.is_none() || !on() {
            return;
        }
        if let Some(url) = self.stream_previews.url(key).map(str::to_owned) {
            self.open_plain_picture(url.clone(), url, (1280, 720), cx);
        }
    }
}
