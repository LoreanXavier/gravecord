//! The voice invite cards of module 679636: a guild invite to a voice channel the account can see shows who is in
//! that channel and a Join Voice button instead of the server card. For a guild in
//! `2026-05_voice_channel_list_invite_embed` it is the channel-list-style card (`dX` and `dZ`, class map 923710,
//! `*_OC_CcB` in stylesheet 58353) with how long the call has run; otherwise the voice invite embed (`cs`, class maps
//! 183751, 768920 and 323605) with its cloud of faces.
//!
//! The invite's channel is read from the resolved invite (`GET /invites/{code}`); everything the card shows comes
//! from the local stores, as `dZ` reads them: the channel (`ChannelStore`), its voice states (`getVoiceStatesForChannelAlt`
//! of module 607567) and its start time (`VoiceChannelStartTimeStore`, module 725613).

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, Message, User, permissions};
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, ScrollHandle, SharedString, Stateful,
    canvas, div, hsla, img, linear_color_stop, linear_gradient, point, prelude::*, px,
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
    /// `primary`: the brand control.
    Primary,
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
        ButtonLook::Primary => (
            "primary",
            "--control-primary-border-default",
            "--control-primary-text-default",
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

/// Module 402216 `Ay` in its round shape (`liveBadge` over module 805251's `notification` badge), as the channel list
/// draws it beside a streamer: "LIVE" in 12px semibold white on `--badge-critical-background`, 16px tall and 8px in.
pub(super) fn live_pill() -> gpui::Div {
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
        .child(dc_intl::t("dI3q4h").to_uppercase())
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

    /// Module 679636 `cr`: the channel of a resolved invite drawn as a voice card (a guild invite, `G4`, to a guild
    /// voice channel, with a guild) when the account can see it (`canBasicChannel(VIEW_CHANNEL)`): `dX` (which draws
    /// `dZ`, since the channel is known and visible) for a guild in the experiment, `cs` for any other.
    pub(super) fn voice_invite_channel(&self, body: &serde_json::Value) -> Option<(Id, Channel)> {
        let parse = |value: &serde_json::Value| value.as_str()?.parse::<Id>().ok();
        if body["type"].as_u64().unwrap_or(0) != 0 || body["channel"]["type"].as_u64() != Some(2) {
            return None;
        }
        parse(&body["guild"]["id"])?;
        let channel = parse(&body["channel"]["id"])?;
        let guild = self.stores.guild_of_channel(channel)?;
        let known = guild
            .channels
            .iter()
            .find(|known| known.id == channel && known.kind == ChannelKind::Voice)?
            .clone();
        let guild = guild.id;
        self.my_permission(guild, Some(&known), permissions::VIEW_CHANNEL)
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
            .filter(|(guild, _)| self.guild_experiment_on(LIST_INVITE_EXPERIMENT, *guild))
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

    /// `dZ`: the header (channel and call time), the people or the empty line, Join Voice, and the line under the card;
    /// `cs` instead when the guild is not in the experiment.
    pub(super) fn voice_channel_invite_card(
        &self,
        code: &str,
        message: &Message,
        guild: Id,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.guild_experiment_on(LIST_INVITE_EXPERIMENT, guild) {
            return self.voice_invite_embed(code, message, guild, channel, cx);
        }
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
            .when(person.streaming, |this| this.child(live_pill()))
            .into_any_element()
    }
}

/// The pointer over a `cs` card (`D` of module 679636): where it is across the card (-1 to 1 each way) and how near
/// the face cloud (0 to 2), for the cloud's drift.
#[derive(Clone, Copy, Default, PartialEq)]
struct Motion {
    x: f32,
    y: f32,
    proximity: f32,
}

thread_local! {
    static MOTIONS: RefCell<HashMap<String, Motion>> = RefCell::new(HashMap::new());
}

fn motion_of(key: &str) -> Motion {
    MOTIONS.with(|motions| motions.borrow().get(key).copied().unwrap_or_default())
}

/// A face's place in the cloud: from the left and from the top in percent, and its size in pixels.
type Place = (f32, f32, f32);

/// `cn` of module 679636 (`cloud_EbO-ZQ`): with more than four people the first three and a "+N" badge counting the
/// rest; the places (`data-count` 1 to 4, in percent of the 160 by 120 cloud) and the avatars' sizes.
pub(super) fn cloud_layout(people: usize) -> (usize, Option<usize>, &'static [Place]) {
    const ONE: [Place; 1] = [(25.6, 16.6, 80.)];
    const TWO: [Place; 2] = [(20.6, 10., 60.), (54.9, 57.5, 40.)];
    const THREE: [Place; 3] = [(13.1, 4.2, 60.), (56.9, 25., 48.), (33.4, 62.5, 40.)];
    const FOUR: [Place; 4] = [
        (16.2, 4.2, 60.),
        (54.4, 55., 48.),
        (22.5, 63.3, 40.),
        (60., 13.3, 40.),
    ];
    let overflow = (people > 4).then(|| people - 4 + 1);
    let shown = if overflow.is_some() { 3 } else { people };
    let count = (shown + usize::from(overflow.is_some())).min(4);
    let places: &'static [Place] = match count {
        1 => &ONE,
        2 => &TWO,
        3 => &THREE,
        _ => &FOUR,
    };
    (shown, overflow, places)
}

/// `cs`'s labels: who sent the invite, and whether they (or others) are in voice.
pub(super) fn embed_labels(
    mine: bool,
    author_inside: bool,
    people: usize,
) -> (String, Option<String>) {
    if people == 0 {
        let label = if mine {
            dc_intl::t("DVDvCD")
        } else {
            dc_intl::t("TY77rq")
        };
        return (label.to_owned(), Some(dc_intl::t("wM2WTM").to_owned()));
    }
    let others = people - usize::from(author_inside);
    let key = match (mine, author_inside) {
        (true, true) => "2RWMFV",
        (true, false) => "IWXzHV",
        (false, true) => "Da7tZx",
        (false, false) => "3VbQvv",
    };
    // The label is HTML text: an empty plural branch ("them  in voice") reads with one space.
    let label = dc_intl::format(key, &[("othersCount", (others as i64).into())])
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (label, None)
}

impl Root {
    /// Module 679636 `cs` (`container_tPzYCm`), the voice invite card when the guild is not in the list-style
    /// experiment: over a green glow and a field of dots, the channel's breadcrumb, who invited and who is there, the
    /// cloud of faces, and Join Voice (or Open, connected); "Only server members can see who's in voice" under it.
    ///
    /// GAP: the cloud moves with the pointer at once rather than on the spring `ct`, the "+N" badge does not open the
    /// list of everyone on hover (`ci` with `d8`), and Join Voice does not wait on an invite being accepted (`loading`,
    /// `dC`).
    pub(super) fn voice_invite_embed(
        &self,
        code: &str,
        message: &Message,
        guild: Id,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("voice-invite-embed-{}-{code}", message.id);
        let channel_id = channel.id;
        let people = self.voice_people(guild, channel_id);
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let author = message.author.id;
        let author_inside = people.iter().any(|person| person.user.id == author);
        let (label, sublabel) = embed_labels(me == Some(author), author_inside, people.len());
        let inside = self.in_voice_channel(channel_id);
        // `G`: the inviter first, then everyone else by voice affinity.
        let mut faces: Vec<&VoicePerson> = people
            .iter()
            .filter(|person| person.user.id != author)
            .collect();
        faces.sort_by(|a, b| {
            self.vc_probability(b.user.id)
                .partial_cmp(&self.vc_probability(a.user.id))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(first) = people.iter().find(|person| person.user.id == author) {
            faces.insert(0, first);
        }
        let guild_name = self
            .stores
            .guild(guild)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        // `d1` (`eyebrow_ZDtG9L`): the channel's 12px icon, "<server> / <channel>" in `body-xs/medium` and a 12px chevron,
        // 2px apart in `--text-muted` (`--interactive-text-hover` on hover); a click goes to the channel (`iN`).
        let crumb_group = SharedString::from(format!("{key}-crumb"));
        let tint = |name: &'static str| {
            gpui::svg()
                .path(format!("icons/{name}.svg"))
                .size(px(12.))
                .flex_none()
                .text_color(color("--text-muted"))
                .group_hover(crumb_group.clone(), |style| {
                    style.text_color(color("--interactive-text-hover"))
                })
        };
        let crumb = div()
            .id(crumb_group.clone())
            .group(crumb_group.clone())
            .parity("voice-invite-embed-crumb")
            .max_w_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .cursor_pointer()
            .text_color(color("--text-muted"))
            .hover(|style| style.text_color(color("--interactive-text-hover")))
            .child(tint("voice_channel"))
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .child(format!("{guild_name} / {}", channel.name)),
            )
            .child(tint("bot_chevron_small_right"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(guild, Some(channel_id));
                cx.notify();
            }));
        // `labels_tPzYCm`: the label in `body-md/medium` (three lines at most), the sublabel in `body-sm/normal`,
        // `--text-muted`, 2px under it.
        let labels = div()
            .flex()
            .flex_col()
            .max_w_full()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .line_clamp(3)
                    .child(label),
            )
            .children(sublabel.map(|text| {
                div()
                    .mt(px(2.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(text)
            }));
        // `content_tPzYCm`: the two at the two ends of the column; across both columns when nobody is in voice.
        let content = div()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .items_start()
            .justify_between()
            .when(faces.is_empty(), |this| this.w_full())
            .when(!faces.is_empty(), |this| this.flex_1())
            .child(crumb)
            .child(labels);
        let motion = if super::motion::reduced() {
            Motion::default()
        } else {
            motion_of(&key)
        };
        let cloud =
            (!faces.is_empty()).then(|| self.voice_face_cloud(&key, guild, &faces, motion, cx));
        // `body_tPzYCm`: two equal columns 24px (`--space-xl`) apart, 120px tall, 16px (`--space-md`) in except below.
        let body = div()
            .flex()
            .gap(px(24.))
            .h(px(136.))
            .pt(px(16.))
            .px(px(16.))
            .child(content)
            .children(cloud.map(|cloud| {
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .justify_end()
                    .child(cloud)
            }));
        // `chin_tPzYCm`: the button 16px in (none above), over a fade to the card's surface that starts 24px higher.
        let surface = color("--background-surface-high");
        let clear = gpui::Rgba { a: 0., ..surface };
        let button = ds_button(
            format!("{key}-join"),
            if inside {
                dc_intl::t("3xjX0U")
            } else {
                dc_intl::t("gpqgah")
            },
            if inside {
                ButtonLook::Secondary
            } else {
                ButtonLook::Active
            },
            false,
            None,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            // `S`: a member goes where the invite leads (`transitionToInvite`): into the channel's voice, or to the
            // channel when this client is in it already.
            if this.in_voice_channel(channel_id) {
                this.open_voice_view(guild, channel_id, cx);
            } else {
                this.request_join_voice(guild, channel_id, cx);
            }
            cx.notify();
        }));
        let chin = div().relative().p(px(16.)).pt(px(0.)).child(button);
        // `glow_tPzYCm` (#0e4f35, a circle the card's size blurred by 130px, moved 70% right) and `dots_tPzYCm` (20 by
        // 20 dots of 1.25px in `--icon-voice-connected` at 20%, over the right half, faded out from its middle right).
        let card_key = key.clone();
        let card = div()
            .id(SharedString::from(format!("{key}-card")))
            .parity("voice-invite-embed")
            .relative()
            .w_full()
            .max_w(px(432.))
            .min_w(px(160.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(surface)
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.anchor_probe(SharedString::from(format!("{key}-card"))))
            .child(
                div().absolute().top_0().left_0().size_full().child(
                    div()
                        .absolute()
                        .top_0()
                        .left(gpui::relative(0.7))
                        .size_full()
                        .rounded_full()
                        .shadow(vec![BoxShadow {
                            color: gpui::rgb(0x0e4f35).into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(260.),
                            spread_radius: px(0.),
                            inset: false,
                        }]),
                ),
            )
            .child(voice_invite_dots())
            .child(
                // The chin's fade (`chin_tPzYCm:before`, under the faces): from 24px over the chin to its bottom.
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(24. + 16. + 40.))
                    .flex()
                    .flex_col()
                    .child(div().h(px(24.)).bg(linear_gradient(
                        180.,
                        linear_color_stop(clear, 0.),
                        linear_color_stop(surface, 1.),
                    )))
                    .child(div().flex_1().bg(surface)),
            )
            .child(body)
            .child(chin)
            .on_mouse_move(
                cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                    if super::motion::reduced() {
                        return;
                    }
                    let anchors = this.tip_anchors.borrow();
                    let Some(bounds) = anchors
                        .get(&SharedString::from(format!("{card_key}-card")))
                        .copied()
                    else {
                        return;
                    };
                    let cloud = anchors
                        .get(&SharedString::from(format!("{card_key}-cloud")))
                        .copied();
                    drop(anchors);
                    let width = f32::from(bounds.size.width).max(1.);
                    let height = f32::from(bounds.size.height).max(1.);
                    let x = (f32::from(event.position.x - bounds.left()) / width) * 2. - 1.;
                    let y = (f32::from(event.position.y - bounds.top()) / height) * 2. - 1.;
                    // `l`: twice `exp(-distance / (0.2 * diagonal))` from the cloud's centre.
                    let proximity = cloud.map_or(0., |cloud| {
                        let centre = cloud.center();
                        let dx = f32::from(event.position.x - centre.x);
                        let dy = f32::from(event.position.y - centre.y);
                        2. * (-(dx * dx + dy * dy).sqrt()
                            / (0.2 * (width * width + height * height).sqrt()))
                        .exp()
                    });
                    let next = Motion { x, y, proximity };
                    let changed = MOTIONS.with(|motions| {
                        motions.borrow_mut().insert(card_key.clone(), next) != Some(next)
                    });
                    if changed {
                        cx.notify();
                    }
                }),
            )
            .on_hover(cx.listener({
                let key = key.clone();
                move |_, inside: &bool, _, cx| {
                    if !*inside {
                        MOTIONS.with(|motions| motions.borrow_mut().remove(&key));
                        cx.notify();
                    }
                }
            }));
        // `footer_tPzYCm`: EyeSlashIcon (14px) 2px before `fkg9mQ` in `body-sm/medium`, `--text-muted`, one line.
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
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(card)
            .child(footer)
            .into_any_element()
    }

    /// `cn`: the faces at their places in the 160 by 120 cloud, each on the card's surface with `--shadow-medium` and
    /// the name in a tooltip; the cloud drifts up to 6px with the pointer and grows up to 4% near it.
    fn voice_face_cloud(
        &self,
        key: &str,
        guild: Id,
        faces: &[&VoicePerson],
        motion: Motion,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (shown, overflow, places) = cloud_layout(faces.len());
        let (width, height) = (160., 120.);
        let shadow = || {
            vec![BoxShadow {
                color: hsla(0., 0., 0., 0.16),
                offset: point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }]
        };
        let mut items: Vec<AnyElement> = faces
            .iter()
            .take(shown)
            .zip(places.iter())
            .map(|(person, (left, top, size))| {
                let (avatar_key, avatar_url) =
                    self.guild_avatar_source_in(&person.user, Some(guild));
                let picture = self.images.get(&avatar_key).cloned();
                if picture.is_none() {
                    self.want_image(avatar_key, avatar_url, cx);
                }
                let face = div()
                    .size(px(*size))
                    .rounded_full()
                    .bg(color("--background-surface-high"))
                    .shadow(shadow())
                    .overflow_hidden()
                    .children(picture.map(|picture| img(picture).size(px(*size)).rounded_full()));
                div()
                    .absolute()
                    .left(px(width * left / 100.))
                    .top(px(height * top / 100.))
                    .child(self.with_tip(
                        format!("{key}-face-{}", person.user.id),
                        Tip::body(Body::Text(person.name.clone().into()), Side::Top),
                        face,
                        cx,
                    ))
                    .into_any_element()
            })
            .collect();
        if let Some(count) = overflow {
            let (left, top, _) = places[3];
            items.push(
                div()
                    .absolute()
                    .left(px(width * left / 100.))
                    .top(px(height * top / 100.))
                    .size(px(40.))
                    .rounded_full()
                    .bg(color("--background-mod-strong"))
                    .shadow(shadow())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(if count > 99 {
                        ">99".to_owned()
                    } else {
                        format!("+{count}")
                    })
                    .into_any_element(),
            );
        }
        let scale = 1. + motion.proximity / 2. * 0.08;
        div()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .left(px(motion.x * 6.))
            .top(px(motion.y * 6.))
            .paint_scale(scale, point(0.5, 0.5))
            .child(self.anchor_probe(SharedString::from(format!("{key}-cloud"))))
            .children(items)
            .into_any_element()
    }
}

