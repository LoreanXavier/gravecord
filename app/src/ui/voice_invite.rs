//! The channel-list-style voice invite card (module 679636 `dX` and `dZ`, class map 923710, `*_OC_CcB` in stylesheet
//! 58353): a guild invite to a voice channel the account can see shows who is in that channel, how long its call has
//! run and a Join Voice button, instead of the server card, for a guild in
//! `2026-05_voice_channel_list_invite_embed`.
//!
//! The invite's channel is read from the resolved invite (`GET /invites/{code}`); everything the card shows comes
//! from the local stores, as `dZ` reads them: the channel (`ChannelStore`), its voice states (`getVoiceStatesForChannelAlt`
//! of module 607567) and its start time (`VoiceChannelStartTimeStore`, module 725613).

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, Message, User, permissions};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, SharedString, Stateful, div, img,
    linear_color_stop, linear_gradient, prelude::*, px,
};
use std::cell::RefCell;
use std::collections::HashMap;

/// Module 679636 `dn` (`di`, location `InviteEmbed.isStreamInvite`): treatment 1 draws `dX`; the default is off.
pub(super) const LIST_INVITE_EXPERIMENT: &str = "2026-05_voice_channel_list_invite_embed";

/// `memberList_OC_CcB`: `max-height: 125px`.
const MEMBER_LIST_MAX: f32 = 125.;

thread_local! {
    /// The member scroller of each card (`useRef` `g` of `dZ`), for the bottom gradient.
    static SCROLLERS: RefCell<HashMap<String, ScrollHandle>> = RefCell::new(HashMap::new());
}

fn scroller(key: &str) -> ScrollHandle {
    SCROLLERS.with(|scrollers| {
        scrollers
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone()
    })
}

/// One person in a voice channel, as `VoiceStateStore` keeps them for a channel (module 607567 `L`).
pub(super) struct VoicePerson {
    pub user: User,
    /// `nick`: the member's nickname, else the user's name (`getName`).
    pub name: String,
    /// `voiceState.selfStream`.
    pub streaming: bool,
}

/// Module 607567 `R`: a channel's voice states sort by `{selfStream ? "\0" : "\x01"}{name lower case}\0{userId}`:
/// streamers first, then the name, then the id as text.
pub(super) fn order_key(streaming: bool, name: &str, user: Id) -> (bool, String, String) {
    (!streaming, name.to_lowercase(), user.to_string())
}

/// The design system button (module 821609 `$` over 657718 `S`) in the variants these cards use: `md` is 40px tall
/// with 16px text and a 20px icon (`refresh_sm`), `sm` 32px with 14px text and a 16px icon (`xs`), both 8px round
/// with a 1px border, the icon 4px before the text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonLook {
    /// `active`: the green connected control.
    Active,
    Secondary,
}

pub(super) fn ds_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    look: ButtonLook,
    small: bool,
    leading: Option<&'static str>,
) -> Stateful<gpui::Div> {
    let (family, border, text) = match look {
        ButtonLook::Active => (
            "connected",
            "--control-connected-border-default",
            "--control-connected-text-default",
        ),
        ButtonLook::Secondary => (
            "secondary",
            "--border-muted",
            "--control-secondary-text-default",
        ),
    };
    let state = move |part: &str, state: &str| color(&format!("--control-{family}-{part}-{state}"));
    let (hover, pressed) = (
        (
            state("background", "hover"),
            state("border", "hover"),
            state("text", "hover"),
        ),
        (
            state("background", "active"),
            state("border", "active"),
            state("text", "active"),
        ),
    );
    let icon_size = if small { 16. } else { 20. };
    div()
        .id(id.into())
        .w_full()
        .h(px(if small { 32. } else { 40. }))
        .px(px(if small { 11. } else { 15. }))
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(&format!("--control-{family}-background-default")))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .cursor_pointer()
        .text_size(px(if small { 14. } else { 16. }))
        .line_height(px(if small { 18. } else { 20. }))
        .font_weight(FontWeight(500.))
        .text_color(color(text))
        .hover(move |style| style.bg(hover.0).border_color(hover.1).text_color(hover.2))
        .active(move |style| {
            style
                .bg(pressed.0)
                .border_color(pressed.1)
                .text_color(pressed.2)
        })
        .children(leading.map(|name| {
            gpui::svg()
                .path(format!("icons/{name}.svg"))
                .size(px(icon_size))
                .flex_none()
                .text_color(color(text))
        }))
        .child(div().whitespace_nowrap().child(label.into()))
}

