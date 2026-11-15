//! The messages this account is sending: the row that shows while the request runs, the failed row after it, and the
//! notices Clyde adds.
//!
//! * Module 138399 `_sendMessage`: `createMessage` (963852) gives the message its nonce as id and the state
//!   `SENDING`, it is dispatched as `MESSAGE_CREATE` with `optimistic`, and the request waits in the message queue
//!   (870801, one at a time). A failure dispatches `MESSAGE_SEND_FAILED` (596344: state `SEND_FAILED`) and fails every
//!   message still waiting in that channel (`cancelPendingSendRequests`).
//! * Module 596344 and 755364 (`receiveMessage`): the real message, which carries the same nonce, replaces the local
//!   row in place.
//! * Module 74096: `isSending` (opacity .5, not for a command) and `isFailed` (`--text-feedback-critical` for the text,
//!   links and inline code) sit on the message content. Module 891522: a sending message has no hover bar; a failed
//!   one has Retry and Delete only. Module 519222: Retry and "Resend message" delete the local row and send again with
//!   the same nonce, Delete on a failed message is local and asks nothing.

use super::tooltip::{Side, Tip};
use super::{Root, color, icon, motion};
use crate::net::{Request, Upload};
use crate::send_policy::{self, ChannelFacts, Failure, Followup, Outcome};
use dc_state::{Id, Message};
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

/// Discord's epoch, for the time a snowflake carries.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
/// `EphemeralMessageStore` (module 931480) keeps this many per channel.
const EPHEMERAL_KEPT: usize = 50;
/// `EPHEMERAL` of the message flags.
const FLAG_EPHEMERAL: u64 = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SendState {
    Sending,
    Failed,
}

/// A message of this account that the server has not confirmed.
#[derive(Clone, Debug)]
pub(super) struct LocalSend {
    pub channel: Id,
    pub nonce: String,
    pub state: SendState,
    pub content: String,
    pub files: Vec<Upload>,
    pub reply: Option<(Option<Id>, Id, bool)>,
    pub sticker_ids: Vec<Id>,
    /// A forward: the guild (if any), channel and id of the message it points at (`message_reference` type 1).
    pub forward: Option<(Option<Id>, Id, Id)>,
    /// The row, kept so a reloaded window can show it again.
    pub message: Option<Message>,
}

/// What a message to send is made of.
#[derive(Default)]
struct Parts {
    content: String,
    files: Vec<Upload>,
    reply: Option<(Option<Id>, Id, bool)>,
    sticker_ids: Vec<Id>,
    forward: Option<(Option<Id>, Id, Id)>,
}

#[derive(Default)]
pub(super) struct State {
    sends: HashMap<Id, LocalSend>,
    /// `nonce -> local id`, kept until the request ends so an early gateway echo does not orphan `in_flight`.
    nonce_ids: HashMap<String, Id>,
    /// The messages waiting for the queue, oldest first (module 870801 sends one at a time).
    queue: VecDeque<Id>,
    in_flight: Option<(Id, String)>,
    last_local_id: Id,
    /// Harness: nothing is posted, so a row stays in the sending state until a step ends it.
    pub hold: bool,
    /// The notices Clyde added, per channel.
    notices: HashMap<Id, Vec<Message>>,
    /// The file a sending message is uploading, with the bytes sent and its size (the placeholder's bar).
    pub progress: HashMap<Id, (usize, u64, u64)>,
}

impl State {
    pub fn state_of(&self, id: Id) -> Option<SendState> {
        self.sends.get(&id).map(|send| send.state)
    }

    fn in_flight_id(&self) -> Option<Id> {
        self.in_flight.as_ref().map(|(id, _)| *id)
    }

    pub(super) fn next_local_id(&mut self, nonce_id: Id, channel_latest_id: Id) -> Id {
        let next = nonce_id
            .max(channel_latest_id.saturating_add(1))
            .max(self.last_local_id.saturating_add(1));
        self.last_local_id = next;
        next
    }

    fn begin(&mut self, id: Id, send: LocalSend) {
        self.nonce_ids.insert(send.nonce.clone(), id);
        self.sends.insert(id, send);
        self.queue.push_back(id);
    }

    /// The next message to post, when none is on its way.
    fn next(&mut self) -> Option<Id> {
        if self.in_flight.is_some() || self.hold {
            return None;
        }
        while let Some(id) = self.queue.pop_front() {
            if let Some(send) = self.sends.get(&id)
                && send.state == SendState::Sending
            {
                self.in_flight = Some((id, send.nonce.clone()));
                return Some(id);
            }
        }
        None
    }

