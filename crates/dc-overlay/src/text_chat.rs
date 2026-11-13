//! `Overlay-v3-Text-Chat-Store` (module 288641): the chats the overlay's Chat widget lists and which one is open.
//!
//! - Entries are kept in two groups (private channels first, then guild channels), each ordered by mentions before
//!   unreads before read ones, more mentions first, then the most recent activity, then the channel id (`y`).
//! - `OVERLAY_MOUNTED` (once, `x`): the channel selected in the app joins as the open chat; then up to three private
//!   channels with a message in the last 30 minutes (not muted, not message requests, in the private channel order);
//!   with nothing open, the voice channel's text chat opens unless it is minimized (`B`).
//! - Selecting a chat (`OVERLAY_TEXT_CHAT_SELECT_CHANNEL`) opens it and adds it when missing; removing the open one
//!   opens the next entry (`F`), or the voice channel's chat.
//! - Leaving or switching the voice channel closes its chat when it was open and opens the next (`V`).
//!
//! What the store reads from other stores is handed in: [`ChannelInfo`] for one channel, [`Mount`] at mount time.

use std::collections::HashMap;

/// `M`: private channels with a message in the last 30 minutes join at mount.
pub const RECENT_MS: f64 = 30. * 60_000.;
/// At most three of them.
pub const RECENT_COUNT: usize = 3;

/// How an entry came (`B9` of module 41984; only `MANUAL` is raised here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Manual,
    Notification,
    AutomaticChannelSelect,
}

/// What the store reads about a channel (`w`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ChannelInfo {
    pub is_private: bool,
    pub mention_count: u32,
    pub has_unread: bool,
    /// The timestamp of its last message (ms), when it has one.
    pub last_message_ms: Option<f64>,
}

/// One tracked chat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub channel: u64,
    pub source: Source,
    pub added_on_ms: f64,
    pub last_activity_ms: f64,
    pub is_private: bool,
    pub mention_count: u32,
    pub has_unread: bool,
}

impl Entry {
    /// The sort key of `y`, as an ordered tuple.
    fn key(&self) -> (u8, u8, std::cmp::Reverse<u32>, std::cmp::Reverse<i64>, u64) {
        let state = if self.mention_count > 0 {
            0
        } else if self.has_unread {
            1
        } else {
            2
        };
        (
            u8::from(!self.is_private),
            state,
            std::cmp::Reverse(self.mention_count),
            std::cmp::Reverse(self.last_activity_ms.floor() as i64),
            self.channel,
        )
    }
}

/// A private channel as the mount reads it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PrivateChannel {
    pub channel: u64,
    pub muted: bool,
    pub message_request: bool,
    pub info: ChannelInfo,
}

/// What `x` reads at mount.
#[derive(Clone, Debug, Default)]
pub struct Mount {
    pub now_ms: f64,
    /// `SelectedChannelStore.getChannelId()`, when it is a channel the client knows.
    pub app_channel: Option<(u64, ChannelInfo)>,
    pub voice_channel: Option<u64>,
    /// `getPrivateChannelIds()`, in the private channel order.
    pub private_channels: Vec<PrivateChannel>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    entries: HashMap<u64, Entry>,
    /// `D`.
    selected: Option<u64>,
    /// `b`: when the store was set up for this overlay; none until mounted.
    mounted_at: Option<f64>,
    /// `v`.
    minimized: bool,
    /// `P`: the voice channel last seen.
    voice: Option<u64>,
    /// `y.version`: bumps with every change of the entries.
    version: u64,
}

fn entry(channel: u64, source: Source, added: f64, activity: f64, info: ChannelInfo) -> Entry {
    Entry {
        channel,
        source,
        added_on_ms: added,
        last_activity_ms: activity,
        is_private: info.is_private,
        mention_count: info.mention_count,
        has_unread: info.has_unread,
    }
}

