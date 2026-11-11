//! Polls: what a poll message carries, how votes change it, and the footer's time label.
//!
//! Ports the poll view model `T` in module 951727, the vote handlers in module 180662 and the message record's
//! `addReaction` / `removeReaction` for vote reactions (module 383233). The official client keeps each answer's votes
//! as a reaction of type VOTE (2) whose emoji id is the answer id; here the counts live on the poll itself, which
//! the REST message carries as `poll.results.answer_counts` (`{id, count, me_voted}`).

use crate::message_kinds::{Poll, PollEmoji};
use crate::{Id, Stores, id, text};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// `layout_type` (module 744593): the default list of answers, or square image answers.
pub const LAYOUT_DEFAULT: u64 = 1;
pub const LAYOUT_IMAGE_ONLY_ANSWERS: u64 = 2;

/// A number the gateway may send as a JSON number or as a decimal string (ids over 2^53 come as strings).
fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

impl Poll {
    /// A `poll` object of a message: `question.text`, `answers[]` (`answer_id`, `poll_media`), `expiry`,
    /// `allow_multiselect`, `layout_type` and `results` (`is_finalized`, `answer_counts`).
    pub fn from_value(value: &Value) -> Poll {
        let results = &value["results"];
        let answers: Vec<&Value> = value["answers"].as_array().into_iter().flatten().collect();
        let count_of = |answer: &Value| {
            results["answer_counts"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|count| number(&count["id"]) == number(&answer["answer_id"]))
        };
        Poll {
            question: text(&value["question"]["text"]).unwrap_or_default(),
            answers: answers
                .iter()
                .map(|answer| text(&answer["poll_media"]["text"]).unwrap_or_default())
                .collect(),
            expiry: value["expiry"]
                .as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc)),
            multiselect: value["allow_multiselect"].as_bool().unwrap_or(false),
            finalized: results["is_finalized"].as_bool().unwrap_or(false),
            counts: answers
                .iter()
                .map(|answer| {
                    count_of(answer)
                        .and_then(|count| count["count"].as_u64())
                        .unwrap_or(0)
                })
                .collect(),
            answer_ids: answers
                .iter()
                .enumerate()
                .map(|(index, answer)| number(&answer["answer_id"]).unwrap_or(index as u64 + 1))
                .collect(),
            emoji: answers
                .iter()
                .map(|answer| {
                    let emoji = &answer["poll_media"]["emoji"];
                    let emoji = emoji.is_object().then(|| PollEmoji {
                        id: id(&emoji["id"]),
                        name: text(&emoji["name"]).unwrap_or_default(),
                        animated: emoji["animated"].as_bool().unwrap_or(false),
                    });
                    emoji.filter(|emoji| emoji.id.is_some() || !emoji.name.is_empty())
                })
                .collect(),
            attachments: answers
                .iter()
                .map(|answer| {
                    answer["poll_media"]["attachment_ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(id)
                        .collect()
                })
                .collect(),
            me_voted: answers
                .iter()
                .map(|answer| {
                    count_of(answer)
                        .and_then(|count| count["me_voted"].as_bool())
                        .unwrap_or(false)
                })
                .collect(),
            layout: value["layout_type"].as_u64().unwrap_or(LAYOUT_DEFAULT),
        }
    }

    /// The position of an answer id.
    pub fn index_of(&self, answer: u64) -> Option<usize> {
        self.answer_ids.iter().position(|known| *known == answer)
    }

    /// `aw` in module 969632: all votes together, the divisor of every percentage.
    pub fn total_votes(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// Whether this account has a vote on the poll (`hasVoteRecorded`: some reaction has `me_vote`).
    pub fn has_vote(&self) -> bool {
        self.me_voted.iter().any(|voted| *voted)
    }

    /// The answer ids this account voted for.
    pub fn my_votes(&self) -> Vec<u64> {
        self.me_voted
            .iter()
            .enumerate()
            .filter(|(_, voted)| **voted)
            .filter_map(|(index, _)| self.answer_ids.get(index).copied())
            .collect()
    }

    /// The most votes any one answer has (`H` in module 951727).
    pub fn top_count(&self) -> u64 {
        self.counts.iter().copied().max().unwrap_or(0)
    }

    /// An answer is a leader when it has as many votes as the best one and at least one (`c >= H && 0 !== c`).
    pub fn is_leader(&self, index: usize) -> bool {
        let count = self.counts.get(index).copied().unwrap_or(0);
        count != 0 && count >= self.top_count()
    }

    /// `Math.round(100 * votes / total)`, 0 when nobody voted.
    pub fn percentage(&self, index: usize) -> u32 {
        let total = self.total_votes();
        if total == 0 {
            return 0;
        }
        let count = self.counts.get(index).copied().unwrap_or(0);
        // `Math.round` rounds halves up.
        ((100.0 * count as f64 / total as f64) + 0.5).floor() as u32
    }

    /// The poll has closed: its expiry is not after `now` (`T` in module 951727: no time left to show).
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        // `results.is_finalized` is what the server sets when the poll ends; its new expiry can still be a moment
        // ahead of this machine's clock.
        self.finalized || self.expiry.is_none_or(|expiry| expiry <= now)
    }

    /// The footer's time left (`u` in module 708676): `Nd left` above one day, `Nh left` above one hour, else
    /// `Nm left` (`< 1m left` under a minute). None once the poll has closed. Moment's `diff` truncates.
    pub fn time_left(&self, now: DateTime<Utc>) -> Option<String> {
        if self.finalized {
            return None;
        }
        let expiry = self.expiry?;
        if expiry <= now {
            return None;
        }
        let span = expiry - now;
        let days = span.num_days();
        if days > 1 {
            return Some(format!("{days}d left"));
        }
        let hours = span.num_hours();
        if hours > 1 {
            return Some(format!("{hours}h left"));
        }
        let minutes = span.num_minutes();
        Some(if minutes == 0 {
            "< 1m left".to_owned()
        } else {
            format!("{minutes}m left")
        })
    }

    /// A vote arrived (`addReaction`) or went (`removeReaction`) for an answer. A change this account made itself
    /// is applied when it was not applied before: the gateway echoes the vote the client already counted.
    pub fn apply_vote(&mut self, answer: u64, add: bool, mine: bool) {
        self.normalize();
        let Some(index) = self.index_of(answer) else {
            return;
        };
        if add {
            if !(mine && self.me_voted[index]) {
                self.counts[index] += 1;
            }
            self.me_voted[index] |= mine;
        } else {
            if !(mine && !self.me_voted[index]) {
                self.counts[index] = self.counts[index].saturating_sub(1);
            }
            if mine {
                self.me_voted[index] = false;
            }
        }
    }

    /// The vote this account just submitted, applied at once (`D` in module 180662 dispatches an optimistic add
    /// for each new answer and an optimistic remove for each dropped one).
    pub fn set_my_votes(&mut self, answers: &[u64]) {
        self.normalize();
        for index in 0..self.answer_ids.len() {
            let answer = self.answer_ids[index];
            let wanted = answers.contains(&answer);
            if wanted != self.me_voted[index] {
                self.apply_vote(answer, wanted, true);
            }
        }
    }

    /// Fills the per-answer vectors a poll made by hand may lack, so every index is safe.
    pub fn normalize(&mut self) {
        let n = self.answers.len();
        self.counts.resize(n, 0);
        self.me_voted.resize(n, false);
        self.emoji.resize(n, None);
        self.attachments.resize(n, Vec::new());
        if self.answer_ids.len() < n {
            let from = self.answer_ids.len();
            self.answer_ids
                .extend((from..n).map(|index| index as u64 + 1));
        }
    }
}

