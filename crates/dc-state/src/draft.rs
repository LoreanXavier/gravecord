//! `DraftStore` (module 31717): what the user typed in each channel's message box and has not sent. The state is
//! keyed by user, then channel, then draft type, each entry `{timestamp, draft, command?}`, and is a persisted store:
//! `localStorage["DraftStore"] = {_state, _version: 2}` (module 855986). The action creators are module 465532.
//!
//! The store has no age limit: a draft stays until it is sent, emptied, its channel goes away or the user logs out.

use crate::Id;
use serde_json::{Map, Value, json};

/// The `persistKey` and `localStorage` key.
pub const PERSIST_KEY: &str = "DraftStore";

/// `migrations.length`: the `_version` the store writes.
pub const VERSION: u64 = 2;

/// `I()` cuts a saved draft to its first 4500 UTF-16 code units (`substr(0, 4500)`).
pub const MAX_DRAFT_LENGTH: usize = 4500;

/// The draft types (`C` of module 31717).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DraftType {
    ChannelMessage = 0,
    ThreadSettings = 1,
    FirstThreadMessage = 2,
    ApplicationLauncherCommand = 3,
    Poll = 4,
    SlashCommand = 5,
    ForwardContextMessage = 6,
    InteractionModal = 7,
    ScheduledMessage = 8,
}

impl DraftType {
    fn key(self) -> String {
        (self as u8).to_string()
    }
}

/// One entry of `getRecentlyEditedDrafts`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentDraft {
    pub channel: Id,
    pub timestamp: i64,
    pub draft: String,
}

#[derive(Default)]
pub struct DraftStore {
    /// `f`: user id, then channel id, then draft type, to the entry object.
    state: Map<String, Value>,
    /// `AuthenticationStore.getId()`.
    user: Option<String>,
    /// The store emitted a change since the last write (a `PersistedStore` writes on change).
    dirty: bool,
    /// READY came without the private channels (a user account gets them in READY_SUPPLEMENTAL): the prune waits
    /// for them, so a DM's draft is not taken for a channel the client does not know.
    prune_pending: bool,
}

/// `substr(0, 4500)`, kept on a character boundary.
fn cut(draft: &str) -> &str {
    let mut units = 0;
    for (at, c) in draft.char_indices() {
        units += c.len_utf16();
        if units > MAX_DRAFT_LENGTH {
            return &draft[..at];
        }
    }
    draft
}

