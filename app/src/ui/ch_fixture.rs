//! Capture-only harness steps (`ch ...`) for the voice status, announcement and Stage work: inspect the fixture guild,
//! make a channel or a message in it through the client's own REST path, and feed a gateway dispatch to the stores.
//! Every write names the fixture guild only.

use super::Root;
use crate::net::Request;
use dc_state::{ChannelKind, Id};
use serde_json::{Value, json};

const FIXTURE_GUILD: Id = 1_568_692_564_637_524_109;

impl Root {
    /// `ch features`, `ch create <announcement|stage> <name>`, `ch post <channel> <text>`,
    /// `ch inject <EVENT> <json>`.
    pub(crate) fn script_ch(&mut self, words: &[&str]) -> String {
        match words {
            ["features"] => {
                let Some(guild) = self.stores.guilds.iter().find(|g| g.id == FIXTURE_GUILD) else {
                    return "the fixture guild is not loaded".to_owned();
                };
                let channels: Vec<String> = guild
                    .channels
                    .iter()
                    .map(|c| {
                        let kind = match c.kind {
                            ChannelKind::Text => 0,
                            ChannelKind::Voice => 2,
                            ChannelKind::Category => 4,
                            ChannelKind::Other(n) => n,
                        };
                        format!("{}:{}:{}", c.id, kind, c.name)
                    })
                    .collect();
                format!(
                    "features {:?}; channels {}",
                    guild.features,
                    channels.join(", ")
                )
            }
            // The fixture guild is a shared reference: Community stays off, so the wizard's own steps are refused.
            ["community", _] => "refused: the fixture guild keeps its settings".to_owned(),
            // `ch rest <GET|PATCH|DELETE> <route> [json]`: only the allow rule's routes (`rest_allowed`).
            ["rest", method, route, body @ ..] => {
                if !rest_allowed(method, route) {
                    return "refused: not an allowed route".to_owned();
                }
                let body = if body.is_empty() {
                    None
                } else {
                    match serde_json::from_str::<Value>(&body.join(" ")) {
                        Ok(value) => Some(value),
                        Err(error) => return format!("bad json: {error}"),
                    }
                };
                let method = match *method {
                    "GET" => reqwest::Method::GET,
                    "PATCH" => reqwest::Method::PATCH,
                    _ => reqwest::Method::DELETE,
                };
                self.fetched.remove("ch-rest");
                let _ = self.requests.send(Request::Rest {
                    key: Some("ch-rest".to_owned()),
                    method,
                    route: (*route).to_owned(),
                    body,
                });
                "rest request sent".to_owned()
            }
            // `ch result`: the status of the last `ch rest` and the few fields restoring the guild needs.
            ["result"] => match self.fetched.get("ch-rest") {
                None => "no answer yet".to_owned(),
                Some(answer) => summarise(answer),
            },
            // No channel is ever made in the fixture guild: announcement and Stage channels come from `ch inject`.
            ["create", ..] => "refused: the fixture guild keeps its channels".to_owned(),
            ["post", channel, text @ ..] => {
                let Ok(channel) = channel.parse::<Id>() else {
                    return "bad channel id".to_owned();
                };
                let in_fixture = self
                    .stores
                    .guilds
                    .iter()
                    .find(|g| g.id == FIXTURE_GUILD)
                    .is_some_and(|g| g.channels.iter().any(|c| c.id == channel));
                if !in_fixture {
                    return "refused: not a channel of the fixture guild".to_owned();
                }
                let _ = self.requests.send(Request::Rest {
                    key: Some("ch-post".to_owned()),
                    method: reqwest::Method::POST,
                    route: format!("/channels/{channel}/messages"),
                    body: Some(json!({"content": text.join(" ")})),
                });
                "post request sent".to_owned()
            }
            // `ch stats <channel> <guilds_following> <users_seen>`: the follower numbers a publish confirm reads.
            ["stats", channel, guilds, seen] => {
                let (Ok(channel), Ok(guilds), Ok(seen)) = (
                    channel.parse::<Id>(),
                    guilds.parse::<u64>(),
                    seen.parse::<u64>(),
                ) else {
                    return "usage: ch stats <channel> <guilds> <seen>".to_owned();
                };
                self.follower_stats.insert(
                    channel,
                    super::announcements::FollowerStats {
                        guilds_following: Some(guilds),
                        users_seen: Some(seen),
                    },
                );
                "stats set".to_owned()
            }
            // `ch msg <channel> <id> <kind> <flags> <own|other> [ref_guild] <text...>`: a message that exists only in
            // this client (a `MESSAGE_CREATE` fed to the stores); `other` is a webhook, and with a reference guild
            // the message points at a message of that server.
            ["msg", channel, message, kind, flags, who, rest @ ..] => {
                let (Ok(channel), Ok(message), Ok(kind), Ok(flags)) = (
                    channel.parse::<Id>(),
                    message.parse::<Id>(),
                    kind.parse::<u64>(),
                    flags.parse::<u64>(),
                ) else {
                    return "usage: ch msg <channel> <id> <kind> <flags> <own|other> [ref_guild] <text>"
                        .to_owned();
                };
                let author = match (*who, self.stores.current_user.as_ref()) {
                    ("own", Some(me)) => json!({"id": me.id.to_string(), "username": me.username}),
                    _ => {
                        json!({"id": "1999000000000000777", "username": "Announcements", "bot": true})
                    }
                };
                let (reference, text) = match rest.split_first() {
                    Some((first, tail)) if first.parse::<Id>().is_ok() && !tail.is_empty() => (
                        Some(json!({
                            "guild_id": first,
                            "channel_id": "1999000000000000555",
                            "message_id": "1999000000000000556",
                        })),
                        tail.join(" "),
                    ),
                    _ => (None, rest.join(" ")),
                };
                let mut data = json!({
                    "id": message.to_string(),
                    "channel_id": channel.to_string(),
                    "content": text,
                    "author": author,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "type": kind,
                    "flags": flags,
                    "mentions": [],
                    "attachments": [],
                    "embeds": [],
                });
                if let Some(reference) = reference {
                    data["message_reference"] = reference;
                    data["webhook_id"] = json!("1999000000000000777");
                }
                self.stores.dispatch("MESSAGE_CREATE", &data);
                self.data_revision += 1;
                self.rebuild_rows();
                "message injected".to_owned()
            }
            // `ch complete <channel>`: the channel's history is whole (its first message is on screen), as after
            // the fetch of a channel with few messages; nothing is requested.
            ["complete", channel] => {
                let Ok(channel) = channel.parse::<Id>() else {
                    return "bad channel id".to_owned();
                };
                let window = self.stores.windows.entry(channel).or_default();
                window.ready = true;
                window.has_fetched = true;
                window.has_more_before = false;
                self.data_revision += 1;
                self.rebuild_rows();
                "history marked whole".to_owned()
            }
            // `ch call <channel>`: this account counts as connected to the channel of the open guild, with no voice
            // state sent and no media opened.
            ["call", "off"] => {
                self.call = None;
                self.data_revision += 1;
                "call dropped".to_owned()
            }
            ["call", channel] => {
                let (Some(guild), Ok(channel)) = (self.guild, channel.parse::<Id>()) else {
                    return "no guild or bad channel id".to_owned();
                };
                self.fake_guild_call(guild, channel);
                self.data_revision += 1;
                "call faked".to_owned()
            }
            // `ch blocker <on|off>`: shows the Follow bar as an account without Send Messages would see it.
            ["blocker", state] => {
                self.follow_blocker_forced = *state == "on";
                self.data_revision += 1;
                format!("follow bar forced {state}")
            }
            ["inject", event, json @ ..] => match serde_json::from_str::<Value>(&json.join(" ")) {
                Ok(data) => {
                    self.stores.dispatch(event, &data);
                    self.data_revision += 1;
                    format!("injected {event}")
                }
                Err(error) => format!("bad json: {error}"),
            },
            // `ch webhooks <json array>`: the open server's webhook list as `GET /guilds/{id}/webhooks` would answer
            // (memory only; no webhook is made on the server).
            ["webhooks", json @ ..] => {
                let (Some(guild), Ok(list)) =
                    (self.guild, serde_json::from_str::<Value>(&json.join(" ")))
                else {
                    return "needs an open server and a json array".to_owned();
                };
                let hooks = list.as_array().cloned().unwrap_or_default();
                self.stores
                    .integrations
                    .webhooks_loaded(guild, None, &hooks);
                self.data_revision += 1;
                "webhooks set".to_owned()
            }
            // `ch vs <me|user id> <leave|flags...>`: a voice state of that person in the fixture voice channel, fed to the
            // stores only (nothing is sent, nobody joins). Flags: selfmute, selfdeaf, mute, deaf.
            ["vs", who, flags @ ..] => {
                let user = match *who {
                    "me" => self.stores.current_user.as_ref().map(|me| me.id),
                    other => other.parse::<Id>().ok(),
                };
                let Some(user) = user else {
                    return "usage: ch vs <me|user id> <leave|selfmute|selfdeaf|mute|deaf>"
                        .to_owned();
                };
                let leaving = flags.contains(&"leave");
                let data = json!({
                    "guild_id": FIXTURE_GUILD.to_string(),
                    // `stage` puts the person in the Stage channel the base script injects.
                    "channel_id": if leaving {
                        Value::Null
                    } else if flags.contains(&"stage") {
                        json!("1999000000000000103")
                    } else {
                        json!("1568692565136375901")
                    },
                    "user_id": user.to_string(),
                    "session_id": "harness",
                    "self_mute": flags.contains(&"selfmute"),
                    "self_deaf": flags.contains(&"selfdeaf"),
                    "mute": flags.contains(&"mute"),
                    "deaf": flags.contains(&"deaf"),
                    "suppress": flags.contains(&"suppress"),
                });
                self.stores.dispatch("VOICE_STATE_UPDATE", &data);
                self.data_revision += 1;
                format!("voice state injected for {user}")
            }
            _ => "usage: ch features | create | post | inject | vs | webhooks".to_owned(),
        }
    }
}

