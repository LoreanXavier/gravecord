//! MemberCount (Vencord `src/plugins/memberCount`, GPL-3.0-or-later): the online, total and in-voice counts of a server
//! at the top of the member list, and the online and total counts in the server's tooltip. Not part of the pinned
//! client.
//!
//! The counts are the stores' own: the member list's groups (everyone not in "offline") for the open channel, else the
//! guild's online count (`GUILD_MEMBER_LIST_UPDATE`); the member count of the guild; the voice states in channels you
//! may see. The original also asks the gateway for the online count of a server whose list was never opened
//! (`ChannelActionCreators.preload`); plugin code sends nothing here, so such a server shows "?" as the original does
//! until its count arrives.

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div,
    px, svg,
};
use serde_json::Value;

const ID: &str = "MemberCount";

/// The counts shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Counts {
    pub online: Option<u64>,
    pub total: u64,
    pub voice: u64,
}

/// `Intl.NumberFormat()` in English: thousands with commas.
pub(super) fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Which of the two places is on: the member list (`memberList`) and the tooltip (`toolTip`), and `voiceActivity`.
fn places(state: &State) -> Option<(bool, bool, bool)> {
    state.is_on(ID).then(|| {
        let flag = |key| state.value(ID, key) == Value::Bool(true);
        (flag("memberList"), flag("toolTip"), flag("voiceActivity"))
    })
}

fn current() -> Option<(bool, bool, bool)> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(places)
}

impl Root {
    fn member_counts(&self, guild: Id, tooltip: bool, voice: bool) -> Option<Counts> {
        let total = self.stores.member_counts.get_member_count(guild)?;
        // The member list of the open channel, when it is this server's (`ChannelMemberStore.getProps`).
        let listed = (!tooltip && self.guild == Some(guild))
            .then(|| self.stores.member_lists.get(&guild))
            .flatten()
            .filter(|list| !list.groups.is_empty())
            .map(|list| {
                list.groups
                    .iter()
                    .filter(|(id, _)| id != "offline")
                    .map(|(_, count)| count)
                    .sum()
            });
        let online = listed.or_else(|| self.stores.member_counts.get_online_count(guild));
        let voice = if voice && !tooltip {
            let channels: Vec<dc_state::Channel> = self
                .stores
                .guild(guild)
                .map(|known| known.channels.clone())
                .unwrap_or_default();
            self.stores
                .voice_states
                .get(&guild)
                .map(|states| {
                    states
                        .iter()
                        .filter(|state| state.channel_id != 0)
                        .filter(|state| {
                            channels.iter().any(|channel| {
                                channel.id == state.channel_id
                                    && self.can_in_channel(
                                        channel,
                                        dc_state::permissions::VIEW_CHANNEL,
                                    )
                            })
                        })
                        .count() as u64
                })
                .unwrap_or(0)
        } else {
            0
        };
        Some(Counts {
            online,
            total,
            voice,
        })
    }

    /// The widget at the top of the member list (`vc-membercount-member-list`).
    pub(in crate::ui) fn member_count_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (list, _, voice) = current()?;
        let guild = self.guild?;
        if !list {
            return None;
        }
        let counts = self.member_counts(guild, false, voice)?;
        let online = counts.online.map_or_else(|| "?".to_owned(), number);
        let mut row = div()
            .mt(px(16.))
            .px(px(16.))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(13.6))
            .text_size(px(16.))
            .line_height(px(19.2));
        row = row.child(self.with_tip(
            "membercount-online",
            Tip::body(
                Body::Text(format!("{online} online in this channel").into()),
                Side::Bottom,
            ),
            part(dot(18., true), online, "--green-360"),
            cx,
        ));
        row = row.child(self.with_tip(
            "membercount-total",
            Tip::body(
                Body::Text(format!("{} total server members", number(counts.total)).into()),
                Side::Bottom,
            ),
            part(dot(15., false), number(counts.total), "--primary-400"),
            cx,
        ));
        if voice && counts.voice > 0 {
            row = row.child(
                self.with_tip(
                    "membercount-voice",
                    Tip::body(
                        Body::Text(format!("{} members in voice", number(counts.voice)).into()),
                        Side::Bottom,
                    ),
                    part(
                        svg()
                            .path("icons/plugin_voice.svg")
                            .size(px(15.))
                            .text_color(color("--primary-400"))
                            .into_any_element(),
                        number(counts.voice),
                        "--primary-400",
                    ),
                    cx,
                ),
            );
        }
        Some(row.into_any_element())
    }

    /// The counts under a server's name in its tooltip (`vc-membercount-tooltip`): online and total.
    pub(in crate::ui) fn member_count_tooltip(&self, guild: Id) -> Option<AnyElement> {
        let (_, tooltip, _) = current()?;
        if !tooltip {
            return None;
        }
        let counts = self.member_counts(guild, true, false)?;
        let online = counts.online.map_or_else(|| "?".to_owned(), number);
        Some(
            div()
                .mt(px(4.))
                .ml(px(2.))
                .flex()
                .gap(px(13.6))
                .text_size(px(16.))
                .line_height(px(20.))
                .child(part(dot(18., true), online, "--green-360"))
                .child(part(dot(15., false), number(counts.total), "--primary-400"))
                .into_any_element(),
        )
    }
}

/// `CircleIcon`: a dot of radius 8 in 24 (online: filled with `--status-online`; total: a 4px ring of
/// `--icon-status-offline`, in 24 too).
fn dot(size: f32, filled: bool) -> AnyElement {
    let diameter = size * 16. / 24.;
    let inset = (size - diameter) / 2.;
    let circle = div().size(px(diameter)).rounded_full();
    let circle = if filled {
        circle.bg(color("--status-online"))
    } else {
        circle
            .border(px(size * 4. / 24.))
            .border_color(color("--icon-status-offline"))
    };
    div()
        .size(px(size))
        .p(px(inset))
        .child(circle)
        .into_any_element()
}

/// One count: its icon and number, 8px apart (`vc-membercount-container`).
fn part(icon: AnyElement, text: String, tint: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("membercount-{tint}-{text}")))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(icon)
        .child(div().text_color(color(tint)).child(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_read_as_intl_does() {
        assert_eq!(number(0), "0");
        assert_eq!(number(999), "999");
        assert_eq!(number(1000), "1,000");
        assert_eq!(number(1234567), "1,234,567");
    }

    #[test]
    fn each_place_has_its_switch() {
        assert_eq!(places(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(places(&state), Some((true, true, true)));
        state.set_value(ID, "toolTip", &json!(false));
        state.set_value(ID, "voiceActivity", &json!(false));
        assert_eq!(places(&state), Some((true, false, false)));
    }
}