    /// The request of `id` ended: the queue goes on.
    fn ended(&mut self, id: Id) {
        if self.in_flight_id() == Some(id) {
            self.in_flight = None;
        }
        self.queue.retain(|known| *known != id);
        self.progress.remove(&id);
        if !self.sends.contains_key(&id) {
            self.nonce_ids.retain(|_, mapped| *mapped != id);
        }
    }

    /// Clears `in_flight` by local id or request nonce so an early gateway echo cannot stall the queue.
    fn ended_for(&mut self, id: Option<Id>, nonce: &str) {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|(flying_id, flying_nonce)| {
                id == Some(*flying_id) || flying_nonce == nonce
            })
        {
            self.in_flight = None;
        }
        if let Some(id) = id {
            self.ended(id);
        } else {
            self.nonce_ids.remove(nonce);
        }
    }

    fn remove_send(&mut self, id: Id) -> Option<LocalSend> {
        let removed = self.sends.remove(&id);
        if self.in_flight_id() != Some(id)
            && let Some(ref send) = removed
            && self.nonce_ids.get(&send.nonce).copied() == Some(id)
        {
            self.nonce_ids.remove(&send.nonce);
        }
        removed
    }

    /// `cancelPendingSendRequests(channel)`: what still waits in that channel, which now fails with the message.
    fn cancel_waiting(&mut self, channel: Id) -> Vec<Id> {
        let waiting: Vec<Id> = self
            .queue
            .iter()
            .copied()
            .filter(|id| {
                self.sends
                    .get(id)
                    .is_some_and(|send| send.channel == channel)
            })
            .collect();
        self.queue.retain(|id| !waiting.contains(id));
        waiting
    }

    fn id_of_nonce(&self, nonce: &str) -> Option<Id> {
        if let Some((id, flying_nonce)) = &self.in_flight
            && flying_nonce == nonce
        {
            return Some(*id);
        }
        if let Some(&id) = self.nonce_ids.get(nonce) {
            return Some(id);
        }
        self.sends
            .iter()
            .find(|(_, send)| send.nonce == nonce)
            .map(|(id, _)| *id)
    }

    fn remember_notice(&mut self, message: Message) {
        let list = self.notices.entry(message.channel_id).or_default();
        list.push(message);
        while list.len() > EPHEMERAL_KEPT {
            list.remove(0);
        }
    }

    pub fn forget_notice(&mut self, id: Id) {
        for list in self.notices.values_mut() {
            list.retain(|known| known.id != id);
        }
    }

    /// The oldest message still sending, for the harness.
    #[cfg(any(test, feature = "capture"))]
    fn first_sending(&self) -> Option<Id> {
        self.queue
            .iter()
            .copied()
            .chain(self.in_flight_id())
            .find(|id| {
                self.sends
                    .get(id)
                    .is_some_and(|send| send.state == SendState::Sending)
            })
    }
}

/// The nonce `createMessage` stamps is a snowflake, and the message takes it as its id.
pub(super) fn id_of(nonce: &str) -> Option<Id> {
    nonce.parse().ok().filter(|id| *id > 0)
}

fn json_str(value: &Value) -> Option<std::borrow::Cow<'_, str>> {
    if let Some(s) = value.as_str() {
        return Some(std::borrow::Cow::Borrowed(s));
    }
    if let Some(n) = value.as_u64() {
        return Some(std::borrow::Cow::Owned(n.to_string()));
    }
    value
        .as_i64()
        .filter(|n| *n > 0)
        .map(|n| std::borrow::Cow::Owned(n.to_string()))
}

fn json_id(value: &Value) -> Option<Id> {
    json_str(value).and_then(|s| id_of(&s))
}

/// `createMessage` (module 963852) as the JSON the store reads.
fn local_message_value(
    id: Id,
    nonce: &str,
    channel: Id,
    content: &str,
    author: Value,
    reply: Option<(Option<Id>, Id, bool)>,
    now: chrono::DateTime<chrono::Utc>,
) -> Value {
    let mut value = json!({
        "id": id.to_string(),
        "type": if reply.is_some() { 19 } else { 0 },
        "content": content,
        "channel_id": channel.to_string(),
        "author": author,
        "attachments": [],
        "embeds": [],
        "pinned": false,
        "mentions": [],
        "mention_channels": [],
        "mention_roles": [],
        "mention_everyone": false,
        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
        "tts": false,
        "nonce": nonce,
    });
    if let Some((guild, message, _)) = reply {
        let mut reference =
            json!({"channel_id": channel.to_string(), "message_id": message.to_string()});
        if let Some(guild) = guild {
            reference["guild_id"] = json!(guild.to_string());
        }
        value["message_reference"] = reference;
    }
    value
}

