//! TypingIndicator (Vencord `src/plugins/typingIndicator`, GPL-3.0-or-later): who is typing in a channel, by the
//! channel's name in the channel list: up to three 16px avatars and the three dots, with the chat's "is typing" line
//! as the tooltip. Not part of the pinned client.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, avatar_source, color, typing_avatar};
use crate::plugins::State;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, img, px};
use serde_json::Value;
use std::time::Instant;

const ID: &str = "TypingIndicator";

/// `IndicatorMode`: dots 1, avatars 2.
const DOTS: i64 = 1;
const AVATARS: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settings {
    current_channel: bool,
    muted_channels: bool,
    ignored_users: bool,
    blocked_users: bool,
    mode: i64,
}

impl Settings {
    fn read(state: &State) -> Option<Self> {
        if !state.is_on(ID) {
            return None;
        }
        let flag = |key| state.value(ID, key) == Value::Bool(true);
        Some(Self {
            current_channel: flag("includeCurrentChannel"),
            muted_channels: flag("includeMutedChannels"),
            ignored_users: flag("includeIgnoredUsers"),
            blocked_users: flag("includeBlockedUsers"),
            mode: state
                .value(ID, "indicatorMode")
                .as_i64()
                .unwrap_or(DOTS | AVATARS),
        })
    }
}

/// Who the indicator names: the typing users without you, and without blocked or ignored people unless included.
pub(super) fn shown_users(
    settings: Settings,
    typing: &[Id],
    me: Option<Id>,
    blocked: impl Fn(Id) -> bool,
    ignored: impl Fn(Id) -> bool,
) -> Vec<Id> {
    typing
        .iter()
        .copied()
        .filter(|user| Some(*user) != me)
        .filter(|user| !(blocked(*user) && !settings.blocked_users))
        .filter(|user| !(ignored(*user) && !settings.ignored_users))
        .collect()
}

impl Root {
    /// The indicator of a channel row, or nothing.
    pub(in crate::ui) fn typing_indicator(
        &self,
        channel: Id,
        guild: Option<Id>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let settings = crate::plugins::with(Settings::read)?;
        if !settings.muted_channels
            && let Some(guild) = guild
            && self
                .stores
                .guild_settings
                .channel_muted(guild, channel, chrono::Utc::now())
        {
            return None;
        }
        if !settings.current_channel && self.channel == Some(channel) {
            return None;
        }
        let typing = self.stores.typing_users(channel, Instant::now());
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let users = shown_users(
            settings,
            &typing,
            me,
            |user| self.stores.relationships.get(&user) == Some(&2),
            |user| self.stores.is_ignored(user),
        );
        if users.is_empty() {
            return None;
        }
        let name = |user: Id| {
            let nick = guild
                .and_then(|guild| self.stores.members.get(guild, user))
                .and_then(|member| member.nick.clone());
            nick.or_else(|| {
                self.stores
                    .users
                    .get(&user)
                    .map(|known| known.display_name().to_owned())
            })
            .unwrap_or_default()
        };
        let text: String = match users.as_slice() {
            [a] => dc_intl::format_runs("lJ9sZX", &[("a", name(*a).into())]),
            [a, b] => {
                dc_intl::format_runs("rB0CUa", &[("a", name(*a).into()), ("b", name(*b).into())])
            }
            [a, b, c] => dc_intl::format_runs(
                "StKThj",
                &[
                    ("a", name(*a).into()),
                    ("b", name(*b).into()),
                    ("c", name(*c).into()),
                ],
            ),
            _ => dc_intl::format_runs("Q8lUnE", &[]),
        }
        .into_iter()
        .map(|(text, _)| text)
        .collect();
        let mut row = div().h(px(20.)).flex().items_center();
        if settings.mode & AVATARS != 0 {
            // `UserSummaryItem` at 16px, three at most, each over the one before.
            row = row.child(div().ml(px(6.)).flex().children(
                users.iter().take(3).enumerate().map(|(index, user)| {
                    let picture = self
                        .stores
                        .users
                        .get(user)
                        .and_then(|known| self.images.get(&avatar_source(known).0).cloned());
                    div()
                        .size(px(16.))
                        .rounded_full()
                        .when(index > 0, |this| this.ml(px(-4.)))
                        .border_1()
                        .border_color(color("--background-base-lower"))
                        .bg(color("--background-mod-normal"))
                        .overflow_hidden()
                        .children(picture.map(|picture| img(picture).size_full().rounded_full()))
                }),
            ));
        }
        if settings.mode & DOTS != 0 {
            // `ThreeDots` with `dotRadius` 3, themed.
            row = row.child(
                div()
                    .ml(px(6.))
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .child(typing_avatar::three_dots(3.)),
            );
        }
        Some(
            self.with_tip(
                format!("typing-indicator-{channel}"),
                Tip::body(Body::Text(text.into()), Side::Top),
                row,
                cx,
            )
            .into_any_element(),
        )
    }
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_indicator_leaves_out_who_its_settings_leave_out() {
        assert!(Settings::read(&State::default()).is_none());
        let mut state = State::default();
        state.set_enabled(ID, true);
        let settings = Settings::read(&state).unwrap();
        assert_eq!(settings.mode, DOTS | AVATARS);
        assert!(settings.current_channel && !settings.muted_channels);
        let users = shown_users(
            settings,
            &[1, 2, 3, 4],
            Some(1),
            |user| user == 2,
            |user| user == 3,
        );
        assert_eq!(users, [4]);
        state.set_value(ID, "includeBlockedUsers", &json!(true));
        state.set_value(ID, "includeIgnoredUsers", &json!(true));
        state.set_value(ID, "indicatorMode", &json!(1));
        let settings = Settings::read(&state).unwrap();
        assert_eq!(settings.mode, DOTS);
        let users = shown_users(
            settings,
            &[1, 2, 3, 4],
            Some(1),
            |user| user == 2,
            |user| user == 3,
        );
        assert_eq!(users, [2, 3, 4]);
    }
}
