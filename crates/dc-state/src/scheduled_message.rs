//! scheduledMessageStore (module 199160), the scheduling rules of module 566908 and the scheduled-message draft of
//! DraftStore (module 31717, draft type 8). The store holds the user's scheduled messages by id, which the server lists
//! at `GET /users/@me/scheduled-messages`; it has no gateway events, it follows the actions its requests dispatch
//! (module 151282). Times are local wall-clock times, as the official client computes them with moment.

use crate::{Id, id, text};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, TimeZone, Timelike};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// `ScheduledMessageState` (module 551640).
pub const STATE_SCHEDULED: u64 = 0;
pub const STATE_ERROR_SCHEDULED_MESSAGES_DISABLED: u64 = 1;
pub const STATE_ERROR_USER_NOT_FOUND: u64 = 2;
pub const STATE_ERROR_USER_CANNOT_USE_SCHEDULED_MESSAGES: u64 = 3;
pub const STATE_ERROR_CHANNEL_NOT_FOUND: u64 = 4;
pub const STATE_ERROR_SEND_FAILED: u64 = 5;

/// `TOO_MANY_SCHEDULED_MESSAGES`, the error code of a create that is over the limit (module 652215).
pub const TOO_MANY_SCHEDULED_MESSAGES: u64 = 640_003;

/// `S()`: a message can be scheduled at least 600 seconds ahead.
const MIN_LEAD_SECONDS: i64 = 600;
/// `N()`: and at most 691200 seconds (8 days) ahead.
const MAX_LEAD_SECONDS: i64 = 691_200;
/// Module 853297: an edit may move the time at most 2592000 seconds (30 days) past the message's creation.
const MAX_AFTER_CREATION_SECONDS: i64 = 2_592_000;
/// The Discord epoch, for the creation time of a snowflake.
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// An uploaded attachment of a scheduled message (`attachment_uploads`).
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentUpload {
    pub filename: Option<String>,
    pub uploaded_filename: Option<String>,
    pub description: Option<String>,
    pub title: Option<String>,
}

/// `create_args`: what the message will be sent with.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateArgs {
    pub channel_id: Option<Id>,
    pub content: String,
    pub kind: Option<u64>,
    pub flags: Option<u64>,
    pub message_reference: Value,
}

/// `Lg`: a scheduled message as the server describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledMessageSend {
    pub user_id: Option<Id>,
    pub scheduled_message_id: Id,
    /// ISO 8601, as received.
    pub send_at_timestamp: String,
    pub create_args: CreateArgs,
    pub state: u64,
    pub attachment_uploads: Vec<AttachmentUpload>,
    /// `message_preview` stamped with the send time: the message the list shows.
    pub record: Value,
}

impl ScheduledMessageSend {
    pub fn from_value(value: &Value) -> Option<Self> {
        let args = &value["create_args"];
        let mut record = value["message_preview"].clone();
        if let Some(object) = record.as_object_mut() {
            object.insert("timestamp".to_owned(), value["send_at_timestamp"].clone());
        }
        Some(Self {
            user_id: id(&value["user_id"]),
            scheduled_message_id: id(&value["scheduled_message_id"])?,
            send_at_timestamp: text(&value["send_at_timestamp"])?,
            create_args: CreateArgs {
                channel_id: id(&args["channel_id"]),
                content: text(&args["content"]).unwrap_or_default(),
                kind: args["type"].as_u64(),
                flags: args["flags"].as_u64(),
                message_reference: args["message_reference"].clone(),
            },
            state: value["state"].as_u64().unwrap_or(STATE_SCHEDULED),
            attachment_uploads: value["attachment_uploads"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|upload| AttachmentUpload {
                    filename: text(&upload["filename"]),
                    uploaded_filename: text(&upload["uploaded_filename"]),
                    description: text(&upload["description"]),
                    title: text(&upload["title"]),
                })
                .collect(),
            record,
        })
    }
}

/// The store. `FETCH_SCHEDULED_MESSAGES*` and `SCHEDULED_MESSAGES_{CREATE,UPDATE,DELETE,SEND_NOW}_*` are its handlers.
#[derive(Default)]
pub struct ScheduledMessageStore {
    loading: bool,
    messages: HashMap<Id, ScheduledMessageSend>,
    pending_removal: HashSet<Id>,
}

