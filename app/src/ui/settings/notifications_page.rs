//! What the Notifications page's rows that live outside the settings proto and the sound lists do.
//!
//! - Email switches (`email_list_item_*`, module 158935 `vq`): `GET /users/@me/email-settings` reads
//!   `categories`, a change is `PATCH {settings: {categories: {name: on}}}` (module 840559 `CA`); Unsubscribe turns
//!   every marketing category off (`NI`).
//! - Text-to-speech notifications: the `NotificationSettingsStore` (module 803224) keeps `ttsType` in this machine's
//!   `notifications` store, `NEVER` by default. An incoming message is spoken by the rules of module 596339 `vm`.
//! - Push notification inactive timeout (`mobile_notification_delay`): the account's `afk_timeout`, in seconds.

use crate::net::Request;
use crate::storage;
use crate::ui::Root;
use gpui::Context;
use serde_json::{Value, json};
use std::collections::HashMap;

pub(in crate::ui) const EMAIL_KEY: &str = "settings-email";
pub(in crate::ui) const EMAIL_WRITE_KEY: &str = "settings-email-write";
const EMAIL_ROUTE: &str = "/users/@me/email-settings";
pub(in crate::ui) const UNSUBSCRIBE: &str = "unsubscribe_from_all_marketing_emails";
pub(in crate::ui) const TTS_NOTIFICATIONS: &str = "text_to_speech_notifications";
pub(in crate::ui) const MOBILE_DELAY: &str = "mobile_notification_delay";

/// `Zk` of module 723923: the marketing categories, in the order the page lists them.
pub(in crate::ui) const EMAIL_CATEGORIES: [&str; 5] = [
    "communication",
    "social",
    "updates_and_announcements",
    "tips",
    "recommendations_and_events",
];

/// `NOTIFICATIONS_SET_TTS_TYPE` values, in the radio's order (module 158935 `vw`).
pub(in crate::ui) const TTS_TYPES: [&str; 3] = ["ALL_CHANNELS", "SELECTED_CHANNEL", "NEVER"];

/// The seconds `mobile_notification_delay` offers: one to ten minutes (module 158935 `vV`: lodash
/// `range(1, 11)` stops before 11).
pub(in crate::ui) const DELAYS: [u64; 10] = [60, 120, 180, 240, 300, 360, 420, 480, 540, 600];

#[derive(Default)]
pub(in crate::ui) struct State {
    pub categories: Option<HashMap<String, bool>>,
    asked: bool,
    /// The messages already spoken, newest last (`vl` of module 596339 keeps ten).
    spoken: Vec<u64>,
}

/// `email_list_item_communication` -> `communication`.
pub(in crate::ui) fn email_category(key: &str) -> Option<&'static str> {
    let name = key.strip_prefix("email_list_item_")?;
    EMAIL_CATEGORIES
        .iter()
        .find(|known| **known == name)
        .copied()
}

pub(in crate::ui) fn email_patch(categories: &[(&str, bool)]) -> Request {
    let map: serde_json::Map<String, Value> = categories
        .iter()
        .map(|(name, on)| ((*name).to_owned(), json!(on)))
        .collect();
    Request::Rest {
        key: Some(EMAIL_WRITE_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: EMAIL_ROUTE.to_owned(),
        body: Some(json!({"settings": {"categories": map}})),
    }
}

pub(in crate::ui) fn categories_of(body: &Value) -> Option<HashMap<String, bool>> {
    let map = body["categories"].as_object()?;
    Some(
        map.iter()
            .filter_map(|(name, on)| Some((name.clone(), on.as_bool()?)))
            .collect(),
    )
}

/// `Ra9Pwk`'s button is disabled once every marketing category is off (`useDisabled`).
pub(in crate::ui) fn nothing_to_unsubscribe(categories: &HashMap<String, bool>) -> bool {
    EMAIL_CATEGORIES
        .iter()
        .all(|name| !categories.get(*name).copied().unwrap_or(false))
}

/// The radio index of the stored `ttsType`.
pub(in crate::ui) fn tts_index() -> usize {
    let stored = storage::get("notifications").unwrap_or(Value::Null);
    TTS_TYPES
        .iter()
        .position(|name| stored["ttsType"].as_str() == Some(*name))
        .unwrap_or(2)
}

pub(in crate::ui) fn set_tts_index(index: usize) {
    let mut value = storage::get("notifications")
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    value["ttsType"] = json!(TTS_TYPES[index.min(2)]);
    storage::set("notifications", value);
}

/// Module 596339 `vm` after the message is known to be a new one: spoken when the /tts command is allowed and the
/// message is a /tts message of the channel on screen, or when the type asks for all channels, or for the selected
/// one and this is it, and the author is somebody else. Never while deafened or for blocked and ignored authors,
/// never in a muted channel, and a message once.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn should_speak(
    tts_type: &str,
    command_allowed: bool,
    message_is_tts: bool,
    author_is_me: bool,
    in_selected: bool,
    self_deaf: bool,
    author_blocked: bool,
    channel_muted: bool,
    already_spoken: bool,
) -> bool {
    if self_deaf {
        return false;
    }
    let command = command_allowed && message_is_tts && in_selected;
    let all = !author_is_me
        && (tts_type == "ALL_CHANNELS" || (tts_type == "SELECTED_CHANNEL" && in_selected));
    (command || all) && !author_blocked && !already_spoken && !channel_muted
}

