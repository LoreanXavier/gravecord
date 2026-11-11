//! Where the app was last: the channel last open in each server, and the server (or Home) last shown. The official
//! keeps these in `SelectedChannelStore` and `SelectedGuildStore`, saved on the machine, so a server opens on the
//! channel it was left on and the app starts where it was closed.
use crate::storage;
use dc_state::Id;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::HashMap;

const KEY: &str = "SelectedChannelStore";

/// What is kept: a channel per server, and the place last shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Places {
    channels: HashMap<Id, Id>,
    /// `Some(None)` is Home; `None` is nothing saved yet.
    last: Option<Option<Id>>,
}

impl Places {
    pub(super) fn parse(value: &Value) -> Self {
        let id = |text: &str| text.parse::<Id>().ok();
        let channels = value["channels"]
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(guild, channel)| Some((id(guild)?, id(channel.as_str()?)?)))
                    .collect()
            })
            .unwrap_or_default();
        let last = match &value["last"] {
            Value::String(text) if text == "@me" => Some(None),
            Value::String(text) => id(text).map(Some),
            _ => None,
        };
        Self { channels, last }
    }

    pub(super) fn to_value(&self) -> Value {
        let channels: Map<String, Value> = self
            .channels
            .iter()
            .map(|(guild, channel)| (guild.to_string(), json!(channel.to_string())))
            .collect();
        let last = match self.last {
            Some(Some(guild)) => json!(guild.to_string()),
            Some(None) => json!("@me"),
            None => Value::Null,
        };
        json!({"channels": channels, "last": last})
    }

    pub(super) fn remember(&mut self, guild: Id, channel: Option<Id>) {
        if let Some(channel) = channel {
            self.channels.insert(guild, channel);
        }
        self.last = Some(Some(guild));
    }

    pub(super) fn remember_home(&mut self) {
        self.last = Some(None);
    }

    pub(super) fn channel_of(&self, guild: Id) -> Option<Id> {
        self.channels.get(&guild).copied()
    }

    pub(super) fn last(&self) -> Option<Option<Id>> {
        self.last
    }
}

thread_local! {
    static PLACES: RefCell<Option<Places>> = const { RefCell::new(None) };
}

/// A harness run always starts on the same screen and leaves nothing behind.
fn kept() -> bool {
    !storage::is_capture_run()
}

fn with<R>(change: impl FnOnce(&mut Places) -> R) -> R {
    PLACES.with(|places| {
        let mut places = places.borrow_mut();
        let places = places.get_or_insert_with(|| {
            if kept() {
                storage::get(KEY)
                    .map(|value| Places::parse(&value))
                    .unwrap_or_default()
            } else {
                Places::default()
            }
        });
        change(places)
    })
}

fn save(places: &Places) {
    if kept() {
        storage::set(KEY, places.to_value());
    }
}

pub(super) fn remember(guild: Id, channel: Option<Id>) {
    with(|places| {
        let before = places.clone();
        places.remember(guild, channel);
        if *places != before {
            save(places);
        }
    });
}

pub(super) fn remember_home() {
    with(|places| {
        if places.last() != Some(None) {
            places.remember_home();
            save(places);
        }
    });
}

pub(super) fn channel_of(guild: Id) -> Option<Id> {
    with(|places| places.channel_of(guild))
}

/// The place to start on: `Some(Some(guild))`, `Some(None)` for Home, or `None` when nothing was saved.
pub(super) fn last() -> Option<Option<Id>> {
    with(|places| places.last())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_server_keeps_its_own_channel_and_the_last_place_wins() {
        let mut places = Places::default();
        assert_eq!(places.last(), None);
        places.remember(1, Some(10));
        places.remember(2, Some(20));
        places.remember(1, Some(11));
        assert_eq!(places.channel_of(1), Some(11));
        assert_eq!(places.channel_of(2), Some(20));
        assert_eq!(places.last(), Some(Some(1)));
        // A server opened on a page without a channel keeps the channel it had.
        places.remember(2, None);
        assert_eq!(places.channel_of(2), Some(20));
        assert_eq!(places.last(), Some(Some(2)));
        places.remember_home();
        assert_eq!(places.last(), Some(None));
        assert_eq!(places.channel_of(1), Some(11));
    }

    #[test]
    fn what_is_saved_reads_back_the_same_and_junk_reads_as_nothing() {
        let mut places = Places::default();
        places.remember(1554855651514122281, Some(1554855652386410528));
        places.remember(7, Some(70));
        assert_eq!(Places::parse(&places.to_value()), places);
        places.remember_home();
        assert_eq!(Places::parse(&places.to_value()), places);
        assert_eq!(Places::parse(&Value::Null), Places::default());
        let junk = json!({"channels": {"x": "y", "5": 6}, "last": 3});
        assert_eq!(Places::parse(&junk), Places::default());
    }
}
