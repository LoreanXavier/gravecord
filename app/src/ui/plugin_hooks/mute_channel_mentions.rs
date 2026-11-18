//! MuteChannelMentions (Equicord `src/userplugins/muteChannelMentions`, GPL-3.0-or-later): the mention badges of the
//! channels you pick are hidden on the channel and on the server icon (`ReadStateStore.getMentionCount`); with
//! `onlyEveryone` only @everyone, @here and role pings go, and mentions of you by name since the channel was read
//! still show. Channel and thread menus get the "Hide mention badge" check. Nothing is sent to Discord. Not part of
//! the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row};
use crate::plugins::State;
use dc_state::Id;
use dc_state::read_state::HiddenMentions;
use serde_json::Value;
use std::collections::HashSet;

const ID: &str = "MuteChannelMentions";

/// `getHiddenIds`: the comma-separated ids, trimmed, empty ones dropped.
pub(super) fn hidden_ids(text: &str) -> HashSet<Id> {
    text.split(',')
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

/// `(channelIds, onlyEveryone)` while the plugin is on.
fn read(state: &State) -> Option<(HashSet<Id>, bool)> {
    state.is_on(ID).then(|| {
        (
            hidden_ids(state.value(ID, "channelIds").as_str().unwrap_or_default()),
            state.value(ID, "onlyEveryone") != Value::Bool(false),
        )
    })
}

fn settings() -> Option<(HashSet<Id>, bool)> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(read)
}

impl Root {
    /// Keeps the read states' filter in step with the settings; the direct mentions kept so far stay.
    pub(in crate::ui) fn mute_channel_mentions_sync(&mut self) {
        let wanted = settings();
        let hidden = &mut self.stores.read_states.hidden;
        match (wanted, hidden.as_mut()) {
            (None, None) => {}
            (None, Some(_)) => *hidden = None,
            (Some((channels, direct_only)), Some(current)) => {
                if current.channels != channels || current.direct_only != direct_only {
                    current
                        .direct
                        .retain(|channel, _| channels.contains(channel));
                    current.channels = channels;
                    current.direct_only = direct_only;
                }
            }
            (Some((channels, direct_only)), None) => {
                *hidden = Some(HiddenMentions {
                    channels,
                    direct_only,
                    direct: Default::default(),
                });
            }
        }
    }

    /// The "Hide mention badge" check at the end of the group holding Mark As Read, for a channel or thread menu.
    pub(in crate::ui) fn with_hide_mention_row(
        &self,
        mut items: Vec<Entry>,
        channel: Option<Id>,
    ) -> Vec<Entry> {
        let (Some(channel), Some((channels, _))) = (channel, settings()) else {
            return items;
        };
        let mut row = Row::new(Act::PluginHideMentionBadge, "Hide mention badge");
        row.check = Some(channels.contains(&channel));
        // `findGroupChildrenByChildId("mark-channel-read")`, else the end of the menu.
        let at = items
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::MarkRead))
            .map(|start| {
                items[start..]
                    .iter()
                    .position(|entry| matches!(entry, Entry::Separator))
                    .map_or(items.len(), |end| start + end)
            })
            .unwrap_or(items.len());
        items.insert(at, Entry::Row(row));
        items
    }

    /// The check: the channel joins or leaves `channelIds` (written back comma and space separated).
    pub(in crate::ui) fn toggle_hidden_mentions(&mut self, channel: Id) {
        let Some((channels, _)) = settings() else {
            return;
        };
        let mut ids: Vec<Id> = channels.into_iter().collect();
        ids.sort_unstable();
        if let Some(at) = ids.iter().position(|id| *id == channel) {
            ids.remove(at);
        } else {
            ids.push(channel);
        }
        let text = ids.iter().map(Id::to_string).collect::<Vec<_>>().join(", ");
        crate::plugins::update(|state| {
            state.set_value(ID, "channelIds", &Value::String(text));
        });
        self.mute_channel_mentions_sync();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_parse_as_the_original_splits_them() {
        assert_eq!(hidden_ids(""), HashSet::new());
        assert_eq!(hidden_ids("5, 6,,7 "), [5, 6, 7].into());
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some((HashSet::new(), true)));
        state.set_value(ID, "channelIds", &json!("9"));
        state.set_value(ID, "onlyEveryone", &json!(false));
        assert_eq!(read(&state), Some(([9].into(), false)));
    }
}