/// `pO` of module 963852: the private message Clyde shows.
fn notice_value(
    id: Id,
    channel: Id,
    text: &str,
    name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Value {
    json!({
        "id": id.to_string(),
        "type": 0,
        "flags": FLAG_EPHEMERAL,
        "content": text,
        "channel_id": channel.to_string(),
        "author": {"id": "1", "username": "Clyde", "discriminator": "0000", "avatar": "clyde", "bot": true, "public_flags": 65536},
        "attachments": [],
        "embeds": [],
        "pinned": false,
        "mentions": [],
        "mention_channels": [],
        "mention_roles": [],
        "mention_everyone": false,
        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
        "tts": false,
        "loggingName": name,
    })
}

impl Root {
    /// What the row of this message is: sending, failed, or neither.
    pub(super) fn send_state_of(&self, message: Id) -> Option<SendState> {
        self.send_ui.state_of(message)
    }

    /// The message this account is sending (or that failed) under this id.
    pub(super) fn send_ui_send(&self, id: Id) -> Option<&LocalSend> {
        self.send_ui.sends.get(&id)
    }

    /// The file being uploaded for the message, with the bytes sent and its size.
    pub(super) fn send_ui_progress(&self, id: Id) -> Option<(usize, u64, u64)> {
        self.send_ui.progress.get(&id).copied()
    }

    fn me_json(&self) -> Value {
        match &self.stores.current_user {
            Some(me) => json!({
                "id": me.id.to_string(),
                "username": me.username,
                "avatar": me.avatar,
                "global_name": me.global_name,
                "discriminator": me.discriminator,
            }),
            None => json!({"id": "0", "username": ""}),
        }
    }

    fn insert_local(&mut self, message: Message) {
        self.stores.receive_message(message);
    }

    fn remove_local(&mut self, channel: Id, id: Id) {
        self.stores.dispatch(
            "MESSAGE_DELETE",
            &json!({"id": id.to_string(), "channel_id": channel.to_string()}),
        );
    }

    /// The composer's send: the row shows at once, the request goes out when the queue has its turn.
    pub(super) fn start_local_send(
        &mut self,
        channel: Id,
        content: String,
        files: Vec<Upload>,
        reply: Option<(Option<Id>, Id, bool)>,
        sticker_ids: Vec<Id>,
    ) {
        let nonce = crate::net::nonce();
        let parts = Parts {
            content,
            files,
            reply,
            sticker_ids,
            forward: None,
        };
        self.send_with_nonce(channel, nonce, parts);
    }

    /// A message of text alone that goes through the queue: a GIF's link, a profile or invite link.
    pub(super) fn start_local_text(&mut self, channel: Id, content: String) {
        self.start_local_send(channel, content, Vec::new(), None, Vec::new());
    }

    /// A message of one sticker (the picker's, answering the reply bar).
    pub(super) fn start_local_sticker(
        &mut self,
        channel: Id,
        reply: Option<(Option<Id>, Id, bool)>,
        sticker: Id,
    ) {
        self.start_local_send(channel, String::new(), Vec::new(), reply, vec![sticker]);
    }

    /// A forward of `message` (of `source_channel`, in `guild` if any) into `channel`: an empty message that points at
    /// the source with `message_reference` type 1.
    pub(super) fn start_local_forward(&mut self, channel: Id, source: (Option<Id>, Id, Id)) {
        let parts = Parts {
            forward: Some(source),
            ..Parts::default()
        };
        self.send_with_nonce(channel, crate::net::nonce(), parts);
    }

    fn send_with_nonce(&mut self, channel: Id, nonce: String, parts: Parts) {
        let Parts {
            content,
            files,
            reply,
            sticker_ids,
            forward,
        } = parts;
        let Some(nonce_id) = id_of(&nonce) else {
            return;
        };
        let (msg_latest, max_ts) = self
            .stores
            .messages
            .get(&channel)
            .map_or((0, None), |list| {
                let latest = list.iter().map(|m| m.id).max().unwrap_or(0);
                let ts = list.iter().map(|m| m.timestamp).max();
                (latest, ts)
            });
        let guild_latest = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .find(|known| known.id == channel)
            .and_then(|known| known.last_message_id)
            .unwrap_or(0);
        let dm_latest = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .map(|known| known.last_message_id)
            .unwrap_or(0);
        let channel_latest_id = msg_latest.max(guild_latest).max(dm_latest);
        let id = self.send_ui.next_local_id(nonce_id, channel_latest_id);
        let now = match max_ts {
            Some(ts) => ts.max(chrono::Utc::now()),
            None => chrono::Utc::now(),
        };
        let mut message = Message::from_value(&local_message_value(
            id,
            &nonce,
            channel,
            &content,
            self.me_json(),
            reply,
            now,
        ));
        if let (Some(message), Some((_, replied, _))) = (message.as_mut(), reply) {
            message.referenced = self
                .stores
                .messages
                .get(&channel)
                .and_then(|list| list.iter().find(|known| known.id == replied))
                .cloned()
                .map(Box::new);
        }
        // `sticker_items` of `createMessage`'s caller: the stickers the store knows.
        if let Some(message) = message.as_mut() {
            message.sticker_items = sticker_ids
                .iter()
                .filter_map(|sticker| self.stores.stickers.sticker(*sticker))
                .map(|sticker| (sticker.id, sticker.name.clone(), sticker.format_type))
                .collect();
        }
        // A message of files alone shows the placeholder of its uploads (`upload_progress.rs`).
        let shown = !content.is_empty()
            || !sticker_ids.is_empty()
            || !files.is_empty()
            || forward.is_some();
        if shown && let Some(message) = message.clone() {
            self.insert_local(message);
        }
        self.send_ui.begin(
            id,
            LocalSend {
                channel,
                nonce,
                state: SendState::Sending,
                content,
                files,
                reply,
                sticker_ids,
                forward,
                message,
            },
        );
        self.send_pump();
        self.rebuild_rows();
        if self.channel == Some(channel) {
            self.list.scroll_to_end();
        }
    }

    /// Posts the next message the queue holds.
    fn send_pump(&mut self) {
        let Some(id) = self.send_ui.next() else {
            return;
        };
        let Some(send) = self.send_ui.sends.get(&id) else {
            return;
        };
        let _ = self.requests.send(Request::SendLocal {
            nonce: send.nonce.clone(),
            channel: send.channel,
            content: send.content.clone(),
            files: send.files.clone(),
            reply: send.reply,
            sticker_ids: send.sticker_ids.clone(),
            forward: send.forward,
        });
    }

    /// The end of a request (`Update::SendResult`).
    pub(super) fn send_finished(
        &mut self,
        channel: Id,
        nonce: &str,
        outcome: Outcome,
        cx: &mut Context<Self>,
    ) {
        let resolved = self.send_ui.id_of_nonce(nonce).or_else(|| id_of(nonce));
        self.send_ui.ended_for(resolved, nonce);
        let Some(id) = resolved else {
            self.send_pump();
            return;
        };
        match outcome {
            Outcome::Sent(body) => {
                // `SLOWMODE_RESET_COOLDOWN` follows a message that went through.
                self.set_slowmode(channel, dc_state::slowmode::Kind::SendMessage, None, cx);
                if self.send_ui.sends.contains_key(&id) {
                    let real_id = json_id(&body["id"]);
                    // `receiveMessage(channel, response, optimistic)`: the same path a gateway event takes.
                    self.apply_update(
                        crate::net::Update::Dispatch {
                            name: "MESSAGE_CREATE".to_owned(),
                            data: body,
                        },
                        cx,
                    );
                    // The echo may have come first and already took the row; this takes it otherwise.
                    if self.send_ui.remove_send(id).is_some() && real_id != Some(id) {
                        self.remove_local(channel, id);
                    }
                }
            }
            Outcome::Failed(failure) => self.send_failed(channel, id, &failure, cx),
            // A cancelled or silently failed upload leaves no row and no failed message.
            Outcome::Dropped => self.drop_local(channel, id),
        }
        self.send_pump();
        self.rebuild_rows();
        cx.notify();
    }

    /// `Update::SendProgress`: the bar of the placeholder row moves.
    pub(super) fn send_progress(
        &mut self,
        nonce: &str,
        index: usize,
        sent: u64,
        total: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.send_ui.id_of_nonce(nonce).or_else(|| id_of(nonce))
            && self.send_ui.state_of(id) == Some(SendState::Sending)
        {
            self.send_ui.progress.insert(id, (index, sent, total));
            cx.notify();
        }
    }

    /// The cancel button of a placeholder row: the upload stops if it runs, or the message leaves the queue.
    pub(super) fn cancel_upload(&mut self, id: Id, cx: &mut Context<Self>) {
        let Some(send) = self.send_ui.sends.get(&id) else {
            return;
        };
        let (channel, nonce) = (send.channel, send.nonce.clone());
        if self.send_ui.in_flight_id() == Some(id) {
            // The task answers with `Outcome::Dropped`.
            crate::upload_flow::cancel(&nonce);
            return;
        }
        self.send_ui.ended(id);
        self.drop_local(channel, id);
        self.send_pump();
        self.rebuild_rows();
        cx.notify();
    }

    fn drop_local(&mut self, channel: Id, id: Id) {
        if self.send_ui.remove_send(id).is_some() {
            self.remove_local(channel, id);
        }
    }

    /// `MESSAGE_SEND_FAILED` and what the failure asks besides (module 138399).
    fn send_failed(&mut self, channel: Id, id: Id, failure: &Failure, cx: &mut Context<Self>) {
        let mut failed = vec![id];
        failed.extend(self.send_ui.cancel_waiting(channel));
        for id in failed {
            self.mark_failed(channel, id);
        }
        // Polls, forwards and inventory sends never get a notice; the composer sends none of them.
        match send_policy::followup(failure, false) {
            Followup::Slowmode { seconds } => self.set_slowmode(
                channel,
                dc_state::slowmode::Kind::SendMessage,
                Some((seconds * 1000.) as u64),
                cx,
            ),
            Followup::Clyde(code) => {
                let notice = send_policy::clyde_notice(code, self.channel_facts(channel));
                self.clyde_says(channel, &notice.text, &notice.name);
            }
            Followup::ExplicitContent => {
                let facts = self.channel_facts(channel);
                let notice =
                    send_policy::explicit_media_notice(facts.is_dm, self.is_group_dm(channel));
                self.clyde_says(channel, &notice.text, &notice.name);
            }
            Followup::Automod | Followup::PowermodeDisabled | Followup::Nothing => {}
        }
    }

    fn mark_failed(&mut self, channel: Id, id: Id) {
        let Some(send) = self.send_ui.sends.get_mut(&id) else {
            return;
        };
        send.state = SendState::Failed;
        let message = send.message.clone();
        // A message that had no row while it was sending gets one now, so it can be retried or deleted.
        let shown = self
            .stores
            .messages
            .get(&channel)
            .is_some_and(|list| list.iter().any(|known| known.id == id));
        if let (false, Some(message)) = (shown, message) {
            self.insert_local(message);
        }
    }

    fn is_group_dm(&self, channel: Id) -> bool {
        self.stores
            .private_channels
            .iter()
            .any(|known| known.id == channel && known.kind == 3)
    }

    /// What the channel looks like to a notice's text (`messageGetter` of module 138399).
    fn channel_facts(&self, channel: Id) -> ChannelFacts {
        let private = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel);
        let guild_channel = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .find(|known| known.id == channel);
        ChannelFacts {
            slowmode_seconds: guild_channel.map_or(0, |known| known.rate_limit_per_user),
            is_dm: private.is_some_and(|known| known.kind == 1),
            is_forum_like: guild_channel
                .is_some_and(|known| matches!(known.kind, dc_state::ChannelKind::Other(15 | 16))),
            dm_with_bot: private.is_some_and(|known| {
                known.kind == 1
                    && known.recipients.len() == 1
                    && self
                        .stores
                        .users
                        .get(&known.recipients[0])
                        .is_some_and(|user| user.bot)
            }),
        }
    }

    /// `sendBotMessage`: a private message from Clyde, kept here only.
    pub(super) fn clyde_says(&mut self, channel: Id, text: &str, name: &str) {
        let now = chrono::Utc::now();
        let id = ((now.timestamp_millis() as u64).saturating_sub(DISCORD_EPOCH_MS)) << 22;
        let Some(message) = Message::from_value(&notice_value(id, channel, text, name, now)) else {
            return;
        };
        let (key, url) = super::avatar_source(&message.author);
        self.request_image(key, url);
        self.send_ui.remember_notice(message.clone());
        self.insert_local(message);
        self.rebuild_rows();
        if self.guild.is_none()
            && self.channel == Some(channel)
            && self.list.is_scrolled_to_end() != Some(false)
        {
            self.list.scroll_to_end();
        }
    }

    /// A reloaded window keeps no local row: the messages still unconfirmed, and Clyde's notices, are put back.
    pub(super) fn reinsert_local_sends(&mut self, channel: Id) {
        let mut again: Vec<Message> = self
            .send_ui
            .sends
            .values()
            .filter(|send| send.channel == channel)
            .filter_map(|send| send.message.clone())
            .filter(|message| send_row_wanted(&self.send_ui, message.id))
            .collect();
        again.extend(
            self.send_ui
                .notices
                .get(&channel)
                .cloned()
                .unwrap_or_default(),
        );
        for message in again {
            let present = self
                .stores
                .messages
                .get(&channel)
                .is_some_and(|list| list.iter().any(|known| known.id == message.id));
            if !present {
                self.insert_local(message);
            }
        }
    }

    /// A gateway message of this account whose nonce is a row here: the real message takes the row's place.
    pub(super) fn send_echo(&mut self, data: &Value) {
        let Some(nonce) = json_str(&data["nonce"]) else {
            return;
        };
        let (Some(id), Some(channel)) = (
            self.send_ui.id_of_nonce(nonce.as_ref()),
            json_id(&data["channel_id"]),
        ) else {
            return;
        };
        let mine = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| json_id(&data["author"]["id"]) == Some(me.id));
        let real_id = json_id(&data["id"]);
        if mine && self.send_ui.remove_send(id).is_some() && real_id != Some(id) {
            self.remove_local(channel, id);
        }
    }

    /// Retry in the hover bar and "Resend message": the row goes and the same message is sent again with its nonce.
    pub(super) fn retry_send(&mut self, id: Id, cx: &mut Context<Self>) {
        let Some(send) = self.send_ui.remove_send(id) else {
            return;
        };
        self.remove_local(send.channel, id);
        let parts = Parts {
            content: send.content,
            files: send.files,
            reply: send.reply,
            sticker_ids: send.sticker_ids,
            forward: send.forward,
        };
        self.send_with_nonce(send.channel, send.nonce, parts);
        cx.notify();
    }

    /// Delete on a failed message: local, with no request and no question (`RC` of module 519222).
    pub(super) fn delete_failed(&mut self, id: Id, cx: &mut Context<Self>) {
        let Some(send) = self.send_ui.remove_send(id) else {
            return;
        };
        self.send_ui.ended(id);
        self.remove_local(send.channel, id);
        self.rebuild_rows();
        cx.notify();
    }

    /// The hover bar of a failed message: Retry and Delete (module 891522 `nf`), in the box of the ordinary bar.
    pub(super) fn failed_toolbar(&self, id: Id, cx: &mut Context<Self>) -> impl IntoElement {
        let button =
            |cx: &mut Context<Self>, name: &'static str, tip: &'static str, key: &'static str| {
                self.with_tip(
                    key,
                    Tip::text(tip, Side::Top),
                    div()
                        .id(SharedString::from(format!("toolbar-button-{name}")))
                        .relative()
                        .size(px(28.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .active(|style| style.bg(color("--interactive-background-active")))
                        .child(motion::messages::icon_box(
                            name,
                            icon(name, 20., "--interactive-text-default"),
                        ))
                        .child(motion::hover_probe(motion::messages::hover_key(name)))
                        .child(motion::messages::press_probe(name)),
                    cx,
                )
                .cursor_pointer()
            };
        div()
            .id(SharedString::from(format!("toolbar-{id}")))
            .occlude()
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                // The bar reaches above its row, so it has to keep the row's hover alive.
                if *inside {
                    this.hovered = Some(id);
                    cx.notify();
                }
            }))
            .absolute()
            .top(px(-25.))
            .right(px(14.))
            .h(px(34.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .child({
                let target = button(cx, "msg_retry", dc_intl::tr("Retry"), "toolbar-retry");
                target.on_click(cx.listener(move |this, _, _, cx| {
                    this.tip = None;
                    this.retry_send(id, cx);
                }))
            })
            .child({
                let target = button(
                    cx,
                    "msg_trash",
                    dc_intl::tr("Delete"),
                    "toolbar-delete-failed",
                );
                target.on_click(cx.listener(move |this, _, _, cx| {
                    this.tip = None;
                    this.delete_failed(id, cx);
                }))
            })
    }

    /// Capture only: `send hold|release|fail [<status> [<code> [<retry after>]]]|state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_send(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["hold"] => {
                self.send_ui.hold = true;
                "send: held, nothing is posted".to_owned()
            }
            ["release"] => {
                self.send_ui.hold = false;
                self.send_pump();
                "send: released".to_owned()
            }
            // The bar of the placeholder row: `send progress <sent> <total>` for the first file of the oldest sending.
            ["progress", sent, total] => {
                let Some(id) = self.send_ui.first_sending() else {
                    return "send: nothing is sending".to_owned();
                };
                let (Ok(sent), Ok(total)) = (sent.parse::<u64>(), total.parse::<u64>()) else {
                    return "send: progress <sent> <total>".to_owned();
                };
                self.send_ui.progress.insert(id, (0, sent, total));
                cx.notify();
                format!("send: progress {sent}/{total}")
            }
            // An upload that ends with nothing to tell: the row goes (design/golden/windows/upload/after-failure).
            ["drop"] => {
                let Some(id) = self.send_ui.first_sending() else {
                    return "send: nothing is sending".to_owned();
                };
                let Some(send) = self.send_ui.sends.get(&id) else {
                    return "send: nothing is sending".to_owned();
                };
                let (channel, nonce) = (send.channel, send.nonce.clone());
                self.send_finished(channel, &nonce, Outcome::Dropped, cx);
                format!("send: dropped {nonce}")
            }
            ["fail", rest @ ..] => {
                let Some(id) = self.send_ui.first_sending() else {
                    return "send: nothing is sending".to_owned();
                };
                let Some(channel) = self.send_ui.sends.get(&id).map(|send| send.channel) else {
                    return "send: nothing is sending".to_owned();
                };
                let number = |at: usize| rest.get(at).and_then(|word| word.parse::<u64>().ok());
                let failure = Failure {
                    status: number(0).map(|status| status as u16),
                    code: number(1),
                    message: None,
                    retry_after: rest.get(2).and_then(|word| word.parse().ok()),
                };
                let nonce = self.send_ui.sends[&id].nonce.clone();
                self.send_finished(channel, &nonce, Outcome::Failed(failure), cx);
                format!("send: failed {nonce}")
            }
            // The notice of design/golden/windows/failed-message/clyde-notice-darker-cozy@1.png.
            ["clyde"] => {
                let Some(channel) = self.channel else {
                    return "send: no channel open".to_owned();
                };
                self.clyde_says(
                    channel,
                    "Hello. Beep. Boop. If you wanna invite friends to this server, click the server name in the top left and select \"Invite to Server\". Beep!",
                    "clyde-notice",
                );
                "send: Clyde notice shown".to_owned()
            }
            ["state"] => {
                let mut lines: Vec<String> = self
                    .send_ui
                    .sends
                    .iter()
                    .map(|(id, send)| format!("{id} {:?} channel {}", send.state, send.channel))
                    .collect();
                lines.sort();
                format!(
                    "send: {} (hold {}, in flight {:?}, queued {})",
                    lines.join("; "),
                    self.send_ui.hold,
                    self.send_ui.in_flight_id(),
                    self.send_ui.queue.len()
                )
            }
            _ => "send: hold | release | fail [status [code [retry]]] | clyde | state".to_owned(),
        }
    }
}