impl Stores {
    fn poll_of(&mut self, channel: Id, message: Id) -> Option<&mut Poll> {
        self.messages
            .get_mut(&channel)?
            .iter_mut()
            .find(|known| known.id == message)?
            .extra
            .poll
            .as_mut()
    }

    /// `MESSAGE_POLL_VOTE_ADD` / `MESSAGE_POLL_VOTE_REMOVE`: `{user_id, channel_id, message_id, answer_id}`.
    pub(crate) fn poll_vote(&mut self, data: &Value, add: bool) {
        let (Some(channel), Some(message), Some(user), Some(answer)) = (
            id(&data["channel_id"]),
            id(&data["message_id"]),
            id(&data["user_id"]),
            number(&data["answer_id"]),
        ) else {
            return;
        };
        let mine = self.current_user.as_ref().map(|me| me.id) == Some(user);
        if let Some(poll) = self.poll_of(channel, message) {
            poll.apply_vote(answer, add, mine);
        }
    }

    /// `MESSAGE_POLL_VOTE_ADD_MANY`: `{channel_id, message_id, votes: [{answer_id, users}]}`.
    pub(crate) fn poll_vote_many(&mut self, data: &Value) {
        let (Some(channel), Some(message)) = (id(&data["channel_id"]), id(&data["message_id"]))
        else {
            return;
        };
        let me = self.current_user.as_ref().map(|me| me.id);
        let Some(poll) = self.poll_of(channel, message) else {
            return;
        };
        for vote in data["votes"].as_array().into_iter().flatten() {
            let Some(answer) = number(&vote["answer_id"]) else {
                continue;
            };
            for user in vote["users"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
            {
                poll.apply_vote(answer, true, Some(user) == me);
            }
        }
    }

    /// This account's vote, applied before the server answers. The previous vote comes back to undo it.
    pub fn poll_set_my_votes(
        &mut self,
        channel: Id,
        message: Id,
        answers: &[u64],
    ) -> Option<Vec<u64>> {
        let poll = self.poll_of(channel, message)?;
        let before = poll.my_votes();
        poll.set_my_votes(answers);
        Some(before)
    }

    /// A message update that carries `poll` (the results arrive finalized when the poll ends).
    pub(crate) fn poll_update(&mut self, channel: Id, message: Id, poll: &Value) {
        if !poll.is_object() {
            return;
        }
        let fresh = Poll::from_value(poll);
        let Some(known) = self.poll_of(channel, message) else {
            return;
        };
        // A partial update may leave the answers out: keep what is known then.
        if fresh.answers.is_empty() {
            known.finalized |= fresh.finalized;
            if fresh.expiry.is_some() {
                known.expiry = fresh.expiry;
            }
            return;
        }
        // A gateway update is the same for every viewer, so its `me_voted` is never this account's: the votes it
        // had stay as they were (observed on the fixture: the update that ends a poll lists `me_voted: false`).
        let mine = known.me_voted.clone();
        *known = fresh;
        if mine.len() == known.me_voted.len() {
            known.me_voted = mine;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "question": {"text": "Which day?"},
            "answers": [
                {"answer_id": 1, "poll_media": {"text": "Friday", "emoji": {"name": "\u{1F389}"}}},
                {"answer_id": 2, "poll_media": {"text": "Saturday", "emoji": {"id": "123", "name": "party"}}},
                {"answer_id": 3, "poll_media": {"text": "Sunday"}},
            ],
            "expiry": "2026-11-12T10:00:00.000000+00:00",
            "allow_multiselect": true,
            "layout_type": 1,
            "results": {"is_finalized": false, "answer_counts": [
                {"id": 1, "count": 3, "me_voted": true},
                {"id": 2, "count": 1, "me_voted": false},
            ]},
        })
    }

    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(time)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_poll_object_is_read_value_for_value() {
        let poll = Poll::from_value(&sample());
        assert_eq!(poll.question, "Which day?");
        assert_eq!(poll.answers, ["Friday", "Saturday", "Sunday"]);
        assert_eq!(poll.answer_ids, [1, 2, 3]);
        assert_eq!(poll.counts, [3, 1, 0]);
        assert_eq!(poll.me_voted, [true, false, false]);
        assert!(poll.multiselect && !poll.finalized);
        assert_eq!(poll.emoji[0].as_ref().unwrap().name, "\u{1F389}");
        assert_eq!(poll.emoji[1].as_ref().unwrap().id, Some(123));
        assert!(poll.emoji[2].is_none());
        assert_eq!(poll.my_votes(), [1]);
        assert_eq!(poll.total_votes(), 4);
    }

