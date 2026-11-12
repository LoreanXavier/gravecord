//! ChannelMemberCountStore (module 596339): the online and total member counts the gateway reports for the channel
//! whose member list is open.
//!
//! `CHANNEL_MEMBER_COUNT_UPDATE {guild_id, channel_id, presence_count, member_count}` (module 225123 maps them to
//! `online` and `total`) is stored when either is present. `requestCount(guild, channel)` remembers the channel and
//! asks the socket (op 36 `REQUEST_CHANNEL_MEMBER_COUNT`); `CONNECTION_OPEN` asks again for the remembered channel and
//! forgets the counts of channels the client no longer has.

use crate::{Id, Stores, id};
use serde_json::Value;
use std::collections::HashMap;

/// `{online, total}`; both null for a channel with no count (`Mo`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemberCount {
    pub online: Option<u64>,
    pub total: Option<u64>,
}

#[derive(Default)]
pub struct ChannelMemberCounts {
    counts: HashMap<Id, MemberCount>,
    /// The channel `requestCount` was last called for (`Mu`).
    requested: Option<(Id, Id)>,
    /// Set by `CONNECTION_OPEN` when `requested` has to be asked for again.
    resend: Option<(Id, Id)>,
    pub version: u64,
}

impl ChannelMemberCounts {
    /// `getMemberCount`.
    pub fn get_member_count(&self, channel: Id) -> MemberCount {
        self.counts.get(&channel).copied().unwrap_or_default()
    }

    /// `requestCount`: remembers the channel; the caller sends the request for it.
    pub fn request_count(&mut self, guild: Id, channel: Id) {
        self.requested = Some((guild, channel));
    }

    /// The `(guild, channel)` the socket has to be asked for again after a new connection.
    pub fn take_resend(&mut self) -> Option<(Id, Id)> {
        self.resend.take()
    }
}

impl Stores {
    fn channel_known(&self, channel: Id) -> bool {
        self.private_channels
            .iter()
            .any(|known| known.id == channel)
            || self.guilds.iter().any(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
    }

    pub(crate) fn channel_member_counts_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                let known: Vec<Id> = self
                    .channel_member_counts
                    .counts
                    .keys()
                    .copied()
                    .filter(|channel| self.channel_known(*channel))
                    .collect();
                let store = &mut self.channel_member_counts;
                store.counts.retain(|channel, _| known.contains(channel));
                store.resend = store.requested;
                store.version += 1;
            }
            "CHANNEL_MEMBER_COUNT_UPDATE" => {
                let Some(channel) = id(&data["channel_id"]) else {
                    return;
                };
                let online = data["presence_count"].as_u64();
                let total = data["member_count"].as_u64();
                if online.is_some() || total.is_some() {
                    self.channel_member_counts
                        .counts
                        .insert(channel, MemberCount { online, total });
                    self.channel_member_counts.version += 1;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_count_update_is_stored_and_an_empty_one_is_not() {
        let mut stores = Stores::default();
        assert_eq!(
            stores.channel_member_counts.get_member_count(50),
            MemberCount::default()
        );
        stores.dispatch(
            "CHANNEL_MEMBER_COUNT_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50", "member_count": 120, "presence_count": 31}),
        );
        assert_eq!(
            stores.channel_member_counts.get_member_count(50),
            MemberCount {
                online: Some(31),
                total: Some(120)
            }
        );
        // Neither number present: the stored count stays.
        stores.dispatch(
            "CHANNEL_MEMBER_COUNT_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50"}),
        );
        assert_eq!(
            stores.channel_member_counts.get_member_count(50).total,
            Some(120)
        );
        // One number alone replaces both.
        stores.dispatch(
            "CHANNEL_MEMBER_COUNT_UPDATE",
            &json!({"guild_id": "5", "channel_id": "50", "member_count": 121}),
        );
        assert_eq!(
            stores.channel_member_counts.get_member_count(50),
            MemberCount {
                online: None,
                total: Some(121)
            }
        );
    }

    #[test]
    fn a_new_connection_asks_again_and_forgets_unknown_channels() {
        let mut stores = Stores::default();
        stores.channel_member_counts.request_count(5, 50);
        stores.dispatch(
            "CHANNEL_MEMBER_COUNT_UPDATE",
            &json!({"channel_id": "50", "member_count": 3, "presence_count": 1}),
        );
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me", "discriminator": "0"},
                "guilds": [{"id": "5", "name": "g", "channels": [{"id": "51", "type": 0, "name": "x"}]}],
                "private_channels": [],
            }),
        );
        assert_eq!(stores.channel_member_counts.take_resend(), Some((5, 50)));
        assert_eq!(stores.channel_member_counts.take_resend(), None);
        // Channel 50 is not in the new READY.
        assert_eq!(
            stores.channel_member_counts.get_member_count(50),
            MemberCount::default()
        );
    }
}