impl Root {
    /// Settings opened: the email categories are asked for once (`initialize` of the email category).
    pub(in crate::ui) fn notification_page_opened(&mut self) {
        if self.settings_extra.notif.asked {
            return;
        }
        self.settings_extra.notif.asked = true;
        let _ = self.requests.send(Request::Fetch {
            key: EMAIL_KEY.to_owned(),
            route: EMAIL_ROUTE.to_owned(),
        });
    }

    /// An answer of this page's requests: true when the key was ours.
    pub(in crate::ui) fn notification_page_fetched(&mut self, key: &str, body: &Value) -> bool {
        match key {
            EMAIL_KEY => {
                if let Some(categories) = categories_of(body) {
                    self.settings_extra.notif.categories = Some(categories);
                }
                true
            }
            EMAIL_WRITE_KEY => {
                // `EMAIL_SETTINGS_UPDATE_SUCCESS` takes the server's answer; a failure (`FAILURE`) restores the
                // previous values by reading them again. A refused run keeps what the click set.
                if body["ok"].as_bool() == Some(true) {
                    if let Some(categories) = categories_of(&body["body"]) {
                        self.settings_extra.notif.categories = Some(categories);
                    }
                } else if body["refused"].as_bool() != Some(true) {
                    let _ = self.requests.send(Request::Fetch {
                        key: EMAIL_KEY.to_owned(),
                        route: EMAIL_ROUTE.to_owned(),
                    });
                }
                true
            }
            _ => false,
        }
    }

    /// The value of one of this page's switches.
    pub(in crate::ui) fn notification_switch_value(&self, key: &str) -> Option<bool> {
        let name = email_category(key)?;
        Some(
            self.settings_extra
                .notif
                .categories
                .as_ref()
                .and_then(|categories| categories.get(name).copied())
                .unwrap_or(false),
        )
    }

    /// A row of this page changed. True when the key was one of its own.
    pub(in crate::ui) fn notification_page_changed(
        &mut self,
        key: &'static str,
        on: bool,
        index: usize,
    ) -> bool {
        if let Some(name) = email_category(key) {
            self.setting_choices.remove(key);
            self.settings_extra
                .notif
                .categories
                .get_or_insert_with(HashMap::new)
                .insert(name.to_owned(), on);
            let _ = self.requests.send(email_patch(&[(name, on)]));
            return true;
        }
        if key == TTS_NOTIFICATIONS {
            self.setting_choices.remove(key);
            set_tts_index(index);
            return true;
        }
        false
    }

    /// The radio index of the rows that keep their value outside the generic choices.
    pub(in crate::ui) fn notification_page_index(&self, key: &str) -> Option<usize> {
        (key == TTS_NOTIFICATIONS).then(tts_index)
    }

    /// Unsubscribe from all marketing emails: every marketing category off, in one request.
    pub(in crate::ui) fn unsubscribe_marketing(&mut self) {
        let categories = self
            .settings_extra
            .notif
            .categories
            .get_or_insert_with(HashMap::new);
        if nothing_to_unsubscribe(categories) {
            return;
        }
        for name in EMAIL_CATEGORIES {
            categories.insert(name.to_owned(), false);
        }
        let all: Vec<(&str, bool)> = EMAIL_CATEGORIES.iter().map(|name| (*name, false)).collect();
        let _ = self.requests.send(email_patch(&all));
    }

    /// Whether the Unsubscribe button is locked.
    pub(in crate::ui) fn unsubscribe_disabled(&self) -> bool {
        self.settings_extra
            .notif
            .categories
            .as_ref()
            .is_none_or(nothing_to_unsubscribe)
    }

