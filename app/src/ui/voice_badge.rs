//! The author's voice channel in a message header and its hover card (module 75604 `G`, `k` and `w`, placed by
//! module 549587 among the header's badges, after the role icon): a 20px icon of the guild voice channel the author
//! is in, for a guild in `2026-03_voice_badge` (module 593643). Resting on it 50ms opens the card to its right; it
//! closes 175ms after the pointer has left both (`J_` of module 451988 for each delay).
//!
//! Styles: `voiceChannelBadge_M2o0HZ` (module 396975) and `*_f4FLXc` (module 971718), both in stylesheet 990185, over
//! `popoutContainer_j9Vvzj` (module 188645 `Uq`, stylesheet 996969).

use super::tooltip::{Body, Side, Tip};
use super::voice_invite::{ButtonLook, VoicePerson, ds_button};
use super::{Root, color, icon};
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Guild, Id, Message, permissions};
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, FontWeight, IntoElement, Pixels, SharedString,
    anchored, deferred, div, hsla, img, point, prelude::*, px,
};
use std::time::Duration;

/// Module 593643: treatment 1 (`enabled: true`) shows the badge; the default is off.
pub(super) const VOICE_BADGE_EXPERIMENT: &str = "2026-03_voice_badge";
/// `J_(50, …)` and `J_(175, …)` of `k`.
const OPEN_AFTER: u64 = 50;
const CLOSE_AFTER: u64 = 175;
/// `maxUsers: 8` of the card's avatars.
const MAX_USERS: usize = 8;
/// `STAGE_VOICE`.
const STAGE: ChannelKind = ChannelKind::Other(13);

/// Which card a hover opened.
#[derive(Clone, PartialEq)]
pub(super) enum HoverCard {
    /// `w`: a guild voice channel, beside the badge of a message by `author`.
    Channel { guild: Id, channel: Id, author: Id },
    /// Module 772800: a person in a voice channel, beside their row in the channel list.
    User(super::voice_user_popout::VoiceUserTarget),
}

/// Module 342952: open 200px user-list popout state triggered from `+N` overflow pill.
pub(super) struct AvatarListPopout {
    pub title: String,
    pub users: Vec<(Id, String, String)>,
}

/// The hover cards' state: what is open and where, and where the pointer is.
#[derive(Default)]
pub(super) struct VoiceHover {
    open: Option<(HoverCard, SharedString, Bounds<Pixels>)>,
    /// The trigger the pointer is over.
    over_target: Option<SharedString>,
    over_card: bool,
    /// Bumped by every enter, so only the latest open delay acts.
    opening: u64,
    /// Bumped by every enter and leave, so only the latest close delay acts. Kept apart from `opening`: a leave
    /// reported after the next row's enter must not cancel that row's open.
    closing: u64,
    /// Module 342952: open 200px user-list popout triggered from `+N` overflow pill.
    pub(super) avatar_list_popout: Option<AvatarListPopout>,
}

/// Module 342952 (`661248` `*_NbMTpy`): official avatar stack sizes (`16/20/24/32px`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AvatarStackSize {
    Size16,
    Size20,
    Size24,
    Size32,
}

impl AvatarStackSize {
    pub(crate) fn px_size(self) -> f32 {
        match self {
            Self::Size16 => 16.,
            Self::Size20 => 20.,
            Self::Size24 => 24.,
            Self::Size32 => 32.,
        }
    }

    pub(crate) fn overlap_px(self) -> f32 {
        match self {
            Self::Size16 => -3.,
            Self::Size20 => -5.,
            Self::Size24 => -6.,
            Self::Size32 => -8.,
        }
    }

    pub(crate) fn pill_px(self) -> f32 {
        match self {
            Self::Size16 => 4.,
            Self::Size20 => 6.,
            Self::Size24 => 8.,
            Self::Size32 => 10.,
        }
    }

    pub(crate) fn font_px(self) -> f32 {
        match self {
            Self::Size16 | Self::Size20 => 10.,
            Self::Size24 => 12.,
            Self::Size32 => 14.,
        }
    }
}

