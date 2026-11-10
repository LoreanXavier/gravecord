//! Sidebar action visibility (616740, 770666, 508654, 931991 and 363487).
use super::Root;
use crate::net::Request;
use dc_state::{
    ChannelKind, Guild,
    permissions::{self, Me},
};
use serde_json::Value;

const EVENTS: &str = "guild-nav:events:";
const CREATE_EVENTS: u64 = 1 << 44;

pub(super) fn can_create_events(guild: &Guild, me: Me) -> bool {
    permissions::can_in_guild(guild, me, permissions::ADMINISTRATOR)
        || permissions::can_in_guild(guild, me, CREATE_EVENTS)
        || guild.channels.iter().any(|channel| {
            let required = match channel.kind {
                ChannelKind::Voice => {
                    CREATE_EVENTS | permissions::VIEW_CHANNEL | permissions::CONNECT
                }
                ChannelKind::Other(13) => {
                    CREATE_EVENTS
                        | permissions::VIEW_CHANNEL
                        | permissions::MANAGE_CHANNELS
                        | (1 << 22)
                        | permissions::MOVE_MEMBERS
                }
                _ => return false,
            };
            permissions::can_in_channel(guild, channel, me, required)
        })
}

fn visible_events(guild: &Guild, me: Me, events: &Value) -> bool {
    if guild.features.iter().any(|feature| feature == "HUB") {
        return false;
    }
    can_create_events(guild, me)
        || events.as_array().into_iter().flatten().any(|event| {
            if matches!(event["status"].as_u64(), Some(3 | 4)) {
                return false;
            }
            let Some(channel) = event["channel_id"].as_str() else {
                return true;
            };
            channel
                .parse::<u64>()
                .ok()
                .and_then(|id| guild.channels.iter().find(|channel| channel.id == id))
                .is_some_and(|channel| {
                    permissions::can_in_channel(guild, channel, me, permissions::VIEW_CHANNEL)
                })
        })
}

impl Root {
    pub(super) fn sidebar_has_events(&self) -> bool {
        let (Some(guild), Some(me)) = (self.current_guild(), self.stores.me()) else {
            return false;
        };
        visible_events(
            guild,
            me,
            self.fetched
                .get(&format!("{EVENTS}{}", guild.id))
                .unwrap_or(&Value::Null),
        )
    }

    pub(super) fn sidebar_has_boosts(&self) -> bool {
        self.current_guild()
            .zip(self.stores.me())
            .is_some_and(|(guild, me)| {
                permissions::can_in_guild(guild, me, permissions::ADMINISTRATOR)
            })
    }

    pub(super) fn prepare_sidebar_events(&mut self) {
        let Some(guild) = self.current_guild() else {
            return;
        };
        let id = guild.id;
        let key = format!("{EVENTS}{id}");
        self.retry_failed_fetch(&key);
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/guilds/{id}/scheduled-events"),
            });
        }
    }

    pub(super) fn sidebar_event_update(&mut self, name: &str, data: &Value) {
        if !matches!(
            name,
            "GUILD_SCHEDULED_EVENT_CREATE"
                | "GUILD_SCHEDULED_EVENT_UPDATE"
                | "GUILD_SCHEDULED_EVENT_DELETE"
        ) {
            return;
        }
        let (Some(guild), Some(id)) = (data["guild_id"].as_str(), data["id"].as_str()) else {
            return;
        };
        let value = self
            .fetched
            .entry(format!("{EVENTS}{guild}"))
            .or_insert_with(|| Value::Array(vec![]));
        if !value.is_array() {
            *value = Value::Array(vec![]);
        }
        if let Some(events) = value.as_array_mut() {
            events.retain(|event| event["id"].as_str() != Some(id));
            if name != "GUILD_SCHEDULED_EVENT_DELETE" {
                events.push(data.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sidebar_events_follow_permissions_status_and_channel_visibility() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"fixture"},"guilds":[{
                "id":"100","name":"fixture","owner_id":"2",
                "roles":[{"id":"100","name":"everyone","permissions":"0"}],
                "channels":[{"id":"200","name":"hidden","type":2}]
            }]}),
        );
        let me = stores.me().unwrap();
        let guild = &mut stores.guilds[0];
        assert!(!visible_events(guild, me, &json!([])));
        assert!(visible_events(
            guild,
            me,
            &json!([{"status":1,"channel_id":null}])
        ));
        assert!(!visible_events(
            guild,
            me,
            &json!([{"status":1,"channel_id":"200"}])
        ));
        assert!(!visible_events(
            guild,
            me,
            &json!([{"status":3,"channel_id":null},{"status":4,"channel_id":null}])
        ));
        guild.role_permissions.insert(100, CREATE_EVENTS);
        assert!(visible_events(guild, me, &json!([])));
        guild.features.push("HUB".into());
        assert!(!visible_events(guild, me, &json!([])));
    }
}
