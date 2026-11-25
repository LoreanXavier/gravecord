//! What the client asks the gateway to send about each guild (op 37 `GUILD_SUBSCRIPTIONS_BULK`). Port of the
//! subscription manager of `GuildSubscriptionsStore` (module 661670: guild features, member list channels and the
//! reconnect paths), its per-guild channel ranges (module 36124), the member list's range maths (module 361610 `NJ`)
//! and the bulk sender (module 868 `updateGuildSubscriptions`).

use crate::Id;
use serde_json::{Map, Value, json};

/// A member list range, first and last row.
pub type Range = [u64; 2];

/// 36124 `L`: the ranges a channel starts with.
pub const DEFAULT_RANGES: [Range; 1] = [[0, 99]];

/// 36124: the channels a guild keeps ranges for (an LRU cache of `max: 5`).
const MAX_CHANNELS: usize = 5;

/// 868 `updateGuildSubscriptions`: a bulk send holds at most this many characters of `[guild, subscription]`.
const BULK_LIMIT: usize = 15360;

/// 361610 `NJ`: the 100-row ranges a member list asks for when its scroller is `y` down and `height` tall with
/// `row_height` rows. Half a screen of rows is kept on each side; the first range is always there.
pub fn ranges_for(y: f32, height: f32, row_height: f32) -> Vec<Range> {
    if row_height <= 0. || !y.is_finite() || !height.is_finite() {
        return DEFAULT_RANGES.to_vec();
    }
    let rows = |pixels: f32, extra: i64| -> u64 {
        ((f64::from(pixels) / f64::from(row_height)).ceil() as i64 + extra).max(0) as u64
    };
    let mut ranges = Vec::new();
    let mut push = |start: u64| {
        let end = start + 99;
        ranges.push([start, end]);
        end + 1
    };
    let around = rows(0.5 * height, 0) as i64;
    let mut start = rows(y, -around);
    let end = rows(y + height, around);
    if start > 0 {
        start = push(0).max(start);
    }
    start = 100 * (start / 100);
    while start <= end {
        start = push(start);
    }
    ranges
}

/// One guild's subscription state.
#[derive(Clone, Debug, Default)]
struct Guild {
    id: Id,
    /// Member list channels and their ranges, the most recently used first (36124).
    channels: Vec<(Id, Vec<Range>)>,
    typing: bool,
    activities: bool,
    threads: bool,
    /// Sent at least once (661670 `_subscribed`).
    subscribed: bool,
}

impl Guild {
    /// 661670 `get`: the guild's whole subscription.
    fn whole(&self) -> Value {
        json!({
            "typing": self.typing,
            "threads": self.threads,
            "activities": self.activities,
            "members": [],
            "member_updates": false,
            "channels": self.channels_value(),
            "thread_member_lists": [],
        })
    }

    fn channels_value(&self) -> Value {
        let mut channels = Map::new();
        for (channel, ranges) in &self.channels {
            channels.insert(channel.to_string(), json!(ranges));
        }
        Value::Object(channels)
    }
}

/// 661670 `U`: the subscriptions of every guild and the changes not sent yet.
#[derive(Clone, Debug, Default)]
pub struct GuildSubscriptions {
    guilds: Vec<Guild>,
    pending: Map<String, Value>,
}

impl GuildSubscriptions {
    fn guild_mut(&mut self, guild: Id) -> &mut Guild {
        if let Some(index) = self.guilds.iter().position(|known| known.id == guild) {
            return &mut self.guilds[index];
        }
        self.guilds.push(Guild {
            id: guild,
            ..Guild::default()
        });
        self.guilds.last_mut().expect("just pushed")
    }

    fn enqueue(&mut self, guild: Id, field: &str, value: Value) {
        let entry = self
            .pending
            .entry(guild.to_string())
            .or_insert_with(|| json!({}));
        entry[field] = value;
    }

    /// `subscribeToGuild`: typing, activities and threads, each sent the first time.
    pub fn subscribe_guild(&mut self, guild: Id) {
        let known = self.guild_mut(guild);
        let wanted = [
            ("typing", !std::mem::replace(&mut known.typing, true)),
            (
                "activities",
                !std::mem::replace(&mut known.activities, true),
            ),
            ("threads", !std::mem::replace(&mut known.threads, true)),
        ];
        for (field, new) in wanted {
            if new {
                self.enqueue(guild, field, json!(true));
            }
        }
    }

