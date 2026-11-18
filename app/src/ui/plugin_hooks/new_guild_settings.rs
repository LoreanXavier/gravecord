//! NewGuildSettings (Vencord `src/plugins/newGuildSettings`, GPL-3.0-or-later): a server you join gets your defaults
//! (`applyDefaultSettings`): muted (`guild`), mobile push off (`mobilePush`), @everyone and @here suppressed
//! (`everyone`), role mentions suppressed (`role`), highlights off (`highlights`), new events muted (`events`), the
//! notification level of `messages` unless it is "Server default", Show All Channels in a server that only shows the
//! channels you picked (`showAllChannels`), and voice channel names hidden (`voiceChannels`). A join is a server whose
//! `GUILD_CREATE` arrives with your `joined_at` from the last two minutes, however you joined (an invite, Discovery, a
//! join request approved); a lurked server has no `joined_at`. The server menu's privacy group gets "Apply
//! NewGuildSettings", and a folder's menu "Apply NewGuildSettings to Folder" (each server 250 ms apart).
//!
//! GAP: the CogWheel icon of the row in the server header's dropdown (the app has no such row there). Not part of the
//! pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use crate::plugins::State;
use dc_state::Id;
use gpui::Context;
use serde_json::{Map, Value, json};
use std::time::Duration;

const ID: &str = "NewGuildSettings";
/// `OPT_IN_ENABLED` of a server's notification settings flags.
const OPT_IN_ENABLED: u64 = 16384;

/// The defaults.
#[derive(Clone, Debug, PartialEq)]
struct Defaults {
    fields: Map<String, Value>,
    show_all: bool,
    hide_voice_names: bool,
}

fn defaults(state: &State) -> Option<Defaults> {
    if !state.is_on(ID) {
        return None;
    }
    let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
    let mut fields = Map::new();
    fields.insert("muted".to_owned(), json!(flag("guild")));
    fields.insert("mobile_push".to_owned(), json!(!flag("mobilePush")));
    fields.insert("suppress_everyone".to_owned(), json!(flag("everyone")));
    fields.insert("suppress_roles".to_owned(), json!(flag("role")));
    fields.insert("mute_scheduled_events".to_owned(), json!(flag("events")));
    fields.insert(
        "notify_highlights".to_owned(),
        json!(if flag("highlights") { 1 } else { 0 }),
    );
    let level = state.value(ID, "messages").as_u64().unwrap_or(3);
    if level != 3 {
        fields.insert("message_notifications".to_owned(), json!(level));
    }
    Some(Defaults {
        fields,
        show_all: flag("showAllChannels"),
        hide_voice_names: flag("voiceChannels"),
    })
}

fn current() -> Option<Defaults> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(defaults)
}

/// A server just joined: its `joined_at` is within two minutes of `now`.
fn fresh_join(joined_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    joined_at
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .is_some_and(|joined| {
            let age = now.signed_duration_since(joined);
            age >= chrono::Duration::seconds(-30) && age <= chrono::Duration::minutes(2)
        })
}

impl Root {
    /// A `GUILD_CREATE`: a server you just joined gets the defaults.
    pub(in crate::ui) fn new_guild_settings_dispatch(&mut self, name: &str, data: &Value) {
        if name != "GUILD_CREATE" || current().is_none() {
            return;
        }
        let joined = data["joined_at"]
            .as_str()
            .or_else(|| data["properties"]["joined_at"].as_str());
        if !fresh_join(joined, chrono::Utc::now()) {
            return;
        }
        if let Some(guild) = data["id"].as_str().and_then(|id| id.parse::<Id>().ok()) {
            self.apply_new_guild_settings(guild);
        }
    }