/// A row whose message is a failed or sending message of this account.
fn send_row_wanted(state: &State, id: Id) -> bool {
    state.sends.contains_key(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(channel: Id, nonce: &str) -> LocalSend {
        LocalSend {
            channel,
            nonce: nonce.to_owned(),
            state: SendState::Sending,
            content: "hello".to_owned(),
            files: Vec::new(),
            reply: None,
            sticker_ids: Vec::new(),
            forward: None,
            message: None,
        }
    }

    #[test]
    fn the_nonce_is_the_id() {
        assert_eq!(id_of("1234567890"), Some(1_234_567_890));
        assert_eq!(id_of("x"), None);
        assert_eq!(id_of("0"), None);
    }

    #[test]
    fn the_queue_posts_one_message_at_a_time() {
        let mut state = State::default();
        state.begin(1, send(10, "1"));
        state.begin(2, send(10, "2"));
        assert_eq!(state.next(), Some(1));
        // The first is still on its way.
        assert_eq!(state.next(), None);
        state.ended(1);
        assert_eq!(state.next(), Some(2));
    }

    #[test]
    fn a_held_queue_posts_nothing() {
        let mut state = State {
            hold: true,
            ..State::default()
        };
        state.begin(1, send(10, "1"));
        assert_eq!(state.next(), None);
        assert_eq!(state.first_sending(), Some(1));
        state.hold = false;
        assert_eq!(state.next(), Some(1));
    }

    #[test]
    fn a_failure_cancels_what_waits_in_its_channel_only() {
        let mut state = State::default();
        state.begin(1, send(10, "1"));
        state.begin(2, send(10, "2"));
        state.begin(3, send(20, "3"));
        state.begin(4, send(10, "4"));
        assert_eq!(state.next(), Some(1));
        state.ended(1);
        assert_eq!(state.cancel_waiting(10), vec![2, 4]);
        // The other channel's message still has its turn.
        assert_eq!(state.next(), Some(3));
    }

    #[test]
    fn a_failed_message_is_not_posted_again_by_the_queue() {
        let mut state = State::default();
        state.begin(1, send(10, "1"));
        state.sends.get_mut(&1).unwrap().state = SendState::Failed;
        assert_eq!(state.next(), None);
        assert_eq!(state.state_of(1), Some(SendState::Failed));
        assert_eq!(state.id_of_nonce("1"), Some(1));
    }

    #[test]
    fn the_local_row_is_a_message_of_this_account_with_the_nonce_as_id() {
        let value = local_message_value(
            7 << 22,
            &(7_u64 << 22).to_string(),
            55,
            "a message",
            json!({"id": "9", "username": "me"}),
            Some((Some(3), 44, true)),
            chrono::Utc::now(),
        );
        let message = Message::from_value(&value).expect("a message");
        assert_eq!(message.id, 7 << 22);
        assert_eq!(message.channel_id, 55);
        assert_eq!(message.author.id, 9);
        assert_eq!(message.content, "a message");
        assert_eq!(message.kind, 19);
        assert_eq!(message.reference_id, Some(44));
        let plain = local_message_value(
            1,
            "1",
            2,
            "x",
            json!({"id": "9", "username": "me"}),
            None,
            chrono::Utc::now(),
        );
        assert_eq!(Message::from_value(&plain).expect("a message").kind, 0);
    }

    #[test]
    fn next_local_id_sorts_after_lagging_nonce_and_resolves_nonce() {
        let mut state = State::default();
        let first = state.next_local_id(500, 1000);
        assert!(first > 1000);
        state.begin(first, send(10, "500"));
        assert_eq!(state.id_of_nonce("500"), Some(first));
        let second = state.next_local_id(501, first);
        assert!(second > first);
        assert!(state.next_local_id(5000, second) >= 5000);
    }

    #[test]
    fn echo_before_send_result_with_lagging_nonce_releases_in_flight_and_pumps_queue() {
        let mut state = State::default();
        let first = state.next_local_id(500, 1000);
        assert!(first > 1000);
        assert_ne!(first, 500);
        state.begin(first, send(10, "500"));
        let second = state.next_local_id(501, first);
        state.begin(second, send(10, "501"));
        let third = state.next_local_id(502, second);
        state.begin(third, send(10, "502"));

        assert_eq!(state.next(), Some(first));
        assert_eq!(state.in_flight_id(), Some(first));

        // Simulate gateway MESSAGE_CREATE echo arriving before HTTP SendResult.
        assert!(state.remove_send(first).is_some());
        assert_eq!(state.id_of_nonce("500"), Some(first));

        // Simulate HTTP Update::SendResult resolving the nonce and ending in_flight.
        let resolved = state.id_of_nonce("500").or_else(|| id_of("500"));
        assert_eq!(resolved, Some(first));
        state.ended_for(resolved, "500");
        assert_eq!(state.in_flight_id(), None);
        assert_eq!(state.next(), Some(second));
        state.ended_for(Some(second), "501");
        assert_eq!(state.next(), Some(third));
    }

    #[test]
    fn json_str_and_json_id_accept_string_and_integer_values() {
        assert_eq!(json_str(&json!("12345")).as_deref(), Some("12345"));
        assert_eq!(json_str(&json!(12345)).as_deref(), Some("12345"));
        assert_eq!(json_str(&json!(0)).as_deref(), Some("0"));
        assert_eq!(json_str(&json!(null)), None);
        assert_eq!(json_id(&json!("99")), Some(99));
        assert_eq!(json_id(&json!(99)), Some(99));
        assert_eq!(json_id(&json!(0)), None);
        assert_eq!(json_id(&json!(-5)), None);
    }

    #[test]
    fn a_notice_is_private_and_comes_from_clyde() {
        let value = notice_value(9 << 22, 5, "text", "NAME", chrono::Utc::now());
        let message = Message::from_value(&value).expect("a message");
        assert_eq!(message.author.id, 1);
        assert_eq!(message.author.username, "Clyde");
        assert!(message.author.bot);
        assert_eq!(message.flags & FLAG_EPHEMERAL, FLAG_EPHEMERAL);
        assert_eq!(message.content, "text");
    }

    #[test]
    fn only_fifty_notices_are_kept_per_channel() {
        let mut state = State::default();
        for index in 0..60_u64 {
            let value = notice_value(index + 1, 5, "t", "N", chrono::Utc::now());
            state.remember_notice(Message::from_value(&value).expect("a message"));
        }
        assert_eq!(state.notices[&5].len(), EPHEMERAL_KEPT);
        assert_eq!(state.notices[&5][0].id, 11);
        state.forget_notice(11);
        assert_eq!(state.notices[&5].len(), EPHEMERAL_KEPT - 1);
    }
}
