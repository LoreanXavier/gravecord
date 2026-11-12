//! Integration, webhook and boost events: the state the official client keeps for them, and the reads they ask for.
//!
//! - `INTEGRATION_CREATE {application, guild_id}`, `INTEGRATION_UPDATE` and `INTEGRATION_DELETE {application_id,
//!   guild_id}` (module 225123): `MyGuildApplicationsStore` (module 680646) adds or removes the server under the
//!   application; `ApplicationStore` (module 587895 `g`) forgets the server's cached embedded applications.
//! - `GUILD_INTEGRATIONS_UPDATE {guild_id}`: the same cache is forgotten, and `GuildSettingsStore` (module 555337
//!   `eG`) reads the integrations of the open server again.
//! - `WEBHOOKS_UPDATE {guild_id, channel_id}`: `WebhooksStore` (module 718116) reads that channel's webhooks again
//!   when the server's list is known (`fetchForChannel`, module 824953). The REST answers come back through
//!   `webhooks_loaded` as the action with a `webhooks` list.
//! - `GUILD_APPLIED_BOOSTS_UPDATE {guild_id}`: `GuildPowerupsManager` (module 681637) reads the server's power-ups
//!   and boosts again.
//!
//! A read the official client starts is a `Refetch` here; the screen that owns the request drains them with
//! `take_refetches`.

use crate::{Id, Stores, id, text};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};

/// A read a gateway event asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Refetch {
    /// `GET /guilds/{guild}/integrations` for the server in settings.
    GuildIntegrations(Id),
    /// `GET /channels/{channel}/webhooks`.
    WebhooksForChannel { guild: Id, channel: Id },
    /// The server's power-ups and applied boosts.
    GuildPowerups(Id),
}

#[derive(Default)]
pub struct GuildIntegrations {
    /// `MyGuildApplicationsStore.applicationIdToGuildIds`.
    application_guilds: HashMap<String, BTreeSet<Id>>,
    /// `ApplicationStore`'s per-server embedded applications (`d`): the servers they are cached for.
    embedded_cached: HashSet<Id>,
    /// `WebhooksStore`'s `E`: guild to its webhooks, in the order they arrived.
    webhooks: HashMap<Id, Vec<Value>>,
    webhook_error: Option<Value>,
    refetches: Vec<Refetch>,
    /// Counts every change that a screen could show.
    pub version: u64,
}

impl GuildIntegrations {
    /// `MyGuildApplicationsStore.getGuildIdsForApplication`.
    pub fn guilds_for_application(&self, application: &str) -> Option<&BTreeSet<Id>> {
        self.application_guilds.get(application)
    }

    /// `MyGuildApplicationsStore` after `FETCH_INTEGRATION_APPLICATION_IDS_FOR_MY_GUILDS_SUCCESS`.
    pub fn set_my_guild_applications(&mut self, by_guild: &[(Id, Vec<String>)]) {
        self.application_guilds.clear();
        for (guild, applications) in by_guild {
            for application in applications {
                self.application_guilds
                    .entry(application.clone())
                    .or_default()
                    .insert(*guild);
            }
        }
        self.version += 1;
    }

    /// `GUILD_EMBEDDED_APPLICATIONS_FETCH_SUCCESS`.
    pub fn embedded_applications_fetched(&mut self, guild: Id) {
        self.embedded_cached.insert(guild);
    }

    pub fn has_embedded_applications(&self, guild: Id) -> bool {
        self.embedded_cached.contains(&guild)
    }

    /// `WebhooksStore.getWebhooksForGuild`.
    pub fn webhooks_for_guild(&self, guild: Id) -> &[Value] {
        self.webhooks.get(&guild).map_or(&[], Vec::as_slice)
    }

    /// `WebhooksStore.getWebhooksForChannel`.
    pub fn webhooks_for_channel(&self, guild: Id, channel: Id) -> Vec<&Value> {
        self.webhooks_for_guild(guild)
            .iter()
            .filter(|hook| id(&hook["channel_id"]) == Some(channel))
            .collect()
    }

    /// Whether the server's webhooks were ever read (`E[guild]` exists).
    pub fn webhooks_known(&self, guild: Id) -> bool {
        self.webhooks.contains_key(&guild)
    }

