//! PreviewMessage (Vencord `src/plugins/previewMessage`, GPL-3.0-or-later): while the message box holds text or files,
//! the chat bar's first button is an eye ("Preview Message"); it shows the message as it would go out, as a message
//! only you can see (`sendBotMessage` with you as the author): the text, and the files with their pictures. Nothing is
//! sent. Not part of the pinned client.

use super::super::Root;
use dc_state::{Id, Message};
use gpui::Context;
use serde_json::{Value, json};

const ID: &str = "PreviewMessage";
/// `MessageFlags.EPHEMERAL`.
const EPHEMERAL: u64 = 64;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// A file of the message box as the preview shows it: its name, size, type and picture size.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct File {
    pub(in crate::ui) name: String,
    pub(in crate::ui) size: u64,
    pub(in crate::ui) content_type: Option<&'static str>,
    pub(in crate::ui) picture: Option<(u32, u32)>,
}

/// The preview message: yours, ephemeral, with the files as attachments (ids `id + 1 …`).
pub(in crate::ui) fn preview_value(
    id: Id,
    channel: Id,
    author: &Value,
    text: &str,
    files: &[File],
    now: chrono::DateTime<chrono::Utc>,
) -> Value {
    let attachments: Vec<Value> = files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let mut attachment = json!({
                "id": (id + 1 + index as u64).to_string(),
                "filename": file.name,
                "size": file.size,
                "url": format!("https://preview.invalid/{}", file.name),
                "proxy_url": format!("https://preview.invalid/{}", file.name),
            });
            if let Some(kind) = file.content_type {
                attachment["content_type"] = json!(kind);
            }
            if let Some((width, height)) = file.picture {
                attachment["width"] = json!(width);
                attachment["height"] = json!(height);
            }
            attachment
        })
        .collect();
    json!({
        "id": id.to_string(),
        "type": 0,
        "flags": EPHEMERAL,
        "content": text,
        "channel_id": channel.to_string(),
        "author": author,
        "attachments": attachments,
        "embeds": [],
        "pinned": false,
        "mentions": [],
        "mention_roles": [],
        "mention_everyone": false,
        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
        "tts": false,
    })
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

impl Root {
    /// Whether the chat bar shows the eye: the plugin is on and the box holds text or files.
    pub(in crate::ui) fn preview_message_shown(&self, cx: &gpui::App) -> bool {
        on() && (!self.composer.read(cx).text().trim().is_empty() || !self.pending_files.is_empty())
    }

    /// The eye was pressed: the message box as a message only you see.
    pub(in crate::ui) fn preview_message(&mut self, cx: &mut Context<Self>) {
        let (Some(channel), Some(me)) = (self.channel, self.stores.current_user.clone()) else {
            return;
        };
        let text = self.composer.read(cx).text().to_owned();
        let files: Vec<File> = self
            .pending_files
            .iter()
            .map(|pending| pending.preview_file())
            .collect();
        let now = chrono::Utc::now();
        let id = ((now.timestamp_millis() as u64).saturating_sub(DISCORD_EPOCH_MS)) << 22;
        let author = json!({
            "id": me.id.to_string(),
            "username": me.username,
            "global_name": me.global_name,
            "avatar": me.avatar,
            "discriminator": "0",
        });
        let Some(message) =
            Message::from_value(&preview_value(id, channel, &author, &text, &files, now))
        else {
            return;
        };
        // The pictures are the message box's own: each attachment's picture is put where the message looks for it.
        for (index, pending) in self.pending_files.iter().enumerate() {
            if let Some(picture) = pending.preview_picture() {
                self.images
                    .insert(format!("attachment:{}", id + 1 + index as u64), picture);
            }
        }
        self.show_local_only(message);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_is_yours_ephemeral_and_carries_the_files() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let author = json!({"id": "7", "username": "me", "discriminator": "0"});
        let files = vec![File {
            name: "a.png".to_owned(),
            size: 10,
            content_type: Some("image/png"),
            picture: Some((4, 3)),
        }];
        let value = preview_value(100, 5, &author, "hi", &files, now);
        let message = Message::from_value(&value).unwrap();
        assert_eq!(message.author.id, 7);
        assert_eq!(message.content, "hi");
        assert_eq!(value["flags"], json!(EPHEMERAL));
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].id, 101);
        assert_eq!(message.attachments[0].width, Some(4));
    }
}
