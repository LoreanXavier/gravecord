//! The plugins' slash commands (Vencord's `CommandsAPI`): a plugin's command joins the built-in ones
//! (`inputType: BUILT_IN`, application `-1`, the "Built-In" section) while its plugin is on, and runs here. A command
//! answers in the conversation as `sendMessage` does, or privately from Clyde (`sendBotMessage`).
//!
//! - SpotifyShareCommands (Vencord `src/plugins/spotifyShareCommands`): /track, /album and /artist send the link of what
//!   your Spotify plays now (`open.spotify.com/track/<id>`, the album's, the first artist's), after the optional
//!   message; without a track, Clyde says "You're not listening to any music.".
//! - FriendInvites (Vencord `src/plugins/friendInvites`): /create friend invite (`POST /users/@me/invites`), /view friend
//!   invites (`GET`) and /revoke friend invites (`DELETE`), each answered by Clyde with the invites' links, expiry and
//!   uses.
//!
//! GAP: SpotifyShareCommands' "Failed to find the track on spotify." for a local file: the app's Spotify store keeps
//! no player for a track without an id, so a local file reads as no music. Not part of the pinned client.

use super::super::Root;
use super::super::slash::{Builtin, Cmd, Filled, built_in, message_option, string_value};
use dc_state::Id;
use gpui::Context;
use serde_json::{Value, json};

/// What a command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Does {
    Track,
    Album,
    Artist,
    CreateInvite,
    ViewInvites,
    RevokeInvites,
}

struct Command {
    plugin: &'static str,
    name: &'static str,
    description: &'static str,
    does: Does,
}

const COMMANDS: &[Command] = &[
    Command {
        plugin: "SpotifyShareCommands",
        name: "track",
        description: "Share your current Spotify track in chat",
        does: Does::Track,
    },
    Command {
        plugin: "SpotifyShareCommands",
        name: "album",
        description: "Share your current Spotify album in chat",
        does: Does::Album,
    },
    Command {
        plugin: "SpotifyShareCommands",
        name: "artist",
        description: "Share your current Spotify artist in chat",
        does: Does::Artist,
    },
    Command {
        plugin: "FriendInvites",
        name: "create friend invite",
        description: "Generates a friend invite link.",
        does: Does::CreateInvite,
    },
    Command {
        plugin: "FriendInvites",
        name: "view friend invites",
        description: "View a list of all generated friend invites.",
        does: Does::ViewInvites,
    },
    Command {
        plugin: "FriendInvites",
        name: "revoke friend invites",
        description: "Revokes all generated friend invites.",
        does: Does::RevokeInvites,
    },
];

/// The answers to FriendInvites' requests come back under this key: `<prefix><does>:<channel>`.
pub(in crate::ui) const KEY: &str = "plugin-friend-invites:";

/// `<t:…:R>` of an ISO time.
fn relative(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|time| format!("<t:{}:R>", time.timestamp()))
        .unwrap_or_else(|_| "<t:0:R>".to_owned())
}

/// The line of a created invite.
fn created_line(invite: &Value) -> String {
    format!(
        "discord.gg/{} · Expires: {} · Max uses: `{}`",
        invite["code"].as_str().unwrap_or_default(),
        relative(invite["expires_at"].as_str().unwrap_or_default()),
        invite["max_uses"]
    )
}

/// The list of all invites, or the empty answer.
fn list_lines(invites: &Value) -> String {
    let lines: Vec<String> = invites
        .as_array()
        .into_iter()
        .flatten()
        .map(|invite| {
            format!(
                "_discord.gg/{}_ · Expires: {} · Times used: `{}/{}`",
                invite["code"].as_str().unwrap_or_default(),
                relative(invite["expires_at"].as_str().unwrap_or_default()),
                invite["uses"],
                invite["max_uses"]
            )
        })
        .collect();
    if lines.is_empty() {
        "You have no active friend invites!".to_owned()
    } else {
        lines.join("\n")
    }
}

/// The text sent for a Spotify command: the link after the optional message.
fn share_text(message: &str, link: &str) -> String {
    if message.is_empty() {
        link.to_owned()
    } else {
        format!("{message} {link}")
    }
}