    /// `WebhooksStore.error`.
    pub fn webhook_error(&self) -> Option<&Value> {
        self.webhook_error.as_ref()
    }

    /// `WEBHOOKS_UPDATE` with its `webhooks` list (the answer of a read): without a channel the list is the whole
    /// server's; with one it replaces that channel's webhooks and keeps the others.
    pub fn webhooks_loaded(&mut self, guild: Id, channel: Option<Id>, list: &[Value]) {
        self.webhook_error = None;
        let mut next: Vec<Value> = match channel {
            Some(channel) => self
                .webhooks
                .get(&guild)
                .into_iter()
                .flatten()
                .filter(|hook| id(&hook["channel_id"]) != Some(channel))
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        next.extend(list.iter().cloned());
        self.webhooks.insert(guild, next);
        self.version += 1;
    }

    /// `WEBHOOKS_UPDATE` with an `error`.
    pub fn webhooks_failed(&mut self, error: Value) {
        self.webhook_error = Some(error);
        self.version += 1;
    }

    /// `WEBHOOK_CREATE` and `WEBHOOK_UPDATE`.
    pub fn webhook_put(&mut self, guild: Id, webhook: &Value) {
        let list = self.webhooks.entry(guild).or_default();
        match list.iter_mut().find(|known| known["id"] == webhook["id"]) {
            Some(known) => *known = webhook.clone(),
            None => list.push(webhook.clone()),
        }
        self.version += 1;
    }

    /// `WEBHOOK_DELETE`.
    pub fn webhook_delete(&mut self, guild: Id, webhook: &str) {
        if let Some(list) = self.webhooks.get_mut(&guild) {
            list.retain(|known| known["id"].as_str() != Some(webhook));
        }
        self.version += 1;
    }

    /// The reads the events asked for since the last call.
    pub fn take_refetches(&mut self) -> Vec<Refetch> {
        std::mem::take(&mut self.refetches)
    }

    fn integration_event(&mut self, name: &str, data: &Value) {
        let Some(guild) = id(&data["guild_id"]) else {
            return;
        };
        match name {
            "INTEGRATION_CREATE" => {
                if let Some(application) = text(&data["application"]["id"]) {
                    self.application_guilds
                        .entry(application)
                        .or_default()
                        .insert(guild);
                }
            }
            "INTEGRATION_DELETE" => {
                if let Some(application) = text(&data["application_id"])
                    && let Some(guilds) = self.application_guilds.get_mut(&application)
                {
                    guilds.remove(&guild);
                }
            }
            _ => {}
        }
        self.embedded_cached.remove(&guild);
        if name == "GUILD_INTEGRATIONS_UPDATE" {
            self.refetches.push(Refetch::GuildIntegrations(guild));
        }
        self.version += 1;
    }
}

impl Stores {
    pub(crate) fn guild_integrations_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.integrations;
        match name {
            "INTEGRATION_CREATE"
            | "INTEGRATION_UPDATE"
            | "INTEGRATION_DELETE"
            | "GUILD_INTEGRATIONS_UPDATE" => store.integration_event(name, data),
            "WEBHOOKS_UPDATE" => {
                let (Some(guild), channel) = (id(&data["guild_id"]), id(&data["channel_id"]))
                else {
                    return;
                };
                match data["webhooks"].as_array() {
                    Some(list) => store.webhooks_loaded(guild, channel, list),
                    None if !data["error"].is_null() => {
                        store.webhooks_failed(data["error"].clone())
                    }
                    None => {
                        if let Some(channel) = channel
                            && store.webhooks.contains_key(&guild)
                        {
                            store.webhook_error = None;
                            store
                                .refetches
                                .push(Refetch::WebhooksForChannel { guild, channel });
                        }
                    }
                }
            }
            "GUILD_APPLIED_BOOSTS_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    store.refetches.push(Refetch::GuildPowerups(guild));
                }
            }
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    store.embedded_cached.remove(&guild);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook(id: &str, channel: &str) -> Value {
        json!({"id": id, "type": 1, "guild_id": "5", "channel_id": channel, "name": "hook"})
    }

