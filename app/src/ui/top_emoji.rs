//! "Top Emoji in {guild}": the server's most used emoji at the head of the emoji picker.
//!
//! `TopEmojiStore` (module 354430, persisted as `TopEmojiStore`: `topEmojisByGuildId`) keeps the ids of the last
//! answer of `GET /guilds/{id}/top-emojis` (module 451731 `l2`, items sorted by `emoji_rank`). Module 17591 asks again
//! only when the in-memory answer is older than a day (`topEmojisTTL`, module 121681). The section (modules 121681,
//! 818645, 920134) lists the top emoji the account can find in the server, without the newly added ones, cut so that
//! they and the server's newly added emoji (custom emoji created in the last 60 days, the newest three) fill one row
//! of 9; it comes before Favorites and is drawn only when the picker belongs to a server.
use super::Root;
use super::picker::Item;
use dc_state::Id;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) const KEY_PREFIX: &str = "top-emojis:";
/// Where the persisted store lives in storage.json.
const STORE_KEY: &str = "TopEmojiStore";
/// `topEmojisTTL`: `moment().add(1, "days")`.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// `J = fromTimestamp(Date.now() - 60 days)` in module 121681.
const NEWLY_ADDED_DAYS: u64 = 60;
/// `.slice(0, 3)` of the newly added emoji per server.
const NEWLY_ADDED_KEPT: usize = 3;
/// `rowSize = 9` of module 818645.
const ROW_SIZE: usize = 9;
/// The Discord epoch, for a snowflake's creation time.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

#[derive(Default)]
struct State {
    /// `el`: this session's answers and when they go stale.
    fresh: HashMap<Id, (Vec<Id>, Instant)>,
    /// `getIsFetching`.
    fetching: HashSet<Id>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut slot = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(slot.get_or_insert_with(State::default))
}

/// The persisted ids of `guild` (`getTopEmojiIdsByGuildId`).
fn persisted(guild: Id) -> Option<Vec<Id>> {
    let store = crate::storage::get(STORE_KEY)?;
    ids_of(&store["topEmojisByGuildId"][guild.to_string()])
}

fn ids_of(value: &Value) -> Option<Vec<Id>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|id| id.as_str()?.parse().ok())
            .collect(),
    )
}

/// The answer's emoji ids in rank order (`items.map(...).sort((e, t) => e.rank - t.rank)`).
fn ranked_ids(body: &Value) -> Option<Vec<Id>> {
    let mut items: Vec<(i64, Id)> = body["items"]
        .as_array()?
        .iter()
        .filter_map(|item| {
            let id = item["emoji_id"].as_str()?.parse().ok()?;
            Some((item["emoji_rank"].as_i64().unwrap_or(i64::MAX), id))
        })
        .collect();
    items.sort_by_key(|(rank, _)| *rank);
    Some(items.into_iter().map(|(_, id)| id).collect())
}

fn created_ms(id: Id) -> u64 {
    (id >> 22) + DISCORD_EPOCH_MS
}

/// The newest custom emoji created after `since_ms`, newest first, three at most (module 121681).
fn newly_added(emojis: &[(Id, bool)], since_ms: u64) -> Vec<Id> {
    let mut ids: Vec<Id> = emojis
        .iter()
        .filter(|(id, available)| *available && created_ms(*id) >= since_ms)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.truncate(NEWLY_ADDED_KEPT);
    ids
}

/// Module 818645 `W`: the top emoji (without the newly added ones) cut to leave room for the newly added in one row,
/// then the newly added.
fn visible(top: &[Id], newly: &[Id]) -> Vec<Id> {
    let top: Vec<Id> = top
        .iter()
        .copied()
        .filter(|id| !newly.contains(id))
        .collect();
    let room = ROW_SIZE.saturating_sub(newly.len());
    top.into_iter()
        .take(room)
        .chain(newly.iter().copied())
        .collect()
}

impl Root {
    /// Module 17591: asks for the server's top emoji unless this session's answer is less than a day old or a request
    /// is out.
    pub(super) fn top_emoji_fetch(&mut self, guild: Id) {
        let ask = with_state(|state| {
            let fresh = state
                .fresh
                .get(&guild)
                .is_some_and(|(_, until)| Instant::now() < *until);
            if fresh || state.fetching.contains(&guild) {
                false
            } else {
                state.fetching.insert(guild);
                true
            }
        });
        if ask {
            let _ = self.requests.send(crate::net::Request::Rest {
                key: Some(format!("{KEY_PREFIX}{guild}")),
                method: reqwest::Method::GET,
                route: format!("/guilds/{guild}/top-emojis"),
                body: None,
            });
        }
    }