impl ScheduledMessageStore {
    /// `loading`.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// `getScheduledMessagesForInbox`.
    pub fn get_scheduled_messages_for_inbox(&self) -> &HashMap<Id, ScheduledMessageSend> {
        &self.messages
    }

    /// The inbox list's order: the latest send time first (`sQ` sorts by the later date first).
    pub fn sorted_for_inbox(&self) -> Vec<&ScheduledMessageSend> {
        let mut list: Vec<&ScheduledMessageSend> = self.messages.values().collect();
        list.sort_by(|a, b| {
            let (a_time, b_time) = (
                parse_iso(&a.send_at_timestamp),
                parse_iso(&b.send_at_timestamp),
            );
            b_time
                .cmp(&a_time)
                .then(b.scheduled_message_id.cmp(&a.scheduled_message_id))
        });
        list
    }

    /// `getMessagesPendingRemoval`: sending or cancelling is in flight.
    pub fn get_messages_pending_removal(&self) -> &HashSet<Id> {
        &self.pending_removal
    }

    pub fn fetch_start(&mut self) {
        self.loading = true;
    }

    /// `FETCH_SCHEDULED_MESSAGES_SUCCESS`: the list replaces everything held.
    pub fn fetch_success(&mut self, messages: Vec<ScheduledMessageSend>) {
        self.messages = messages
            .into_iter()
            .map(|message| (message.scheduled_message_id, message))
            .collect();
        self.loading = false;
    }

    pub fn fetch_failure(&mut self) {
        self.loading = false;
    }

    /// `SCHEDULED_MESSAGES_CREATE_SUCCESS` and `SCHEDULED_MESSAGES_UPDATE_SUCCESS`.
    pub fn put(&mut self, message: ScheduledMessageSend) {
        self.messages.insert(message.scheduled_message_id, message);
    }

    /// `SCHEDULED_MESSAGES_DELETE_START` and `SCHEDULED_MESSAGES_SEND_NOW_START` (`o`): false when it was already
    /// pending.
    pub fn removal_start(&mut self, message: Id) -> bool {
        self.pending_removal.insert(message)
    }

    /// `SCHEDULED_MESSAGES_DELETE_SUCCESS` and `SCHEDULED_MESSAGES_SEND_NOW_SUCCESS` (`d`): false when nothing changed.
    pub fn removal_success(&mut self, message: Id) -> bool {
        let pending = self.pending_removal.remove(&message);
        let held = self.messages.remove(&message).is_some();
        pending || held
    }

    /// `SCHEDULED_MESSAGES_DELETE_FAILURE` and `SCHEDULED_MESSAGES_SEND_NOW_FAILURE` (`c`).
    pub fn removal_failure(&mut self, message: Id) -> bool {
        self.pending_removal.remove(&message)
    }

    /// `LOGOUT` and `CONNECTION_OPEN`.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The scheduled-message draft of a channel (`DraftStore` type 8): the time the composer will schedule its next message
/// for, kept until that message is created.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledDraft {
    /// ISO 8601 UTC, as `toISOString()` writes it.
    pub scheduled_timestamp: String,
    /// When the draft last changed, in milliseconds.
    pub timestamp: i64,
}

#[derive(Default)]
pub struct ScheduledDrafts {
    drafts: HashMap<Id, ScheduledDraft>,
}

impl ScheduledDrafts {
    /// `getScheduledMessage`.
    pub fn get_scheduled_message(&self, channel: Id) -> Option<&ScheduledDraft> {
        self.drafts.get(&channel)
    }

    /// `SCHEDULED_MESSAGE_DRAFT_CHANGE`.
    pub fn change(&mut self, channel: Id, scheduled_timestamp: String, now_ms: i64) {
        self.drafts.insert(
            channel,
            ScheduledDraft {
                scheduled_timestamp,
                timestamp: now_ms,
            },
        );
    }

    /// `DRAFT_CLEAR` of type 8, and the draft's removal when a create succeeds.
    pub fn clear(&mut self, channel: Id) -> bool {
        self.drafts.remove(&channel).is_some()
    }

    /// `CHANNEL_DELETE` and `THREAD_DELETE`, and `LOGOUT`.
    pub fn reset(&mut self) {
        self.drafts.clear();
    }
}

/// An ISO 8601 time as an instant.
pub fn parse_iso(text: &str) -> Option<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(text.get(..text.len().min(200))?)
        .ok()
        .map(|time| time.with_timezone(&chrono::Utc))
}

