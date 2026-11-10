//! Notification dispatch, title/body formatting and window focus.
use super::Root;
use crate::notifications::Notice;
use dc_state::{
    Id,
    notifications::{DeviceSettings, message_decision},
};
use gpui::Context;
use serde_json::Value;
use std::time::Instant;

fn fallback_body(message: &Value) -> String {
    if let Some(sticker) = message["sticker_items"]
        .as_array()
        .and_then(|items| items.first())
    {
        return format!(
            "Sent a sticker: {}",
            sticker["name"].as_str().unwrap_or_default()
        );
    }
    if let Some(question) = message["poll"]["question"]["text"].as_str() {
        return format!("[poll] {question}");
    }
    if let Some(content) = message["content"].as_str().filter(|text| !text.is_empty()) {
        return dc_markdown::parse(content)
            .iter()
            .map(super::block_text)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(embed) = message["embeds"]
        .as_array()
        .and_then(|embeds| embeds.first())
    {
        let title = embed["title"].as_str().unwrap_or_default();
        if let Some(description) = embed["description"].as_str() {
            return if title.is_empty() {
                description.to_owned()
            } else {
                format!("{title} {description}")
            };
        }
        if !title.is_empty() {
            return title.to_owned();
        }
        if let Some(field) = embed["fields"].as_array().and_then(|fields| fields.first()) {
            return format!(
                "{} {}",
                field["name"].as_str().unwrap_or_default(),
                field["value"].as_str().unwrap_or_default()
            );
        }
    }
    if message["flags"].as_u64().unwrap_or(0) & (1 << 13) != 0 {
        return "Sent a voice message".to_owned();
    }
    message["attachments"]
        .as_array()
        .and_then(|files| files.first())
        .and_then(|file| file["filename"].as_str())
        .map(|name| format!("Uploaded {name}"))
        .unwrap_or_default()
}

impl Root {
    pub(super) fn refresh_notification_badge(&mut self) {
        let counts = dc_state::unread::total_counts(&self.stores, chrono::Utc::now());
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let disabled = !crate::notifications::switch_value("enable_unread_message_badge", user)
            .unwrap_or(true);
        let count = dc_state::unread::badge_count(
            counts.high_mentions,
            dc_state::unread::incoming_requests(&self.stores),
            counts.unread,
            disabled,
        );
        self.notification_runtime.badge(count);
        self.refresh_tray(
            !disabled
                && (counts.unread
                    || counts.mentions > 0
                    || dc_state::unread::incoming_requests(&self.stores) > 0),
        );
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_notification(&mut self, action: &str, cx: &mut Context<Self>) {
        if action == "badge-art" {
            #[cfg(windows)]
            crate::notifications::capture_badge_art();
        } else if action == "badge-preview" {
            self.notification_runtime.badge(3);
            self.notification_runtime.badge(-1);
            self.notification_runtime.badge(0);
            self.notification_runtime.flash(true);
            self.notification_runtime.flash(false);
        } else if action == "preview" {
            self.notification_runtime.show(Notice {
                channel: 1568692565136375900,
                message: 1,
                title: "Notification preview".to_owned(),
                body: "Capture delivery check".to_owned(),
            });
            crate::sounds::play_with("message1", 0.4, dc_voice::sound::Output::Default);
        } else if action == "open-fixture" {
            self.open_notification_channel(1568692565136375900, cx);
        }
    }
    fn notification_device(&self) -> DeviceSettings {
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let on = |key| crate::notifications::switch_value(key, user).unwrap_or(false);
        let streamer = on("streaming_enable_streamer_mode");
        DeviceSettings {
            desktop: on("desktop_notifications"),
            message_sound: !crate::sounds::is_sound_disabled("message1"),
            selected_channel_sound: crate::sounds::switch_value(crate::sounds::SELECTED_CHANNEL)
                .unwrap_or(false),
            taskbar_flash: on("task_bar_flashing"),
            streamer_notifications: streamer && on("streamer_mode_disable_notifications"),
            streamer_sounds: streamer && on("streamer_mode_disable_sounds"),
            quiet_mode: self.stores.notification_quiet_mode(),
            voice_channel: self.call.as_ref().map(|call| call.channel),
            ..DeviceSettings::default()
        }
    }

    pub(super) fn notify_message(&mut self, message: &Value, cx: &mut Context<Self>) {
        let decision = message_decision(
            &self.stores,
            message,
            self.channel,
            self.notification_focused,
            self.notification_device(),
            chrono::Utc::now(),
        );
        if let Some(sound) = decision.sound {
            self.notification_sound(sound, cx);
        }
        if decision.flash && !self.notification_focused {
            self.notification_runtime.flash(true);
        }
        if !decision.desktop {
            return;
        }
        let Some(channel) = message["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let Some(id) = message["id"].as_str().and_then(|id| id.parse::<Id>().ok()) else {
            return;
        };
        let Some(author) = dc_state::User::from_value(&message["author"]) else {
            return;
        };
        let mut name = author.display_name().to_owned();
        let title = if let Some((guild, target)) = self.stores.guilds.iter().find_map(|guild| {
            guild
                .channels
                .iter()
                .find(|known| known.id == channel)
                .map(|target| (guild, target))
        }) {
            if let Some(nick) = self
                .stores
                .members
                .get(guild.id, author.id)
                .and_then(|member| member.nick.as_ref())
            {
                name = nick.clone();
            }
            let parent = target
                .parent_id
                .and_then(|id| guild.channels.iter().find(|known| known.id == id));
            let category = parent
                .map(|parent| format!(", \u{2068}{}\u{2069}", parent.name))
                .unwrap_or_default();
            format!(
                "\u{2068}{name}\u{2069} (\u{2068}#{}\u{2069}{category})",
                target.name
            )
        } else if let Some(group) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel && known.kind == 3)
        {
            let group_name = group.name.clone().unwrap_or_else(|| {
                group
                    .recipients
                    .iter()
                    .filter_map(|id| self.stores.users.get(id))
                    .map(|user| user.display_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            format!("\u{2068}{name}\u{2069} (\u{2068}{group_name}\u{2069})")
        } else {
            name
        };
        self.notification_runtime.show(Notice {
            channel,
            message: id,
            title,
            body: fallback_body(message),
        });
    }

    fn notification_sound(&mut self, sound: &'static str, cx: &mut Context<Self>) {
        let delay = match self.notification_sounds.request(sound, Instant::now()) {
            crate::notifications::SoundAction::Play => {
                crate::sounds::play_with(sound, 0.4, dc_voice::sound::Output::Default);
                return;
            }
            crate::notifications::SoundAction::Pending => return,
            crate::notifications::SoundAction::Schedule(delay) => delay,
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, _| {
                if let Some(sound) = this.notification_sounds.finish(Instant::now()) {
                    crate::sounds::play_with(sound, 0.4, dc_voice::sound::Output::Default);
                }
            });
        })
        .detach();
    }

    pub(super) fn notification_focus_changed(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.notification_focused = focused;
        if focused {
            self.notification_runtime.flash(false);
            if let Some(channel) = self.channel {
                self.notification_runtime.clear(channel);
            }
            self.read_current_channel();
        }
        cx.notify();
    }

    pub(super) fn open_notification_channel(&mut self, channel: Id, cx: &mut Context<Self>) {
        if let Some(guild) = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))
            .map(|guild| guild.id)
        {
            self.select(guild, Some(channel));
        } else if self
            .stores
            .private_channels
            .iter()
            .any(|known| known.id == channel)
        {
            self.select_home(Some(channel));
        } else {
            return;
        }
        self.notification_runtime.clear(channel);
        self.request_images();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn notification_fallbacks_use_the_pinned_strings() {
        assert_eq!(
            fallback_body(&json!({"sticker_items":[{"name":"wave"}]})),
            "Sent a sticker: wave"
        );
        assert_eq!(
            fallback_body(&json!({"poll":{"question":{"text":"When?"}}})),
            "[poll] When?"
        );
        assert_eq!(
            fallback_body(&json!({"attachments":[{"filename":"picture.png"}]})),
            "Uploaded picture.png"
        );
        assert_eq!(
            fallback_body(&json!({"flags":8192})),
            "Sent a voice message"
        );
        assert_eq!(fallback_body(&json!({"content":"**hello**"})), "hello");
    }
}
