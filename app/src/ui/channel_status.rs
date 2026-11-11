//! Voice channel status: the request for a guild's statuses and the one that sets a status.
//!
//! A guild's statuses and call start times are asked for once per connection, when one of its voice channels is
//! shown (`useChannelStatus` in module 882840 calls module 52074: op 43 `REQUEST_CHANNEL_INFO`, `fields: ["status",
//! "voice_start_time"]`). Setting or clearing one is `PUT /channels/<id>/voice-status {status}` (`Rsh.UPDATE_VOICE_CHANNEL_STATUS`,
//! `updateVoiceChannelStatus` in module 435183); the answer is a 204, and the change comes back as
//! `VOICE_CHANNEL_STATUS_UPDATE`.

use super::Root;
use crate::net::Request;
use dc_proto::op::Op;
use dc_state::ChannelKind;
#[cfg(feature = "capture")]
use dc_state::Id;
use serde_json::json;

impl Root {
    /// Asks the gateway for the statuses of the guild in view when none were asked for on this connection and the
    /// guild has a voice channel.
    pub(super) fn channel_info_check(&mut self) {
        let Some(guild) = self.guild else {
            return;
        };
        if self.stores.channel_status.has_requested(guild) {
            return;
        }
        let has_voice = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .is_some_and(|known| {
                known
                    .channels
                    .iter()
                    .any(|channel| channel.kind == ChannelKind::Voice)
            });
        if !has_voice {
            return;
        }
        self.stores.dispatch(
            "FETCH_CHANNEL_INFO",
            &json!({"guild_id": guild.to_string()}),
        );
        let _ = self.requests.send(Request::StreamOp {
            op: Op::RequestChannelInfo,
            data: json!({"guild_id": guild.to_string(), "fields": ["status", "voice_start_time"]}),
        });
    }

    #[cfg(feature = "capture")]
    /// `updateVoiceChannelStatus`: sets the status of a voice channel, or clears it with an empty text.
    pub(super) fn set_voice_status(&mut self, channel: Id, status: &str) {
        let _ = self.requests.send(Request::Rest {
            key: Some("voice-status".to_owned()),
            method: reqwest::Method::PUT,
            route: format!("/channels/{channel}/voice-status"),
            body: Some(json!({"status": status})),
        });
    }

    /// The harness's `voice status <text>` and `voice status clear`: only ever the fixture guild's General voice
    /// channel, and only while this client is in it.
    #[cfg(feature = "capture")]
    pub(crate) fn script_voice_status(&mut self, words: &[&str]) -> String {
        const FIXTURE_VOICE: Id = 1_568_692_565_136_375_901;
        let shown = |root: &Self| {
            root.stores
                .guilds
                .iter()
                .find(|guild| {
                    guild
                        .channels
                        .iter()
                        .any(|channel| channel.id == FIXTURE_VOICE)
                })
                .and_then(|guild| {
                    root.stores
                        .channel_status
                        .get_channel_status(guild.id, FIXTURE_VOICE)
                })
                .unwrap_or("none")
                .to_owned()
        };
        // No words only reports what the store holds.
        if words.is_empty() {
            return format!("store shows: {}", shown(self));
        }
        let text = words.join(" ");
        let status = if text == "clear" { "" } else { text.as_str() };
        self.set_voice_status(FIXTURE_VOICE, status);
        format!(
            "status request sent ({} chars); store shows: {}",
            status.chars().count(),
            shown(self)
        )
    }
}