/// An ISO 8601 time as the local wall-clock time.
pub fn to_local(text: &str) -> Option<NaiveDateTime> {
    parse_iso(text).map(|time| time.with_timezone(&Local).naive_local())
}

/// The local wall-clock time of a snowflake's creation (`extractTimestamp`).
pub fn snowflake_local(snowflake: Id) -> Option<NaiveDateTime> {
    let millis = i64::try_from(snowflake >> 22).ok()? + DISCORD_EPOCH_MS;
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|time| time.naive_local())
}

/// `S()` / `_O`: the earliest time that may be scheduled.
pub fn earliest(now: NaiveDateTime) -> NaiveDateTime {
    now + Duration::seconds(MIN_LEAD_SECONDS)
}

/// `N()` / `yp`: the latest time that may be scheduled. An existing message (created at `created`) may not move past
/// 30 days after its creation either.
pub fn latest(now: NaiveDateTime, created: Option<NaiveDateTime>) -> NaiveDateTime {
    let ahead = now + Duration::seconds(MAX_LEAD_SECONDS);
    match created {
        Some(created) => ahead.min(created + Duration::seconds(MAX_AFTER_CREATION_SECONDS)),
        None => ahead,
    }
}

/// `US()`: the modal's default, the next whole hour, or the one after it when that is under 10 minutes away.
pub fn default_time(now: NaiveDateTime) -> NaiveDateTime {
    let next = now
        .with_minute(0)
        .and_then(|time| time.with_second(0))
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(now)
        + Duration::hours(1);
    if next < earliest(now) {
        next + Duration::hours(1)
    } else {
        next
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    TooSoon,
    TooFar,
}

impl TimeError {
    /// `w/fgvh` and `Nt0tz7`.
    pub fn text(self) -> &'static str {
        match self {
            Self::TooSoon => "Scheduled time is too soon",
            Self::TooFar => "Scheduled time is too far away",
        }
    }
}

/// `Vf`: what is wrong with a chosen time, if anything.
pub fn time_error(
    chosen: NaiveDateTime,
    now: NaiveDateTime,
    created: Option<NaiveDateTime>,
) -> Option<TimeError> {
    if chosen < earliest(now) {
        Some(TimeError::TooSoon)
    } else if chosen > latest(now, created) {
        Some(TimeError::TooFar)
    } else {
        None
    }
}

/// `FF`: the quick choices of the "Schedule Message" submenu: 9:00 and 13:00 today (tomorrow once they are less than
/// 10 minutes away or past) and next Monday at 9:00. The labels are `qINKo/`, `tjIn9i`, `qT6LjY`, `EMRZyS` and `+P5MmK`.
pub fn quick_times(now: NaiveDateTime) -> [(&'static str, NaiveDateTime); 3] {
    let soonest = earliest(now);
    let at = |hour: u32| now.date().and_hms_opt(hour, 0, 0).unwrap_or(now);
    let (nine, one) = (at(9), at(13));
    let monday = now.date()
        - Duration::days(i64::from(now.date().weekday().num_days_from_monday()))
        + Duration::weeks(1);
    [
        if nine > soonest {
            ("Today at 9:00am", nine)
        } else {
            ("Tomorrow at 9:00am", nine + Duration::days(1))
        },
        if one > soonest {
            ("Today at 1:00pm", one)
        } else {
            ("Tomorrow at 1:00pm", one + Duration::days(1))
        },
        (
            "Monday at 9:00am",
            monday.and_hms_opt(9, 0, 0).unwrap_or(now),
        ),
    ]
}

/// `C`: how many messages may be scheduled at once, and whether Nitro raises it. Only Nitro tier 2 has 25.
pub fn limit(has_tier_two: bool) -> (u32, bool) {
    if has_tier_two { (25, false) } else { (1, true) }
}

/// `Y8`: the state line of a listed message, and whether it is an error.
pub fn state_message(state: u64) -> Option<(bool, &'static str)> {
    Some(match state {
        STATE_SCHEDULED => (false, "Scheduled"),
        STATE_ERROR_CHANNEL_NOT_FOUND => (true, "Message could not be sent"),
        STATE_ERROR_USER_NOT_FOUND => (true, "Message could not be sent"),
        STATE_ERROR_USER_CANNOT_USE_SCHEDULED_MESSAGES => {
            (true, "Message could not be sent, permission was denied")
        }
        STATE_ERROR_SEND_FAILED => (true, "Message could not be sent"),
        STATE_ERROR_SCHEDULED_MESSAGES_DISABLED => (true, "Message could not be sent"),
        _ => return None,
    })
}

/// The composer bar's line (`MQcRX/`): `Scheduling message for November 10 at 2:00 PM`.
pub fn bar_label(when: NaiveDateTime) -> String {
    format!(
        "Scheduling message for {} at {}",
        when.format("%B %-d"),
        short_time(when)
    )
}

/// The toast after a create (`CvHu/j`): `Message scheduled for November 10, 2026 at 2:00 PM`.
pub fn scheduled_toast(when: NaiveDateTime) -> String {
    format!(
        "Message scheduled for {} at {}",
        when.format("%B %-d, %Y"),
        short_time(when)
    )
}

/// `LT`: `2:00 PM`.
fn short_time(when: NaiveDateTime) -> String {
    let (pm, hour) = when.time().hour12();
    format!(
        "{hour}:{:02} {}",
        when.minute(),
        if pm { "PM" } else { "AM" }
    )
}

/// What `pr` puts in the body of `POST /users/@me/scheduled-messages`.
pub struct CreateRequest<'a> {
    pub channel: Id,
    pub content: &'a str,
    /// ISO 8601 UTC with milliseconds (`toISOString()`).
    pub scheduled_timestamp: &'a str,
    pub flags: Option<u64>,
    pub message_reference: Option<Value>,
    pub allowed_mentions: Option<Value>,
    pub sticker_ids: &'a [Id],
    pub attachments: Vec<Value>,
}