    #[test]
    fn percentages_round_like_math_round_and_leaders_tie() {
        let mut poll = Poll::from_value(&sample());
        assert_eq!(
            (0..3).map(|i| poll.percentage(i)).collect::<Vec<_>>(),
            [75, 25, 0]
        );
        assert!(poll.is_leader(0) && !poll.is_leader(1) && !poll.is_leader(2));
        poll.counts = vec![1, 1, 0];
        assert!(poll.is_leader(0) && poll.is_leader(1));
        assert_eq!(poll.percentage(0), 50);
        poll.counts = vec![1, 1, 1];
        // 33.33 rounds down, and the leaders are all three.
        assert_eq!(poll.percentage(2), 33);
        poll.counts = vec![0, 0, 0];
        assert!(!poll.is_leader(0));
        assert_eq!(poll.percentage(0), 0);
        // 1 of 8 is 12.5 and rounds up to 13.
        poll.counts = vec![1, 7, 0];
        assert_eq!(poll.percentage(0), 13);
    }

    #[test]
    fn the_footer_label_follows_the_official_steps() {
        let poll = Poll::from_value(&sample());
        assert_eq!(
            poll.time_left(at("2026-11-09T09:00:00Z")).unwrap(),
            "3d left"
        );
        // One day and five hours: not above one day, so hours.
        assert_eq!(
            poll.time_left(at("2026-11-11T05:00:00Z")).unwrap(),
            "29h left"
        );
        assert_eq!(
            poll.time_left(at("2026-11-12T08:30:00Z")).unwrap(),
            "90m left"
        );
        assert_eq!(
            poll.time_left(at("2026-11-12T09:59:30Z")).unwrap(),
            "< 1m left"
        );
        assert_eq!(poll.time_left(at("2026-11-12T10:00:00Z")), None);
        assert!(poll.is_expired(at("2026-11-12T10:00:00Z")));
        assert!(!poll.is_expired(at("2026-11-12T09:59:59Z")));
    }