    /// `applyDefaultSettings`.
    pub(in crate::ui) fn apply_new_guild_settings(&mut self, guild: Id) {
        let Some(defaults) = current() else {
            return;
        };
        let mut fields = defaults.fields;
        // `toggleShowAllChannels` of a Community server that shows only the picked channels.
        let community = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.features.iter().any(|feature| feature == "COMMUNITY"));
        let flags = self
            .stores
            .guild_settings
            .guilds
            .get(&guild)
            .map(|settings| settings.flags);
        if defaults.show_all
            && community
            && let Some(flags) = flags.filter(|flags| flags & OPT_IN_ENABLED != 0)
        {
            fields.insert("flags".to_owned(), json!(flags ^ OPT_IN_ENABLED));
        }
        let mut settings = self
            .stores
            .guild_settings
            .guilds
            .remove(&guild)
            .unwrap_or_else(|| dc_state::guild_settings::GuildSettings::from_value(&json!({})));
        for (field, value) in &fields {
            settings.apply(field, value);
        }
        self.stores.guild_settings.guilds.insert(guild, settings);
        let _ = self.requests.send(crate::net::Request::GuildSettings {
            body: json!({"guilds": {guild.to_string(): Value::Object(fields)}}),
        });
        if defaults.hide_voice_names {
            self.hide_voice_names_in(guild);
        }
    }

    /// `applyVoiceNameHidingToGuild`: every voice channel's names hidden (the Hide Names check).
    fn hide_voice_names_in(&mut self, guild: Id) {
        let Some(known) = self.stores.guild(guild) else {
            return;
        };
        let voice: Vec<Id> = known
            .channels
            .iter()
            .filter(|channel| {
                matches!(
                    channel.kind,
                    dc_state::ChannelKind::Voice | dc_state::ChannelKind::Other(13)
                )
            })
            .map(|channel| channel.id)
            .collect();
        let key = super::super::context_menu::HIDDEN_NAMES;
        let mut stored = crate::storage::get(key).unwrap_or_else(|| json!({}));
        for id in voice {
            stored[id.to_string()] = json!(true);
        }
        crate::storage::set(key, stored);
    }

    /// The server menu with "Apply NewGuildSettings" at the end of its privacy group (before ServerInfo's row).
    pub(in crate::ui) fn with_new_guild_settings_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if current().is_none() || !matches!(target, Target::Guild(_)) {
            return items;
        }
        let at = items
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::EditProfile))
            .map_or(items.len(), |at| at + 1);
        items.insert(
            at,
            Entry::Row(Row::new(
                Act::PluginApplyNewGuildSettings,
                "Apply NewGuildSettings",
            )),
        );
        items
    }

    /// Whether a folder's menu carries "Apply NewGuildSettings to Folder".
    pub(in crate::ui) fn new_guild_settings_on(&self) -> bool {
        current().is_some()
    }

    /// "Apply NewGuildSettings to Folder": each server of the folder, 250 ms apart.
    pub(in crate::ui) fn apply_new_guild_settings_to_folder(
        &mut self,
        folder: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(guilds) = self
            .stores
            .folders
            .folder(folder)
            .map(|folder| folder.guilds.clone())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            for guild in guilds {
                if this
                    .update(cx, |this, _| this.apply_new_guild_settings(guild))
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_switches() {
        let mut state = State::default();
        assert_eq!(defaults(&state), None);
        state.set_enabled(ID, true);
        let on = defaults(&state).unwrap();
        assert_eq!(on.fields["muted"], json!(true));
        assert_eq!(on.fields["mobile_push"], json!(false));
        assert_eq!(on.fields["notify_highlights"], json!(1));
        assert!(
            !on.fields.contains_key("message_notifications"),
            "server default sends no level"
        );
        assert!(on.show_all && !on.hide_voice_names);
        state.set_value(ID, "messages", &json!(1));
        state.set_value(ID, "guild", &json!(false));
        let some = defaults(&state).unwrap();
        assert_eq!(some.fields["message_notifications"], json!(1));
        assert_eq!(some.fields["muted"], json!(false));
    }

    #[test]
    fn only_a_recent_join_counts() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(fresh_join(Some("2026-11-18T11:59:10+00:00"), now));
        assert!(!fresh_join(Some("2026-11-01T11:59:10+00:00"), now));
        assert!(!fresh_join(None, now));
    }
}
