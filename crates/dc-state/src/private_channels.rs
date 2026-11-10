//! PrivateChannelSortStore/PrivateChannelReadStateStore (645959/131677).
use crate::{PrivateChannel, Stores, read_state::snowflake_at};
use std::cmp::Reverse;

/// Favorites precede the recency-sorted ordinary list; message requests/spam are not in either.
pub fn unread_channels(stores: &Stores) -> Vec<&PrivateChannel> {
    let mut channels: Vec<_> = stores
        .private_channels
        .iter()
        .filter(|channel| {
            matches!(channel.kind, 1 | 3)
                && channel.id != 131
                && channel.flags & 0x4000000 == 0
                && !channel.is_request()
                && stores.read_states.mentions(channel.id) > 0
        })
        .collect();
    channels.sort_by_key(|channel| {
        let favorite = stores
            .guild_settings
            .guilds
            .get(&0)
            .and_then(|settings| settings.channels.get(&channel.id))
            .is_some_and(|settings| settings.flags & 2048 != 0);
        let latest = stores
            .read_states
            .latest
            .get(&channel.id)
            .copied()
            .filter(|id| *id != 0)
            .unwrap_or(if channel.last_message_id == 0 {
                channel.id
            } else {
                channel.last_message_id
            });
        let request = channel.request_timestamp.map_or(0, |stamp| {
            snowflake_at(stamp.timestamp_millis().max(0) as u64)
        });
        (!favorite, Reverse(latest.max(request) >> 22))
    });
    channels.truncate(20);
    channels
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn ids(stores: &Stores) -> Vec<u64> {
        unread_channels(stores)
            .iter()
            .map(|channel| channel.id)
            .collect()
    }

    #[test]
    fn rail_limits_sorted_mentions_after_request_and_unlisted_filtering() {
        let channels: Vec<Value> = (0..26u64)
            .map(|i| {
                json!({
                    "id":(100+i).to_string(),"type":if i==2 {3} else {1},
                    "recipient_ids":["2"],"last_message_id":((i+1)<<22).to_string(),
                    "is_message_request":i==24,"is_spam":i==23,
                    "flags":if i==22 {0x4000000} else {0}
                })
            })
            .collect();
        let reads: Vec<Value> = (0..26u64)
            .map(|i| {
                json!({
                    "id":(100+i).to_string(),"last_message_id":"0","mention_count":u64::from(i!=25)
                })
            })
            .collect();
        let mut stores = Stores::default();
        stores.dispatch("READY",&json!({"user":{"id":"1","username":"self"},
            "private_channels":channels,"read_state":{"entries":reads},
            "user_guild_settings":{"entries":[{"guild_id":null,"channel_overrides":[{"channel_id":"100","flags":2048}]}]}
        }));
        let expected: Vec<_> = std::iter::once(100).chain((103..=121).rev()).collect();
        assert_eq!(ids(&stores), expected);
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id":"100","message_id":(1u64<<22).to_string()}),
        );
        assert_eq!(ids(&stores)[0], 121);
        assert!(!ids(&stores).contains(&100));
        stores.dispatch("MESSAGE_CREATE",&json!({"id":(30u64<<22).to_string(),"channel_id":"125",
            "author":{"id":"2","username":"sender"},"content":"fixture","type":0,"timestamp":"2026-11-10T00:00:00Z"
        }));
        assert_eq!(stores.read_states.mentions(125), 1);
        assert_eq!(ids(&stores)[0], 125);
        assert_eq!(ids(&stores).len(), 20);
    }

    #[test]
    fn recency_uses_live_messages_request_timestamp_and_channel_id_fallback() {
        let mut stores = Stores::default();
        stores.dispatch("READY",&json!({"user":{"id":"1","username":"self"},
            "private_channels":[
                {"id":"200","type":1,"last_message_id":(5u64<<22).to_string()},
                {"id":(8u64<<22).to_string(),"type":1},
                {"id":"400","type":3,"last_message_id":(2u64<<22).to_string(),"is_message_request_timestamp":"2015-01-01T00:00:00.010Z"}
            ],"read_state":{"entries":[
                {"id":"200","mention_count":1},{"id":(8u64<<22).to_string(),"mention_count":1},{"id":"400","mention_count":1}
            ]}
        }));
        assert_eq!(ids(&stores), [400, 8 << 22, 200]);
        stores.dispatch(
            "CHANNEL_UPDATES",
            &json!({"channels":[{"id":"200","last_message_id":(11u64<<22).to_string()}]}),
        );
        assert_eq!(ids(&stores), [200, 400, 8 << 22]);
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id":"200","last_message_id":(3u64<<22).to_string()}),
        );
        assert_eq!(ids(&stores)[0], 200);
    }

    #[test]
    fn partial_private_channel_updates_preserve_fields_and_separate_request_flags() {
        let mut stores = Stores::default();
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id":"10","type":3,"recipient_ids":["2","3"],
                "name":"before","icon":"hash","is_message_request":true,"is_spam":true,
                "is_message_request_timestamp":"2026-11-10T00:00:00Z"
            }),
        );
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id":"10","icon":null,"is_spam":false}),
        );
        let channel = &stores.private_channels[0];
        assert_eq!(channel.name.as_deref(), Some("before"));
        assert_eq!(channel.recipients, [2, 3]);
        assert!(channel.icon.is_none());
        assert!(channel.message_request);
        assert!(!channel.spam);
        assert!(channel.is_request());
        stores.dispatch("CHANNEL_UPDATES",&json!({"channels":[{"id":"10","name":"after","is_message_request":false,"is_message_request_timestamp":null}]}));
        for _ in 0..2 {
            stores.dispatch(
                "CHANNEL_RECIPIENT_ADD",
                &json!({"channel_id":"10","user":{"id":"4","username":"new"}}),
            );
        }
        stores.dispatch(
            "CHANNEL_RECIPIENT_REMOVE",
            &json!({"channel_id":"10","user":{"id":"2"}}),
        );
        let channel = &stores.private_channels[0];
        assert_eq!(channel.name.as_deref(), Some("after"));
        assert_eq!(channel.recipients, [3, 4]);
        assert!(!channel.is_request());
        assert!(channel.request_timestamp.is_none());
        stores.dispatch("CHANNEL_DELETE", &json!({"id":"10"}));
        assert!(stores.private_channels.is_empty());
    }
}