/// Module 342952 `Z` and `y`: how many avatars show, and the overflow count for any `max_users`.
pub(crate) fn stack_counts_for(people: usize, max_users: usize) -> (usize, Option<usize>) {
    if people > max_users && max_users > 0 {
        (max_users - 1, Some(people - max_users + 1))
    } else {
        (people.min(max_users), None)
    }
}

/// Module 342952 (`N`): visible pill string (`+N` or `>99`).
pub(crate) fn avatar_stack_overflow_text(count: usize) -> String {
    if count > 99 {
        ">99".to_owned()
    } else {
        format!("+{count}")
    }
}

/// Module 342952 (`RGr9tj`): screen-reader overflow text `"and N others"`.
pub(crate) fn avatar_stack_sr_label(count: usize) -> String {
    dc_intl::format("RGr9tj", &[("count", (count as f64).into())])
}

/// Module 342952 (`R8Z8Qr`): interactive button `aria-label` `"View all (N)"`.
pub(crate) fn avatar_stack_button_aria(count: usize) -> String {
    dc_intl::format("R8Z8Qr", &[("count", (count as f64).into())])
}

/// Module 10862: the icon of a guild voice channel: a Stage (locked when the account cannot connect or it is
/// private), an age-restricted channel, a locked one, or the plain speaker.
pub(super) fn vocal_glyph(guild: &Guild, channel: &Channel, connect: bool) -> &'static str {
    let locked = !connect || super::server_onboarding::is_private(guild, channel);
    match channel.kind {
        STAGE if locked => "stage_channel_lock",
        STAGE => "stage_channel",
        _ if channel.nsfw => "voice_channel_nsfw",
        _ if locked => "voice_channel_lock",
        _ => "voice_channel",
    }
}

/// Module 342952 `Z` and `y`: how many avatars show, and the count of the "+N" pill (`count: users - maxUsers + 1`)
/// when there are more people than `maxUsers`, whose last place it takes.
#[allow(dead_code)]
pub(super) fn stack_counts(people: usize) -> (usize, Option<usize>) {
    if people > MAX_USERS {
        (MAX_USERS - 1, Some(people - MAX_USERS + 1))
    } else {
        (people, None)
    }
}

/// `isGuildVocal`: a voice or Stage channel.
fn vocal(channel: &Channel) -> bool {
    channel.kind == ChannelKind::Voice || channel.kind == STAGE
}

impl Root {
    /// `G`: the guild voice channel the author is in, when the account may see and connect to it, the message is not
    /// in a voice channel's text chat and the guild has the badge.
    ///
    /// GAP: `(0,d.r)() && UK(channel)` (modules 597306 and 323073) also hide it for an age-gated channel under
    /// `AGE_GATED_SPACES`; this client does not model that gate.
    fn author_voice_channel(&self, message: &Message) -> Option<(Id, Channel)> {
        let guild = self.current_guild()?;
        if !self.guild_experiment_on(VOICE_BADGE_EXPERIMENT, guild.id)
            || self.current_channel().is_some_and(vocal)
        {
            return None;
        }
        let state = self
            .stores
            .voice_states
            .get(&guild.id)?
            .iter()
            .find(|state| state.user_id == message.author.id)?;
        let channel = guild
            .channels
            .iter()
            .find(|channel| channel.id == state.channel_id && vocal(channel))?;
        (self.my_permission(guild.id, Some(channel), permissions::VIEW_CHANNEL)
            && self.my_permission(guild.id, Some(channel), permissions::CONNECT))
        .then(|| (guild.id, channel.clone()))
    }