    /// `subscribeChannel` (36124 `subscribe`): the channel's member list ranges. Looking the channel up makes it the
    /// most recently used; nothing is sent when its ranges are already these, or when `only_if_new` and it has some.
    /// A sixth channel pushes out the least recently used. True when the guild's channels changed.
    pub fn subscribe_channel(
        &mut self,
        guild: Id,
        channel: Id,
        ranges: &[Range],
        only_if_new: bool,
    ) -> bool {
        let known = self.guild_mut(guild);
        let existing = known
            .channels
            .iter()
            .position(|(id, _)| *id == channel)
            .map(|index| known.channels.remove(index));
        if let Some(existing) = existing {
            let unchanged = only_if_new || existing.1 == ranges;
            known.channels.insert(0, existing);
            if unchanged {
                return false;
            }
            known.channels[0].1 = ranges.to_vec();
        } else {
            known.channels.insert(0, (channel, ranges.to_vec()));
            known.channels.truncate(MAX_CHANNELS);
        }
        let channels = known.channels_value();
        self.enqueue(guild, "channels", channels);
        true
    }

    /// The ranges kept for a channel's member list.
    pub fn ranges(&self, guild: Id, channel: Id) -> Option<&[Range]> {
        let known = self.guilds.iter().find(|known| known.id == guild)?;
        known
            .channels
            .iter()
            .find(|(id, _)| *id == channel)
            .map(|(_, ranges)| ranges.as_slice())
    }

    /// `flush`: the changes to send (guild id to subscription), and the guilds now count as subscribed.
    pub fn take_pending(&mut self) -> Option<Map<String, Value>> {
        if self.pending.is_empty() {
            return None;
        }
        let pending = std::mem::take(&mut self.pending);
        for guild in pending.keys() {
            if let Ok(id) = guild.parse::<Id>() {
                self.guild_mut(id).subscribed = true;
            }
        }
        Some(pending)
    }

    /// `clearWithoutFlushing`: the guild's channels and features go without telling the gateway. Without
    /// `with_threads` it keeps its threads subscription (and so stays subscribed when it has one).
    fn clear_without_flushing(&mut self, guild: Id, with_threads: bool) {
        self.pending.remove(&guild.to_string());
        let Some(index) = self.guilds.iter().position(|known| known.id == guild) else {
            return;
        };
        let known = &mut self.guilds[index];
        known.channels.clear();
        known.typing = false;
        known.activities = false;
        if with_threads {
            known.threads = false;
        }
        if with_threads || !known.threads {
            known.subscribed = false;
        }
        if !known.subscribed && !known.threads {
            self.guilds.remove(index);
        }
    }

    /// 661670 `w`: every subscribed guild but those in `keep` (the open guild) is cleared.
    fn clear_others(&mut self, keep: &[Id], with_threads: bool) {
        let others: Vec<Id> = self
            .guilds
            .iter()
            .filter(|known| known.subscribed && !keep.contains(&known.id))
            .map(|known| known.id)
            .collect();
        for guild in others {
            self.clear_without_flushing(guild, with_threads);
        }
    }

    /// `CONNECTION_OPEN` (`k` after `w(true)`): a new session. The guilds other than `keep` are dropped, and the
    /// whole subscription of each guild still subscribed (and still `known`) is sent again, member list ranges and all.
    pub fn connection_open(
        &mut self,
        keep: &[Id],
        known: impl Fn(Id) -> bool,
    ) -> Option<Map<String, Value>> {
        self.clear_others(keep, true);
        self.resend(known)
    }

    /// `CONNECTION_RESUMED` (`k`): the whole subscription of every subscribed guild is sent again.
    pub fn connection_resumed(&mut self, known: impl Fn(Id) -> bool) -> Option<Map<String, Value>> {
        self.resend(known)
    }

    /// `CONNECTION_CLOSED` (`w(false)`): the guilds other than `keep` are cleared without telling the gateway, each
    /// keeping its threads subscription.
    pub fn connection_closed(&mut self, keep: &[Id]) {
        self.clear_others(keep, false);
    }