    /// A new message from the gateway: spoken when the text-to-speech rules say so. Nothing is spoken in a harness
    /// run; the decision is noted in the log.
    pub(in crate::ui) fn speak_incoming(&mut self, message: &Value, _cx: &mut Context<Self>) {
        let (Some(channel), Some(id)) = (
            message["channel_id"]
                .as_str()
                .and_then(|v| v.parse::<u64>().ok()),
            message["id"].as_str().and_then(|v| v.parse::<u64>().ok()),
        ) else {
            return;
        };
        let author = message["author"]["id"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let guild = self.stores.guild_of_channel(channel).map(|guild| guild.id);
        let muted = guild.is_some_and(|guild| {
            self.stores
                .guild_settings
                .channel_muted(guild, channel, chrono::Utc::now())
        });
        let tts_type = TTS_TYPES[tts_index()];
        let speak = should_speak(
            tts_type,
            self.pref_on("text_to_speech_command", true),
            message["tts"].as_bool() == Some(true),
            me == Some(author),
            self.channel == Some(channel),
            self.self_deaf,
            self.is_blocked(author) || self.stores.is_ignored(author),
            muted,
            self.settings_extra.notif.spoken.contains(&id),
        );
        if !speak {
            return;
        }
        self.settings_extra.notif.spoken.push(id);
        if self.settings_extra.notif.spoken.len() > 10 {
            self.settings_extra.notif.spoken.remove(0);
        }
        let name = {
            let nick = guild.and_then(|guild| {
                self.stores
                    .members
                    .get(guild, author)
                    .and_then(|member| member.nick.clone())
            });
            nick.or_else(|| {
                dc_state::User::from_value(&message["author"])
                    .map(|user| user.display_name().to_owned())
            })
            .unwrap_or_default()
        };
        let content = message["content"].as_str().unwrap_or("");
        tracing::info!("text to speech: message {id} spoken");
        if crate::inert::on() || crate::storage::is_capture_run() {
            return;
        }
        let text = crate::speech::utterance(&name, content, None, |_| None);
        self.speech.speak(text, self.speech_rate());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_rows_name_their_categories() {
        assert_eq!(email_category("email_list_item_social"), Some("social"));
        assert_eq!(email_category("email_list_item_family_center_digest"), None);
        assert_eq!(email_category("desktop_notifications"), None);
    }

    #[test]
    fn an_email_change_patches_the_categories_of_the_settings() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = email_patch(&[("tips", false)])
        else {
            panic!("not rest");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me/email-settings");
        assert_eq!(
            body,
            Some(json!({"settings": {"categories": {"tips": false}}}))
        );
    }

    #[test]
    fn unsubscribe_turns_all_five_off_and_locks_when_they_are() {
        let mut categories =
            categories_of(&json!({"categories": {"tips": true, "social": false}})).unwrap();
        assert!(!nothing_to_unsubscribe(&categories));
        for name in EMAIL_CATEGORIES {
            categories.insert(name.to_owned(), false);
        }
        assert!(nothing_to_unsubscribe(&categories));
        let Request::Rest { body, .. } = email_patch(&EMAIL_CATEGORIES.map(|name| (name, false)))
        else {
            panic!("not rest");
        };
        assert_eq!(
            body.unwrap()["settings"]["categories"]
                .as_object()
                .unwrap()
                .len(),
            5
        );
    }

    #[test]
    fn speech_follows_the_type_and_the_channel() {
        let args = |tts: &str, command, is_tts, mine, here, deaf, blocked, muted, again| {
            should_speak(
                tts, command, is_tts, mine, here, deaf, blocked, muted, again,
            )
        };
        // Never speaks nothing, unless it is a /tts message of this channel and the command is allowed.
        assert!(!args(
            "NEVER", true, false, false, true, false, false, false, false
        ));
        assert!(args(
            "NEVER", true, true, false, true, false, false, false, false
        ));
        assert!(!args(
            "NEVER", false, true, false, true, false, false, false, false
        ));
        assert!(!args(
            "NEVER", true, true, false, false, false, false, false, false
        ));
        // All channels: anyone else's message anywhere, never my own.
        assert!(args(
            "ALL_CHANNELS",
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            false
        ));
        assert!(!args(
            "ALL_CHANNELS",
            true,
            false,
            true,
            false,
            false,
            false,
            false,
            false
        ));
        // Selected channel only.
        assert!(args(
            "SELECTED_CHANNEL",
            true,
            false,
            false,
            true,
            false,
            false,
            false,
            false
        ));
        assert!(!args(
            "SELECTED_CHANNEL",
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            false
        ));
        // Silence rules.
        assert!(!args(
            "ALL_CHANNELS",
            true,
            false,
            false,
            true,
            true,
            false,
            false,
            false
        ));
        assert!(!args(
            "ALL_CHANNELS",
            true,
            false,
            false,
            true,
            false,
            true,
            false,
            false
        ));
        assert!(!args(
            "ALL_CHANNELS",
            true,
            false,
            false,
            true,
            false,
            false,
            true,
            false
        ));
        assert!(!args(
            "ALL_CHANNELS",
            true,
            false,
            false,
            true,
            false,
            false,
            false,
            true
        ));
    }

    #[test]
    fn the_delay_options_are_one_to_eleven_minutes() {
        assert_eq!(DELAYS.first(), Some(&60));
        assert_eq!(DELAYS.last(), Some(&600));
        assert_eq!(DELAYS.len(), 10);
    }
}
