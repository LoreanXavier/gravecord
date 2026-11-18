//! GreetStickerPicker (Vencord `src/plugins/greetStickerPicker`, GPL-3.0-or-later): a right click on "Wave to say hi!"
//! opens `greet-sticker-picker`: "Greet Mode" with the radios Greet and Message, then "Greet Stickers" with the six
//! welcome stickers by the first word of their description (Wumpus, Nelly, Clyde, Choco, Doggo, Peach). A sticker is
//! sent as a reply that mentions the newcomer: in Greet mode by `sendGreetMessage` (`POST /channels/<id>/greet`, three
//! greets at most), in Message mode as an ordinary message with the sticker (`_sendMessage`, as often as you like).
//!
//! GAP: the "Unholy Multi-Greet" submenu, behind the private settings `unholyMultiGreetEnabled` and
//! `multiGreetChoices` that the original's settings page does not show. Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row};
use crate::plugins::State;
use dc_state::Id;
use gpui::Context;
use serde_json::{Value, json};

const ID: &str = "GreetStickerPicker";

/// `WELCOME_STICKERS`' first words, in their order (module 117096 `tJ`).
const NAMES: [&str; 6] = ["Wumpus", "Nelly", "Clyde", "Choco", "Doggo", "Peach"];

/// Whether `greetMode` is Message (else Greet), while the plugin is on.
fn message_mode(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "greetMode").as_str() == Some("Message"))
}

fn current() -> Option<bool> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(message_mode)
}

/// The menu's entries for the mode `message`.
pub(in crate::ui) fn items(message: bool) -> Vec<Entry> {
    let mut greet = Row::new(Act::PluginGreetMode(false), "Greet");
    greet.check = Some(!message);
    greet.radio = true;
    let mut normal = Row::new(Act::PluginGreetMode(true), "Message");
    normal.check = Some(message);
    normal.radio = true;
    let mut out = vec![
        Entry::Heading("Greet Mode"),
        Entry::Row(greet),
        Entry::Row(normal),
        Entry::Separator,
        Entry::Heading("Greet Stickers"),
    ];
    out.extend(
        NAMES
            .iter()
            .enumerate()
            .map(|(index, name)| Entry::Row(Row::new(Act::PluginGreetSticker(index as u8), *name))),
    );
    out
}

impl Root {
    /// Whether the wave button takes a right click.
    pub(in crate::ui) fn greet_picker_on(&self) -> bool {
        current().is_some()
    }

    /// The menu's rows, or none with the plugin off.
    pub(in crate::ui) fn greet_picker_items(&self) -> Vec<Entry> {
        current().map(items).unwrap_or_default()
    }

    /// A row ran: the mode changes, or the sticker goes out.
    pub(in crate::ui) fn greet_picker_run(
        &mut self,
        act: Act,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) {
        let Some(normal) = current() else {
            return;
        };
        match act {
            Act::PluginGreetMode(to_message) => {
                crate::plugins::update(|state| {
                    state.set_value(
                        ID,
                        "greetMode",
                        &Value::from(if to_message { "Message" } else { "Greet" }),
                    );
                });
            }
            Act::PluginGreetSticker(index) => {
                let Some(sticker) = super::super::dm::welcome_sticker_at(usize::from(index)) else {
                    return;
                };
                let guild = self.guild;
                if normal {
                    // `_sendMessage` with `stickerIds` and the reply options (`shouldMention`).
                    let mut reference = json!({
                        "channel_id": channel.to_string(),
                        "message_id": message.to_string(),
                    });
                    if let Some(guild) = guild {
                        reference["guild_id"] = json!(guild.to_string());
                    }
                    let _ = self.requests.send(crate::net::Request::Rest {
                        key: None,
                        method: reqwest::Method::POST,
                        route: format!("/channels/{channel}/messages"),
                        body: Some(json!({
                            "content": "",
                            "tts": false,
                            "sticker_ids": [sticker.to_string()],
                            "message_reference": reference,
                        })),
                    });
                } else {
                    self.send_greet(channel, guild, message, sticker);
                }
            }
            _ => return,
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_the_mode_and_the_six_stickers() {
        let labels: Vec<String> = items(false)
            .iter()
            .map(|entry| match entry {
                Entry::Row(row) => format!(
                    "{}{}",
                    row.label,
                    if row.check == Some(true) { "*" } else { "" }
                ),
                Entry::Separator => "-".to_owned(),
                Entry::Heading(text) => format!("# {text}"),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "# Greet Mode",
                "Greet*",
                "Message",
                "-",
                "# Greet Stickers",
                "Wumpus",
                "Nelly",
                "Clyde",
                "Choco",
                "Doggo",
                "Peach"
            ]
        );
        let mut state = State::default();
        assert_eq!(message_mode(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(message_mode(&state), Some(false));
    }
}
