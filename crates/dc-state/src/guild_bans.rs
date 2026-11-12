//! The ban list the server settings Bans page shows (`GuildSettingsStore`, module 555337: `eo`, `es`, `ea`).
//!
//! `GUILD_SETTINGS_LOADED_BANS` replaces the list; `GUILD_SETTINGS_LOADED_BANS_BATCH` (a search answer) adds to it,
//! starting a new list when it is for another server. `GUILD_BAN_ADD {guild_id, user}` adds `{user, reason: null}`
//! and `GUILD_BAN_REMOVE` removes the user, both only while a list of that server is loaded. `UserStore` caches the
//! banned user whatever the list holds (module 287809 `ed`).

use crate::{Id, Stores, User, id};
use serde_json::{Value, json};

/// One row: the ban object REST returns, `{user, reason}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ban {
    pub user: Id,
    pub entry: Value,
}

#[derive(Default)]
pub struct GuildBans {
    guild: Option<Id>,
    /// `None` until the page loaded a list (`eo`).
    bans: Option<Vec<Ban>>,
    /// Counts every change (`ea`).
    pub version: u64,
}

impl GuildBans {
    /// The server whose list is loaded.
    pub fn guild(&self) -> Option<Id> {
        self.guild
    }

    pub fn is_loaded(&self, guild: Id) -> bool {
        self.guild == Some(guild) && self.bans.is_some()
    }

    /// The bans of a server in the order they were added, as REST rows (`{user, reason}`).
    pub fn rows(&self, guild: Id) -> Vec<Value> {
        if self.guild != Some(guild) {
            return Vec::new();
        }
        self.bans
            .iter()
            .flatten()
            .map(|ban| ban.entry.clone())
            .collect()
    }

    fn set(&mut self, user: Id, entry: Value) {
        let list = self.bans.get_or_insert_with(Vec::new);
        match list.iter_mut().find(|ban| ban.user == user) {
            // A `Map` keeps the place of a key that is set again.
            Some(known) => known.entry = entry,
            None => list.push(Ban { user, entry }),
        }
        self.version += 1;
    }

    /// `GUILD_SETTINGS_LOADED_BANS`: the whole list of a server.
    pub fn loaded(&mut self, guild: Id, bans: &[Value]) {
        self.guild = Some(guild);
        self.bans = Some(Vec::new());
        for ban in bans {
            if let Some(user) = id(&ban["user"]["id"]) {
                self.set(user, ban.clone());
            }
        }
        self.version += 1;
    }

    /// `GUILD_SETTINGS_LOADED_BANS_BATCH`: more bans of a server (a search answer).
    pub fn loaded_batch(&mut self, guild: Id, bans: &[Value]) {
        if self.guild != Some(guild) || self.bans.is_none() {
            self.guild = Some(guild);
            self.bans = Some(Vec::new());
        }
        for ban in bans {
            if let Some(user) = id(&ban["user"]["id"]) {
                self.set(user, ban.clone());
            }
        }
        self.version += 1;
    }

    /// The Revoke Ban button drops the row at once; the gateway's `GUILD_BAN_REMOVE` then finds nothing to remove.
    pub fn revoked(&mut self, guild: Id, user: Id) {
        self.remove(guild, user);
    }

    fn remove(&mut self, guild: Id, user: Id) {
        if self.guild != Some(guild) {
            return;
        }
        if let Some(list) = self.bans.as_mut() {
            list.retain(|ban| ban.user != user);
            self.version += 1;
        }
    }
}

impl Stores {
    pub(crate) fn guild_bans_dispatch(&mut self, name: &str, data: &Value) {
        if name != "GUILD_BAN_ADD" && name != "GUILD_BAN_REMOVE" {
            return;
        }
        let Some(user) = User::from_value(&data["user"]) else {
            return;
        };
        let Some(guild) = id(&data["guild_id"]) else {
            return;
        };
        let user_id = user.id;
        self.users.insert(user_id, user);
        let bans = &mut self.guild_bans;
        if bans.bans.is_none() || bans.guild != Some(guild) {
            return;
        }
        if name == "GUILD_BAN_ADD" {
            bans.set(user_id, json!({"user": data["user"], "reason": null}));
        } else {
            bans.remove(guild, user_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ban(user: u64, name: &str, reason: Option<&str>) -> Value {
        json!({"reason": reason, "user": {"id": user.to_string(), "username": name, "discriminator": "0"}})
    }

    #[test]
    fn a_loaded_list_follows_ban_and_unban_events_of_its_server_only() {
        let mut stores = Stores::default();
        // Nothing loaded: the events only cache the user.
        stores.dispatch(
            "GUILD_BAN_ADD",
            &json!({"guild_id": "5", "user": {"id": "60", "username": "early", "discriminator": "0"}}),
        );
        assert!(!stores.guild_bans.is_loaded(5));
        assert_eq!(stores.users[&60].username, "early");

        stores.guild_bans.loaded(5, &[ban(61, "ann", Some("spam"))]);
        assert!(stores.guild_bans.is_loaded(5));
        stores.dispatch(
            "GUILD_BAN_ADD",
            &json!({"guild_id": "5", "user": {"id": "62", "username": "bob", "discriminator": "0"}}),
        );
        let rows = stores.guild_bans.rows(5);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["user"]["username"], "bob");
        assert!(rows[1]["reason"].is_null());
        // Another server's ban leaves this list alone.
        stores.dispatch(
            "GUILD_BAN_ADD",
            &json!({"guild_id": "6", "user": {"id": "63", "username": "cy", "discriminator": "0"}}),
        );
        assert_eq!(stores.guild_bans.rows(5).len(), 2);
        stores.dispatch(
            "GUILD_BAN_REMOVE",
            &json!({"guild_id": "5", "user": {"id": "61", "username": "ann", "discriminator": "0"}}),
        );
        let rows = stores.guild_bans.rows(5);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["user"]["id"], "62");
    }

    #[test]
    fn a_search_batch_joins_the_list_and_a_batch_for_another_server_starts_a_new_one() {
        let mut bans = GuildBans::default();
        bans.loaded(5, &[ban(61, "ann", None)]);
        bans.loaded_batch(5, &[ban(61, "ann", Some("again")), ban(64, "dee", None)]);
        let rows = bans.rows(5);
        assert_eq!(rows.len(), 2);
        // The key keeps its place and takes the new row.
        assert_eq!(rows[0]["reason"], "again");
        bans.loaded_batch(7, &[ban(70, "eve", None)]);
        assert!(bans.rows(5).is_empty());
        assert_eq!(bans.rows(7).len(), 1);
        bans.revoked(7, 70);
        assert!(bans.rows(7).is_empty());
    }
}
