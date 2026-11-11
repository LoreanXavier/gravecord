//! MessageRequestStore (380335), SpamMessageRequestStore (157550) and MessageRequestPreviewStore (401565).
//!
//! The two request stores are sets of private channel ids kept in step with the channel flags: a message request is
//! `is_message_request && !is_spam`, a spam request is `is_spam`. Here both sets are read off
//! [`Stores::private_channels`], whose flags `CHANNEL_CREATE`, `CHANNEL_UPDATE(S)` and `CHANNEL_DELETE` already
//! maintain; this module keeps what a set cannot derive: the optimistic accepts, the previews and the ready flag.
use crate::{Id, Message, PrivateChannel, Stores, id};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `getMessageRequestPreview`: what the row of a request shows under the name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub loaded: bool,
    pub error: bool,
    pub message: Option<Message>,
}

#[derive(Default)]
pub struct MessageRequests {
    /// `isReady`: set by `CONNECTION_OPEN` (READY) and `CONNECTION_OPEN_SUPPLEMENTAL`.
    ready: bool,
    /// `isAcceptedOptimistic` of MessageRequestStore (`s` in 380335).
    accepted_requests: HashSet<Id>,
    /// `isAcceptedOptimistic` of SpamMessageRequestStore (`s` in 157550).
    accepted_spam: HashSet<Id>,
    previews: HashMap<Id, Preview>,
    /// `d` in 401565: channels whose preview needs no request (they arrived by gateway event).
    no_load: HashSet<Id>,
}

fn is_message_request(channel: &PrivateChannel) -> bool {
    channel.message_request && !channel.spam
}