impl DraftStore {
    /// `initialize` over what the `PersistedStore` read back: the `{_state, _version}` object, run through the
    /// migrations it is behind on, then every channel-message draft that is empty or only whitespace is dropped.
    pub fn initialize(persisted: Option<&Value>, user: Option<Id>) -> Self {
        let user = user.map(|user| user.to_string());
        let mut migrated = false;
        let state = match persisted.and_then(Value::as_object) {
            None => Map::new(),
            Some(object) => {
                let version = object.get("_version").and_then(Value::as_u64);
                let mut state = match version {
                    // A store from before versions: the whole object, less the two fields, is the state.
                    None => {
                        let mut rest = object.clone();
                        rest.remove("_state");
                        rest.remove("_version");
                        Value::Object(rest)
                    }
                    Some(_) => object.get("_state").cloned().unwrap_or(Value::Null),
                };
                let mut at = version.unwrap_or(0);
                while at < VERSION {
                    state = migrate(at, state, user.as_deref());
                    migrated = true;
                    at += 1;
                }
                state.as_object().cloned().unwrap_or_default()
            }
        };
        let mut store = Self {
            state,
            user,
            dirty: migrated,
            prune_pending: false,
        };
        let empties: Vec<(String, String)> = store
            .state
            .iter()
            .flat_map(|(user, channels)| {
                channels
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, types)| {
                        types["0"]["draft"]
                            .as_str()
                            .is_some_and(|draft| draft.trim().is_empty())
                    })
                    .map(|(channel, _)| (user.clone(), channel.clone()))
            })
            .collect();
        for (user, channel) in empties {
            store.remove(&user, &channel, DraftType::ChannelMessage);
        }
        store
    }

    /// `getState`, as the `PersistedStore` writes it.
    pub fn persisted(&self) -> Value {
        json!({ "_state": self.state, "_version": VERSION })
    }

    /// Whether the store changed since the last call: the caller writes `persisted()` when it did.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn channels(&self) -> Option<&Map<String, Value>> {
        self.state.get(self.user.as_deref()?)?.as_object()
    }

    fn entry(&self, channel: Id, kind: DraftType) -> Option<&Value> {
        self.channels()?.get(&channel.to_string())?.get(kind.key())
    }

    /// `getDraft`: the text, or empty.
    pub fn get_draft(&self, channel: Id, kind: DraftType) -> &str {
        self.entry(channel, kind)
            .and_then(|entry| entry["draft"].as_str())
            .unwrap_or("")
    }

    /// `getRecentlyEditedDrafts`: the drafts of a type, newest first.
    pub fn get_recently_edited_drafts(&self, kind: DraftType) -> Vec<RecentDraft> {
        let mut out: Vec<RecentDraft> = self
            .channels()
            .into_iter()
            .flatten()
            .filter_map(|(channel, types)| {
                let entry = types.get(kind.key())?;
                Some(RecentDraft {
                    channel: channel.parse().ok()?,
                    timestamp: entry["timestamp"].as_i64().unwrap_or(0),
                    draft: entry["draft"].as_str().unwrap_or("").to_owned(),
                })
            })
            .collect();
        out.sort_by_key(|draft| std::cmp::Reverse(draft.timestamp));
        out
    }

    /// `p()`: deletes one entry, and the channel once it has none.
    fn remove(&mut self, user: &str, channel: &str, kind: DraftType) -> bool {
        let Some(channels) = self.state.get_mut(user).and_then(Value::as_object_mut) else {
            return false;
        };
        let Some(types) = channels.get_mut(channel).and_then(Value::as_object_mut) else {
            return false;
        };
        let removed = types.remove(&kind.key()).is_some();
        if types.is_empty() {
            channels.remove(channel);
        }
        removed
    }

    /// `I()`, shared by `DRAFT_SAVE` and `DRAFT_CHANGE`. A draft equal to the channel's template counts as empty; an
    /// empty draft deletes the entry; an unchanged one keeps its timestamp.
    fn put(
        &mut self,
        channel: Id,
        draft: &str,
        kind: DraftType,
        template: Option<&str>,
        now_ms: i64,
    ) {
        let draft = if template == Some(draft) { "" } else { draft };
        let Some(user) = self.user.clone() else {
            return;
        };
        if draft.is_empty() {
            self.remove(&user, &channel.to_string(), kind);
            return;
        }
        let draft = cut(draft);
        let channels = self
            .state
            .entry(user)
            .or_insert_with(|| Value::Object(Map::new()));
        if !channels.is_object() {
            *channels = Value::Object(Map::new());
        }
        let types = channels
            .as_object_mut()
            .expect("an object")
            .entry(channel.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !types.is_object() {
            *types = Value::Object(Map::new());
        }
        let types = types.as_object_mut().expect("an object");
        if types.get(&kind.key()).is_some_and(|entry| {
            entry["draft"].as_str() == Some(draft) && entry.get("command").is_none()
        }) {
            return;
        }
        types.insert(kind.key(), json!({ "timestamp": now_ms, "draft": draft }));
    }

    /// `DRAFT_SAVE` (`saveDraft`): stores the text and emits, so the store is written.
    pub fn save(
        &mut self,
        channel: Id,
        draft: &str,
        kind: DraftType,
        template: Option<&str>,
        now_ms: i64,
    ) {
        self.put(channel, draft, kind, template, now_ms);
        self.dirty = true;
    }

    /// `DRAFT_CHANGE` (`changeDraft`, every keystroke): the same change without an emit, so nothing is written until
    /// the next save.
    pub fn change(
        &mut self,
        channel: Id,
        draft: &str,
        kind: DraftType,
        template: Option<&str>,
        now_ms: i64,
    ) {
        self.put(channel, draft, kind, template, now_ms);
    }

    /// `DRAFT_CLEAR` (`clearDraft`).
    pub fn clear(&mut self, channel: Id, kind: DraftType) {
        if let Some(user) = self.user.clone()
            && self.remove(&user, &channel.to_string(), kind)
        {
            self.dirty = true;
        }
    }

    /// `m()`: with every server available, drops the drafts of channels the client no longer knows.
    fn prune(&mut self, unavailable_guilds: usize, exists: &dyn Fn(Id) -> bool) {
        if unavailable_guilds > 0 {
            return;
        }
        let Some(channels) = self
            .user
            .as_deref()
            .and_then(|user| self.state.get_mut(user))
            .and_then(Value::as_object_mut)
        else {
            return;
        };
        channels.retain(|channel, _| channel.parse::<Id>().is_ok_and(exists));
    }

    /// `CONNECTION_OPEN`: the user's map exists from now on, then the prune. Emits nothing.
    pub fn connection_open(
        &mut self,
        user: Id,
        unavailable_guilds: usize,
        exists: &dyn Fn(Id) -> bool,
    ) {
        let user = user.to_string();
        self.state
            .entry(user.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        self.user = Some(user);
        self.prune(unavailable_guilds, exists);
    }

    /// `GUILD_DELETE`: the prune. Emits nothing.
    pub fn guild_delete(&mut self, unavailable_guilds: usize, exists: &dyn Fn(Id) -> bool) {
        self.prune(unavailable_guilds, exists);
    }

    /// `CHANNEL_DELETE` and `THREAD_DELETE`: every draft of the channel goes. Emits nothing.
    pub fn channel_delete(&mut self, channel: Id) {
        if let Some(channels) = self
            .user
            .as_deref()
            .and_then(|user| self.state.get_mut(user))
            .and_then(Value::as_object_mut)
        {
            channels.remove(&channel.to_string());
        }
    }

    /// `LOGOUT`: everything goes, unless the user is only switching accounts.
    pub fn logout(&mut self, switching_account: bool) {
        if !switching_account {
            self.state.clear();
            self.dirty = true;
        }
    }

    /// `MULTI_ACCOUNT_REMOVE_ACCOUNT`.
    pub fn remove_account(&mut self, user: Id) {
        if self.state.remove(&user.to_string()).is_some() {
            self.dirty = true;
        }
    }
}

/// The store's two migrations (`migrations` of module 31717).
fn migrate(from: u64, state: Value, user: Option<&str>) -> Value {
    match from {
        // A draft per channel becomes the channel-message draft of the channel.
        0 => {
            let Value::Object(mut channels) = state else {
                return json!({});
            };
            for value in channels.values_mut() {
                if value.get("timestamp").is_some() {
                    *value = json!({ "0": value.take() });
                }
            }
            Value::Object(channels)
        }
        // Everything moves under the current user.
        _ => match (state, user) {
            (Value::Object(channels), Some(user)) => json!({ user: channels }),
            _ => json!({}),
        },
    }
}

impl crate::Stores {
    /// Whether the client knows the channel (`ChannelStore.getChannel(id) != null`): a server channel, a thread or a
    /// private channel.
    fn draft_channel_known(&self, channel: Id) -> bool {
        self.private_channels
            .iter()
            .any(|known| known.id == channel)
            || self.guilds.iter().any(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
    }

    /// The DraftStore's gateway and session actions.
    pub(crate) fn drafts_dispatch(&mut self, name: &str, data: &Value) {
        let unavailable = self.unavailable_guilds.len();
        match name {
            "READY" => {
                if let Some(user) = self.current_user.as_ref().map(|user| user.id) {
                    let mut drafts = std::mem::take(&mut self.drafts);
                    let dms_pending = !data["private_channels"]
                        .as_array()
                        .is_some_and(|channels| !channels.is_empty());
                    if dms_pending {
                        drafts.connection_open(user, unavailable, &|_| true);
                        drafts.prune_pending = true;
                    } else {
                        drafts.connection_open(user, unavailable, &|channel| {
                            self.draft_channel_known(channel)
                        });
                    }
                    self.drafts = drafts;
                }
            }
            "READY_SUPPLEMENTAL" if self.drafts.prune_pending => {
                let mut drafts = std::mem::take(&mut self.drafts);
                drafts.prune_pending = false;
                drafts.guild_delete(unavailable, &|channel| self.draft_channel_known(channel));
                self.drafts = drafts;
            }
            "GUILD_DELETE" => {
                let mut drafts = std::mem::take(&mut self.drafts);
                drafts.guild_delete(unavailable, &|channel| self.draft_channel_known(channel));
                self.drafts = drafts;
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                if let Some(channel) = crate::id(&data["id"]) {
                    self.drafts.channel_delete(channel);
                }
            }
            "LOGOUT" => self.drafts.logout(data["isSwitchingAccount"] == true),
            "MULTI_ACCOUNT_REMOVE_ACCOUNT" => {
                if let Some(user) = crate::id(&data["userId"]) {
                    self.drafts.remove_account(user);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: Id = 10;

    fn store() -> DraftStore {
        let mut store = DraftStore::initialize(None, None);
        store.connection_open(ME, 0, &|_| true);
        store
    }

    #[test]
    fn each_channel_keeps_its_own_draft() {
        let mut store = store();
        store.change(1, "hello", DraftType::ChannelMessage, None, 5);
        store.change(2, "other", DraftType::ChannelMessage, None, 6);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "hello");
        assert_eq!(store.get_draft(2, DraftType::ChannelMessage), "other");
        assert_eq!(store.get_draft(3, DraftType::ChannelMessage), "");
        assert_eq!(store.get_draft(1, DraftType::FirstThreadMessage), "");
    }

    #[test]
    fn change_does_not_write_and_save_does() {
        let mut store = store();
        store.take_dirty();
        store.change(1, "a", DraftType::ChannelMessage, None, 1);
        assert!(!store.take_dirty());
        store.save(1, "ab", DraftType::ChannelMessage, None, 2);
        assert!(store.take_dirty());
    }

    #[test]
    fn an_empty_draft_or_the_template_deletes_the_entry() {
        let mut store = store();
        store.save(1, "text", DraftType::ChannelMessage, None, 1);
        store.save(1, "", DraftType::ChannelMessage, None, 2);
        assert_eq!(store.persisted()["_state"]["10"], json!({}));
        store.save(
            1,
            "Template",
            DraftType::ChannelMessage,
            Some("Template"),
            3,
        );
        assert_eq!(store.persisted()["_state"]["10"], json!({}));
    }

    #[test]
    fn a_draft_is_cut_at_4500_utf16_units() {
        let mut store = store();
        let long = "x".repeat(4499) + "\u{1F600}";
        store.save(1, &long, DraftType::ChannelMessage, None, 1);
        assert_eq!(
            store.get_draft(1, DraftType::ChannelMessage),
            "x".repeat(4499)
        );
        store.save(1, &"y".repeat(5000), DraftType::ChannelMessage, None, 1);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage).len(), 4500);
    }

    #[test]
    fn an_unchanged_draft_keeps_its_timestamp() {
        let mut store = store();
        store.save(1, "same", DraftType::ChannelMessage, None, 1);
        store.save(1, "same", DraftType::ChannelMessage, None, 9);
        assert_eq!(store.persisted()["_state"]["10"]["1"]["0"]["timestamp"], 1);
    }

    #[test]
    fn the_persisted_shape_round_trips_and_drops_blank_drafts() {
        let mut store = store();
        store.save(1, "kept", DraftType::ChannelMessage, None, 7);
        store.save(2, "   ", DraftType::ChannelMessage, None, 8);
        store.save(2, "thread first", DraftType::FirstThreadMessage, None, 8);
        let written = store.persisted();
        assert_eq!(written["_version"], 2);
        assert_eq!(
            written["_state"]["10"]["1"]["0"],
            json!({"timestamp": 7, "draft": "kept"})
        );
        let back = DraftStore::initialize(Some(&written), Some(ME));
        assert_eq!(back.get_draft(1, DraftType::ChannelMessage), "kept");
        assert_eq!(back.get_draft(2, DraftType::ChannelMessage), "");
        assert_eq!(
            back.get_draft(2, DraftType::FirstThreadMessage),
            "thread first"
        );
    }

    #[test]
    fn old_stores_migrate_under_the_user() {
        let v0 = json!({"5": {"timestamp": 1, "draft": "old"}});
        let store = DraftStore::initialize(Some(&v0), Some(ME));
        assert_eq!(store.get_draft(5, DraftType::ChannelMessage), "old");
        let v1 = json!({"_state": {"5": {"0": {"timestamp": 1, "draft": "v1"}}}, "_version": 1});
        let mut store = DraftStore::initialize(Some(&v1), Some(ME));
        assert_eq!(store.get_draft(5, DraftType::ChannelMessage), "v1");
        assert!(store.take_dirty());
        let unknown_user = DraftStore::initialize(Some(&v1), None);
        assert_eq!(unknown_user.persisted()["_state"], json!({}));
    }

    #[test]
    fn connection_open_prunes_unknown_channels_only_with_every_guild_available() {
        let mut store = store();
        store.save(1, "a", DraftType::ChannelMessage, None, 1);
        store.save(2, "b", DraftType::ChannelMessage, None, 1);
        store.connection_open(ME, 1, &|channel| channel == 1);
        assert_eq!(store.get_draft(2, DraftType::ChannelMessage), "b");
        store.connection_open(ME, 0, &|channel| channel == 1);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "a");
        assert_eq!(store.get_draft(2, DraftType::ChannelMessage), "");
    }

    #[test]
    fn deletes_clears_and_logout() {
        let mut store = store();
        store.save(1, "a", DraftType::ChannelMessage, None, 1);
        store.save(2, "b", DraftType::ChannelMessage, None, 1);
        store.save(3, "c", DraftType::ChannelMessage, None, 1);
        store.channel_delete(1);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "");
        store.take_dirty();
        store.clear(2, DraftType::ChannelMessage);
        assert!(store.take_dirty());
        store.clear(2, DraftType::ChannelMessage);
        assert!(!store.take_dirty());
        store.logout(true);
        assert_eq!(store.get_draft(3, DraftType::ChannelMessage), "c");
        store.logout(false);
        assert_eq!(store.get_draft(3, DraftType::ChannelMessage), "");
    }

    #[test]
    fn recently_edited_drafts_are_newest_first() {
        let mut store = store();
        store.save(1, "old", DraftType::ChannelMessage, None, 1);
        store.save(2, "new", DraftType::ChannelMessage, None, 2);
        let recent = store.get_recently_edited_drafts(DraftType::ChannelMessage);
        assert_eq!(
            recent.iter().map(|draft| draft.channel).collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn drafts_belong_to_their_account() {
        let mut store = store();
        store.save(1, "mine", DraftType::ChannelMessage, None, 1);
        store.connection_open(11, 0, &|_| true);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "");
        store.connection_open(ME, 0, &|_| true);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "mine");
        store.remove_account(ME);
        assert_eq!(store.get_draft(1, DraftType::ChannelMessage), "");
    }
}