/// The request body of `POST /users/@me/scheduled-messages` (`pr`): `undefined` members are left out as
/// `JSON.stringify` does, `attachments` is always there.
pub fn create_body(request: CreateRequest<'_>) -> Value {
    let mut body = json!({
        "channel_id": request.channel.to_string(),
        "content": request.content,
        "scheduled_timestamp": request.scheduled_timestamp,
    });
    if let Some(flags) = request.flags.filter(|flags| *flags != 0) {
        body["flags"] = json!(flags);
    }
    if let Some(reference) = request.message_reference {
        body["message_reference"] = reference;
    }
    if let Some(mentions) = request.allowed_mentions {
        body["allowed_mentions"] = mentions;
    }
    if !request.sticker_ids.is_empty() {
        body["sticker_ids"] = json!(
            request
                .sticker_ids
                .iter()
                .map(Id::to_string)
                .collect::<Vec<_>>()
        );
    }
    body["attachments"] = Value::Array(request.attachments);
    body
}

/// The body of `PATCH /users/@me/scheduled-messages/{id}` (`Eg`).
pub fn update_body(scheduled_timestamp: &str, content: Option<&str>, flags: Option<u64>) -> Value {
    json!({
        "scheduled_timestamp": scheduled_timestamp,
        "content": content,
        "flags": flags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn message(id: u64, when: &str) -> ScheduledMessageSend {
        ScheduledMessageSend::from_value(&json!({
            "user_id": "1",
            "scheduled_message_id": id.to_string(),
            "send_at_timestamp": when,
            "create_args": {"channel_id": "5", "content": "hi", "flags": 0},
            "state": 0,
            "attachment_uploads": [{"filename": "a.png", "uploaded_filename": "u/a.png"}],
            "message_preview": {"id": "9", "content": "hi"},
        }))
        .unwrap()
    }

    #[test]
    fn the_default_is_the_next_hour_or_the_one_after() {
        assert_eq!(
            default_time(at(2026, 11, 10, 13, 10)),
            at(2026, 11, 10, 14, 0)
        );
        // 14:00 is under 10 minutes from 13:55, so the one after it.
        assert_eq!(
            default_time(at(2026, 11, 10, 13, 55)),
            at(2026, 11, 10, 15, 0)
        );
        assert_eq!(
            default_time(at(2026, 11, 10, 13, 50)),
            at(2026, 11, 10, 14, 0)
        );
        assert_eq!(
            default_time(at(2026, 11, 10, 23, 55)),
            at(2026, 11, 11, 1, 0)
        );
    }

    #[test]
    fn the_time_must_be_between_ten_minutes_and_eight_days_ahead() {
        let now = at(2026, 11, 10, 13, 10);
        assert_eq!(earliest(now), at(2026, 11, 10, 13, 20));
        assert_eq!(latest(now, None), at(2026, 11, 18, 13, 10));
        assert_eq!(
            time_error(at(2026, 11, 10, 13, 15), now, None),
            Some(TimeError::TooSoon)
        );
        assert_eq!(time_error(at(2026, 11, 10, 13, 20), now, None), None);
        assert_eq!(time_error(at(2026, 11, 18, 13, 10), now, None), None);
        assert_eq!(
            time_error(at(2026, 11, 18, 13, 11), now, None),
            Some(TimeError::TooFar)
        );
        assert_eq!(TimeError::TooSoon.text(), "Scheduled time is too soon");
        assert_eq!(TimeError::TooFar.text(), "Scheduled time is too far away");
    }

    #[test]
    fn an_edit_stays_within_30_days_of_creation() {
        let now = at(2026, 11, 10, 13, 10);
        // Created 25 days ago: the 30-day limit is 5 days ahead, nearer than 8.
        let created = at(2026, 10, 16, 13, 10);
        assert_eq!(latest(now, Some(created)), at(2026, 11, 15, 13, 10));
        assert_eq!(
            time_error(at(2026, 11, 16, 9, 0), now, Some(created)),
            Some(TimeError::TooFar)
        );
        // Created today: the 8 days ahead are nearer.
        assert_eq!(latest(now, Some(now)), at(2026, 11, 18, 13, 10));
    }

    #[test]
    fn a_snowflake_gives_its_creation_time() {
        // 2026-11-10T12:00:00Z as a snowflake.
        let millis = 1_794_657_600_000_i64;
        let snowflake = ((millis - DISCORD_EPOCH_MS) as u64) << 22;
        let local = snowflake_local(snowflake).unwrap();
        let utc = Local.from_local_datetime(&local).earliest().unwrap();
        assert_eq!(utc.timestamp_millis(), millis);
    }

    #[test]
    fn quick_times_roll_over_when_they_have_passed() {
        let morning = quick_times(at(2026, 11, 10, 7, 0));
        assert_eq!(morning[0], ("Today at 9:00am", at(2026, 11, 10, 9, 0)));
        assert_eq!(morning[1], ("Today at 1:00pm", at(2026, 11, 10, 13, 0)));
        // Tuesday: next Monday is the 16th.
        assert_eq!(morning[2], ("Monday at 9:00am", at(2026, 11, 16, 9, 0)));
        let noon = quick_times(at(2026, 11, 10, 12, 55));
        assert_eq!(noon[0], ("Tomorrow at 9:00am", at(2026, 11, 11, 9, 0)));
        // 13:00 is 5 minutes away, under the 10-minute lead.
        assert_eq!(noon[1], ("Tomorrow at 1:00pm", at(2026, 11, 11, 13, 0)));
        // On a Monday "next Monday" is a week on.
        assert_eq!(
            quick_times(at(2026, 11, 9, 8, 0))[2].1,
            at(2026, 11, 16, 9, 0)
        );
        // On a Sunday it is the next day.
        assert_eq!(
            quick_times(at(2026, 11, 15, 8, 0))[2].1,
            at(2026, 11, 16, 9, 0)
        );
    }

    #[test]
    fn labels_follow_the_official_strings() {
        assert_eq!(
            bar_label(at(2026, 11, 10, 14, 0)),
            "Scheduling message for November 10 at 2:00 PM"
        );
        assert_eq!(
            bar_label(at(2026, 12, 5, 0, 15)),
            "Scheduling message for December 5 at 12:15 AM"
        );
        assert_eq!(
            scheduled_toast(at(2026, 11, 10, 14, 0)),
            "Message scheduled for November 10, 2026 at 2:00 PM"
        );
    }

    #[test]
    fn the_limit_is_one_without_nitro_tier_two() {
        assert_eq!(limit(false), (1, true));
        assert_eq!(limit(true), (25, false));
        assert_eq!(state_message(STATE_SCHEDULED), Some((false, "Scheduled")));
        assert_eq!(
            state_message(STATE_ERROR_USER_CANNOT_USE_SCHEDULED_MESSAGES),
            Some((true, "Message could not be sent, permission was denied"))
        );
        assert_eq!(state_message(99), None);
    }

    #[test]
    fn the_create_body_leaves_out_what_is_undefined() {
        let plain = create_body(CreateRequest {
            channel: 5,
            content: "hi",
            scheduled_timestamp: "2026-11-10T14:00:00.000Z",
            flags: Some(0),
            message_reference: None,
            allowed_mentions: None,
            sticker_ids: &[],
            attachments: vec![],
        });
        assert_eq!(
            plain,
            json!({
                "channel_id": "5",
                "content": "hi",
                "scheduled_timestamp": "2026-11-10T14:00:00.000Z",
                "attachments": [],
            })
        );
        let full = create_body(CreateRequest {
            channel: 5,
            content: "hi",
            scheduled_timestamp: "2026-11-10T14:00:00.000Z",
            flags: Some(4),
            message_reference: Some(json!({"channel_id": "5", "message_id": "7"})),
            allowed_mentions: Some(json!({"parse": ["users"], "replied_user": false})),
            sticker_ids: &[8],
            attachments: vec![json!({"id": "0"})],
        });
        assert_eq!(full["flags"], 4);
        assert_eq!(full["message_reference"]["message_id"], "7");
        assert_eq!(full["allowed_mentions"]["replied_user"], false);
        assert_eq!(full["sticker_ids"], json!(["8"]));
        assert_eq!(full["attachments"][0]["id"], "0");
    }

    #[test]
    fn a_listed_message_is_read_with_its_preview_stamped() {
        let listed = message(3, "2026-11-10T14:00:00+00:00");
        assert_eq!(listed.scheduled_message_id, 3);
        assert_eq!(listed.create_args.channel_id, Some(5));
        assert_eq!(listed.attachment_uploads.len(), 1);
        assert_eq!(listed.record["timestamp"], "2026-11-10T14:00:00+00:00");
        assert_eq!(listed.record["content"], "hi");
    }

    #[test]
    fn the_store_follows_its_actions() {
        let mut store = ScheduledMessageStore::default();
        store.fetch_start();
        assert!(store.loading());
        store.fetch_success(vec![
            message(1, "2026-11-10T14:00:00+00:00"),
            message(2, "2026-11-12T14:00:00+00:00"),
        ]);
        assert!(!store.loading());
        // The later message comes first in the inbox.
        let order: Vec<Id> = store
            .sorted_for_inbox()
            .iter()
            .map(|message| message.scheduled_message_id)
            .collect();
        assert_eq!(order, [2, 1]);
        store.put(message(3, "2026-11-11T14:00:00+00:00"));
        assert_eq!(store.get_scheduled_messages_for_inbox().len(), 3);
        assert!(store.removal_start(1));
        assert!(!store.removal_start(1));
        assert!(store.get_messages_pending_removal().contains(&1));
        assert!(store.removal_failure(1));
        assert!(!store.removal_failure(1));
        assert!(store.removal_start(2));
        assert!(store.removal_success(2));
        assert!(!store.removal_success(2));
        assert_eq!(store.get_scheduled_messages_for_inbox().len(), 2);
        store.fetch_start();
        store.fetch_failure();
        assert!(!store.loading());
        store.reset();
        assert!(store.get_scheduled_messages_for_inbox().is_empty());
    }

    #[test]
    fn a_channel_keeps_one_draft_until_its_message_is_created() {
        let mut drafts = ScheduledDrafts::default();
        assert!(drafts.get_scheduled_message(5).is_none());
        drafts.change(5, "2026-11-10T14:00:00.000Z".to_owned(), 100);
        drafts.change(6, "2026-11-11T14:00:00.000Z".to_owned(), 101);
        drafts.change(5, "2026-11-10T15:00:00.000Z".to_owned(), 102);
        let draft = drafts.get_scheduled_message(5).unwrap();
        assert_eq!(draft.scheduled_timestamp, "2026-11-10T15:00:00.000Z");
        assert_eq!(draft.timestamp, 102);
        assert!(drafts.clear(5));
        assert!(!drafts.clear(5));
        assert!(drafts.get_scheduled_message(6).is_some());
        drafts.reset();
        assert!(drafts.get_scheduled_message(6).is_none());
    }

    #[test]
    fn the_update_body_carries_the_new_time() {
        assert_eq!(
            update_body("2026-11-10T14:00:00.000Z", None, None),
            json!({"scheduled_timestamp": "2026-11-10T14:00:00.000Z", "content": null, "flags": null})
        );
    }
}
