//! ContentInventoryStore and member-feed predicates from 99753, 214693, 574520 and 289287.

use crate::Id;
use chrono::DateTime;
use serde_json::Value;
use std::collections::HashMap;

pub const GLOBAL_FEED: &str = "global feed";

fn id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

fn date(value: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[derive(Clone)]
pub struct Entry {
    pub id: Id,
    pub author: Id,
    pub author_type: u64,
    pub kind: u64,
    pub participants: Vec<Id>,
    pub traits: Vec<Value>,
    pub extra: Value,
    pub expires_at: Option<i64>,
    content: Value,
}

impl Entry {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            author: id(&value["author_id"])?,
            author_type: value["author_type"].as_u64().unwrap_or(0),
            kind: value["content_type"].as_u64()?,
            participants: value["participants"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
                .collect(),
            traits: value["traits"].as_array().cloned().unwrap_or_default(),
            extra: value["extra"].clone(),
            expires_at: date(&value["expires_at"]),
            content: value.clone(),
        })
    }

    /// 428249 passes the original content object to the message's unverified_content field.
    pub fn unverified_content(&self) -> &Value {
        &self.content
    }

    pub fn trait_value(&self, kind: u64, field: &str) -> Option<&Value> {
        self.traits.iter().find(|t| t["type"] == kind)?.get(field)
    }

    pub fn is_live(&self) -> bool {
        self.trait_value(3, "is_live")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        self.expires_at.is_some_and(|expires| expires < now_ms)
    }

    pub fn timestamp_ms(&self) -> i64 {
        (self.id >> 22) as i64 + 1_420_070_400_000
    }

    /// 83971: application id or game/activity name; Spotify listening sessions require a Spotify party.
    pub fn matches_activity(&self, activity: &Value) -> bool {
        if self.author_type != 1 {
            return false;
        }
        match activity["type"].as_u64() {
            Some(0) if self.extra.get("application_id").is_some() => {
                activity
                    .get("application_id")
                    .is_some_and(|app| app == &self.extra["application_id"])
                    || self
                        .extra
                        .get("game_name")
                        .or_else(|| self.extra.get("activity_name"))
                        .and_then(Value::as_str)
                        .is_some_and(|name| activity["name"] == name)
            }
            Some(2) if self.kind == 5 => {
                self.extra["entries"][0]["media"]["provider"] == 1
                    && activity["party"]["id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("spotify:"))
            }
            _ => false,
        }
    }

    pub fn can_show_in_member_list(
        &self,
        now_ms: i64,
        author_status: &str,
        matching_activity: bool,
    ) -> bool {
        matches!(self.kind, 1 | 2 | 3 | 5 | 6 | 8)
            && !self.expired(now_ms)
            && (self.kind != 5 || matching_activity)
            && (!self.is_live() || !matches!(author_status, "offline" | "invisible"))
    }

    /// 583846.aJ and the pinned English EOrEJl/LRNgHp/raJpz3/KkvKhi/sDtO6D/ITymou strings.
    pub fn age_text(&self, now_ms: i64) -> String {
        let seconds = (now_ms - self.timestamp_ms()) / 1000;
        let absolute = seconds.unsigned_abs();
        let (divisor, unit) = if absolute < 60 {
            (1., "s")
        } else if absolute < 3600 {
            (60., "m")
        } else if absolute < 12 * 3600 {
            (3600., "h")
        } else if absolute < 9 * 86400 {
            (86400., "d")
        } else if absolute < 4 * 7 * 86400 {
            (7. * 86400., "w")
        } else {
            (30. * 86400., "mo")
        };
        let count = (seconds as f64 / divisor + 0.5).floor() as i64;
        format!("{count}{unit} ago")
    }
}

pub struct Feed {
    pub entries: Vec<Entry>,
    /// `unranked_game_entries`: the game feed's unranked entries (214693 reads them when `unrankedEntries` is set).
    pub unranked_game_entries: Vec<Entry>,
    pub request_id: Option<String>,
    pub expires_at: Option<i64>,
    pub refresh_stale_inbox_after_ms: Option<u64>,
    refresh_cursor: Option<String>,
}

impl Feed {
    pub fn refresh_cursor(&self) -> Option<&str> {
        self.refresh_cursor.as_deref()
    }
}

#[derive(Default)]
pub struct ContentInventoryStore {
    feeds: HashMap<String, Feed>,
    fetched_at: HashMap<String, i64>,
}

impl ContentInventoryStore {
    pub fn get_feed(&self, feed: &str) -> Option<&Feed> {
        self.feeds.get(feed)
    }

    pub fn last_fetch_date(&self, feed: &str) -> Option<i64> {
        self.fetched_at.get(feed).copied()
    }