impl Store {
    fn set(&mut self, entry: Entry) -> bool {
        let changed = self.entries.get(&entry.channel) != Some(&entry);
        self.entries.insert(entry.channel, entry);
        if changed {
            self.version += 1;
        }
        changed
    }

    /// `getSessionEntries()`: every entry in order, and the version.
    pub fn entries(&self) -> (Vec<Entry>, u64) {
        let mut list: Vec<Entry> = self.entries.values().copied().collect();
        list.sort_by_key(Entry::key);
        (list, self.version)
    }

    pub fn is_tracked(&self, channel: u64) -> bool {
        self.entries.contains_key(&channel)
    }

    /// `getSelectedChannelId()`.
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    /// `getVoiceChatMinimized()`.
    pub fn voice_chat_minimized(&self) -> bool {
        self.minimized
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted_at.is_some()
    }

    /// `G`, on `OVERLAY_V3_NATIVE_DESTROY_HOST_WINDOW`.
    pub fn clear(&mut self) {
        *self = Self {
            version: self.version + 1,
            ..Self::default()
        };
    }

    /// `F`: the first entry other than `except`.
    fn next_after(&self, except: Option<u64>) -> Option<u64> {
        let (list, _) = self.entries();
        list.iter()
            .map(|entry| entry.channel)
            .find(|channel| Some(*channel) != except)
    }

    /// `B`: with nothing open and the voice chat not minimized, the voice channel's chat opens.
    fn open_voice(&mut self, voice: Option<u64>) -> bool {
        if let Some(voice) = voice
            && !self.minimized
            && self.selected.is_none()
        {
            self.selected = Some(voice);
            return true;
        }
        false
    }

    /// `OVERLAY_MOUNTED` (`x`, only when not mounted yet). Returns whether it changed anything.
    pub fn mount(&mut self, mount: &Mount) -> bool {
        if self.mounted_at.is_some() {
            return false;
        }
        self.clear();
        let now = mount.now_ms;
        self.mounted_at = Some(now);
        self.voice = mount.voice_channel;
        if let Some((channel, info)) = mount.app_channel {
            self.selected = Some(channel);
            let activity = info.last_message_ms.unwrap_or(now);
            self.set(entry(channel, Source::Manual, now, activity, info));
        }
        let mut added = 0;
        for private in &mount.private_channels {
            if added >= RECENT_COUNT {
                break;
            }
            if self.entries.contains_key(&private.channel)
                || !private.info.is_private
                || private.muted
                || private.message_request
            {
                continue;
            }
            let Some(last) = private.info.last_message_ms else {
                continue;
            };
            if now - last > RECENT_MS {
                continue;
            }
            self.set(entry(
                private.channel,
                Source::Manual,
                now,
                last,
                private.info,
            ));
            added += 1;
        }
        self.open_voice(mount.voice_channel);
        true
    }

    /// `OVERLAY_TEXT_CHAT_SELECT_CHANNEL`.
    pub fn select(
        &mut self,
        channel: u64,
        source: Source,
        info: ChannelInfo,
        now: f64,
        voice: Option<u64>,
    ) {
        if self.mounted_at.is_none() {
            self.mounted_at = Some(now);
        }
        self.selected = Some(channel);
        if voice == Some(channel) && self.minimized {
            self.minimized = false;
        }
        if !self.entries.contains_key(&channel) {
            let activity = info.last_message_ms.unwrap_or(now);
            self.set(entry(channel, source, now, activity, info));
        }
    }

    /// `OVERLAY_TEXT_CHAT_ADD_OR_UPDATE_CHANNEL`: the activity only moves forward; nothing open opens it.
    pub fn add_or_update(
        &mut self,
        channel: u64,
        source: Source,
        activity_ms: f64,
        info: ChannelInfo,
    ) -> bool {
        let known = self.entries.get(&channel).copied();
        let activity = known.map_or(activity_ms, |known| known.last_activity_ms.max(activity_ms));
        let added = known.map_or(activity_ms, |known| known.added_on_ms);
        let changed = self.set(entry(channel, source, added, activity, info));
        let opened = self.selected.is_none();
        if opened {
            self.selected = Some(channel);
        }
        changed || opened
    }