impl MessageRequests {
    /// Runs after the channel lists took the event, as the stores wait for ChannelStore.
    pub(crate) fn apply(&mut self, name: &str, data: &Value, channels: &[PrivateChannel]) {
        match name {
            "READY" | "READY_SUPPLEMENTAL" => {
                self.ready = true;
                self.accepted_requests.clear();
                self.accepted_spam.clear();
                if name == "READY" {
                    self.previews.clear();
                    self.no_load.clear();
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
                if id(&data["guild_id"]).is_none() {
                    self.channel_changed(name == "CHANNEL_CREATE", data, channels);
                }
            }
            "CHANNEL_DELETE" => {
                if let Some(channel) = id(&data["id"]) {
                    self.accepted_requests.remove(&channel);
                    self.accepted_spam.remove(&channel);
                    self.no_load.remove(&channel);
                    self.previews.remove(&channel);
                }
            }
            "MESSAGE_CREATE" => {
                if let Some(message) = Message::from_value(data)
                    && is_request_channel(channels, message.channel_id)
                {
                    self.previews.insert(
                        message.channel_id,
                        Preview {
                            loaded: true,
                            error: false,
                            message: Some(message),
                        },
                    );
                }
            }
            "MESSAGE_UPDATE" => {
                let Some(channel) = id(&data["channel_id"]) else {
                    return;
                };
                if let Some(message) = self
                    .previews
                    .get_mut(&channel)
                    .and_then(|preview| preview.message.as_mut())
                {
                    if let Some(content) = data["content"].as_str() {
                        message.content = content.to_owned();
                        message.edited = data["edited_timestamp"].is_string();
                    }
                    if let Some(embeds) = data["embeds"].as_array() {
                        message.embeds = embeds.iter().map(crate::Embed::from_value).collect();
                    }
                }
            }
            "MESSAGE_DELETE" => {
                if let Some(channel) = id(&data["channel_id"])
                    && is_request_channel(channels, channel)
                {
                    self.previews.insert(
                        channel,
                        Preview {
                            loaded: true,
                            error: false,
                            message: None,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    fn channel_changed(&mut self, created: bool, data: &Value, channels: &[PrivateChannel]) {
        let Some(channel) = id(&data["id"]) else {
            return;
        };
        let Some(known) = channels.iter().find(|known| known.id == channel) else {
            return;
        };
        // 380335 `d` and 157550 `o`: an accepted request that is no longer in its set drops its optimistic mark.
        if !is_message_request(known) {
            self.accepted_requests.remove(&channel);
        }
        if !known.spam {
            self.accepted_spam.remove(&channel);
        }
        if created {
            if known.is_request() {
                self.no_load.insert(channel);
            }
        } else if !known.is_request() {
            self.no_load.remove(&channel);
            self.previews.remove(&channel);
        }
    }

    /// `MESSAGE_REQUEST_ACCEPT_OPTIMISTIC`: the accept request succeeded, the channel update has not arrived yet.
    pub fn accept_optimistic(&mut self, channel: Id) {
        self.accepted_requests.insert(channel);
        self.accepted_spam.insert(channel);
    }

    /// `LOAD_MESSAGE_REQUESTS_SUPPLEMENTAL_DATA_SUCCESS`: `body` is the array of `{channel_id, message_preview}`;
    /// a requested channel the answer leaves out is loaded with no message.
    pub fn supplemental_success(
        &mut self,
        requested: &[Id],
        body: &Value,
        channels: &[PrivateChannel],
    ) {
        let mut missing: HashSet<Id> = requested.iter().copied().collect();
        for entry in body.as_array().into_iter().flatten() {
            let Some(channel) = id(&entry["channel_id"]) else {
                continue;
            };
            let message = Message::from_value(&entry["message_preview"]);
            self.set_preview(channel, message, false, channels);
            missing.remove(&channel);
        }
        for channel in missing {
            self.set_preview(channel, None, false, channels);
        }
    }

    /// `LOAD_MESSAGE_REQUESTS_SUPPLEMENTAL_DATA_ERROR`.
    pub fn supplemental_error(&mut self, requested: &[Id], channels: &[PrivateChannel]) {
        for channel in requested {
            self.set_preview(*channel, None, true, channels);
        }
    }

    /// `h` in 401565: only a request or spam channel keeps a preview.
    fn set_preview(
        &mut self,
        channel: Id,
        message: Option<Message>,
        error: bool,
        channels: &[PrivateChannel],
    ) {
        if !is_request_channel(channels, channel) {
            return;
        }
        self.previews.insert(
            channel,
            Preview {
                loaded: true,
                error,
                message,
            },
        );
    }
}

fn is_request_channel(channels: &[PrivateChannel], channel: Id) -> bool {
    channels
        .iter()
        .any(|known| known.id == channel && known.is_request())
}

/// 790499/411976 order: newest `lastMessageId` first (`eS` in 793706).
fn newest_first(channels: &mut [&PrivateChannel]) {
    channels.sort_by_key(|channel| std::cmp::Reverse((channel.last_message_id, channel.id)));
}

impl Stores {
    /// `MessageRequestStore.getMessageRequestChannelIds`, in the list's order (`eS` in 793706).
    pub fn message_request_ids(&self) -> Vec<Id> {
        let mut channels: Vec<&PrivateChannel> = self
            .private_channels
            .iter()
            .filter(|channel| is_message_request(channel))
            .collect();
        newest_first(&mut channels);
        channels.iter().map(|channel| channel.id).collect()
    }

    /// `SpamMessageRequestStore.getSpamChannelIds`, in the list's order.
    pub fn spam_request_ids(&self) -> Vec<Id> {
        let mut channels: Vec<&PrivateChannel> = self
            .private_channels
            .iter()
            .filter(|channel| channel.spam)
            .collect();
        newest_first(&mut channels);
        channels.iter().map(|channel| channel.id).collect()
    }

    /// `getMessageRequestsCount`: spam requests are not counted here.
    pub fn message_requests_count(&self) -> usize {
        self.private_channels
            .iter()
            .filter(|channel| is_message_request(channel))
            .count()
    }

    /// `getSpamChannelsCount`.
    pub fn spam_requests_count(&self) -> usize {
        self.private_channels
            .iter()
            .filter(|channel| channel.spam)
            .count()
    }

    /// `isMessageRequest` of MessageRequestStore (a spam channel is not one).
    pub fn is_message_request(&self, channel: Id) -> bool {
        self.private_channels
            .iter()
            .any(|known| known.id == channel && is_message_request(known))
    }

    /// `isSpam` of SpamMessageRequestStore.
    pub fn is_spam_request(&self, channel: Id) -> bool {
        self.private_channels
            .iter()
            .any(|known| known.id == channel && known.spam)
    }

    /// 309199 `k_`: in neither the DM list nor the unread counts.
    pub fn is_request_or_spam(&self, channel: Id) -> bool {
        is_request_channel(&self.private_channels, channel)
    }

    /// 309199 `SM`: the "Message Requests" row of the DM list shows while either list is not empty.
    pub fn message_requests_entry_visible(&self) -> bool {
        self.message_requests_count() > 0 || self.spam_requests_count() > 0
    }

    /// The private channels a picker offers (quick switcher, forward): the official pickers read the sorted DM list
    /// (645959 PrivateChannelSortStore), which holds neither requests nor spam.
    pub fn pickable_private_channels(&self) -> impl Iterator<Item = &crate::PrivateChannel> {
        self.private_channels
            .iter()
            .filter(|channel| !channel.is_request())
    }

    /// `isReady` of MessageRequestStore.
    pub fn message_requests_ready(&self) -> bool {
        self.message_requests.ready
    }

    /// `isAcceptedOptimistic`.
    pub fn is_accepted_optimistic(&self, channel: Id) -> bool {
        self.message_requests.accepted_requests.contains(&channel)
            || self.message_requests.accepted_spam.contains(&channel)
    }

    /// `getMessageRequestPreview` with the lookup of 978914: while the store has no message, the newest one the
    /// channel's window holds stands in.
    pub fn message_request_preview(&self, channel: Id) -> Preview {
        let stored = self
            .message_requests
            .previews
            .get(&channel)
            .cloned()
            .unwrap_or_default();
        if stored.message.is_none()
            && let Some(latest) = self
                .private_channels
                .iter()
                .find(|known| known.id == channel)
                .map(|known| known.last_message_id)
                .filter(|latest| *latest != 0)
            && let Some(message) = self
                .messages
                .get(&channel)
                .and_then(|list| list.iter().find(|known| known.id == latest))
        {
            return Preview {
                loaded: true,
                error: false,
                message: Some(message.clone()),
            };
        }
        stored
    }

    /// `shouldLoadMessageRequestPreview`.
    pub fn should_load_message_request_preview(&self, channel: Id) -> bool {
        !self.message_requests.no_load.contains(&channel)
    }

    /// Channels of `ids` that still need their preview from `/users/@me/message-requests/supplemental-data`
    /// (978914: not loaded, no message, and not marked as arrived by event).
    pub fn message_request_previews_to_load(&self, ids: &[Id]) -> Vec<Id> {
        ids.iter()
            .copied()
            .filter(|channel| {
                let preview = self.message_request_preview(*channel);
                !preview.loaded
                    && preview.message.is_none()
                    && self.should_load_message_request_preview(*channel)
            })
            .collect()
    }

    /// `MESSAGE_REQUEST_ACCEPT_OPTIMISTIC`.
    pub fn message_request_accept_optimistic(&mut self, channel: Id) {
        self.message_requests.accept_optimistic(channel);
    }

    /// `LOAD_MESSAGE_REQUESTS_SUPPLEMENTAL_DATA_SUCCESS`.
    pub fn message_requests_supplemental_success(&mut self, requested: &[Id], body: &Value) {
        let channels = std::mem::take(&mut self.private_channels);
        self.message_requests
            .supplemental_success(requested, body, &channels);
        self.private_channels = channels;
    }

    /// `LOAD_MESSAGE_REQUESTS_SUPPLEMENTAL_DATA_ERROR`.
    pub fn message_requests_supplemental_error(&mut self, requested: &[Id]) {
        let channels = std::mem::take(&mut self.private_channels);
        self.message_requests
            .supplemental_error(requested, &channels);
        self.private_channels = channels;
    }

    /// Dispatch hook, after the channel lists have taken `name`.
    pub(crate) fn message_requests_dispatch(&mut self, name: &str, data: &Value) {
        let channels = std::mem::take(&mut self.private_channels);
        self.message_requests.apply(name, data, &channels);
        self.private_channels = channels;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(id: u64, last: u64, spam: bool) -> Value {
        json!({"id": id.to_string(), "type": 1, "recipient_ids": ["9"],
            "last_message_id": (last << 22).to_string(),
            "is_message_request": true, "is_spam": spam,
            "is_message_request_timestamp": "2026-11-10T00:00:00Z"})
    }

    fn ready(channels: Vec<Value>) -> Stores {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "1", "username": "me"}, "users": [{"id": "9", "username": "other"}],
                "private_channels": channels, "read_state": {"entries": []}}),
        );
        stores
    }

    fn message(channel: u64, id: u64, content: &str) -> Value {
        json!({"id": (id << 22).to_string(), "channel_id": channel.to_string(), "type": 0,
            "author": {"id": "9", "username": "other"}, "content": content,
            "timestamp": "2026-11-10T00:00:00Z"})
    }

    #[test]
    fn sets_split_requests_from_spam_and_order_newest_first() {
        let stores = ready(vec![
            request(10, 5, false),
            request(11, 9, false),
            request(12, 7, true),
            json!({"id": "13", "type": 1, "recipient_ids": ["9"], "last_message_id": "7"}),
        ]);
        assert_eq!(stores.message_request_ids(), [11, 10]);
        assert_eq!(stores.spam_request_ids(), [12]);
        assert_eq!(stores.message_requests_count(), 2);
        assert_eq!(stores.spam_requests_count(), 1);
        assert!(stores.is_message_request(10));
        assert!(!stores.is_message_request(12));
        assert!(stores.is_spam_request(12));
        assert!(stores.is_request_or_spam(12));
        assert!(!stores.is_request_or_spam(13));
        assert!(stores.message_requests_ready());
        assert!(stores.message_requests_entry_visible());
    }

    #[test]
    fn the_entry_shows_for_spam_alone_and_hides_with_no_requests() {
        let mut stores = ready(vec![request(12, 7, true)]);
        assert!(stores.message_requests_entry_visible());
        assert_eq!(stores.message_requests_count(), 0);
        stores.dispatch("CHANNEL_DELETE", &json!({"id": "12", "type": 1}));
        assert!(!stores.message_requests_entry_visible());
    }

    #[test]
    fn a_request_leaves_the_list_when_accepted_updated_or_deleted() {
        let mut stores = ready(vec![
            request(10, 5, false),
            request(11, 9, false),
            request(12, 7, true),
        ]);
        stores.message_request_accept_optimistic(10);
        // The optimistic mark alone does not remove the row: the list waits for the channel update.
        assert_eq!(stores.message_requests_count(), 2);
        assert!(stores.is_accepted_optimistic(10));
        stores.dispatch(
            "CHANNEL_UPDATES",
            &json!({"channels": [{"id": "10", "type": 1, "is_message_request": false}]}),
        );
        assert_eq!(stores.message_request_ids(), [11]);
        assert!(!stores.is_accepted_optimistic(10));
        // Spam marked as not spam becomes an ordinary conversation, or a plain request if the flag stays.
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id": "12", "type": 1, "is_spam": false}),
        );
        assert_eq!(stores.message_request_ids(), [11, 12]);
        assert_eq!(stores.spam_requests_count(), 0);
        stores.dispatch("CHANNEL_DELETE", &json!({"id": "11", "type": 1}));
        assert_eq!(stores.message_request_ids(), [12]);
    }

    #[test]
    fn pickers_skip_requests_and_spam() {
        let stores = ready(vec![
            request(10, 5, false),
            request(11, 9, true),
            request(12, 7, false),
        ]);
        assert_eq!(stores.pickable_private_channels().count(), 0);
        let mut stores = stores;
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id": "11", "type": 1, "is_spam": false, "is_message_request": false}),
        );
        let ids: Vec<_> = stores.pickable_private_channels().map(|c| c.id).collect();
        assert_eq!(ids, [11]);
    }

    #[test]
    fn a_new_request_arrives_by_channel_create_and_counts() {
        let mut stores = ready(vec![]);
        assert!(!stores.message_requests_entry_visible());
        stores.dispatch("CHANNEL_CREATE", &request(20, 3, false));
        assert_eq!(stores.message_requests_count(), 1);
        assert!(stores.message_requests_entry_visible());
        assert!(!stores.should_load_message_request_preview(20));
        stores.dispatch("MESSAGE_CREATE", &message(20, 3, "hello"));
        let preview = stores.message_request_preview(20);
        assert!(preview.loaded);
        assert_eq!(preview.message.as_ref().unwrap().content, "hello");
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": (3u64 << 22).to_string(), "channel_id": "20", "content": "hello!",
                "edited_timestamp": "2026-11-10T00:01:00Z"}),
        );
        assert_eq!(
            stores.message_request_preview(20).message.unwrap().content,
            "hello!"
        );
        stores.dispatch(
            "MESSAGE_DELETE",
            &json!({"id": (3u64 << 22).to_string(), "channel_id": "20"}),
        );
        assert!(stores.message_request_preview(20).message.is_none());
        assert!(stores.message_request_preview(20).loaded);
    }

    #[test]
    fn previews_load_from_supplemental_data_and_errors_are_kept() {
        let mut stores = ready(vec![request(10, 5, false), request(11, 9, false)]);
        assert_eq!(stores.message_request_previews_to_load(&[10, 11]), [10, 11]);
        stores.message_requests_supplemental_success(
            &[10, 11],
            &json!([{"channel_id": "10", "message_preview": message(10, 5, "psst")}]),
        );
        assert_eq!(
            stores.message_request_preview(10).message.unwrap().content,
            "psst"
        );
        // Left out of the answer: loaded, with no message.
        let eleven = stores.message_request_preview(11);
        assert!(eleven.loaded && eleven.message.is_none() && !eleven.error);
        assert!(
            stores
                .message_request_previews_to_load(&[10, 11])
                .is_empty()
        );
        stores.message_requests_supplemental_error(&[10]);
        let ten = stores.message_request_preview(10);
        assert!(ten.error && ten.message.is_none());
        // A channel that is not a request keeps no preview.
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id": "10", "type": 1, "is_message_request": false}),
        );
        assert_eq!(stores.message_request_preview(10), Preview::default());
    }

    #[test]
    fn requests_never_count_as_unread_mentions() {
        let mut stores = ready(vec![request(10, 5, false), request(12, 7, true)]);
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({"id": (8u64 << 22).to_string(), "channel_id": "10", "type": 0,
                "author": {"id": "9", "username": "other"}, "content": "hi",
                "timestamp": "2026-11-10T00:00:00Z"}),
        );
        let counts = crate::unread::total_counts(&stores, chrono::Utc::now());
        assert_eq!(counts.mentions, 0);
        assert!(crate::private_channels::unread_channels(&stores).is_empty());
        stores.dispatch(
            "CHANNEL_UPDATE",
            &json!({"id": "10", "type": 1, "is_message_request": false}),
        );
        let counts = crate::unread::total_counts(&stores, chrono::Utc::now());
        assert_eq!(counts.mentions, 1);
    }
}