impl Root {
    /// A guild experiment's treatment 1 (`enabled: true`) for a guild; not eligible is the default config (off).
    pub(super) fn guild_experiment_on(&self, name: &str, guild: Id) -> bool {
        self.stores
            .guild_experiment_bucket(name, guild, super::elapsed::now_ms())
            == Some(1)
    }

    /// The people in a guild voice channel in the store's order (module 607567). Someone the user store does not
    /// know is still there (`L`: a placeholder user named "..."), with the member's user when the voice state brought
    /// one.
    pub(super) fn voice_people(&self, guild: Id, channel: Id) -> Vec<VoicePerson> {
        let mut people: Vec<VoicePerson> = self
            .stores
            .voice_members(guild, channel)
            .into_iter()
            .map(|state| {
                let member = self.stores.members.get(guild, state.user_id);
                let user = self
                    .stores
                    .users
                    .get(&state.user_id)
                    .cloned()
                    .or_else(|| {
                        self.stores
                            .current_user
                            .clone()
                            .filter(|me| me.id == state.user_id)
                    })
                    .or_else(|| member.map(|member| member.user.clone()))
                    .filter(|user| user.id == state.user_id)
                    .unwrap_or_else(|| User {
                        id: state.user_id,
                        username: "...".to_owned(),
                        ..User::default()
                    });
                let name = member
                    .and_then(|member| member.nick.clone())
                    .unwrap_or_else(|| user.display_name().to_owned());
                VoicePerson {
                    user,
                    name,
                    streaming: state.self_stream,
                }
            })
            .collect();
        people
            .sort_by_cached_key(|person| order_key(person.streaming, &person.name, person.user.id));
        people
    }

    /// Module 679636 `cr`: the channel of a resolved invite drawn as `dX` (a guild invite, `G4`, to a guild voice
    /// channel, with a guild) when the account can see it (`canBasicChannel(VIEW_CHANNEL)`) and the invite's guild is
    /// in the experiment. `dX` then draws `dZ`, since the channel is known and visible.
    ///
    /// GAP: with the experiment off, `cr` draws the voice invite card `cs` (the channel, the inviter's line, a
    /// moving stack of members, Join or Open Voice); this client draws the server card there.
    pub(super) fn voice_invite_channel(&self, body: &serde_json::Value) -> Option<(Id, Channel)> {
        let parse = |value: &serde_json::Value| value.as_str()?.parse::<Id>().ok();
        if body["type"].as_u64().unwrap_or(0) != 0 || body["channel"]["type"].as_u64() != Some(2) {
            return None;
        }
        let invite_guild = parse(&body["guild"]["id"])?;
        let channel = parse(&body["channel"]["id"])?;
        let guild = self.stores.guild_of_channel(channel)?;
        let known = guild
            .channels
            .iter()
            .find(|known| known.id == channel && known.kind == ChannelKind::Voice)?
            .clone();
        let guild = guild.id;
        (self.my_permission(guild, Some(&known), permissions::VIEW_CHANNEL)
            && self.guild_experiment_on(LIST_INVITE_EXPERIMENT, invite_guild))
        .then_some((guild, known))
    }