    #[test]
    fn votes_count_once_and_a_gateway_echo_changes_nothing() {
        let mut poll = Poll::from_value(&sample());
        // Someone else votes, then takes it back.
        poll.apply_vote(2, true, false);
        assert_eq!(poll.counts, [3, 2, 0]);
        poll.apply_vote(2, false, false);
        assert_eq!(poll.counts, [3, 1, 0]);
        // This account changes its vote from 1 to 3 (optimistic).
        poll.set_my_votes(&[3]);
        assert_eq!(poll.counts, [2, 1, 1]);
        assert_eq!(poll.my_votes(), [3]);
        // The echoes of its own change come back: nothing moves.
        poll.apply_vote(1, false, true);
        poll.apply_vote(3, true, true);
        assert_eq!(poll.counts, [2, 1, 1]);
        // Removing the vote.
        poll.set_my_votes(&[]);
        assert_eq!(poll.counts, [2, 1, 0]);
        assert!(!poll.has_vote());
        // Unknown answers are ignored.
        poll.apply_vote(99, true, false);
        assert_eq!(poll.total_votes(), 2 + 1);
    }

    #[test]
    fn gateway_events_change_the_stored_message() {
        let mut stores = Stores {
            current_user: Some(crate::User {
                id: 5,
                ..crate::User::default()
            }),
            ..Stores::default()
        };
        let message = Message::from_value(&json!({
            "id": "20", "channel_id": "10", "content": "", "type": 0,
            "timestamp": "2026-11-11T10:00:00.000000+00:00",
            "author": {"id": "7", "username": "a"},
            "poll": sample(),
        }))
        .unwrap();
        assert!(message.extra.poll.is_some());
        stores.messages.insert(10, vec![message]);
        let event = |answer: u64, user: &str| json!({"channel_id": "10", "message_id": "20", "user_id": user, "answer_id": answer});
        stores.dispatch("MESSAGE_POLL_VOTE_ADD", &event(3, "9"));
        stores.dispatch("MESSAGE_POLL_VOTE_ADD", &event(3, "5"));
        let poll = |stores: &Stores| stores.messages[&10][0].extra.poll.clone().unwrap();
        assert_eq!(poll(&stores).counts, [3, 1, 2]);
        assert_eq!(poll(&stores).my_votes(), [1, 3]);
        stores.dispatch("MESSAGE_POLL_VOTE_REMOVE", &event(1, "5"));
        assert_eq!(poll(&stores).counts, [2, 1, 2]);
        stores.dispatch(
            "MESSAGE_POLL_VOTE_ADD_MANY",
            &json!({"channel_id": "10", "message_id": "20",
                "votes": [{"answer_id": 2, "users": ["11", "12"]}]}),
        );
        assert_eq!(poll(&stores).counts, [2, 3, 2]);
        // The poll ends: the update carries the results as finalized.
        let mut done = sample();
        done["results"] = json!({"is_finalized": true, "answer_counts": [
            {"id": 1, "count": 2, "me_voted": false}, {"id": 2, "count": 3}, {"id": 3, "count": 2, "me_voted": true}]});
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "20", "channel_id": "10", "poll": done}),
        );
        let ended = poll(&stores);
        assert!(ended.finalized);
        assert_eq!(ended.counts, [2, 3, 2]);
        // The update's `me_voted` flags belong to nobody in particular: this account keeps what it had (answer 3).
        assert_eq!(ended.my_votes(), [3]);
    }
}