    /// `TOP_EMOJIS_FETCH_SUCCESS` / `_FAILURE`.
    pub(super) fn top_emoji_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(guild) = key
            .strip_prefix(KEY_PREFIX)
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return false;
        };
        let ids = (!body["fetch_failed"].as_bool().unwrap_or(false))
            .then(|| ranked_ids(body))
            .flatten();
        with_state(|state| {
            state.fetching.remove(&guild);
            if let Some(ids) = &ids {
                state
                    .fresh
                    .insert(guild, (ids.clone(), Instant::now() + TTL));
            }
        });
        if let Some(ids) = ids {
            let mut store = crate::storage::get(STORE_KEY)
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            if !store["topEmojisByGuildId"].is_object() {
                store["topEmojisByGuildId"] = json!({});
            }
            store["topEmojisByGuildId"][guild.to_string()] =
                json!(ids.iter().map(Id::to_string).collect::<Vec<_>>());
            crate::storage::set(STORE_KEY, store);
            self.refresh_open_picker(cx);
            cx.notify();
        }
        true
    }

    /// The "Top Emoji in {guild}" section's emoji for the server in view, in order; empty when there are none.
    pub(super) fn top_guild_emoji(&self, guild: Id) -> Vec<Item> {
        let Some(server) = self.stores.guilds.iter().find(|g| g.id == guild) else {
            return Vec::new();
        };
        let top = with_state(|state| state.fresh.get(&guild).map(|(ids, _)| ids.clone()))
            .or_else(|| persisted(guild))
            .unwrap_or_default();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        let since = now_ms.saturating_sub(NEWLY_ADDED_DAYS * 24 * 60 * 60 * 1000);
        let all: Vec<(Id, bool)> = server.emojis.iter().map(|e| (e.id, e.available)).collect();
        let newly = newly_added(&all, since);
        // `getById`: only emoji this account can find; ids of emoji that are gone are skipped.
        let known: Vec<Id> = top
            .into_iter()
            .filter(|id| server.emojis.iter().any(|e| e.id == *id && e.available))
            .collect();
        visible(&known, &newly)
            .into_iter()
            .filter_map(|id| server.emojis.iter().find(|e| e.id == id))
            .map(|emoji| Item::Custom {
                id: emoji.id,
                name: emoji.name.clone(),
                animated: emoji.animated,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snowflake_at(ms: u64) -> Id {
        (ms - DISCORD_EPOCH_MS) << 22
    }

    #[test]
    fn the_answer_is_read_in_rank_order() {
        let body = json!({"items": [
            {"emoji_id": "30", "emoji_rank": 3},
            {"emoji_id": "10", "emoji_rank": 1},
            {"emoji_id": "20", "emoji_rank": 2},
        ]});
        assert_eq!(ranked_ids(&body), Some(vec![10, 20, 30]));
        assert_eq!(ranked_ids(&json!({})), None);
    }

    #[test]
    fn newly_added_are_the_newest_three_of_the_last_sixty_days() {
        let now = DISCORD_EPOCH_MS + 400 * 86_400_000;
        let since = now - NEWLY_ADDED_DAYS * 86_400_000;
        let old = snowflake_at(since - 1000);
        let recent: Vec<Id> = (1..=4)
            .map(|d| snowflake_at(now - d * 86_400_000))
            .collect();
        let mut all: Vec<(Id, bool)> = recent.iter().map(|id| (*id, true)).collect();
        all.push((old, true));
        assert_eq!(
            newly_added(&all, since),
            vec![recent[0], recent[1], recent[2]]
        );
    }

    #[test]
    fn top_emoji_fill_one_row_of_nine_with_the_newly_added_last() {
        let top: Vec<Id> = (1..=12).collect();
        assert_eq!(visible(&top, &[]), (1..=9).collect::<Vec<_>>());
        let shown = visible(&top, &[100, 3]);
        assert_eq!(shown, vec![1, 2, 4, 5, 6, 7, 8, 100, 3]);
    }
}
