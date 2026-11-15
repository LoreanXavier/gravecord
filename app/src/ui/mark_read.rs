//! Marking many channels read at once. The official client does not ack channel by channel: the server's Mark As Read
//! (module 567035 `A`, Shift+Esc in module 795737) and the inbox's Mark All as Read (module 182179 `K`) dispatch
//! `BULK_ACK`, which module 218461 acks locally and then sends as `POST /read-states/ack-bulk` with
//! `{read_states: [{channel_id, message_id, read_state_type}]}`, 100 entries to a request, one request a second.

use super::Root;
use crate::net::Request;
use dc_state::Id;
use gpui::Context;
use serde_json::json;
use std::time::Duration;

/// Module 218461 `BULK_ACK`: `ep.splice(0, 100)` per request.
const CHUNK: usize = 100;
/// The same loop waits `BK(1e3)` before the next chunk.
const CHUNK_GAP: Duration = Duration::from_millis(1000);
/// `READ_STATE_TYPES.CHANNEL` (module 790782).
const CHANNEL: u64 = 0;

/// `BULK_ACK` of module 616740 / `ST`: one request per 100 `(channel, message)` pairs.
pub(super) fn bulk_ack_requests(reads: &[(Id, Id)]) -> Vec<Request> {
    reads
        .chunks(CHUNK)
        .map(|chunk| Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: "/read-states/ack-bulk".to_owned(),
            body: Some(json!({
                "read_states": chunk
                    .iter()
                    .map(|(channel, message)| json!({
                        "channel_id": channel.to_string(),
                        "message_id": message.to_string(),
                        "read_state_type": CHANNEL,
                    }))
                    .collect::<Vec<_>>(),
            })),
        })
        .collect()
}

impl Root {
    /// The channels and joined, unarchived threads of a server that have messages past their ack, each with the
    /// newest message it holds (module 567035 `f`: `lastMessageId`, kept only when `hasUnreadOrMentions`).
    pub(super) fn guild_unread_reads(&self, guild: Id) -> Vec<(Id, Id)> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let ids = known.channels.iter().map(|channel| channel.id).chain(
            known
                .threads
                .iter()
                .filter(|thread| thread.joined && !thread.archived)
                .map(|thread| thread.id),
        );
        ids.filter(|id| self.has_unread_or_mentions(*id))
            .map(|id| (id, self.ack_target_id(id)))
            .collect()
    }

    pub(super) fn ack_guild_events(&mut self, guild: Id) {
        const GUILD_EVENT: u64 = 1;
        let now = dc_state::read_state::snowflake_at(super::picker::now_ms());
        let latest = self
            .stores
            .read_states
            .typed
            .get(&(guild, GUILD_EVENT))
            .map(|state| state.last)
            .unwrap_or(0)
            .max(
                self.stores
                    .scheduled_events
                    .get_guild_scheduled_events_for_guild(guild)
                    .into_iter()
                    .map(|event| event.id)
                    .max()
                    .unwrap_or(0),
            );
        if self
            .stores
            .read_states
            .non_channel_ack(guild, GUILD_EVENT, (latest > 0).then_some(latest), now)
        {
            self.data_revision += 1;
            let entity = if latest > 0 { latest } else { now };
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::POST,
                route: format!("/guilds/{guild}/ack/{GUILD_EVENT}/{entity}"),
                body: Some(json!({})),
            });
        }
    }

    pub(super) fn mark_guilds_read(&mut self, guilds: &[Id], cx: &mut Context<Self>) {
        let reads: Vec<(Id, Id)> = guilds
            .iter()
            .flat_map(|guild| self.guild_unread_reads(*guild))
            .collect();
        self.mark_read_bulk(&reads, cx);
        for &guild in guilds {
            self.ack_guild_events(guild);
        }
    }

    /// Acks every pair here at once, and sends the bulk requests: the first now, the others a second apart.
    pub(super) fn mark_read_bulk(&mut self, reads: &[(Id, Id)], cx: &mut Context<Self>) {
        if reads.is_empty() {
            return;
        }
        for (channel, message) in reads {
            if let Some(state) = self.stores.read_states.states.get_mut(channel) {
                state.manual = false;
            }
            if self.channel == Some(*channel) {
                self.unread_from = None;
            }
            self.stores.read_states.ack_local(*channel, *message, false);
        }
        if self
            .channel
            .is_some_and(|open| reads.iter().any(|(channel, _)| *channel == open))
        {
            self.rebuild_rows();
        }
        self.data_revision += 1;
        let mut requests = bulk_ack_requests(reads).into_iter();
        if let Some(first) = requests.next() {
            let _ = self.requests.send(first);
        }
        let rest: Vec<Request> = requests.collect();
        if rest.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            for request in rest {
                cx.background_executor().timer(CHUNK_GAP).await;
                if this
                    .update(cx, |this, _| {
                        let _ = this.requests.send(request);
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bulk_route_takes_a_hundred_reads_to_a_request() {
        let reads: Vec<(Id, Id)> = (1..=250).map(|n| (n, n * 10)).collect();
        let requests = bulk_ack_requests(&reads);
        assert_eq!(requests.len(), 3);
        let Request::Rest {
            method,
            route,
            body: Some(body),
            ..
        } = &requests[0]
        else {
            panic!("not rest");
        };
        assert_eq!(*method, reqwest::Method::POST);
        assert_eq!(route, "/read-states/ack-bulk");
        let list = body["read_states"].as_array().expect("list");
        assert_eq!(list.len(), 100);
        assert_eq!(
            list[0],
            json!({"channel_id": "1", "message_id": "10", "read_state_type": 0})
        );
        let Request::Rest {
            body: Some(last), ..
        } = &requests[2]
        else {
            panic!("not rest");
        };
        assert_eq!(last["read_states"].as_array().expect("list").len(), 50);
        assert!(bulk_ack_requests(&[]).is_empty());
    }
}