/// The only routes `ch rest` sends: reads and edits under the fixture guild, and the delete of the one channel the
/// community wizard made there. The guild itself is never deleted.
const RULES_CHANNEL: Id = 1_570_186_623_927_193_621;

fn rest_allowed(method: &str, route: &str) -> bool {
    let path = route.split('?').next().unwrap_or("");
    if path.contains("..") || path.contains('#') {
        return false;
    }
    let guild = format!("/guilds/{FIXTURE_GUILD}");
    let in_guild = path == guild
        || path
            .strip_prefix(&guild)
            .is_some_and(|rest| rest.starts_with('/'));
    // Reads only: the fixture guild's settings and structure are never written from the harness.
    let _ = RULES_CHANNEL;
    match method {
        "GET" => in_guild,
        _ => false,
    }
}

/// Status plus the values a restore reads: no tokens, no message content.
fn summarise(answer: &Value) -> String {
    let status = &answer["status"];
    let body = &answer["body"];
    let mut out = format!("status {status}");
    for key in [
        "verification_level",
        "explicit_content_filter",
        "default_message_notifications",
        "features",
        "rules_channel_id",
        "public_updates_channel_id",
        "permissions",
    ] {
        if !body[key].is_null() {
            out.push_str(&format!("; {key}={}", body[key]));
        }
    }
    for entry in body["audit_log_entries"].as_array().into_iter().flatten() {
        let changes: Vec<String> = entry["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| format!("{}:{}->{}", c["key"], c["old_value"], c["new_value"]))
            .collect();
        out.push_str(&format!(
            "; entry {} action {} target {} [{}]",
            entry["id"],
            entry["action_type"],
            entry["target_id"],
            changes.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reads_under_the_fixture_guild_are_reachable() {
        let guild = format!("/guilds/{FIXTURE_GUILD}");
        assert!(rest_allowed("GET", &format!("{guild}/audit-logs?limit=50")));
        // No write of any kind, not another guild, not a prefix lookalike.
        assert!(!rest_allowed("PATCH", &guild));
        assert!(!rest_allowed(
            "PATCH",
            &format!("{guild}/roles/{FIXTURE_GUILD}")
        ));
        assert!(!rest_allowed(
            "DELETE",
            &format!("/channels/{RULES_CHANNEL}")
        ));
        assert!(!rest_allowed("DELETE", &guild));
        assert!(!rest_allowed("GET", "/guilds/1554855651514122281"));
        assert!(!rest_allowed("GET", &format!("{guild}9")));
        assert!(!rest_allowed(
            "GET",
            &format!("{guild}/../1554855651514122281")
        ));
        assert!(!rest_allowed("POST", &guild));
    }
}
