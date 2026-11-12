//! `DELETED_ENTITY_IDS {guild_id, channels?, roles?, emojis?, stickers?}` (module 74362 `EntityVersionsManager`):
//! each list the server sends names the entities of that kind that still exist; what the client holds for the
//! guild and is not named has been deleted, so it goes the way the official client sends it: a `CHANNEL_DELETE`
//! and a `GUILD_ROLE_DELETE` for each, the emoji and sticker lists filtered. `LAST_MESSAGES {guild_id, messages}`
//! (`MESSAGE_PREVIEWS_LOADED`, modules 152685 and 666103) is the last message of channels the client did not open.

use crate::{Id, Message, Stores, id};
use serde_json::{Value, json};
use std::collections::HashSet;

fn ids(list: &Value) -> Option<HashSet<Id>> {
    Some(list.as_array()?.iter().filter_map(id).collect())
}

impl Stores {
    pub(crate) fn deleted_entities_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "DELETED_ENTITY_IDS" => self.deleted_entity_ids(data),
            "LAST_MESSAGES" => self.last_messages(data),
            "READY" | "LOGOUT" => self.previews.clear(),
            _ => {}
        }
    }

    fn deleted_entity_ids(&mut self, data: &Value) {
        let Some(guild_id) = id(&data["guild_id"]) else {
            return;
        };
        if let Some(keep) = ids(&data["channels"]) {
            let gone: Vec<Id> = self
                .guilds
                .iter()
                .filter(|guild| guild.id == guild_id)
                .flat_map(|guild| guild.channels.iter().map(|channel| channel.id))
                .filter(|channel| !keep.contains(channel))
                .collect();
            for channel in gone {
                self.dispatch(
                    "CHANNEL_DELETE",
                    &json!({"id": channel.to_string(), "guild_id": guild_id.to_string()}),
                );
            }
        }
        if let Some(keep) = ids(&data["roles"]) {
            let gone: Vec<Id> = self
                .guilds
                .iter()
                .filter(|guild| guild.id == guild_id)
                .flat_map(|guild| guild.roles.keys().copied())
                .filter(|role| !keep.contains(role))
                .collect();
            for role in gone {
                self.dispatch(
                    "GUILD_ROLE_DELETE",
                    &json!({"guild_id": guild_id.to_string(), "role_id": role.to_string()}),
                );
            }
        }
        if let Some(keep) = ids(&data["emojis"])
            && let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id)
        {
            guild.emojis.retain(|emoji| keep.contains(&emoji.id));
        }
        if let Some(keep) = ids(&data["stickers"]) {
            self.stickers.retain_guild_stickers(guild_id, &keep);
        }
    }

    /// `MESSAGE_PREVIEWS_LOADED`: the last message of each channel, for the lists that show one (kept apart from
    /// the loaded windows, which only a page load or a live message fills).
    fn last_messages(&mut self, data: &Value) {
        for value in data["messages"].as_array().into_iter().flatten() {
            if let Some(message) = Message::from_value(value) {
                self.previews.insert(message.channel_id, message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_the_server_does_not_list_are_deleted() {
        let mut stores = Stores::default();
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "name": "g", "channels": [
                {"id": "10", "type": 0, "name": "a"}, {"id": "11", "type": 0, "name": "b"}],
                "roles": [{"id": "20", "name": "r"}, {"id": "21", "name": "s"}],
                "emojis": [{"id": "30", "name": "e"}, {"id": "31", "name": "f"}]}),
        );
        stores.dispatch(
            "DELETED_ENTITY_IDS",
            &json!({"guild_id": "1", "channels": ["10"], "roles": ["21"], "emojis": ["31"]}),
        );
        let guild = stores.guilds.iter().find(|guild| guild.id == 1).unwrap();
        let channels: Vec<Id> = guild.channels.iter().map(|channel| channel.id).collect();
        assert_eq!(channels, vec![10]);
        assert_eq!(guild.roles.keys().copied().collect::<Vec<_>>(), vec![21]);
        assert_eq!(
            guild
                .emojis
                .iter()
                .map(|emoji| emoji.id)
                .collect::<Vec<_>>(),
            vec![31]
        );
        // A list the event leaves out is not touched.
        stores.dispatch("DELETED_ENTITY_IDS", &json!({"guild_id": "1", "roles": []}));
        let guild = stores.guilds.iter().find(|guild| guild.id == 1).unwrap();
        assert!(guild.roles.is_empty());
        assert_eq!(guild.channels.len(), 1);
    }

    #[test]
    fn last_messages_become_previews() {
        let mut stores = Stores::default();
        stores.dispatch(
            "LAST_MESSAGES",
            &json!({"guild_id": "1", "messages": [{
                "id": "5", "channel_id": "10", "content": "hi", "type": 0,
                "timestamp": "2026-11-12T01:00:00.000000+00:00",
                "author": {"id": "7", "username": "a", "discriminator": "0"}}]}),
        );
        assert_eq!(stores.previews[&10].content, "hi");
    }
}
