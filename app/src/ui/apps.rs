//! The message menu's Apps submenu: message commands (type 3) from the application command index
//! (`APPLICATION_COMMAND_INDEX_GUILD`/`_CHANNEL`), "No Commands Available" (`YSNlV2`) when there are none, and a
//! chosen command sent as an interaction (`POST /interactions`, type 2) aimed at the message.

use super::Root;
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, SharedString, img, prelude::*};
use serde_json::{Value, json};

impl Root {
    fn command_index_key(&self) -> Option<String> {
        match (self.guild, self.channel) {
            (Some(guild), _) => Some(format!("command-index:guild:{guild}")),
            (None, Some(channel)) => Some(format!("command-index:channel:{channel}")),
            _ => None,
        }
    }

    pub(super) fn request_command_index(&mut self) {
        let Some(key) = self.command_index_key() else {
            return;
        };
        if !self.requested.insert(key.clone()) {
            return;
        }
        let route = match (self.guild, self.channel) {
            (Some(guild), _) => format!("/guilds/{guild}/application-command-index"),
            (None, Some(channel)) => format!("/channels/{channel}/application-command-index"),
            _ => return,
        };
        let _ = self.requests.send(Request::Fetch { key, route });
    }

    /// Message commands (`type: 3`) in the loaded index, with their application.
    fn message_commands(&self) -> Vec<(Value, Option<Value>)> {
        let Some(index) = self
            .command_index_key()
            .and_then(|key| self.fetched.get(&key))
        else {
            return Vec::new();
        };
        let applications = index["applications"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        index["application_commands"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|command| command["type"].as_u64() == Some(3))
            .map(|command| {
                let application = applications
                    .iter()
                    .find(|application| application["id"] == command["application_id"])
                    .cloned();
                (command.clone(), application)
            })
            .collect()
    }

    pub(super) fn app_rows(
        &self,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let commands = self.message_commands();
        if commands.is_empty() {
            return vec![(
                self.submenu_item(
                    "apps-none".into(),
                    None,
                    "No Commands Available".to_owned(),
                    34.,
                )
                .into_any_element(),
                34.,
            )];
        }
        commands
            .into_iter()
            .enumerate()
            .map(|(index, (command, application))| {
                let icon = application.as_ref().and_then(|application| {
                    let id = application["id"].as_str()?;
                    let hash = application["icon"].as_str()?;
                    self.images.get(&format!("app-icon:{id}:{hash}")).cloned()
                });
                let leading = icon.map(|picture| {
                    img(picture)
                        .size(gpui::px(20.))
                        .rounded_full()
                        .into_any_element()
                });
                let name = command["name"].as_str().unwrap_or_default().to_owned();
                let guild = self.guild;
                (
                    self.submenu_item(
                        SharedString::from(format!("apps-command-{index}")),
                        leading,
                        name,
                        34.,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.submenu = None;
                        let session = this.stores.session_id.clone().unwrap_or_default();
                        let body = json!({
                            "type": 2,
                            "application_id": command["application_id"],
                            "guild_id": guild.map(|guild| guild.to_string()),
                            "channel_id": channel.to_string(),
                            "session_id": session,
                            "data": {
                                "version": command["version"],
                                "id": command["id"],
                                "name": command["name"],
                                "type": 3,
                                "options": [],
                                "application_command": command,
                                "attachments": [],
                                "target_id": message.to_string(),
                            },
                            "nonce": crate::net::nonce(),
                            "analytics_location": "message_context_menu",
                        });
                        let _ = this.requests.send(Request::Interaction { body });
                        cx.notify();
                    }))
                    .into_any_element(),
                    34.,
                )
            })
            .collect()
    }
}
