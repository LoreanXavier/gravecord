//! BypassStatus (Equicord `src/equicordplugins/bypassStatus`, GPL-3.0-or-later): while your status is `statusToUse`
//! (Do Not Disturb by default), a message still notifies (the client mod's notification and, with
//! `notificationSound`, the message sound) when it mentions you in a server or channel on the lists, or when a listed
//! person writes in your DM with them (anywhere with `allowOutsideOfDms`, when it mentions you). Not for your own
//! messages, empty ones, the open channel while the window has focus, nor `@silent` ones with `respectSilentPings`.
//! The title is the author's display name, then for a server message "(#channel, parent)" ("undefined" for a channel
//! without a parent, as the original reads it). The original reads `globalName` off the raw gateway message, whose
//! field is `global_name`, so its titles start with "undefined"; the name is shown here instead. In the body a 19-digit channel mention reads
//! "#name" and an 18-digit user mention "@globalName". A click opens the message. The server, channel and user menus
//! get "Add Status Bypass" / "Remove Status Bypass" (not on yourself) as a group before their last one. Not part of
//! the pinned client.
//!
//! GAP: for a listed person without a DM channel yet, the original creates one (`getOrEnsurePrivateChannel`) only to
//! compare; nothing is created here, which changes no outcome. The lists are matched as typed with spaces taken out
//! (`processIds` on change).

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use super::plugin_notifications::{Action, Notification};
use crate::plugins::State;
use dc_state::{Id, Message};
use gpui::Context;
use serde_json::Value;

const ID: &str = "BypassStatus";
/// `SILENT_PING_FLAG`.
const SILENT: u64 = 1 << 12;

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    guilds: Vec<String>,
    channels: Vec<String>,
    users: Vec<String>,
    outside_dms: bool,
    sound: bool,
    respect_silent: bool,
    status: String,
}