    /// `OVERLAY_TEXT_CHAT_REMOVE_CHANNEL`.
    pub fn remove(&mut self, channel: u64, voice: Option<u64>) -> bool {
        let removed = self.entries.remove(&channel).is_some();
        if removed {
            self.version += 1;
        }
        if self.selected == Some(channel) {
            self.selected = self
                .next_after(voice)
                .or(if self.minimized { None } else { voice });
            return true;
        }
        removed
    }

    /// `OVERLAY_TEXT_CHAT_REMOVE_ALL_CHANNELS` (Shift+click on the close button).
    pub fn remove_all(&mut self, voice: Option<u64>) -> bool {
        let had = !self.entries.is_empty();
        if had {
            self.entries.clear();
            self.version += 1;
        }
        self.selected = if self.minimized { None } else { voice };
        had
    }

    /// `OVERLAY_TEXT_CHAT_SET_VOICE_CHAT_MINIMIZED`.
    pub fn set_voice_chat_minimized(&mut self, minimized: bool, voice: Option<u64>) -> bool {
        if self.minimized == minimized {
            return false;
        }
        self.minimized = minimized;
        if !minimized {
            self.open_voice(voice);
            return true;
        }
        if voice.is_some() && self.selected == voice {
            self.selected = self.next_after(voice);
        }
        true
    }

    /// `V`, on `VOICE_STATE_UPDATES` and `VOICE_CHANNEL_SELECT`: the voice channel is now `voice`.
    pub fn voice_changed(&mut self, voice: Option<u64>) -> bool {
        let mut changed = false;
        if self.selected.is_some() && voice != self.selected && self.voice == self.selected {
            self.selected = self.next_after(self.voice);
            changed = true;
        }
        if voice.is_none() || self.voice != voice {
            self.voice = voice;
            if self.minimized {
                self.minimized = false;
                changed = true;
            }
        }
        self.open_voice(voice) || changed
    }

    /// `H`, on the acks: the entry's mention and unread state are read again.
    pub fn refresh(&mut self, channel: u64, info: ChannelInfo) -> bool {
        let Some(known) = self.entries.get(&channel).copied() else {
            return false;
        };
        if known.mention_count == info.mention_count && known.has_unread == info.has_unread {
            return false;
        }
        self.set(Entry {
            mention_count: info.mention_count,
            has_unread: info.has_unread,
            ..known
        })
    }
}

/// The Chat widget's rail (`id` of module 415173): the voice channel first, then the entries other than it.
pub fn rail(store: &Store, voice: Option<u64>) -> Vec<u64> {
    let (list, _) = store.entries();
    voice
        .into_iter()
        .chain(
            list.iter()
                .map(|entry| entry.channel)
                .filter(|channel| Some(*channel) != voice),
        )
        .collect()
}