    /// `G` in the header: the channel's icon at 20px in `--text-status-online` (`voiceChannelBadge_M2o0HZ`: `.25rem`
    /// after the name's badges, 1px down), opening `w` on hover.
    pub(super) fn voice_badge(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (guild, channel) = self.author_voice_channel(message)?;
        let known = self.stores.guild(guild)?;
        let glyph = vocal_glyph(known, &channel, true);
        let key = SharedString::from(format!("voice-badge-{}", message.id));
        let card = HoverCard::Channel {
            guild,
            channel: channel.id,
            author: message.author.id,
        };
        let target = key.clone();
        Some(
            div()
                .id(key.clone())
                .parity("voice-badge")
                .relative()
                .flex_none()
                .ml(px(4.))
                .mt(px(1.))
                .size(px(20.))
                .cursor_pointer()
                .child(icon(glyph, 20., "--text-status-online"))
                .child(self.anchor_probe(key))
                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                    if *inside {
                        this.voice_hover_enter(target.clone(), card.clone(), OPEN_AFTER, cx);
                    } else {
                        this.voice_hover_leave(&target, cx);
                    }
                }))
                .into_any_element(),
        )
    }

    /// The pointer reached a trigger: its card opens after `delay` unless it is open already; a pending close stops.
    pub(super) fn voice_hover_enter(
        &mut self,
        target: SharedString,
        card: HoverCard,
        delay: u64,
        cx: &mut Context<Self>,
    ) {
        let hover = &mut self.voice_hover;
        hover.over_target = Some(target.clone());
        hover.opening += 1;
        hover.closing += 1;
        if hover
            .open
            .as_ref()
            .is_some_and(|(open, key, _)| *open == card && *key == target)
        {
            return;
        }
        let generation = hover.opening;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = this.update(cx, |this, cx| {
                let hover = &this.voice_hover;
                if hover.opening != generation || hover.over_target.as_ref() != Some(&target) {
                    return;
                }
                let anchor = this.tip_anchors.borrow().get(&target).copied();
                if let Some(anchor) = anchor {
                    this.voice_hover.open = Some((card, target, anchor));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The pointer left a trigger: the card closes `CLOSE_AFTER` later unless the pointer is over it or a trigger.
    pub(super) fn voice_hover_leave(&mut self, target: &SharedString, cx: &mut Context<Self>) {
        let hover = &mut self.voice_hover;
        if hover.over_target.as_ref() == Some(target) {
            hover.over_target = None;
        }
        self.voice_hover_close_later(cx);
    }

    fn voice_hover_close_later(&mut self, cx: &mut Context<Self>) {
        let hover = &mut self.voice_hover;
        hover.closing += 1;
        let generation = hover.closing;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(CLOSE_AFTER))
                .await;
            let _ = this.update(cx, |this, cx| {
                let hover = &mut this.voice_hover;
                if hover.closing == generation
                    && hover.over_target.is_none()
                    && !hover.over_card
                    && hover.open.take().is_some()
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Whether `user` has a voice state in the guild's `channel`.
    fn in_voice_with(&self, guild: Id, channel: Id, user: Id) -> bool {
        self.stores
            .voice_members(guild, channel)
            .iter()
            .any(|state| state.user_id == user)
    }

    /// The card a hover opened, if any.
    pub(super) fn voice_hover_open(&self) -> Option<&HoverCard> {
        self.voice_hover.open.as_ref().map(|(card, _, _)| card)
    }

    /// The open hover card, to the right of its trigger and kept inside the window: `w` centred on the badge
    /// (`align: "center"`, the default 8px away), 772800 level with the row's top (the default `align: "top"`, 17px
    /// away), and not while the row's profile popout is open (`shouldShow: et && !em`).
    pub(super) fn voice_hover_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (card, _, anchor) = self.voice_hover.open.as_ref()?;
        let (body, at, corner) = match card {
            HoverCard::Channel {
                guild,
                channel,
                author,
            } => (
                // The badge goes with its message, the author's voice state and the experiment; its card with it.
                (self.guild == Some(*guild)
                    && self.guild_experiment_on(VOICE_BADGE_EXPERIMENT, *guild)
                    && self.in_voice_with(*guild, *channel, *author))
                .then(|| self.voice_channel_card(*guild, *channel, cx))??,
                point(
                    anchor.right() + px(8.),
                    anchor.top() + anchor.size.height / 2.,
                ),
                gpui::Anchor::LeftCenter,
            ),
            HoverCard::User(target) => {
                // The row goes when the person leaves the channel; its popout with it.
                if self
                    .profile
                    .as_ref()
                    .is_some_and(|open| open.user == target.user)
                    || !self.in_voice_with(target.guild, target.channel, target.user)
                {
                    return None;
                }
                (
                    self.voice_user_card(*target, cx)?,
                    point(
                        anchor.right() + px(super::voice_user_popout::SPACING),
                        anchor.top(),
                    ),
                    gpui::Anchor::TopLeft,
                )
            }
        };
        Some(
            deferred(
                anchored()
                    .anchor(corner)
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("voice-hover-card")
                            .occlude()
                            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                                this.voice_hover.over_card = *inside;
                                if *inside {
                                    this.voice_hover.closing += 1;
                                } else {
                                    this.voice_hover_close_later(cx);
                                }
                            }))
                            .child(body),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `w`: the channel and its call time over the people (up to 8 avatars) and Join or Open, 180px wide.
    ///
    /// GAP: the preview of the channel's first application stream (module 607407 with the LIVE mark of 402216) is
    /// not drawn; this client has no application stream previews. The avatars keep the store's order: the official
    /// sorts them by voice affinity (`vcProbability`, module 575731) when personalization is allowed.
    fn voice_channel_card(
        &self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let known = self.stores.guild(guild)?;
        let channel = known.channels.iter().find(|one| one.id == channel)?.clone();
        let glyph = vocal_glyph(
            known,
            &channel,
            self.my_permission(guild, Some(&channel), permissions::CONNECT),
        );
        let inside = self.in_voice_channel(channel.id);
        let start = self.voice_call_start(&channel);
        let people = self.voice_people(guild, channel.id);
        let channel_id = channel.id;
        // `popoutHeader_f4FLXc` on `--background-mod-muted`, 8px by 12px inside: `channelName_f4FLXc` (the icon in
        // `--text-feedback-positive`, 4px before `body-sm/medium` text that is cut at the edge) and the time.
        // The official fades the name's last 16px (`channelNameTextFade_f4FLXc`, a mask); it is cut here.
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .py(px(8.))
            .px(px(12.))
            .bg(color("--background-mod-muted"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon(glyph, 20., "--text-feedback-positive"))
                    .child(
                        div().flex_1().min_w(px(0.)).overflow_hidden().child(
                            div()
                                .whitespace_nowrap()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(SharedString::from(channel.name.clone())),
                        ),
                    ),
            )
            .children(start.map(|start| div().flex_none().child(self.elapsed_timer(start, cx))));
        // `3xjX0U` (secondary) when this client is in the channel, else `VJlc0S` (active); `sm`, full width. It
        // connects (or, connected, goes to) the channel (`handleVoiceConnect` with `routeDirectlyToChannel`).
        let button = ds_button(
            format!("voice-card-join-{channel_id}"),
            if inside {
                dc_intl::t("3xjX0U")
            } else {
                dc_intl::t("VJlc0S")
            },
            if inside {
                ButtonLook::Secondary
            } else {
                ButtonLook::Active
            },
            true,
            None,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.voice_hover.open = None;
            if !this.in_voice_channel(channel_id) {
                this.request_join_voice(guild, channel_id, cx);
            }
            this.select(guild, Some(channel_id));
            cx.notify();
        }));
        // `popoutContent_f4FLXc`: 12px inside, 12px between the parts.
        let content = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(12.))
            .children(self.voice_avatar_stack(guild, channel_id, &people, cx))
            .child(button);
        // `popoutContainer_f4FLXc` over `popoutContainer_j9Vvzj`: 180px on `--background-surface-high`, a 1px
        // `--border-faint` edge (its fallback `hsla(240,4%,61%,.04)`), `--radius-md`, `--shadow-border` and
        // `--shadow-low`.
        Some(
            div()
                .parity("voice-badge-card")
                .w(px(180.))
                .rounded(px(12.))
                .border_1()
                .border_color(gpui::rgba(0x9898_a00a))
                .bg(color("--background-surface-high"))
                .shadow(vec![
                    BoxShadow {
                        color: hsla(0., 0., 1., 0.08),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: false,
                    },
                    BoxShadow {
                        color: hsla(0., 0., 0., 0.14),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(4.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                ])
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(header)
                .child(content)
                .into_any_element(),
        )
    }

    /// Module 342952 `A` at 24px (`avatars_NbMTpy`): overlapping avatars with ring cutout,
    /// per-avatar username tooltips, interactive "+N" / ">99" pill (`RGr9tj`/`R8Z8Qr`), and
    /// clickable 200px bottom user-list popout (`.popoutWrapper_NbMTpy` / `.scroller_NbMTpy`).
    fn voice_avatar_stack(
        &self,
        guild: Id,
        channel: Id,
        people: &[VoicePerson],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if people.is_empty() {
            return None;
        }
        let sz = AvatarStackSize::Size24;
        let (shown, overflow) = stack_counts_for(people.len(), MAX_USERS);
        let last = shown.saturating_sub(1);
        let mut items: Vec<AnyElement> = people
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, person)| {
                let (avatar_key, avatar_url) =
                    self.guild_avatar_source_in(&person.user, Some(guild));
                let picture = self.images.get(&avatar_key).cloned();
                if picture.is_none() {
                    self.want_image(avatar_key, avatar_url, cx);
                }
                let is_last = index == last && overflow.is_none();
                let avatar = div()
                    .size(px(sz.px_size()))
                    .rounded_full()
                    .overflow_hidden()
                    .when(!is_last, |d| {
                        d.border_2().border_color(color("--background-base-lower"))
                    })
                    .children(
                        picture.map(|picture| img(picture).size(px(sz.px_size())).rounded_full()),
                    );
                self.with_tip(
                    format!("voice-card-avatar-{channel}-{}", person.user.id),
                    Tip::body(Body::Text(person.name.clone().into()), Side::Top),
                    avatar,
                    cx,
                )
                .when(!is_last, |this| this.mr(px(sz.overlap_px())))
                .into_any_element()
            })
            .collect();
        if let Some(count) = overflow {
            let popout_users: Vec<(Id, String, String)> = people
                .iter()
                .map(|p| (p.user.id, p.name.clone(), p.user.username.clone()))
                .collect();
            let aria = avatar_stack_button_aria(count);
            let sr = avatar_stack_sr_label(count);
            let pill = div()
                .id(SharedString::from(format!(
                    "voice-avatar-overflow-{channel}"
                )))
                .aria_label(SharedString::from(aria))
                .h(px(sz.px_size()))
                .min_w(px(sz.px_size()))
                .px(px(sz.pill_px()))
                .rounded(px(16.))
                .bg(color("--background-base-lowest"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(sz.font_px()))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--interactive-text-default"))
                .hover(|s| s.text_color(color("--interactive-text-hover")))
                .child(avatar_stack_overflow_text(count))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.voice_hover.avatar_list_popout = Some(AvatarListPopout {
                        title: sr.clone(),
                        users: popout_users.clone(),
                    });
                    cx.notify();
                }));
            items.push(pill.into_any_element());
        }
        Some(
            div()
                .flex()
                .items_center()
                .children(items)
                .into_any_element(),
        )
    }

    /// Module 342952 (`.popoutWrapper_NbMTpy` / `.scroller_NbMTpy`): 200px user-list popout
    /// opened by clicking an avatar stack overflow `+N` pill.
    pub(super) fn avatar_stack_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let AvatarListPopout { title, users } = self.voice_hover.avatar_list_popout.as_ref()?;
        let sz = AvatarStackSize::Size24;
        let (shown, overflow) = stack_counts_for(users.len(), 5);
        let stack_preview = div()
            .flex()
            .items_center()
            .children(
                users
                    .iter()
                    .take(shown)
                    .enumerate()
                    .map(|(i, (_, name, _))| {
                        let is_last = i == shown.saturating_sub(1) && overflow.is_none();
                        let ch = name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_ascii_uppercase()
                            .to_string();
                        div()
                            .size(px(sz.px_size()))
                            .rounded_full()
                            .bg(color("--brand-500"))
                            .when(!is_last, |d| {
                                d.border_2()
                                    .border_color(color("--background-surface-high"))
                                    .mr(px(sz.overlap_px()))
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(ch)
                    }),
            )
            .children(overflow.map(|cnt| {
                div()
                    .h(px(sz.px_size()))
                    .min_w(px(sz.px_size()))
                    .px(px(sz.pill_px()))
                    .rounded(px(16.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(sz.font_px()))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(avatar_stack_overflow_text(cnt))
            }));

        Some(
            deferred(
                div()
                    .id("avatar-stack-popout-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x00000044))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.voice_hover.avatar_list_popout = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("avatar-stack-popout-card")
                            .w(px(240.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_xl()
                            .p(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(stack_preview)
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(title.clone()),
                                    ),
                            )
                            .child(div().h(px(1.)).w_full().bg(color("--border-subtle")))
                            .child(div().flex().flex_col().gap(px(2.)).children(
                                users.iter().enumerate().map(|(idx, (uid, disp, uname))| {
                                    let uid_copy = *uid;
                                    let initial = disp
                                        .chars()
                                        .next()
                                        .unwrap_or('?')
                                        .to_ascii_uppercase()
                                        .to_string();
                                    div()
                                        .id(SharedString::from(format!("avatar-stack-user-{idx}")))
                                        .px(px(8.))
                                        .py(px(5.))
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .size(px(24.))
                                                .rounded_full()
                                                .bg(color("--brand-500"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--white"))
                                                .child(initial),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(disp.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(color("--text-muted"))
                                                .child(uname.clone()),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.voice_hover.avatar_list_popout = None;
                                            this.open_profile(
                                                uid_copy,
                                                Bounds::default(),
                                                false,
                                                cx,
                                            );
                                            cx.notify();
                                        }))
                                }),
                            )),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_avatar_stack(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.voice_hover.avatar_list_popout = None;
            cx.notify();
            return "avatar-stack: closed".to_owned();
        }
        let demo = vec![
            (101, "Mina".to_owned(), "@mina".to_owned()),
            (102, "Kaan".to_owned(), "@kaan".to_owned()),
            (103, "Devon".to_owned(), "@devon".to_owned()),
            (104, "Alex".to_owned(), "@alex".to_owned()),
            (105, "Sora".to_owned(), "@sora".to_owned()),
            (106, "Yuna".to_owned(), "@yuna".to_owned()),
            (107, "Tarik".to_owned(), "@tarik".to_owned()),
        ];
        let sr = avatar_stack_sr_label(3);
        self.voice_hover.avatar_list_popout = Some(AvatarListPopout {
            title: sr,
            users: demo,
        });
        cx.notify();
        "avatar-stack: opened".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_eight_people_the_last_place_counts_the_rest() {
        assert_eq!(stack_counts(1), (1, None));
        assert_eq!(stack_counts(8), (8, None));
        assert_eq!(stack_counts(9), (7, Some(2)));
        assert_eq!(stack_counts(120), (7, Some(113)));
    }

    #[test]
    fn avatar_stack_342952_sizes_overflow_and_intl() {
        for key in ["RGr9tj", "R8Z8Qr"] {
            assert!(dc_intl::has_key(key), "missing 342952 key {key}");
        }
        assert_eq!(avatar_stack_overflow_text(3), "+3");
        assert_eq!(avatar_stack_overflow_text(99), "+99");
        assert_eq!(avatar_stack_overflow_text(100), ">99");
        assert_eq!(AvatarStackSize::Size16.overlap_px(), -3.);
        assert_eq!(AvatarStackSize::Size20.overlap_px(), -5.);
        assert_eq!(AvatarStackSize::Size24.overlap_px(), -6.);
        assert_eq!(AvatarStackSize::Size32.overlap_px(), -8.);
        assert!(avatar_stack_sr_label(4).contains('4'));
        assert!(avatar_stack_button_aria(4).contains('4'));
    }
}
