//! MessagePeek (Equicord `src/equicordplugins/messagePeek`, GPL-3.0-or-later): under each DM in the Direct Messages
//! list, the last message ("You: ...", "Name: ...", "sent a GIF", "2 images"), and its age at the row's right. An
//! activity shows instead when the last message is over an hour old. Not part of the pinned client.
//!
//! The last message is the newest one the app holds for the DM. The original also fetches the last message of the 25
//! newest DMs when it starts; plugin code sends nothing here, so a DM shows its preview once its messages are loaded.

use super::super::{Root, color};
use crate::plugins::State;
use dc_state::{Id, Message, PrivateChannel};
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};
use serde_json::Value;

const ID: &str = "MessagePeek";
const ONE_HOUR_MS: u64 = 60 * 60 * 1000;
const SYSTEM_USER: Id = 643_945_264_868_098_049;

/// `formatRelativeTime`.
pub(super) fn relative(diff_ms: u64) -> String {
    let minutes = diff_ms / 60_000;
    let hours = diff_ms / 3_600_000;
    let days = diff_ms / 86_400_000;
    if days >= 365 {
        format!("{}y", days / 365)
    } else if days >= 30 {
        format!("{}mo", days / 30)
    } else if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h")
    } else {
        format!("{}m", minutes.max(1))
    }
}

/// `pluralize`: "1 image", "2 images".
fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

fn attachment_kind(content_type: Option<&str>) -> &'static str {
    match content_type.unwrap_or("") {
        "image/gif" => "GIF",
        kind if kind.starts_with("image/") => "image",
        kind if kind.starts_with("video/") => "video",
        _ => "file",
    }
}

/// `getMessageContent`: the text, a GIF link, a voice message, the attachments, or the sticker's name.
pub(super) fn content_of(message: &Message) -> Option<String> {
    if !message.content.is_empty() {
        let lower = message.content.to_lowercase();
        let gif = lower.split_whitespace().any(|word| {
            (word.starts_with("http://") || word.starts_with("https://"))
                && (word.ends_with(".gif")
                    || word.contains("tenor.com")
                    || word.contains("giphy.com")
                    || word.contains("klipy.com"))
        });
        if gif {
            return Some("sent a GIF".to_owned());
        }
        return Some(message.content.lines().next().unwrap_or("").to_owned());
    }
    if message.is_voice_message() {
        return Some("voice message".to_owned());
    }
    if !message.attachments.is_empty() {
        let kinds: Vec<&str> = message
            .attachments
            .iter()
            .map(|attachment| attachment_kind(attachment.content_type.as_deref()))
            .collect();
        return Some(if kinds.iter().all(|kind| *kind == kinds[0]) {
            plural(kinds.len(), kinds[0])
        } else {
            plural(kinds.len(), "file")
        });
    }
    message
        .sticker_items
        .first()
        .map(|(_, name, _)| name.clone())
}

fn hide_muted_in(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "hideMuted") == Value::Bool(true))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// What the plugin puts under a DM's name.
pub(in crate::ui) enum Peek {
    /// The row keeps its own line (the plugin is off, or it shows the activity).
    Keep,
    /// Nothing under the name.
    Empty,
    Line(AnyElement),
}

impl Root {
    fn last_message(&self, channel: Id) -> Option<&Message> {
        self.stores
            .messages
            .get(&channel)
            .and_then(|list| list.last())
    }

    fn peek_muted(&self, channel: Id) -> bool {
        self.stores
            .guild_settings
            .channel_muted(0, channel, chrono::Utc::now())
    }

    /// `getSubText`: the activity (when there is one and no message of the last hour), else the last message.
    pub(in crate::ui) fn message_peek(&self, channel: &PrivateChannel, has_activity: bool) -> Peek {
        if crate::plugins::bypassed() {
            return Peek::Keep;
        }
        let Some(hide_muted) = crate::plugins::with(hide_muted_in) else {
            return Peek::Keep;
        };
        if hide_muted && self.peek_muted(channel.id) {
            return if has_activity {
                Peek::Keep
            } else {
                Peek::Empty
            };
        }
        let last = self.last_message(channel.id);
        let recent = last.is_some_and(|message| {
            now_ms().saturating_sub(dc_state::read_state::snowflake_ms(message.id)) <= ONE_HOUR_MS
        });
        if has_activity && !recent {
            return Peek::Keep;
        }
        let line = |text: String| {
            Peek::Line(
                div()
                    .min_w(px(0.))
                    .text_ellipsis()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(gpui::FontWeight(500.))
                    .child(text)
                    .into_any_element(),
            )
        };
        // `isSystemDM`.
        if channel.kind == 1 && channel.recipients.first() == Some(&SYSTEM_USER) {
            return line("Official Discord Message".to_owned());
        }
        let Some(message) = last else {
            return if channel.kind == 3 {
                line(format!("{} Members", channel.recipients.len() + 1))
            } else {
                Peek::Empty
            };
        };
        let Some(content) = content_of(message) else {
            return Peek::Empty;
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let author = if me == Some(message.author.id) {
            "You".to_owned()
        } else {
            self.stores
                .friend_nickname(message.author.id)
                .map_or_else(|| message.author.display_name().to_owned(), str::to_owned)
        };
        line(format!("{author}: {content}"))
    }

    /// The `Timestamp` decorator: the last message's age at the row's right, hidden under the pointer.
    pub(in crate::ui) fn message_peek_time(
        &self,
        channel: Id,
        hovered: bool,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() || hovered {
            return None;
        }
        let hide_muted = crate::plugins::with(hide_muted_in)?;
        if hide_muted && self.peek_muted(channel) {
            return None;
        }
        let message = self.last_message(channel)?;
        let age = now_ms().saturating_sub(dc_state::read_state::snowflake_ms(message.id));
        Some(
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(gpui::FontWeight(500.))
                .text_color(color("--text-default"))
                .child(relative(age))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_as_the_original() {
        assert_eq!(relative(10_000), "1m");
        assert_eq!(relative(5 * 60_000), "5m");
        assert_eq!(relative(3 * 3_600_000), "3h");
        assert_eq!(relative(2 * 86_400_000), "2d");
        assert_eq!(relative(40 * 86_400_000), "1mo");
        assert_eq!(relative(400 * 86_400_000), "1y");
    }

    #[test]
    fn contents_read_as_the_original() {
        let mut message = Message::from_value(&serde_json::json!({
            "id": "1", "channel_id": "2", "type": 0, "content": "hi there\nsecond",
            "author": {"id": "3", "username": "a"}, "attachments": [], "embeds": [],
            "timestamp": "2026-11-18T00:00:00+00:00"
        }))
        .unwrap();
        assert_eq!(content_of(&message).as_deref(), Some("hi there"));
        message.content = "look https://tenor.com/view/x".to_owned();
        assert_eq!(content_of(&message).as_deref(), Some("sent a GIF"));
        message.content.clear();
        assert_eq!(content_of(&message), None);
        assert_eq!(plural(1, "image"), "1 image");
        assert_eq!(plural(3, "file"), "3 files");
        assert_eq!(attachment_kind(Some("image/gif")), "GIF");
        assert_eq!(attachment_kind(Some("video/mp4")), "video");
    }
}