    /// `dZ`'s effect: a card whose channel has no start time asks for its guild's start times (module 52074), once.
    pub(super) fn request_voice_invite_start_times(&mut self) {
        let guilds: Vec<Id> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .flat_map(|message| super::invite_embed::invite_codes(&message.content))
            .filter_map(|code| self.fetched.get(&format!("invite:{code}")))
            .filter_map(|body| self.voice_invite_channel(body))
            .filter(|(guild, channel)| {
                self.stores
                    .channel_status
                    .get_start_time(*guild, channel.id)
                    .is_none()
            })
            .map(|(guild, _)| guild)
            .collect();
        for guild in guilds {
            self.request_channel_info(guild);
        }
    }

    /// `dZ`: the header (channel and call time), the people or the empty line, Join Voice, and the line under the card.
    pub(super) fn voice_channel_invite_card(
        &self,
        code: &str,
        message: &Message,
        guild: Id,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("voice-invite-{}-{code}", message.id);
        let channel_id = channel.id;
        let people = self.voice_people(guild, channel_id);
        // `isInChannel` (module 977997): this client is connected to the channel.
        let inside = self.in_voice_channel(channel_id);
        let start = self.voice_call_start(channel);
        let guild_name = self
            .stores
            .guild(guild)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        // `channelName_OC_CcB`: `body-md/normal`, one line, underlined in `--text-muted` on hover; a click selects the
        // channel (`iN` of module 378570), with "<server> / <channel>" over it.
        let name = div()
            .id(SharedString::from(format!("{key}-name")))
            .min_w(px(0.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color(if inside {
                "--text-default"
            } else {
                "--text-muted"
            }))
            .hover(|style| style.underline())
            .child(SharedString::from(channel.name.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(guild, Some(channel_id));
                cx.notify();
            }));
        let name = self
            .with_tip(
                format!("{key}-name-tip"),
                Tip::body(
                    Body::GuildTitle {
                        guild,
                        title: format!("{guild_name} / {}", channel.name).into(),
                    },
                    Side::Top,
                ),
                name,
                cx,
            )
            .min_w(px(0.))
            .flex_shrink(1.);
        // `header_OC_CcB` on `--background-mod-muted`: 16px in, 12px between the channel and the time.
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .p(px(16.))
            .bg(color("--background-mod-muted"))
            .child(
                // `channelInfo_OC_CcB`: 8px apart; the 20px VoiceNormalIcon 2px in, green while anyone is in it.
                div()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_none().pl(px(2.)).child(icon(
                        "voice_channel",
                        20.,
                        if people.is_empty() {
                            "--icon-subtle"
                        } else {
                            "--text-feedback-positive"
                        },
                    )))
                    .child(name),
            )
            .children(start.map(|start| self.elapsed_timer(start, cx)));
        let middle: AnyElement = if people.is_empty() {
            // `emptyState_OC_CcB`: `zSqdrS` in `body-sm/normal`, `text-muted`, 16px in.
            div()
                .p(px(16.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("zSqdrS"))
                .into_any_element()
        } else {
            self.voice_channel_invite_people(&key, guild, &people, cx)
        };
        // `joinButtonRegion_OC_CcB`: the full-width `active` button, `gpqgah` with VoiceNormalIcon; it goes where the
        // invite leads (`transitionToInvite`: the channel, joining its voice).
        let join = div().px(px(16.)).pb(px(16.)).child(
            ds_button(
                format!("{key}-join"),
                dc_intl::t("gpqgah"),
                ButtonLook::Active,
                false,
                Some("voice_channel"),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.request_join_voice(guild, channel_id, cx);
                cx.notify();
            })),
        );
        // `voiceChannelListInvite_OC_CcB`: up to 280px on `--background-surface-high`, `--radius-sm`, a 1px
        // `--border-subtle` outline inside the edge.
        let card = div()
            .parity("voice-invite-card")
            .w_full()
            .max_w(px(280.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(middle)
            .child(join);
        // `footer_OC_CcB`: EyeSlashIcon (14px) 2px before `fkg9mQ` in `body-sm/medium`, `text-muted`, one line.
        let footer = div()
            .min_w(px(0.))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(2.))
            .text_color(color("--text-muted"))
            .child(icon("eye_slash", 14., "--text-muted"))
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .child(dc_intl::t("fkg9mQ")),
            );
        // `container_OC_CcB`: the card and the line under it, 4px apart.
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(card)
            .child(footer)
            .into_any_element()
    }

    /// `memberListWrapper_OC_CcB` and `memberList_OC_CcB`: the rows of module 293277 (`voiceUserRow_OC_CcB`, no
    /// pointer events) in a scroller of at most 125px, 4px in on each side and 8px above and below, with a 48px fade
    /// to the card's surface at the bottom while more rows are below (`scrollGradient_OC_CcB`).
    fn voice_channel_invite_people(
        &self,
        key: &str,
        guild: Id,
        people: &[VoicePerson],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = scroller(key);
        let more_below =
            scroll.max_offset().y > px(1.) && -scroll.offset().y < scroll.max_offset().y - px(1.);
        let rows: Vec<AnyElement> = people
            .iter()
            .map(|person| self.voice_channel_invite_row(guild, person, cx))
            .collect();
        let surface = color("--background-surface-high");
        let clear = gpui::Rgba { a: 0., ..surface };
        div()
            .relative()
            .px(px(4.))
            .child(
                div()
                    .id(SharedString::from(format!("{key}-people")))
                    .max_h(px(MEMBER_LIST_MAX))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .py(px(8.))
                    .pl(px(4.))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .when(more_below, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(48.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(clear, 0.),
                            linear_color_stop(surface, 1.),
                        )),
                )
            })
            .into_any_element()
    }

    /// Module 293277 `ei` as `dZ` passes it (user, nick and `isStreaming` only): 32px tall, 4px by 8px inside, the
    /// 24px avatar 8px before the name (`body-sm/medium`, `--channels-default`), and LIVE for a streamer.
    fn voice_channel_invite_row(
        &self,
        guild: Id,
        person: &VoicePerson,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (avatar_key, avatar_url) = self.guild_avatar_source_in(&person.user, Some(guild));
        let picture = self.images.get(&avatar_key).cloned();
        if picture.is_none() {
            self.want_image(avatar_key, avatar_url, cx);
        }
        div()
            .flex_none()
            .h(px(32.))
            .py(px(4.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .overflow_hidden()
                    .children(picture.map(|picture| img(picture).size(px(24.)).rounded_full())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(SharedString::from(person.name.clone())),
            )
            .when(person.streaming, |this| {
                this.child(
                    div()
                        .flex_none()
                        .h(px(16.))
                        .px(px(8.))
                        .rounded_full()
                        .bg(color("--badge-critical-background"))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(gpui::rgba(0xffffffff))
                        .child(dc_intl::t("dI3q4h").to_uppercase()),
                )
            })
            .into_any_element()
    }
}

/// Capture only: guild experiment buckets set by hand.
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_guild_experiment(
        &mut self,
        name: &str,
        guild: &str,
        bucket: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let (Ok(guild), Ok(bucket)) = (guild.parse::<Id>(), bucket.parse::<i64>()) else {
            return "usage: guild-experiment <name> <guild> <bucket>".to_owned();
        };
        self.stores.guild_experiments.force(name, guild, bucket);
        cx.notify();
        format!(
            "{name} in {guild}: {:?}",
            self.stores
                .guild_experiment_bucket(name, guild, super::elapsed::now_ms())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::order_key;

    #[test]
    fn streamers_come_first_then_names_without_case_then_ids_as_text() {
        let mut people = [
            (false, "beta", 5u64),
            (true, "zed", 9),
            (false, "Alpha", 7),
            (false, "alpha", 10),
        ];
        people.sort_by_key(|(streaming, name, id)| order_key(*streaming, name, *id));
        let order: Vec<u64> = people.iter().map(|(_, _, id)| *id).collect();
        // "10" sorts before "7" as text, as the store's comparator string does.
        assert_eq!(order, vec![9, 10, 7, 5]);
    }
}
