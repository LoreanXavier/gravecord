//! The Commands section of a bot's DM profile (module 612770, `aG` of module 289287 in both sidebar layouts): the
//! application's popular commands (`application.popular_application_command_ids` of the profile answer) found in the
//! channel's application command index (842209 `ZV`), leaving out NSFW commands and commands with sub-commands, each a
//! `sm` secondary "/name" button with its description as a tooltip (990474 `Oh`) in `list_QBk4TQ` (wrapping, 6px
//! apart) under the `body-xs/semibold` heading "Commands" (900179). A press is a command mention's press (990474
//! `C`): "Clear message?" over a draft, then the command's chip in the composer.

use super::Root;
use super::dm_profile::Palette;
use super::tooltip::{Body, Side, Tip};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use serde_json::Value;

/// `ApplicationCommandOptionType` `SUB_COMMAND` and `SUB_COMMAND_GROUP`.
const SUB_COMMAND: u64 = 1;
const SUB_COMMAND_GROUP: u64 = 2;

/// The popular commands of the profile's application, in the profile's order: those the index has, without NSFW
/// ones and ones that have sub-commands (612770's filter). Each is (id, name, description).
pub(super) fn popular_commands(profile: &Value, index: &Value) -> Vec<(String, String, String)> {
    let Some(ids) = profile["application"]["popular_application_command_ids"].as_array() else {
        return Vec::new();
    };
    let commands = index["application_commands"].as_array();
    ids.iter()
        .filter_map(Value::as_str)
        .filter_map(|id| {
            commands?
                .iter()
                .find(|command| command["id"].as_str() == Some(id))
        })
        .filter(|command| command["nsfw"] != true)
        .filter(|command| {
            !command["options"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|option| {
                    matches!(
                        option["type"].as_u64(),
                        Some(SUB_COMMAND | SUB_COMMAND_GROUP)
                    )
                })
        })
        .map(|command| {
            let text = |key: &str| command[key].as_str().unwrap_or_default().to_owned();
            (text("id"), text("name"), text("description"))
        })
        .collect()
}

impl Root {
    /// The profile asks for the channel's index when its application has popular commands.
    pub(super) fn request_profile_commands(&mut self, user: Id) {
        let has_commands = self
            .fetched
            .get(&format!("profile:{user}"))
            .is_some_and(|body| body["application"]["popular_application_command_ids"].is_array());
        if has_commands {
            self.request_command_index();
        }
    }

    pub(super) fn profile_commands_section(
        &self,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let profile = self.fetched.get(&format!("profile:{user}"))?;
        let commands = popular_commands(profile, self.command_index_value()?);
        if commands.is_empty() {
            return None;
        }
        // GAP: 842209 also reads the user-installed index and drops commands the channel's permissions refuse
        // (`zl`); a DM with the bot is read from the channel's index only.
        let buttons: Vec<AnyElement> = commands
            .into_iter()
            .map(|(id, name, description)| {
                let Ok(command) = id.parse::<u64>() else {
                    return div().into_any_element();
                };
                let mention = name.clone();
                let target = self
                    .profile_small_button(
                        SharedString::from(format!("profile-command-{id}")),
                        format!("/{name}"),
                        false,
                        palette,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.press_command_mention(mention.clone(), command, window, cx);
                    }));
                let tip = Tip::body(Body::Text(description.into()), Side::Top);
                self.with_tip(format!("profile-command-tip-{id}"), tip, target, cx)
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .h(px(16.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(palette.token("--text-default"))
                        .child(dc_intl::t("0hKkS+")),
                )
                .child(div().flex().flex_wrap().gap(px(6.)).children(buttons))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn popular_commands_keep_the_profile_order_and_skip_nsfw_and_grouped_ones() {
        let profile = json!({"application": {"id": "9", "popular_application_command_ids": ["3", "1", "2", "4", "8"]}});
        let index = json!({"application_commands": [
            {"id": "1", "name": "play", "description": "Plays"},
            {"id": "2", "name": "admin", "description": "", "options": [{"type": 1, "name": "ban"}]},
            {"id": "3", "name": "roll", "description": "Rolls", "options": [{"type": 3, "name": "sides"}]},
            {"id": "4", "name": "spicy", "description": "", "nsfw": true}
        ]});
        let found = popular_commands(&profile, &index);
        let names: Vec<&str> = found.iter().map(|(_, name, _)| name.as_str()).collect();
        assert_eq!(names, ["roll", "play"]);
        assert!(popular_commands(&json!({}), &index).is_empty());
    }
}