/// `dots_tPzYCm` as drawn: the right half of the card, as tall as it is wide, a dot every twentieth of its width
/// starting a cell and a half to the left, each dot as opaque as the mask (`radial-gradient(100% 50% at 100% 50%)`) is
/// at its centre.
fn voice_invite_dots() -> impl IntoElement {
    let dot = color("--icon-voice-connected");
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width) / 2.;
            if width <= 0. {
                return;
            }
            let cell = width / 20.;
            let left = f32::from(bounds.left()) + width;
            let top = f32::from(bounds.top());
            for row in 0..20 {
                let y = cell * (row as f32 + 0.5);
                for column in 0..22 {
                    let x = cell * (column as f32 - 1.);
                    if !(0. ..=width).contains(&x) {
                        continue;
                    }
                    let fx = (x - width) / width;
                    let fy = (y - width / 2.) / (width / 2.);
                    let mask = (1. - (fx * fx + fy * fy).sqrt()).max(0.);
                    if mask <= 0. {
                        continue;
                    }
                    let mut tone = dot;
                    tone.a *= 0.2 * mask;
                    window.paint_quad(
                        gpui::fill(
                            gpui::Bounds::new(
                                point(px(left + x - 1.25), px(top + y - 1.25)),
                                gpui::size(px(2.5), px(2.5)),
                            ),
                            tone,
                        )
                        .corner_radii(px(1.25)),
                    );
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
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
    use super::{cloud_layout, embed_labels, order_key};

    #[test]
    fn the_cloud_shows_four_at_most_and_counts_the_rest_from_the_fourth() {
        assert_eq!(cloud_layout(1).0, 1);
        assert_eq!(cloud_layout(1).2.len(), 1);
        assert_eq!(cloud_layout(4), (4, None, cloud_layout(4).2));
        assert_eq!(cloud_layout(4).2.len(), 4);
        // Five people: three faces and "+2" in the fourth place.
        let (shown, overflow, places) = cloud_layout(5);
        assert_eq!((shown, overflow, places.len()), (3, Some(2), 4));
        assert_eq!(cloud_layout(2).2[0].2, 60.);
        assert_eq!(cloud_layout(3).2[1].2, 48.);
    }

    #[test]
    fn the_labels_name_the_inviter_and_count_the_others() {
        assert_eq!(
            embed_labels(false, false, 0),
            (
                "Invited you to join voice".to_owned(),
                Some("No one is currently in voice".to_owned())
            )
        );
        assert_eq!(
            embed_labels(true, false, 0).0,
            "Sent an invite to join voice"
        );
        assert_eq!(
            embed_labels(false, true, 3).0,
            "Invited you to join them and 2 others in voice"
        );
        assert_eq!(
            embed_labels(false, true, 1).0,
            "Invited you to join them in voice"
        );
        assert_eq!(
            embed_labels(true, false, 1).0,
            "Sent an invite to join 1 other in voice"
        );
        assert_eq!(embed_labels(false, false, 2).1, None);
    }

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
