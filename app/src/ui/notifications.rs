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

/// `Q` of module 411238: `name (#channel, parent)`, each part isolated.
fn message_title(name: &str, channel: &str, parent: Option<&str>) -> String {
    let parent = parent
        .map(|parent| format!(", \u{2068}{parent}\u{2069}"))
        .unwrap_or_default();
    format!("\u{2068}{name}\u{2069} (\u{2068}#{channel}\u{2069}{parent})")
}

/// The channel and parent names of a channel line: a channel with its category; a thread or forum post with the
/// channel it hangs under. A thread's starter message names the parent and the parent's own parent instead.
fn channel_title_parts(
    guild: &dc_state::Guild,
    channel: Id,
    starter: bool,
) -> Option<(String, Option<String>)> {
    let named = |id: Id| guild.channels.iter().find(|known| known.id == id);
    if let Some(target) = named(channel) {
        let parent = target.parent_id.and_then(named).map(|p| p.name.clone());
        return Some((target.name.clone(), parent));
    }
    let thread = guild.threads.iter().find(|thread| thread.id == channel)?;
    let parent = named(thread.parent_id);
    if starter && let Some(parent) = parent {
        let above = parent.parent_id.and_then(named).map(|p| p.name.clone());
        return Some((parent.name.clone(), above));
    }
    Some((thread.name.clone(), parent.map(|p| p.name.clone())))
}

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
    /// `refresh_notification_badge` at most every `BADGE_EVERY`. It walks every channel of every server, and the client
    /// runs it for each update it gets (a picture, a presence, a typing notice); a refresh asked for inside the interval
    /// waits for the end of it, so the badge is never more than that late.
    pub(super) fn refresh_notification_badge_soon(&mut self, cx: &mut Context<Self>) {
        const BADGE_EVERY: std::time::Duration = std::time::Duration::from_millis(250);
        #[allow(unused_mut)]
        let mut due = self
            .prepared
            .badge_at
            .is_none_or(|at| at.elapsed() >= BADGE_EVERY);
        #[cfg(feature = "capture")]
        if crate::perf::legacy("badge") {
            due = true;
        }
        if due {
            self.prepared.badge_at = Some(Instant::now());
            self.refresh_notification_badge();
        } else if !self.prepared.badge_waiting {
            self.prepared.badge_waiting = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(BADGE_EVERY).await;
                let _ = this.update(cx, |this, _| {
                    this.prepared.badge_waiting = false;
                    this.prepared.badge_at = Some(Instant::now());
                    this.refresh_notification_badge();
                });
            })
            .detach();
        }
    }

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
        self.speak_incoming(message, cx);
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
        // 411238 `J`: the author's name by channel kind (a server's nickname, a DM's friend nickname), then the
        // channel line of `Q`.
        let starter = message["type"].as_u64() == Some(21);
        let guild_title = self.stores.guilds.iter().find_map(|guild| {
            let (channel_name, parent_name) = channel_title_parts(guild, channel, starter)?;
            let nick = self
                .stores
                .members
                .get(guild.id, author.id)
                .and_then(|member| member.nick.clone());
            Some(message_title(
                nick.as_deref().unwrap_or(author.display_name()),
                &channel_name,
                parent_name.as_deref(),
            ))
        });
        let name = self.dm_display_name(&author);
        let title = if let Some(title) = guild_title {
            title
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
    fn threads_and_posts_are_titled_with_their_parent() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user":{"id":"1","username":"fixture"},
                "guilds":[{"id":"100","name":"g","channels":[
                    {"id":"1","type":4,"name":"Text"},
                    {"id":"2","type":15,"name":"forum","parent_id":"1"},
                    {"id":"3","type":0,"name":"chat"}
                ],"threads":[{"id":"20","type":11,"parent_id":"2","name":"Bug report"}]}]
            }),
        );
        let guild = &stores.guilds[0];
        assert_eq!(
            channel_title_parts(guild, 3, false),
            Some(("chat".into(), None))
        );
        assert_eq!(
            channel_title_parts(guild, 20, false),
            Some(("Bug report".into(), Some("forum".into())))
        );
        // A thread's starter message names the parent and its category.
        assert_eq!(
            channel_title_parts(guild, 20, true),
            Some(("forum".into(), Some("Text".into())))
        );
        assert_eq!(channel_title_parts(guild, 99, false), None);
        assert_eq!(
            message_title("Ann", "Bug report", Some("forum")),
            "\u{2068}Ann\u{2069} (\u{2068}#Bug report\u{2069}, \u{2068}forum\u{2069})"
        );
        assert_eq!(
            message_title("Ann", "chat", None),
            "\u{2068}Ann\u{2069} (\u{2068}#chat\u{2069})"
        );
    }
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
