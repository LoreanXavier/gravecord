//! ServerListIndicators (Vencord `src/plugins/serverListIndicators`, GPL-3.0-or-later): a server list item under the
//! Home button with the number of friends online, the number of servers, or both, and the same as its tooltip. Not
//! part of the pinned client.

use super::super::motion::Transitions as _;
use super::super::tooltip::{Body, Tip};
use super::super::{Root, color, icon};
use crate::plugins::State;
use gpui::{AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

const ID: &str = "ServerListIndicators";
/// `IndicatorType`.
const SERVER: i64 = 1;
const FRIEND: i64 = 2;

fn mode_in(state: &State) -> Option<i64> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "mode").as_i64().unwrap_or(FRIEND))
}

/// The tooltip of the item.
pub(super) fn text(mode: i64, friends: usize, servers: usize) -> String {
    match mode {
        SERVER => format!("{servers} Servers"),
        FRIEND => format!("{friends} Friends"),
        _ => format!("{friends} Friends, {servers} Servers"),
    }
}

impl Root {
    /// Friends whose status is anything but offline (`RelationshipStore.getFriendIDs`, `PresenceStore.getStatus`).
    fn online_friends(&self) -> usize {
        self.stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == 1)
            .filter(|(user, _)| !matches!(self.stores.status(**user), "offline" | "invisible" | ""))
            .count()
    }

    /// The servers, with the join requests of servers not joined yet (`UserGuildJoinRequestStore`).
    fn server_count(&self) -> usize {
        self.stores.guilds.len()
    }

    pub(in crate::ui) fn server_list_indicators(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let mode = crate::plugins::with(mode_in)?;
        let (friends, servers) = (self.online_friends(), self.server_count());
        let part = |name: &'static str, count: usize| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(icon(name, 12., "--text-default"))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(count.to_string()),
                )
        };
        let tile = div()
            .id("server-list-indicators")
            .flex_none()
            .size(px(40.))
            .rounded(px(12.))
            .bg(color("--background-mod-subtle"))
            .rule(super::super::motion::Rule::CircleIconButton)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .when_some((mode & FRIEND != 0).then_some(()), |this, _| {
                this.child(part("plugin_friends", friends))
            })
            .when_some((mode & SERVER != 0).then_some(()), |this, _| {
                this.child(part("plugin_server", servers))
            });
        Some(
            self.with_tip(
                "server-list-indicators",
                Tip::rail(Body::Heading(text(mode, friends, servers).into())),
                tile,
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
    fn the_item_says_what_its_mode_counts() {
        assert_eq!(mode_in(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(mode_in(&state), Some(FRIEND));
        state.set_value(ID, "mode", &json!(3));
        assert_eq!(mode_in(&state), Some(3));
        assert_eq!(text(FRIEND, 4, 9), "4 Friends");
        assert_eq!(text(SERVER, 4, 9), "9 Servers");
        assert_eq!(text(3, 4, 9), "4 Friends, 9 Servers");
    }
}