    pub fn set_feed(&mut self, feed: &str, body: &Value, now_ms: i64) {
        self.feeds.insert(
            feed.to_owned(),
            Feed {
                entries: body["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| Entry::from_value(&e["content"]))
                    .collect(),
                unranked_game_entries: body["unranked_game_entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| Entry::from_value(&e["content"]))
                    .collect(),
                request_id: body["request_id"].as_str().map(str::to_owned),
                expires_at: body["wait_ms_until_next_fetch"]
                    .as_i64()
                    .map(|wait| now_ms.saturating_add(wait))
                    .or_else(|| date(&body["expired_at"])),
                refresh_stale_inbox_after_ms: body["refresh_stale_inbox_after_ms"].as_u64(),
                refresh_cursor: body["refresh_token"].as_str().map(str::to_owned),
            },
        );
        self.fetched_at.insert(feed.to_owned(), now_ms);
    }

    pub fn clear_feed(&mut self, feed: &str) {
        self.feeds.remove(feed);
    }

    pub fn connection_open(&mut self) {
        self.feeds.clear();
    }
}

/// 376261: explicit guild flags override the default member-count limit; forums never use this feed.
pub fn member_feed_enabled(features: &[String], member_count: Option<u64>, forum: bool) -> bool {
    !forum
        && if features
            .iter()
            .any(|f| f == "ACTIVITY_FEED_ENABLED_BY_USER")
        {
            true
        } else if features
            .iter()
            .any(|f| f == "ACTIVITY_FEED_DISABLED_BY_USER")
        {
            false
        } else {
            member_count.is_some_and(|count| count < 10000)
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(kind: u64) -> Entry {
        Entry::from_value(&json!({"id":"4194304000","author_id":"2","author_type":1,"content_type":kind,"participants":["2"],"traits":[],"extra":{"application_id":"3","game_name":"Game"}})).unwrap()
    }

    #[test]
    fn member_feed_gating_preserves_unknown_counts_flags_and_forums() {
        assert!(!member_feed_enabled(&[], None, false));
        assert!(member_feed_enabled(&[], Some(9999), false));
        assert!(!member_feed_enabled(&[], Some(10000), false));
        let enabled = "ACTIVITY_FEED_ENABLED_BY_USER".to_owned();
        let disabled = "ACTIVITY_FEED_DISABLED_BY_USER".to_owned();
        assert!(!member_feed_enabled(
            std::slice::from_ref(&disabled),
            Some(1),
            false
        ));
        assert!(member_feed_enabled(
            &[enabled.clone(), disabled],
            None,
            false
        ));
        assert!(!member_feed_enabled(&[enabled], Some(1), true));
    }

    #[test]
    fn expired_live_and_listening_entries_follow_presence() {
        let mut game = entry(1);
        assert!(game.matches_activity(&json!({"type":0,"application_id":"3"})));
        assert!(game.matches_activity(&json!({"type":0,"name":"Game"})));
        assert!(!game.matches_activity(&json!({"type":0,"name":"Other"})));
        assert!(game.can_show_in_member_list(100, "offline", false));
        game.traits.push(json!({"type":3,"is_live":true}));
        assert!(!game.can_show_in_member_list(100, "offline", true));
        assert!(game.can_show_in_member_list(100, "online", true));
        game.expires_at = Some(100);
        assert!(!game.expired(100));
        assert!(game.expired(101));
        let mut music = entry(5);
        music.extra = json!({"entries":[{"media":{"provider":1}}]});
        assert!(!music.can_show_in_member_list(100, "online", false));
        let matched = music.matches_activity(&json!({"type":2,"party":{"id":"spotify:session"}}));
        assert!(matched && music.can_show_in_member_list(100, "online", matched));
        assert!(!music.matches_activity(&json!({"type":2,"party":{"id":"other"}})));
        assert!(!entry(7).can_show_in_member_list(100, "online", true));
    }

    #[test]
    fn feed_keeps_response_order_and_relative_age_uses_entry_timestamp() {
        let mut store = ContentInventoryStore::default();
        store.set_feed(
            GLOBAL_FEED,
            &json!({"request_id":"request","wait_ms_until_next_fetch":5000,"entries":[
                {"rank":9,"content":{"id":"4194304000","author_id":"2","content_type":1}},
                {"rank":1,"content":{"id":"8388608000","author_id":"2","content_type":6}}
            ]}),
            100,
        );
        let feed = store.get_feed(GLOBAL_FEED).unwrap();
        assert_eq!(
            feed.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![4194304000, 8388608000]
        );
        assert_eq!(feed.expires_at, Some(5100));
        let entry = &feed.entries[0];
        for (seconds, expected) in [
            (59, "59s ago"),
            (90, "2m ago"),
            (5400, "2h ago"),
            (43200, "1d ago"),
            (9 * 86400, "1w ago"),
            (28 * 86400, "1mo ago"),
        ] {
            assert_eq!(
                entry.age_text(entry.timestamp_ms() + seconds * 1000),
                expected
            );
        }
        store.connection_open();
        assert!(store.get_feed(GLOBAL_FEED).is_none());
        assert_eq!(store.last_fetch_date(GLOBAL_FEED), Some(100));
    }
}