/// `C` of the widget: the chat pane shows unless nothing is open or the open one is the minimized voice chat.
pub fn pane_shown(store: &Store, voice: Option<u64>) -> bool {
    store
        .selected()
        .is_some_and(|selected| !(store.voice_chat_minimized() && Some(selected) == voice))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private(last: f64) -> ChannelInfo {
        ChannelInfo {
            is_private: true,
            last_message_ms: Some(last),
            ..ChannelInfo::default()
        }
    }

    fn guild() -> ChannelInfo {
        ChannelInfo::default()
    }

    #[test]
    fn mounting_opens_the_app_channel_and_recent_dms() {
        let now = 10_000_000.;
        let dm = |channel, last: f64| PrivateChannel {
            channel,
            muted: false,
            message_request: false,
            info: private(last),
        };
        let mut muted = dm(5, now);
        muted.muted = true;
        let mount = Mount {
            now_ms: now,
            app_channel: Some((100, guild())),
            voice_channel: Some(200),
            private_channels: vec![
                dm(1, now - 1000.),
                muted,
                dm(2, now - RECENT_MS - 1.),
                dm(3, now - 5000.),
                dm(4, now - 2000.),
                dm(6, now - 10.),
            ],
        };
        let mut store = Store::default();
        assert!(store.mount(&mount));
        assert!(!store.mount(&mount), "only once");
        assert_eq!(store.selected(), Some(100));
        let (list, _) = store.entries();
        let channels: Vec<u64> = list.iter().map(|entry| entry.channel).collect();
        // Three recent DMs (1, 3, 4: the muted and the old one are skipped, 6 is past the three), newest first, then
        // the guild channel.
        assert_eq!(channels, [1, 4, 3, 100]);
        assert_eq!(rail(&store, Some(200)), [200, 1, 4, 3, 100]);
        assert!(pane_shown(&store, Some(200)));
    }

    #[test]
    fn mentions_and_unreads_sort_first_within_their_group() {
        let mut store = Store::default();
        let now = 1000.;
        store.add_or_update(1, Source::Manual, 500., private(500.));
        store.add_or_update(
            2,
            Source::Manual,
            100.,
            ChannelInfo {
                has_unread: true,
                ..private(100.)
            },
        );
        store.add_or_update(
            3,
            Source::Manual,
            50.,
            ChannelInfo {
                mention_count: 2,
                ..private(50.)
            },
        );
        store.add_or_update(4, Source::Manual, 900., guild());
        let order: Vec<u64> = store
            .entries()
            .0
            .iter()
            .map(|entry| entry.channel)
            .collect();
        assert_eq!(order, [3, 2, 1, 4]);
        // The first one added opened, the others did not change it.
        assert_eq!(store.selected(), Some(1));
        // Activity only moves forward.
        store.add_or_update(1, Source::Manual, 10., private(10.));
        assert_eq!(
            store
                .entries()
                .0
                .iter()
                .find(|e| e.channel == 1)
                .unwrap()
                .last_activity_ms,
            500.
        );
        // An ack drops the mention.
        assert!(store.refresh(3, private(50.)));
        assert!(!store.refresh(3, private(50.)));
        assert!(!store.refresh(99, private(0.)));
        let _ = now;
    }

    #[test]
    fn selecting_removing_and_the_voice_chat() {
        let mut store = Store::default();
        let voice = Some(200);
        store.select(1, Source::Manual, private(5.), 10., voice);
        store.select(2, Source::Manual, private(7.), 11., voice);
        assert_eq!(store.selected(), Some(2));
        assert!(store.is_tracked(1) && store.is_tracked(2));
        // Closing the open chat opens the next one.
        assert!(store.remove(2, voice));
        assert_eq!(store.selected(), Some(1));
        assert!(store.remove(1, voice));
        // Nothing left: the voice channel's chat.
        assert_eq!(store.selected(), voice);
        // Minimizing the open voice chat closes it; the pane hides.
        assert!(store.set_voice_chat_minimized(true, voice));
        assert_eq!(store.selected(), None);
        assert!(!pane_shown(&store, voice));
        assert!(!store.set_voice_chat_minimized(true, voice));
        assert!(store.set_voice_chat_minimized(false, voice));
        assert_eq!(store.selected(), voice);
        // Leaving the voice channel closes its open chat.
        store.voice_changed(voice);
        store.add_or_update(7, Source::Manual, 1., guild());
        assert!(store.voice_changed(None));
        assert_eq!(store.selected(), Some(7));
        // Shift+close: everything goes.
        assert!(store.remove_all(None));
        assert_eq!(store.selected(), None);
        assert!(store.entries().0.is_empty());
        store.clear();
        assert!(!store.is_mounted());
    }
}
