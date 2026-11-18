//! UserMessagesPronouns (Vencord `src/plugins/userMessagesPronouns`, GPL-3.0-or-later): after a message's time,
//! `• <pronouns>` in the time's own style, with "Pronouns" as its tooltip. The pronouns are the server profile's, else
//! the account's, from a profile already loaded (`UserProfileStore`; nothing is fetched for it). Not part of the
//! pinned client. Its compact-mode place is not drawn: the app has no compact message display.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, color};
use crate::plugins::State;
use dc_state::{Id, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, px};
use serde_json::Value;

const ID: &str = "UserMessagesPronouns";
/// `MessageType.AUTO_MODERATION_ACTION`.
const AUTO_MODERATION_ACTION: u64 = 24;

/// `(capitalized, show_self)`, while the plugin is on.
fn read(state: &State) -> Option<(bool, bool)> {
    state.is_on(ID).then(|| {
        (
            state.value(ID, "pronounsFormat") == "CAPITALIZED",
            state.value(ID, "showSelf") != Value::Bool(false),
        )
    })
}

/// `useDiscordPronouns` + `useFormattedPronouns`: the server profile's pronouns when it has some, else the account's;
/// trimmed, line breaks taken out, and lowercased unless `capitalized`.
pub(super) fn formatted(body: &Value, guild: Option<Id>, capitalized: bool) -> Option<String> {
    let guild_pronouns = guild
        .filter(|guild| {
            body["guild_member_profile"]["guild_id"].as_str() == Some(guild.to_string().as_str())
        })
        .and_then(|_| body["guild_member_profile"]["pronouns"].as_str())
        .filter(|pronouns| !pronouns.is_empty());
    let pronouns = guild_pronouns.or_else(|| body["user_profile"]["pronouns"].as_str())?;
    let mut text: String = pronouns.trim().split('\n').collect();
    if !capitalized {
        text = text.to_lowercase();
    }
    (!text.is_empty()).then_some(text)
}

impl Root {
    /// The `• pronouns` after `message`'s time, or nothing.
    pub(in crate::ui) fn message_pronouns(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let (capitalized, show_self) = crate::plugins::with(read)?;
        let author = &message.author;
        // `shouldShow`.
        if author.bot || author.system || message.kind == AUTO_MODERATION_ACTION {
            return None;
        }
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == author.id);
        if own && !show_self {
            return None;
        }
        let body = self.fetched.get(&format!("profile:{}", author.id))?;
        let text = formatted(body, self.guild, capitalized)?;
        Some(
            self.with_tip(
                format!("message-pronouns-{}", message.id),
                Tip::body(Body::Text(dc_intl::tr("Pronouns").into()), Side::Top),
                div()
                    .text_size(px(12.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(format!("\u{2022} {text}")),
                cx,
            )
            .ml(px(4.))
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn server_pronouns_first_then_the_accounts_formatted() {
        let body = json!({
            "user_profile": {"pronouns": " He/Him\n"},
            "guild_member_profile": {"guild_id": "5", "pronouns": "They/Them"}
        });
        assert_eq!(
            formatted(&body, Some(5), false).as_deref(),
            Some("they/them")
        );
        assert_eq!(
            formatted(&body, Some(5), true).as_deref(),
            Some("They/Them")
        );
        assert_eq!(formatted(&body, Some(6), false).as_deref(), Some("he/him"));
        assert_eq!(formatted(&body, None, true).as_deref(), Some("He/Him"));
        assert_eq!(formatted(&json!({}), None, true), None);
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some((false, true)));
        state.set_value(ID, "pronounsFormat", &json!("CAPITALIZED"));
        state.set_value(ID, "showSelf", &json!(false));
        assert_eq!(read(&state), Some((true, false)));
    }
}