    fn resend(&mut self, known: impl Fn(Id) -> bool) -> Option<Map<String, Value>> {
        let gone: Vec<Id> = self
            .guilds
            .iter()
            .filter(|guild| guild.subscribed && !known(guild.id))
            .map(|guild| guild.id)
            .collect();
        for guild in gone {
            self.clear_without_flushing(guild, true);
        }
        let mut whole = Map::new();
        for guild in self.guilds.iter().filter(|guild| guild.subscribed) {
            whole.insert(guild.id.to_string(), guild.whole());
        }
        (!whole.is_empty()).then_some(whole)
    }

    /// `LOGOUT`.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// 868 `updateGuildSubscriptions`: the op 37 payloads for `subscriptions`, a new one each time the next guild's
/// `[guild, subscription]` would take the current one past 15360 characters.
pub fn bulk_payloads(subscriptions: &Map<String, Value>) -> Vec<Value> {
    let mut payloads = Vec::new();
    let mut batch = Map::new();
    let mut size = 0;
    for (guild, subscription) in subscriptions {
        let length = json!([guild, subscription]).to_string().chars().count();
        if size + length > BULK_LIMIT {
            payloads.push(json!({ "subscriptions": std::mem::take(&mut batch) }));
            size = 0;
        }
        batch.insert(guild.clone(), subscription.clone());
        size += length;
    }
    if size > 0 {
        payloads.push(json!({ "subscriptions": batch }));
    }
    payloads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_screen_asks_for_the_first_hundred() {
        // A 900px member list at the top: rows 0..22 with half a screen below.
        assert_eq!(ranges_for(0., 900., 42.), vec![[0, 99]]);
        assert_eq!(ranges_for(1000., 900., 42.), vec![[0, 99]]);
    }

    #[test]
    fn scrolling_asks_for_the_windows_around_the_view_and_keeps_the_first() {
        // 4000px down (row 96): rows 85..129 are wanted, so the second hundred joins.
        assert_eq!(ranges_for(4000., 900., 42.), vec![[0, 99], [100, 199]]);
        // Row 238 at the top: rows 227..271 sit in the third hundred only; the first stays.
        assert_eq!(ranges_for(10000., 900., 42.), vec![[0, 99], [200, 299]]);
        // Across a hundred boundary both hundreds come.
        assert_eq!(
            ranges_for(8000., 900., 42.),
            vec![[0, 99], [100, 199], [200, 299]]
        );
        assert_eq!(ranges_for(0., 0., 0.), vec![[0, 99]]);
    }

    fn sent(subscriptions: &mut GuildSubscriptions) -> Value {
        Value::Object(subscriptions.take_pending().unwrap_or_default())
    }

    #[test]
    fn a_channel_select_sends_the_features_once_and_the_first_range() {
        let mut subscriptions = GuildSubscriptions::default();
        subscriptions.subscribe_guild(1);
        subscriptions.subscribe_channel(1, 10, &DEFAULT_RANGES, true);
        assert_eq!(
            sent(&mut subscriptions),
            json!({"1": {"typing": true, "activities": true, "threads": true, "channels": {"10": [[0, 99]]}}})
        );
        // Selecting it again sends nothing.
        subscriptions.subscribe_guild(1);
        subscriptions.subscribe_channel(1, 10, &DEFAULT_RANGES, true);
        assert!(subscriptions.take_pending().is_none());
        // The recorded official send for a second channel of a subscribed guild: the channels alone.
        subscriptions.subscribe_channel(1, 11, &DEFAULT_RANGES, true);
        assert_eq!(
            sent(&mut subscriptions),
            json!({"1": {"channels": {"11": [[0, 99]], "10": [[0, 99]]}}})
        );
    }

    #[test]
    fn scrolled_ranges_replace_the_channels_own_and_five_channels_are_kept() {
        let mut subscriptions = GuildSubscriptions::default();
        for channel in 10..16 {
            subscriptions.subscribe_channel(1, channel, &DEFAULT_RANGES, true);
        }
        subscriptions.take_pending();
        assert!(subscriptions.ranges(1, 10).is_none());
        assert!(!subscriptions.subscribe_channel(1, 15, &[[0, 99]], false));
        assert!(subscriptions.subscribe_channel(1, 15, &[[0, 99], [100, 199]], false));
        assert_eq!(
            sent(&mut subscriptions)["1"]["channels"],
            json!({"15": [[0, 99], [100, 199]], "14": [[0, 99]], "13": [[0, 99]], "12": [[0, 99]], "11": [[0, 99]]})
        );
        // A channel select keeps the scrolled ranges.
        assert!(!subscriptions.subscribe_channel(1, 15, &DEFAULT_RANGES, true));
        assert_eq!(
            subscriptions.ranges(1, 15),
            Some(&[[0, 99], [100, 199]][..])
        );
    }

    #[test]
    fn a_new_session_resends_the_open_guild_whole_and_drops_the_others() {
        let mut subscriptions = GuildSubscriptions::default();
        for guild in [1, 2] {
            subscriptions.subscribe_guild(guild);
            subscriptions.subscribe_channel(guild, guild * 10, &DEFAULT_RANGES, true);
        }
        subscriptions.subscribe_channel(1, 10, &[[0, 99], [300, 399]], false);
        subscriptions.take_pending();
        let whole = subscriptions.connection_open(&[1], |_| true).unwrap();
        assert_eq!(
            Value::Object(whole),
            json!({"1": {"typing": true, "threads": true, "activities": true, "members": [], "member_updates": false, "channels": {"10": [[0, 99], [300, 399]]}, "thread_member_lists": []}})
        );
        assert!(subscriptions.ranges(2, 20).is_none());
        // Opening the other guild again subscribes it from the start.
        subscriptions.subscribe_guild(2);
        assert_eq!(
            sent(&mut subscriptions),
            json!({"2": {"typing": true, "activities": true, "threads": true}})
        );
    }

    #[test]
    fn a_resume_resends_every_subscribed_guild() {
        let mut subscriptions = GuildSubscriptions::default();
        assert!(subscriptions.connection_resumed(|_| true).is_none());
        for guild in [1, 2] {
            subscriptions.subscribe_guild(guild);
            subscriptions.subscribe_channel(guild, guild * 10, &DEFAULT_RANGES, true);
        }
        // Not sent yet: nothing to resend.
        assert!(subscriptions.connection_resumed(|_| true).is_none());
        subscriptions.take_pending();
        let whole = subscriptions
            .connection_resumed(|guild| guild != 2)
            .unwrap();
        assert_eq!(whole.keys().collect::<Vec<_>>(), ["1"]);
        assert_eq!(whole["1"]["channels"], json!({"10": [[0, 99]]}));
    }

    #[test]
    fn a_closed_connection_keeps_only_the_threads_of_other_guilds() {
        let mut subscriptions = GuildSubscriptions::default();
        for guild in [1, 2] {
            subscriptions.subscribe_guild(guild);
            subscriptions.subscribe_channel(guild, guild * 10, &DEFAULT_RANGES, true);
        }
        subscriptions.take_pending();
        subscriptions.connection_closed(&[1]);
        let whole = subscriptions.connection_resumed(|_| true).unwrap();
        assert_eq!(whole["1"]["channels"], json!({"10": [[0, 99]]}));
        assert_eq!(
            whole["2"],
            json!({"typing": false, "threads": true, "activities": false, "members": [], "member_updates": false, "channels": {}, "thread_member_lists": []})
        );
    }

    #[test]
    fn bulk_sends_split_past_the_size_limit() {
        let mut subscriptions = Map::new();
        subscriptions.insert("1".into(), json!({"typing": true}));
        assert_eq!(
            bulk_payloads(&subscriptions),
            vec![json!({"subscriptions": {"1": {"typing": true}}})]
        );
        let big: Vec<Range> = (0..2000).map(|n| [n * 100, n * 100 + 99]).collect();
        subscriptions.insert("2".into(), json!({"channels": {"3": big}}));
        let payloads = bulk_payloads(&subscriptions);
        assert_eq!(payloads.len(), 2);
        assert!(payloads[0]["subscriptions"].get("1").is_some());
        assert!(payloads[1]["subscriptions"].get("2").is_some());
    }
}
