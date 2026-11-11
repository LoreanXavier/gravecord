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
            ["community", step] => self.script_community(FIXTURE_GUILD, step),
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
            ["create", kind, name @ ..] => {
                let kind = match *kind {
                    "announcement" => 5,
                    "stage" => 13,
                    _ => return "usage: ch create <announcement|stage> <name>".to_owned(),
                };
                let name = name.join("-");
                let _ = self.requests.send(Request::Rest {
                    key: Some("ch-create".to_owned()),
                    method: reqwest::Method::POST,
                    route: format!("/guilds/{FIXTURE_GUILD}/channels"),
                    body: Some(json!({"name": name, "type": kind})),
                });
                format!("create request sent: type {kind} {name}")
            }
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
            ["inject", event, json @ ..] => match serde_json::from_str::<Value>(&json.join(" ")) {
                Ok(data) => {
                    self.stores.dispatch(event, &data);
                    self.data_revision += 1;
                    format!("injected {event}")
                }
                Err(error) => format!("bad json: {error}"),
            },
            _ => "usage: ch features | create | post | inject".to_owned(),
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
    let rules = path == format!("/channels/{RULES_CHANNEL}");
    match method {
        "GET" | "PATCH" => in_guild || (rules && method == "PATCH"),
        "DELETE" => rules,
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
    fn only_the_fixture_guild_and_the_rules_channel_are_reachable() {
        let guild = format!("/guilds/{FIXTURE_GUILD}");
        assert!(rest_allowed("GET", &format!("{guild}/audit-logs?limit=50")));
        assert!(rest_allowed("PATCH", &guild));
        assert!(rest_allowed(
            "PATCH",
            &format!("{guild}/roles/{FIXTURE_GUILD}")
        ));
        assert!(rest_allowed(
            "DELETE",
            &format!("/channels/{RULES_CHANNEL}")
        ));
        // Not the guild's delete, not another guild, not another channel, not a prefix lookalike.
        assert!(!rest_allowed("DELETE", &guild));
        assert!(!rest_allowed("GET", "/guilds/1554855651514122281"));
        assert!(!rest_allowed("DELETE", "/channels/1568692565136375900"));
        assert!(!rest_allowed("GET", &format!("{guild}9")));
        assert!(!rest_allowed(
            "PATCH",
            &format!("{guild}/../1554855651514122281")
        ));
        assert!(!rest_allowed("POST", &guild));
    }
}