    #[test]
    fn integration_events_track_the_servers_of_an_application_and_drop_the_cached_list() {
        let mut stores = Stores::default();
        stores.integrations.embedded_applications_fetched(5);
        stores.dispatch(
            "INTEGRATION_CREATE",
            &json!({"guild_id": "5", "id": "900", "type": "discord",
                    "application": {"id": "800", "name": "Bot", "bot": {"id": "801"}}}),
        );
        assert!(!stores.integrations.has_embedded_applications(5));
        let guilds = stores.integrations.guilds_for_application("800").unwrap();
        assert!(guilds.contains(&5));
        stores.dispatch(
            "INTEGRATION_CREATE",
            &json!({"guild_id": "6", "application": {"id": "800"}}),
        );
        stores.dispatch(
            "INTEGRATION_UPDATE",
            &json!({"guild_id": "6", "application": {"id": "800"}}),
        );
        stores.dispatch(
            "INTEGRATION_DELETE",
            &json!({"guild_id": "5", "id": "900", "application_id": "800"}),
        );
        let guilds: Vec<Id> = stores
            .integrations
            .guilds_for_application("800")
            .unwrap()
            .iter()
            .copied()
            .collect();
        assert_eq!(guilds, vec![6]);
        // An integration with no application (a Twitch link) adds nothing.
        stores.dispatch(
            "INTEGRATION_CREATE",
            &json!({"guild_id": "7", "type": "twitch"}),
        );
        assert!(stores.integrations.take_refetches().is_empty());
    }

    #[test]
    fn guild_integrations_update_asks_for_the_integrations_again() {
        let mut stores = Stores::default();
        stores.integrations.embedded_applications_fetched(5);
        stores.dispatch("GUILD_INTEGRATIONS_UPDATE", &json!({"guild_id": "5"}));
        assert!(!stores.integrations.has_embedded_applications(5));
        assert_eq!(
            stores.integrations.take_refetches(),
            vec![Refetch::GuildIntegrations(5)]
        );
        assert!(stores.integrations.take_refetches().is_empty());
    }

    #[test]
    fn webhooks_update_reads_one_channel_again_only_when_the_server_list_is_known() {
        let mut stores = Stores::default();
        stores.dispatch(
            "WEBHOOKS_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50"}),
        );
        assert!(stores.integrations.take_refetches().is_empty());
        stores
            .integrations
            .webhooks_loaded(5, None, &[hook("1", "50"), hook("2", "51")]);
        stores.dispatch(
            "WEBHOOKS_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50"}),
        );
        assert_eq!(
            stores.integrations.take_refetches(),
            vec![Refetch::WebhooksForChannel {
                guild: 5,
                channel: 50
            }]
        );
        // The answer replaces that channel's webhooks and keeps the other channel's.
        stores
            .integrations
            .webhooks_loaded(5, Some(50), &[hook("3", "50")]);
        let names: Vec<&str> = stores
            .integrations
            .webhooks_for_guild(5)
            .iter()
            .filter_map(|hook| hook["id"].as_str())
            .collect();
        assert_eq!(names, vec!["2", "3"]);
        assert_eq!(stores.integrations.webhooks_for_channel(5, 50).len(), 1);
        stores.integrations.webhook_delete(5, "2");
        stores.integrations.webhook_put(5, &hook("3", "52"));
        let channels: Vec<Option<Id>> = stores
            .integrations
            .webhooks_for_guild(5)
            .iter()
            .map(|hook| id(&hook["channel_id"]))
            .collect();
        assert_eq!(channels, vec![Some(52)]);
    }

    #[test]
    fn a_webhook_error_is_kept_until_a_list_arrives() {
        let mut stores = Stores::default();
        stores.dispatch(
            "WEBHOOKS_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50", "error": {"code": 50013}}),
        );
        assert_eq!(stores.integrations.webhook_error().unwrap()["code"], 50013);
        stores.dispatch(
            "WEBHOOKS_UPDATE",
            &json!({"guild_id": "5", "webhooks": [hook("1", "50")]}),
        );
        assert!(stores.integrations.webhook_error().is_none());
        assert_eq!(stores.integrations.webhooks_for_guild(5).len(), 1);
    }

    #[test]
    fn applied_boosts_update_asks_for_the_power_ups_again() {
        let mut stores = Stores::default();
        stores.dispatch("GUILD_APPLIED_BOOSTS_UPDATE", &json!({"guild_id": "5"}));
        assert_eq!(
            stores.integrations.take_refetches(),
            vec![Refetch::GuildPowerups(5)]
        );
    }
}
