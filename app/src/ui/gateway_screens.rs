//! What the screens do about gateway events the stores keep (`dc-state`: `notes`, `guild_bans`, `guild_integrations`).
//!
//! - `USER_NOTE_UPDATE` (NoteStore): the note field of an open full profile takes the new text.
//! - `GUILD_BAN_ADD` and `GUILD_BAN_REMOVE` (GuildSettingsStore): the Bans page reads the store's list, so a redraw
//!   is all it needs.
//! - `WEBHOOKS_UPDATE` (WebhooksStore) asks for the channel's webhooks again (`GET /channels/{id}/webhooks`, module
//!   824953 `fetchForChannel`); `GUILD_INTEGRATIONS_UPDATE` (GuildSettingsStore `eG`) reads the open server's
//!   integrations page again; `GUILD_APPLIED_BOOSTS_UPDATE` (GuildPowerupsManager) reads the server again for the
//!   Boost Perks page.
//! - `VOICE_STATE_UPDATE_BATCH {voice_states}` is the same as one `VOICE_STATE_UPDATE` for each state (module 225123
//!   `eH`), so every voice hook of a single update runs for each.

use super::Root;
use super::server_settings::Page;
use crate::net::{Request, Update};
use dc_state::Id;
use dc_state::guild_integrations::Refetch;
use gpui::Context;
use serde_json::Value;

/// The key of one channel's webhooks answer, `guild-webhooks:channel:{guild}:{channel}`.
pub(super) const WEBHOOKS_CHANNEL_PREFIX: &str = "guild-webhooks:channel:";

impl Root {
    /// A batch becomes its single updates; true when the event was one.
    pub(super) fn expand_voice_batch(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if name != "VOICE_STATE_UPDATE_BATCH" {
            return false;
        }
        for state in data["voice_states"].as_array().into_iter().flatten() {
            self.update(
                Update::Dispatch {
                    name: "VOICE_STATE_UPDATE".to_owned(),
                    data: state.clone(),
                },
                cx,
            );
        }
        true
    }

    pub(super) fn gateway_screens_dispatch(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) {
        match name {
            "READY" | "USER_CONNECTIONS_CALLBACK" => self.connection_gateway(name, data),
            "USER_CONNECTIONS_LINK_CALLBACK" => self.connection_link_callback(data, cx),
            "USER_NOTE_UPDATE" => self.note_updated(data, cx),
            // ConnectedAccountsStore (module 417869): a gateway update (not a local one carrying `accounts`) reads
            // the connections again.
            "USER_CONNECTIONS_UPDATE" if data["accounts"].is_null() => {
                // Module 417869 reads them again whatever is open: a link finished from a profile card's Connect
                // button must close its dialog and drop the offer. Only the connections are read: an open settings
                // panel and tab stay where they are.
                self.settings_extra.connections_asked = false;
                self.settings_request_connections();
                cx.notify();
            }
            "GUILD_BAN_ADD" | "GUILD_BAN_REMOVE" => cx.notify(),
            "AUTO_MODERATION_CONTENT_DELETED" => self.automod_content_deleted(data, cx),
            "DM_SETTINGS_UPSELL_SHOW" => self.dm_settings_upsell(data, cx),
            "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK" => {
                self.pins_message_event(name, data, cx)
            }
            _ => {}
        }
        // The emoji pictures of the open channel's messages were asked for only when a page loaded; a live message
        // or reaction (`MESSAGE_REACTION_ADD`, `_ADD_MANY`) with a unicode emoji had no glyph until a reload.
        if matches!(
            name,
            "MESSAGE_CREATE"
                | "MESSAGE_UPDATE"
                | "MESSAGE_REACTION_ADD"
                | "MESSAGE_REACTION_ADD_MANY"
        ) && data["channel_id"].as_str().and_then(|id| id.parse().ok()) == self.channel
        {
            self.request_images();
        }
        for refetch in self.stores.integrations.take_refetches() {
            match refetch {
                Refetch::WebhooksForChannel { guild, channel } => {
                    let _ = self.requests.send(Request::Fetch {
                        key: format!("{WEBHOOKS_CHANNEL_PREFIX}{guild}:{channel}"),
                        route: format!("/channels/{channel}/webhooks"),
                    });
                }
                Refetch::GuildIntegrations(guild) => {
                    if self.settings_page_open(guild, Page::Integrations) {
                        self.request_server_page(guild, Page::Integrations, cx);
                    }
                }
                Refetch::GuildPowerups(guild) => {
                    if self.settings_page_open(guild, Page::BoostPerks) {
                        self.request_boost(guild);
                    }
                }
            }
        }
    }

    /// `ChannelPinsStore` (module 966833): the loaded pins of a channel follow `MESSAGE_UPDATE` (a pinned message with
    /// an author joins at the top or is updated, an unpinned one leaves, one without an author is merged into the
    /// pin it matches), `MESSAGE_DELETE` and `MESSAGE_DELETE_BULK`.
    fn pins_message_event(&mut self, name: &str, data: &Value, cx: &mut Context<Self>) {
        let Some(channel) = data["channel_id"].as_str() else {
            return;
        };
        let Some(items) = self
            .fetched
            .get_mut(&format!("pins:{channel}"))
            .and_then(|body| body["items"].as_array_mut())
        else {
            return;
        };
        let position = |items: &[Value], id: &str| {
            items
                .iter()
                .position(|item| item["message"]["id"].as_str() == Some(id))
        };
        match name {
            "MESSAGE_DELETE" => {
                if let Some(id) = data["id"].as_str() {
                    items.retain(|item| item["message"]["id"].as_str() != Some(id));
                }
            }
            "MESSAGE_DELETE_BULK" => {
                let gone: Vec<&str> = data["ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                items.retain(|item| {
                    !item["message"]["id"]
                        .as_str()
                        .is_some_and(|id| gone.contains(&id))
                });
            }
            "MESSAGE_UPDATE" => {
                let Some(id) = data["id"].as_str() else {
                    return;
                };
                let at = position(items, id);
                if data["author"].is_null() {
                    if let (Some(at), Some(fields)) = (at, data.as_object()) {
                        for (key, value) in fields {
                            items[at]["message"][key] = value.clone();
                        }
                    }
                } else if data["pinned"] == true {
                    match at {
                        Some(at) => items[at]["message"] = data.clone(),
                        None => items.insert(
                            0,
                            serde_json::json!({
                                "pinned_at": chrono::Utc::now().to_rfc3339(),
                                "message": data,
                            }),
                        ),
                    }
                } else if let Some(at) = at {
                    items.remove(at);
                }
            }
            _ => return,
        }
        cx.notify();
    }

    fn settings_page_open(&self, guild: Id, page: Page) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.guild == guild && open.page == page)
    }

    /// The full profile's note field follows the store when another session (or the answer of a read) changed it.
    fn note_updated(&mut self, data: &Value, cx: &mut Context<Self>) {
        if let Some(user) = data["id"].as_str().and_then(|user| user.parse::<Id>().ok()) {
            self.full_profile_note_changed(user, cx);
        }
        cx.notify();
    }
}