/// `processIds`: the ids without spaces, empty ones dropped.
fn process_ids(value: &str) -> Vec<String> {
    value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    let list = |value: Value| process_ids(value.as_str().unwrap_or_default());
    Some(Settings {
        guilds: list(key("guilds")),
        channels: list(key("channels")),
        users: list(key("users")),
        outside_dms: flag("allowOutsideOfDms"),
        sound: flag("notificationSound"),
        respect_silent: flag("respectSilentPings"),
        status: key("statusToUse").as_str().unwrap_or("dnd").to_owned(),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

/// `<#(\d{19})>` and `<@(\d{18})>` replaced with what `name` finds (`undefined` for a channel it does not).
fn replace_mentions(
    text: &str,
    channel_name: &dyn Fn(Id) -> Option<String>,
    user_name: &dyn Fn(Id) -> Option<Option<String>>,
) -> Option<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let (sigil, digits) = match tail.get(1..2) {
            Some("#") => ('#', 19),
            Some("@") => ('@', 18),
            _ => {
                out.push('<');
                rest = &tail[1..];
                continue;
            }
        };
        let body = &tail[2..];
        let number: String = body.chars().take_while(char::is_ascii_digit).collect();
        if number.len() == digits && body[digits..].starts_with('>') {
            let id: Id = number.parse().unwrap_or(0);
            if sigil == '#' {
                out.push('#');
                out.push_str(&channel_name(id).unwrap_or_else(|| "undefined".to_owned()));
            } else {
                // `UserStore.getUser(id).globalName`: an unknown person throws and nothing is shown.
                let name = user_name(id)?;
                out.push('@');
                out.push_str(&name.unwrap_or_else(|| "undefined".to_owned()));
            }
            rest = &body[digits + 1..];
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Some(out)
}

impl Root {
    /// The list setting each menu edits: `guild-context`, `channel-context` and `user-context` (a 1:1 DM's row opens
    /// the user menu; a thread has a menu of its own, which the plugin does not patch).
    fn bypass_list_key(&self, target: Target) -> Option<(&'static str, Id)> {
        match target {
            Target::Guild(guild) => Some(("guilds", guild)),
            Target::Channel(channel) => Some(("channels", channel)),
            Target::User { user, .. } => Some(("users", user)),
            Target::Dm(channel) => self.dm_user(channel).map(|user| ("users", user)),
            _ => None,
        }
    }

    /// The plugin's `MESSAGE_CREATE`.
    pub(in crate::ui) fn bypass_status_dispatch(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) {
        if name != "MESSAGE_CREATE" {
            return;
        }
        let Some(settings) = current() else {
            return;
        };
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return;
        };
        let Some(message) = Message::from_value(data) else {
            return;
        };
        let channel = message.channel_id;
        let guild: Option<Id> = data["guild_id"].as_str().and_then(|id| id.parse().ok());
        if message.content.is_empty()
            || message.author.id == me
            || (self.channel == Some(channel) && self.notification_focused)
            || self.stores.status != settings.status
        {
            return;
        }
        if settings.respect_silent && message.flags & SILENT != 0 {
            return;
        }
        let in_guild =
            guild.or_else(|| self.stores.guild_of_channel(channel).map(|guild| guild.id));
        let guild_data = in_guild.and_then(|id| self.stores.guild(id));
        let guild_settings = in_guild.and_then(|id| self.stores.guild_settings.guilds.get(&id));
        let mentioned = message.mentions_me(
            me,
            guild_data.map_or(&[][..], |guild| guild.my_roles.as_slice()),
            guild_settings.is_some_and(|settings| settings.suppress_everyone),
            guild_settings.is_some_and(|settings| settings.suppress_roles),
        );
        let listed =
            |list: &[String], id: Option<Id>| id.is_some_and(|id| list.contains(&id.to_string()));
        let notify = if (listed(&settings.guilds, guild)
            || listed(&settings.channels, Some(channel)))
            && mentioned
        {
            true
        } else if listed(&settings.users, Some(message.author.id)) {
            let dm = self
                .stores
                .private_channels
                .iter()
                .find(|known| known.kind == 1 && known.recipients == [message.author.id])
                .map(|known| known.id);
            dm == Some(channel) || (mentioned && settings.outside_dms)
        } else {
            false
        };
        if notify {
            self.bypass_status_notify(&settings, &message, guild, cx);
        }
    }

    /// `showNotification`.
    fn bypass_status_notify(
        &mut self,
        settings: &Settings,
        message: &Message,
        guild: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        let channel_name = |id: Id| {
            self.stores
                .guilds
                .iter()
                .flat_map(|guild| guild.channels.iter())
                .find(|channel| channel.id == id)
                .map(|channel| channel.name.clone())
        };
        let user_name = |id: Id| {
            self.stores
                .users
                .get(&id)
                .map(|user| user.global_name.clone())
        };
        let Some(body) = replace_mentions(&message.content, &channel_name, &user_name) else {
            return;
        };
        let author_name = message
            .author
            .global_name
            .clone()
            .unwrap_or_else(|| message.author.username.clone());
        let place = if guild.is_some() {
            let channel = self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| guild.channels.iter())
                .find(|channel| channel.id == message.channel_id);
            let name =
                channel.map_or_else(|| "undefined".to_owned(), |channel| channel.name.clone());
            let parent = channel
                .and_then(|channel| channel.parent_id)
                .and_then(channel_name)
                .unwrap_or_else(|| "undefined".to_owned());
            format!("(#{name}, {parent})")
        } else {
            String::new()
        };
        let icon = self
            .stores
            .users
            .get(&message.author.id)
            .map(|user| super::super::avatar_source(user).1);
        let focused = self.notification_focused;
        self.plugin_notify(
            Notification {
                title: format!("{author_name} {place}"),
                body,
                icon,
                action: Action::JumpTo(message.channel_id, message.id),
            },
            focused,
            cx,
        );
        if settings.sound {
            crate::sounds::play_with("message1", 0.4, dc_voice::sound::Output::Default);
        }
    }

    /// "Add Status Bypass" / "Remove Status Bypass" as a group before the menu's last one.
    pub(in crate::ui) fn with_bypass_status_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        let (Some(settings), Some((list, id))) = (current(), self.bypass_list_key(target)) else {
            return items;
        };
        if list == "users"
            && self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|me| me.id == id)
        {
            return items;
        }
        let ids = match list {
            "guilds" => &settings.guilds,
            "channels" => &settings.channels,
            _ => &settings.users,
        };
        let enabled = ids.contains(&id.to_string());
        let mut row = Row::new(
            Act::PluginStatusBypass,
            if enabled {
                "Remove Status Bypass"
            } else {
                "Add Status Bypass"
            },
        );
        row.status_dot = Some(enabled);
        // `children.splice(-1, 0, group)`: before the last group, a separator between them.
        match items
            .iter()
            .rposition(|entry| matches!(entry, Entry::Separator))
        {
            Some(at) => {
                items.insert(at, Entry::Row(row));
                items.insert(at, Entry::Separator);
            }
            None => {
                items.insert(0, Entry::Separator);
                items.insert(0, Entry::Row(row));
            }
        }
        items
    }

    /// The row was chosen: the id joins or leaves its list.
    pub(in crate::ui) fn bypass_status_toggle(&mut self, target: Target, cx: &mut Context<Self>) {
        let Some((list, id)) = self.bypass_list_key(target) else {
            return;
        };
        crate::plugins::update(|state| {
            let mut ids = process_ids(state.value(ID, list).as_str().unwrap_or_default());
            let id = id.to_string();
            if ids.contains(&id) {
                ids.retain(|known| *known != id);
            } else {
                ids.push(id);
            }
            state.set_value(ID, list, &Value::from(ids.join(", ")));
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_mentions_follow_the_original() {
        assert_eq!(process_ids(" 1, 2 ,,3 "), ["1", "2", "3"]);
        let channel = |id: Id| (id == 1_234_567_890_123_456_789).then(|| "general".to_owned());
        let user = |id: Id| (id == 123_456_789_012_345_678).then(|| Some("Ann".to_owned()));
        assert_eq!(
            replace_mentions(
                "hi <#1234567890123456789> <@123456789012345678> <#12>",
                &channel,
                &user
            )
            .as_deref(),
            Some("hi #general @Ann <#12>")
        );
        // An unknown person throws in the original: no notification.
        assert_eq!(
            replace_mentions("<@999999999999999999>", &channel, &user),
            None
        );
        let mut state = State::default();
        state.set_enabled(ID, true);
        let settings = settings_of(&state).unwrap();
        assert_eq!(settings.status, "dnd");
        assert!(settings.sound && settings.respect_silent && !settings.outside_dms);
    }
}