impl Root {
    /// The commands of the plugins that are on.
    pub(in crate::ui) fn plugin_commands(&self) -> Vec<Cmd> {
        if crate::plugins::bypassed() {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .enumerate()
            .filter(|(_, command)| crate::plugins::on(command.plugin))
            .map(|(index, command)| {
                let options = match command.does {
                    Does::Track | Does::Album | Does::Artist => {
                        vec![message_option("Your message", false)]
                    }
                    _ => Vec::new(),
                };
                built_in(
                    -1000 - index as i32,
                    command.name,
                    Builtin::Plugin(index as u16),
                    command.description,
                    options,
                )
            })
            .collect()
    }

    /// A plugin's command ran.
    pub(in crate::ui) fn run_plugin_command(
        &mut self,
        index: u16,
        filled: &[Filled],
        channel: Id,
        _cx: &mut Context<Self>,
    ) {
        let Some(command) = COMMANDS.get(usize::from(index)) else {
            return;
        };
        if crate::plugins::bypassed() || !crate::plugins::on(command.plugin) {
            return;
        }
        match command.does {
            Does::Track | Does::Album | Does::Artist => {
                let Some(player) = self.stores.spotify.player().cloned() else {
                    self.clyde_says(channel, "You're not listening to any music.", "Clyde");
                    return;
                };
                let link = match command.does {
                    Does::Track => Some(format!(
                        "https://open.spotify.com/track/{}",
                        player.track_id
                    )),
                    // `getTrack().album.id` and `.artists[0].id` (the player's `Track`).
                    Does::Album => Some(&player.track.album_id)
                        .filter(|id| !id.is_empty())
                        .map(|id| format!("https://open.spotify.com/album/{id}")),
                    _ => player
                        .track
                        .artists
                        .first()
                        .map(|(id, _)| format!("https://open.spotify.com/artist/{id}")),
                };
                let Some(link) = link else {
                    self.clyde_says(channel, "Failed to find the track on spotify.", "Clyde");
                    return;
                };
                self.last_typing_sent = None;
                self.send(share_text(&string_value(filled, "message"), &link));
            }
            Does::CreateInvite | Does::ViewInvites | Does::RevokeInvites => {
                let (method, body, tag) = match command.does {
                    Does::CreateInvite => (reqwest::Method::POST, Some(json!({})), "create"),
                    Does::ViewInvites => (reqwest::Method::GET, None, "view"),
                    _ => (reqwest::Method::DELETE, None, "revoke"),
                };
                let _ = self.requests.send(crate::net::Request::Rest {
                    key: Some(format!("{KEY}{tag}:{channel}")),
                    method,
                    route: "/users/@me/invites".to_owned(),
                    body,
                });
            }
        }
    }

    /// FriendInvites' answer: Clyde says what came back. True when the key was ours.
    pub(in crate::ui) fn friend_invites_answered(&mut self, key: &str, body: &Value) -> bool {
        let Some(rest) = key.strip_prefix(KEY) else {
            return false;
        };
        let Some((tag, channel)) = rest.split_once(':') else {
            return true;
        };
        let Ok(channel) = channel.parse::<Id>() else {
            return true;
        };
        // The original's promise rejects on a failed request: nothing is said.
        if body["ok"] != Value::Bool(true) {
            return true;
        }
        let answer = &body["body"];
        let text = match tag {
            "create" => created_line(answer),
            "view" => list_lines(answer),
            _ => "All friend invites have been revoked.".to_owned(),
        };
        self.clyde_says(channel, &text, "Clyde");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_read_as_the_original_writes_them() {
        let invite = json!({"code": "abc", "expires_at": "2026-11-25T10:00:00+00:00", "max_uses": 5, "uses": 1});
        assert_eq!(
            created_line(&invite),
            "discord.gg/abc · Expires: <t:1795600800:R> · Max uses: `5`"
        );
        assert_eq!(
            list_lines(&json!([invite])),
            "_discord.gg/abc_ · Expires: <t:1795600800:R> · Times used: `1/5`"
        );
        assert_eq!(list_lines(&json!([])), "You have no active friend invites!");
        assert_eq!(share_text("", "L"), "L");
        assert_eq!(share_text("hey", "L"), "hey L");
    }

    #[test]
    fn every_plugin_with_commands_is_defined() {
        for command in COMMANDS {
            assert!(
                crate::plugins::registry_has(command.plugin),
                "{}",
                command.plugin
            );
        }
    }
}
