//! Activity Invite, Spotify Listening Party, Request to Stream, and Game Invite message embeds
//! (module 584283, helper copy 456060, stream request 861089 / 191970).
//!
//! Renders inside `Root::accessories` when a message carries `message.extra.activity` (type 1 Join,
//! 2 Spectate, 3 Listen / Spotify party, 5 Request to Stream) or via the capture preview overlay:
//! - Spotify Listening Party: active (`JCvHtx` "{track}\nby {artist}", party count `Zogoou`/`UGei0j`,
//!   `VJlc0S` "Join") and expired (`84qx9r` "The listening party is over!").
//! - Request to Stream: header `nAyuPp` ("Stream Request"), subtitle `8B3U5O`/`d/qbC0`, CTA
//!   `K0spYL` ("Start a Stream") / `5+172e` ("Stream") or disabled state (`u4QmWl` "Request expired",
//!   `P0wwmM` "Already streaming", `qRXats` "Not in voice channel", `43zohO` "Not running game").
//! - Rich Presence / Game Invite: active (`pkq6Vq` "Game Invitation", `gLu7NU`/`65JnWC`, `VJlc0S`
//!   "Join", `Ckxb6j` "Ask to Join") and expired compact/full cards (`Sq/E1I` "This invite has
//!   expired", `x1UXGR` / `Ek+51n`, `s+J8Dl` "Launch Game", `jaYS/h` "Try Game").

use super::{Root, color, editor_style, icon};
use crate::editor::Editor;
use crate::net::Request;
use dc_intl::Arg;
use dc_state::{Id, Message};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;
use std::borrow::Cow;
use std::collections::HashSet;

/// State for the `196492` "Invite to Play" modal (`ActivityInviteManager` + `333189` searchable
/// invite picker: `2tN7ih`, `MAxtks`, `BddRzS`, `5h0QOP`, `HBJiRR`, `Jf0OOQ`, `6F9ivu`, `i6A1Xw`).
#[derive(Clone)]
pub(crate) struct ActivityInviteModalState {
    pub app_id: String,
    pub app_name: String,
    pub party_id: String,
    pub is_private: bool,
    pub query: Entity<Editor>,
    pub sent_ids: HashSet<Id>,
}

/// Builds `POST /channels/{channel_id}/messages` with `{ activity: { type: 1, party_id }, application_id }`
/// (`308368.A`, module 196492).
pub(crate) fn send_activity_invite_request(
    channel_id: Id,
    party_id: &str,
    app_id: &str,
) -> Request {
    Request::Rest {
        key: Some(format!("activity-invite:{channel_id}")),
        method: reqwest::Method::POST,
        route: format!("/channels/{channel_id}/messages"),
        body: Some(json!({
            "content": "",
            "activity": {
                "type": ACTIVITY_JOIN,
                "party_id": party_id,
            },
            "application_id": app_id,
        })),
    }
}

/// Activity invite message kinds from Discord's `MessageActivityTypes` (`584283`).
pub(crate) const ACTIVITY_JOIN: u64 = 1;
pub(crate) const ACTIVITY_SPECTATE: u64 = 2;
pub(crate) const ACTIVITY_LISTEN: u64 = 3;
pub(crate) const ACTIVITY_STREAM_REQUEST: u64 = 5;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum StreamRequestDisabled {
    Expired,
    AlreadyStreaming,
    NotInVoice,
    NotRunningGame,
}

impl StreamRequestDisabled {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Expired => dc_intl::t("u4QmWl"),
            Self::AlreadyStreaming => dc_intl::t("P0wwmM"),
            Self::NotInVoice => dc_intl::t("qRXats"),
            Self::NotRunningGame => dc_intl::t("43zohO"),
        }
    }
}

/// Module 116653 (`932157.U1` + `381438.Y.ADULT`): `discord_classifications` bits that mark an
/// application/game as restricted to adults (bits 0, 3, 4).
pub(crate) const RESTRICTED_TO_ADULT_MASK: u64 = (1 << 0) | (1 << 3) | (1 << 4);

/// Module 116653 (`c` variant prop): whether the age-restricted embed is a game invite or app embed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AgeRestrictedEmbedVariant {
    GameInvite,
    AppEmbed,
}

/// Module 116653 visibility decision (`"display"` | `"block_underage"` | `"block_channel_restriction"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgeRestrictedEmbedDecision {
    Display,
    BlockUnderage,
    BlockChannelRestriction,
}

/// Module 116653 (`c`): evaluates whether an embed with optional `discord_classifications` bitfield
/// may be displayed or must be replaced by a muted age-restriction notice.
pub(crate) fn evaluate_age_restricted_embed_gate(
    discord_classifications: Option<u64>,
    nsfw_allowed: bool,
    is_private_channel: bool,
    channel_nsfw: bool,
) -> AgeRestrictedEmbedDecision {
    let is_adult =
        discord_classifications.is_some_and(|bits| (bits & RESTRICTED_TO_ADULT_MASK) != 0);
    if !is_adult {
        return AgeRestrictedEmbedDecision::Display;
    }
    if !nsfw_allowed {
        return AgeRestrictedEmbedDecision::BlockUnderage;
    }
    if is_private_channel || channel_nsfw {
        AgeRestrictedEmbedDecision::Display
    } else {
        AgeRestrictedEmbedDecision::BlockChannelRestriction
    }
}

/// Module 116653 (`pkq6Vq`/`bZBN64` + `GhU4yl`/`B99UMJ`/`LPOzxB`/`NIZyKq`): localized title & reason.
pub(crate) fn age_restricted_embed_copy(
    variant: AgeRestrictedEmbedVariant,
    decision: AgeRestrictedEmbedDecision,
) -> Option<(&'static str, &'static str)> {
    match (variant, decision) {
        (_, AgeRestrictedEmbedDecision::Display) => None,
        (AgeRestrictedEmbedVariant::GameInvite, AgeRestrictedEmbedDecision::BlockUnderage) => {
            Some((dc_intl::t("pkq6Vq"), dc_intl::t("GhU4yl")))
        }
        (
            AgeRestrictedEmbedVariant::GameInvite,
            AgeRestrictedEmbedDecision::BlockChannelRestriction,
        ) => Some((dc_intl::t("pkq6Vq"), dc_intl::t("B99UMJ"))),
        (AgeRestrictedEmbedVariant::AppEmbed, AgeRestrictedEmbedDecision::BlockUnderage) => {
            Some((dc_intl::t("bZBN64"), dc_intl::t("LPOzxB")))
        }
        (
            AgeRestrictedEmbedVariant::AppEmbed,
            AgeRestrictedEmbedDecision::BlockChannelRestriction,
        ) => Some((dc_intl::t("bZBN64"), dc_intl::t("NIZyKq"))),
    }
}

/// Module 116653 (`.embed_BmHBi0`): 350px muted placeholder card replacing blocked adult embeds.
pub(crate) fn render_age_restricted_embed_placeholder(
    variant: AgeRestrictedEmbedVariant,
    decision: AgeRestrictedEmbedDecision,
) -> Option<AnyElement> {
    let (title, reason) = age_restricted_embed_copy(variant, decision)?;
    Some(
        div()
            .w_full()
            .max_w(px(350.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--background-surface-highest"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child(reason),
            )
            .into_any_element(),
    )
}

#[derive(Clone, Debug)]
pub(crate) enum ActivityInviteVariant {
    Spotify {
        track: String,
        artist: String,
        party_size: u32,
        max_party_size: Option<u32>,
        expired: bool,
    },
    StreamRequest {
        app_name: String,
        requester: String,
        is_author: bool,
        use_start_copy: bool,
        disabled: Option<StreamRequestDisabled>,
    },
    GameInvite {
        app_name: String,
        details: String,
        party_size: u32,
        max_party_size: Option<u32>,
        expired: bool,
        can_launch: bool,
        can_cloud_try: bool,
        spectate: bool,
    },
    /// Module 116653: age-restricted game invite or application embed placeholder (`.embed_BmHBi0`).
    AgeRestricted {
        variant: AgeRestrictedEmbedVariant,
        decision: AgeRestrictedEmbedDecision,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ActivityInviteCard {
    pub id: String,
    pub variant: ActivityInviteVariant,
}

fn party_status_text(listening: bool, size: u32, max: Option<u32>) -> String {
    match (listening, max) {
        (true, Some(max)) if max > 0 => dc_intl::format(
            "Zogoou",
            &[
                ("partySize", Arg::Num(f64::from(size))),
                ("maxPartySize", Arg::Num(f64::from(max))),
            ],
        ),
        (true, _) => dc_intl::format("UGei0j", &[("partySize", Arg::Num(f64::from(size)))]),
        (false, Some(max)) if max > 0 => dc_intl::format(
            "gLu7NU",
            &[
                ("partySize", Arg::Num(f64::from(size))),
                ("maxPartySize", Arg::Num(f64::from(max))),
            ],
        ),
        (false, _) => dc_intl::format("65JnWC", &[("partySize", Arg::Num(f64::from(size)))]),
    }
}

impl Root {
    /// Renders an activity invite / listening party / request-to-stream embed inside chat
    /// message accessories (`app/src/ui.rs` `Root::accessories`, module 584283).
    pub(super) fn activity_invite_embed(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let activity_kind = message.extra.activity?;
        let app_name = message
            .extra
            .application
            .as_ref()
            .map(|(_, name)| name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Activity".to_owned());
        let me = self.stores.me().map(|u| u.id);
        let is_author = me == Some(message.author.id);
        let requester = message
            .author
            .global_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&message.author.username)
            .to_owned();

        let variant = match activity_kind {
            ACTIVITY_LISTEN => ActivityInviteVariant::Spotify {
                track: app_name.clone(),
                artist: requester,
                party_size: 1,
                max_party_size: None,
                expired: false,
            },
            ACTIVITY_STREAM_REQUEST => {
                let in_voice = self
                    .call
                    .as_ref()
                    .is_some_and(|c| c.channel == message.channel_id);
                let streaming = self.own_stream().is_some();
                let disabled = if streaming {
                    Some(StreamRequestDisabled::AlreadyStreaming)
                } else if !in_voice {
                    Some(StreamRequestDisabled::NotInVoice)
                } else {
                    None
                };
                ActivityInviteVariant::StreamRequest {
                    app_name,
                    requester,
                    is_author,
                    use_start_copy: false,
                    disabled,
                }
            }
            ACTIVITY_SPECTATE | ACTIVITY_JOIN => {
                let is_private = self.guild.is_none()
                    || self
                        .stores
                        .private_channels
                        .iter()
                        .any(|c| c.id == message.channel_id);
                let channel_nsfw = self
                    .guild
                    .and_then(|gid| self.stores.guild(gid))
                    .and_then(|g| g.channels.iter().find(|c| c.id == message.channel_id))
                    .is_some_and(|c| c.nsfw);
                let classification_bits =
                    message.extra.application.as_ref().and_then(|(app_id, _)| {
                        self.fetched
                            .get(&format!("game-profile:{app_id}"))
                            .and_then(|v| {
                                v["content_classification"]["discord_classifications"].as_u64()
                            })
                    });
                let decision = evaluate_age_restricted_embed_gate(
                    classification_bits,
                    self.stores.account.nsfw_allowed,
                    is_private,
                    channel_nsfw,
                );
                if decision != AgeRestrictedEmbedDecision::Display {
                    ActivityInviteVariant::AgeRestricted {
                        variant: AgeRestrictedEmbedVariant::GameInvite,
                        decision,
                    }
                } else {
                    ActivityInviteVariant::GameInvite {
                        app_name,
                        details: String::new(),
                        party_size: 1,
                        max_party_size: Some(4),
                        expired: false,
                        can_launch: true,
                        can_cloud_try: false,
                        spectate: activity_kind == ACTIVITY_SPECTATE,
                    }
                }
            }
            _ => return None,
        };

        let card = ActivityInviteCard {
            id: format!("msg-act-{}", message.id),
            variant,
        };
        Some(self.render_activity_invite_card(&card, Some(message.channel_id), cx))
    }

    pub(super) fn render_activity_invite_card(
        &self,
        card: &ActivityInviteCard,
        channel_id: Option<Id>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &card.variant {
            ActivityInviteVariant::Spotify {
                track,
                artist,
                party_size,
                max_party_size,
                expired,
            } => {
                let header =
                    dc_intl::format("/8czH4", &[("name", Arg::Str(Cow::Borrowed("Spotify")))]);
                let bg = linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x1db95433), 0.),
                    linear_color_stop(rgba(0x191414ee), 1.),
                );
                let subtitle = if *expired {
                    dc_intl::t("84qx9r").to_owned()
                } else {
                    dc_intl::format(
                        "JCvHtx",
                        &[
                            ("track", Arg::Str(Cow::Borrowed(track))),
                            ("artist", Arg::Str(Cow::Borrowed(artist))),
                        ],
                    )
                };
                let party = party_status_text(true, *party_size, *max_party_size);
                let card_id = card.id.clone();
                div()
                    .id(format!("act-spotify-{card_id}"))
                    .w(px(400.))
                    .max_w_full()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(bg)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(44.))
                                    .rounded(px(8.))
                                    .bg(rgba(0x1db95444))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("active_now_spotify", 24., "--white")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-muted"))
                                            .child(header.to_uppercase()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(19.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(subtitle),
                                    ),
                            ),
                    )
                    .when(!expired, |el| {
                        el.child(
                            div()
                                .px(px(14.))
                                .py(px(10.))
                                .bg(rgba(0x00000033))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(12.))
                                        .text_color(color("--text-default"))
                                        .child(icon("activity_music", 14., "--status-positive"))
                                        .child(party),
                                )
                                .child(
                                    div()
                                        .id(format!("act-spotify-join-{card_id}"))
                                        .cursor_pointer()
                                        .px(px(14.))
                                        .h(px(30.))
                                        .rounded(px(6.))
                                        .bg(color("--status-positive"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("VJlc0S"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.put_app_toast(
                                                dc_intl::t("VJlc0S").to_owned(),
                                                true,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                    })
                    .into_any_element()
            }
            ActivityInviteVariant::StreamRequest {
                app_name,
                requester,
                is_author,
                use_start_copy,
                disabled,
            } => {
                let card_id = card.id.clone();
                let subtitle = if *is_author {
                    dc_intl::t("8B3U5O").to_owned()
                } else {
                    dc_intl::format(
                        "d/qbC0",
                        &[("username", Arg::Str(Cow::Borrowed(requester)))],
                    )
                };
                let cta_text = if let Some(reason) = disabled {
                    reason.label()
                } else if *use_start_copy {
                    dc_intl::t("K0spYL")
                } else {
                    dc_intl::t("5+172e")
                };
                let can_stream = disabled.is_none();
                div()
                    .id(format!("act-stream-req-{card_id}"))
                    .w(px(400.))
                    .max_w_full()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x5865f233), 0.),
                        linear_color_stop(rgba(0x1e1f22ee), 1.),
                    ))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(44.))
                                    .rounded(px(8.))
                                    .bg(rgba(0x5865f244))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("clips_screen", 22., "--white")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("nAyuPp").to_uppercase()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(app_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-default"))
                                            .child(subtitle),
                                    ),
                            ),
                    )
                    .when(!is_author, |el| {
                        el.child(
                            div()
                                .px(px(14.))
                                .py(px(10.))
                                .bg(rgba(0x00000033))
                                .flex()
                                .justify_end()
                                .child(
                                    div()
                                        .id(format!("act-stream-cta-{card_id}"))
                                        .when(can_stream, |b| b.cursor_pointer())
                                        .when(!can_stream, |b| b.opacity(0.55))
                                        .px(px(14.))
                                        .h(px(32.))
                                        .rounded(px(6.))
                                        .bg(if can_stream {
                                            color("--control-primary-background-default")
                                        } else {
                                            color("--control-secondary-background-default")
                                        })
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(icon("clips_screen", 14., "--white"))
                                        .child(cta_text)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if can_stream {
                                                let _ = channel_id;
                                                this.open_go_live(cx);
                                            }
                                        })),
                                ),
                        )
                    })
                    .into_any_element()
            }
            ActivityInviteVariant::GameInvite {
                app_name,
                details,
                party_size,
                max_party_size,
                expired,
                can_launch,
                can_cloud_try,
                spectate,
            } => {
                let card_id = card.id.clone();
                if *expired && !*can_launch && !*can_cloud_try {
                    // Compact expired invite card (`.compactEmbed_LlTAVD`, max-width 350px).
                    return div()
                        .id(format!("act-game-compact-{card_id}"))
                        .w(px(350.))
                        .max_w_full()
                        .p(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(linear_gradient(
                            45.,
                            linear_color_stop(rgba(0x2b2d31ff), 0.),
                            linear_color_stop(rgba(0x1e1f22ff), 1.),
                        ))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_none()
                                .size(px(32.))
                                .rounded(px(6.))
                                .bg(rgba(0x5865f233))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("bot_game_controller", 18., "--icon-subtle")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("pkq6Vq")),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("Sq/E1I")),
                                ),
                        )
                        .into_any_element();
                }

                let header = if *spectate {
                    dc_intl::format("BBJXVk", &[("name", Arg::Str(Cow::Borrowed(app_name)))])
                } else {
                    dc_intl::t("pkq6Vq").to_owned()
                };
                let party = party_status_text(false, *party_size, *max_party_size);
                let sub_copy = if *expired {
                    if *can_launch {
                        dc_intl::t("x1UXGR").to_owned()
                    } else {
                        dc_intl::t("Ek+51n").to_owned()
                    }
                } else if !details.is_empty() {
                    details.clone()
                } else {
                    party.clone()
                };

                div()
                    .id(format!("act-game-{card_id}"))
                    .w(px(400.))
                    .max_w_full()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x5865f244), 0.),
                        linear_color_stop(rgba(0x232428f5), 1.),
                    ))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(48.))
                                    .rounded(px(10.))
                                    .bg(rgba(0x5865f255))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("bot_game_controller", 26., "--white")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-muted"))
                                            .child(header.to_uppercase()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(app_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-default"))
                                            .child(sub_copy),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .bg(rgba(0x00000033))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(if *expired {
                                        dc_intl::t("Sq/E1I").to_owned()
                                    } else {
                                        party
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .when(*expired && *can_cloud_try, |row| {
                                        row.child(
                                            div()
                                                .id(format!("act-game-try-{card_id}"))
                                                .cursor_pointer()
                                                .px(px(12.))
                                                .h(px(30.))
                                                .rounded(px(6.))
                                                .bg(color("--control-secondary-background-default"))
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(icon(
                                                    "plugin_cloud_download",
                                                    14.,
                                                    "--text-strong",
                                                ))
                                                .child(dc_intl::t("jaYS/h"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.put_app_toast(
                                                        dc_intl::t("jaYS/h").to_owned(),
                                                        true,
                                                        cx,
                                                    );
                                                })),
                                        )
                                    })
                                    .child({
                                        let label = if *expired {
                                            dc_intl::t("s+J8Dl")
                                        } else if *spectate {
                                            dc_intl::t("Ckxb6j")
                                        } else {
                                            dc_intl::t("VJlc0S")
                                        };
                                        div()
                                            .id(format!("act-game-cta-{card_id}"))
                                            .cursor_pointer()
                                            .px(px(14.))
                                            .h(px(30.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.put_app_toast(label.to_owned(), true, cx);
                                            }))
                                    }),
                            ),
                    )
                    .into_any_element()
            }
            ActivityInviteVariant::AgeRestricted { variant, decision } => {
                render_age_restricted_embed_placeholder(*variant, *decision)
                    .unwrap_or_else(|| div().into_any_element())
            }
        }
    }

    /// Preview showcase layer for Activity Invite / Listening Party / Request to Stream embeds
    /// (module 584283, opened via capture harness).
    pub(super) fn activity_invite_preview_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let cards = self.activity_invite_preview.as_ref()?;
        Some(
            deferred(
                div()
                    .id("activity-invite-preview-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activity_invite_preview = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("activity-invite-preview-sheet")
                            .p(px(20.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .children(
                                cards
                                    .iter()
                                    .map(|card| self.render_activity_invite_card(card, None, cx)),
                            ),
                    ),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `activity-invite [spotify|stream|game|expired|close]` (module 584283).
    #[cfg(feature = "capture")]
    pub(crate) fn script_activity_invite(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.activity_invite_preview = None;
                cx.notify();
                "activity-invite: closed".to_owned()
            }
            "age-restricted" => {
                let d1 = evaluate_age_restricted_embed_gate(
                    Some(RESTRICTED_TO_ADULT_MASK),
                    false,
                    false,
                    false,
                );
                let d2 = evaluate_age_restricted_embed_gate(
                    Some(RESTRICTED_TO_ADULT_MASK),
                    true,
                    false,
                    false,
                );
                self.activity_invite_preview = Some(vec![
                    ActivityInviteCard {
                        id: "age-game-underage".to_owned(),
                        variant: ActivityInviteVariant::AgeRestricted {
                            variant: AgeRestrictedEmbedVariant::GameInvite,
                            decision: d1,
                        },
                    },
                    ActivityInviteCard {
                        id: "age-app-channel".to_owned(),
                        variant: ActivityInviteVariant::AgeRestricted {
                            variant: AgeRestrictedEmbedVariant::AppEmbed,
                            decision: d2,
                        },
                    },
                ]);
                cx.notify();
                "activity-invite: age-restricted".to_owned()
            }
            "spotify" => {
                self.activity_invite_preview = Some(vec![
                    ActivityInviteCard {
                        id: "sp-live".to_owned(),
                        variant: ActivityInviteVariant::Spotify {
                            track: "Midnight City".to_owned(),
                            artist: "M83".to_owned(),
                            party_size: 3,
                            max_party_size: Some(6),
                            expired: false,
                        },
                    },
                    ActivityInviteCard {
                        id: "sp-exp".to_owned(),
                        variant: ActivityInviteVariant::Spotify {
                            track: "Midnight City".to_owned(),
                            artist: "M83".to_owned(),
                            party_size: 0,
                            max_party_size: None,
                            expired: true,
                        },
                    },
                ]);
                cx.notify();
                "activity-invite: spotify".to_owned()
            }
            "stream" => {
                self.activity_invite_preview = Some(vec![
                    ActivityInviteCard {
                        id: "sr-live".to_owned(),
                        variant: ActivityInviteVariant::StreamRequest {
                            app_name: "Elden Ring".to_owned(),
                            requester: "lorean xavier".to_owned(),
                            is_author: false,
                            use_start_copy: true,
                            disabled: None,
                        },
                    },
                    ActivityInviteCard {
                        id: "sr-disabled".to_owned(),
                        variant: ActivityInviteVariant::StreamRequest {
                            app_name: "Elden Ring".to_owned(),
                            requester: "lorean xavier".to_owned(),
                            is_author: false,
                            use_start_copy: false,
                            disabled: Some(StreamRequestDisabled::NotInVoice),
                        },
                    },
                ]);
                cx.notify();
                "activity-invite: stream".to_owned()
            }
            "expired" => {
                self.activity_invite_preview = Some(vec![
                    ActivityInviteCard {
                        id: "gm-exp-compact".to_owned(),
                        variant: ActivityInviteVariant::GameInvite {
                            app_name: "Valorant".to_owned(),
                            details: String::new(),
                            party_size: 0,
                            max_party_size: None,
                            expired: true,
                            can_launch: false,
                            can_cloud_try: false,
                            spectate: false,
                        },
                    },
                    ActivityInviteCard {
                        id: "gm-exp-launch".to_owned(),
                        variant: ActivityInviteVariant::GameInvite {
                            app_name: "Starfall Odyssey".to_owned(),
                            details: String::new(),
                            party_size: 0,
                            max_party_size: None,
                            expired: true,
                            can_launch: true,
                            can_cloud_try: true,
                            spectate: false,
                        },
                    },
                ]);
                cx.notify();
                "activity-invite: expired".to_owned()
            }
            "modal-private" => {
                self.open_activity_invite_modal(
                    "111111111111111111".to_owned(),
                    "Starfall Odyssey".to_owned(),
                    "party-demo-1".to_owned(),
                    true,
                    cx,
                );
                "activity-invite: modal-private".to_owned()
            }
            "modal-search" => {
                self.open_activity_invite_modal(
                    "111111111111111111".to_owned(),
                    "Starfall Odyssey".to_owned(),
                    "party-demo-1".to_owned(),
                    false,
                    cx,
                );
                "activity-invite: modal-search".to_owned()
            }
            _ => {
                self.activity_invite_preview = Some(vec![ActivityInviteCard {
                    id: "gm-live".to_owned(),
                    variant: ActivityInviteVariant::GameInvite {
                        app_name: "Starfall Odyssey".to_owned(),
                        details: "Competitive — Ascent".to_owned(),
                        party_size: 3,
                        max_party_size: Some(5),
                        expired: false,
                        can_launch: true,
                        can_cloud_try: false,
                        spectate: false,
                    },
                }]);
                cx.notify();
                "activity-invite: game".to_owned()
            }
        }
    }

    /// Opens the `196492` "Invite to Play" modal (Branch A `is_private` alert, Branch B `333189` picker).
    pub(super) fn open_activity_invite_modal(
        &mut self,
        app_id: String,
        app_name: String,
        party_id: String,
        is_private: bool,
        cx: &mut Context<Self>,
    ) {
        let query = cx.new(|cx| {
            let mut ed = Editor::new(editor_style(), cx);
            ed.set_placeholder(dc_intl::t("5h0QOP").to_owned());
            ed
        });
        self.activity_invite_modal = Some(ActivityInviteModalState {
            app_id,
            app_name,
            party_id,
            is_private,
            query,
            sent_ids: HashSet::new(),
        });
        cx.notify();
    }

    /// Renders the `196492` "Invite to Play" modal overlay (`ActivityInviteManager` + `333189`).
    pub(super) fn activity_invite_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.activity_invite_modal.as_ref()?;
        let title = dc_intl::format(
            "2tN7ih",
            &[("name", Arg::Str(Cow::Borrowed(&modal.app_name)))],
        );

        // Branch A (`isPrivate === true`): 400px modal with `MAxtks` and `BddRzS` ("Okay").
        if modal.is_private {
            let dialog = div()
                .id("activity-invite-private-modal")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(px(400.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(20.))
                                .line_height(px(26.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .id("activity-invite-private-close")
                                .size(px(28.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.activity_invite_modal = None;
                                    cx.notify();
                                }))
                                .child(icon("close", 16., "--interactive-text-default")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(20.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("MAxtks").replace('\n', " ")),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(14.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .rounded_b(px(12.))
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .id("activity-invite-private-ok")
                                .h(px(38.))
                                .px(px(20.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("BddRzS"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.activity_invite_modal = None;
                                    cx.notify();
                                })),
                        ),
                );
            return Some(
                deferred(
                    div()
                        .id("activity-invite-modal-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.activity_invite_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(dialog),
                )
                .with_priority(95)
                .into_any_element(),
            );
        }

        // Branch B (`!isPrivate`): 480px searchable invite modal (`333189`, `5h0QOP`, `HBJiRR`,
        // `Jf0OOQ`, `6F9ivu`, `i6A1Xw`).
        let filter = modal.query.read(cx).text().trim().to_ascii_lowercase();
        let mut playing_rows: Vec<(Id, String, String)> = Vec::new();
        let mut other_rows: Vec<(Id, String, String)> = Vec::new();

        let mut seen_dm_users: HashSet<Id> = HashSet::new();
        for dm in &self.stores.private_channels {
            let (name, sub, same_app) = if let Some(uid) = dm.recipients.first().copied() {
                let user = self.stores.users.get(&uid);
                if user.is_some_and(|u| u.system || u.bot) || !seen_dm_users.insert(uid) {
                    continue;
                }
                let disp = user
                    .map(|u| u.display_name().to_owned())
                    .unwrap_or_else(|| {
                        dm.name
                            .clone()
                            .unwrap_or_else(|| "Direct Message".to_owned())
                    });
                let uname = user.map(|u| u.username.clone()).unwrap_or_default();
                let playing_same = self.stores.presence.get(uid, None).is_some_and(|p| {
                    p.activities.iter().any(|a| {
                        a.get("name")
                            .and_then(|v| v.as_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(&modal.app_name))
                    })
                });
                (disp, uname, playing_same)
            } else {
                let n = dm.name.clone().unwrap_or_else(|| "Group DM".to_owned());
                (n, "Group DM".to_owned(), false)
            };
            if !filter.is_empty()
                && !name.to_ascii_lowercase().contains(&filter)
                && !sub.to_ascii_lowercase().contains(&filter)
            {
                continue;
            }
            if same_app {
                playing_rows.push((dm.id, name, sub));
            } else {
                other_rows.push((dm.id, name, sub));
            }
        }
        if !filter.is_empty() {
            for g in &self.stores.guilds {
                for ch in &g.channels {
                    if ch.kind == dc_state::ChannelKind::Text
                        && ch.name.to_ascii_lowercase().contains(&filter)
                    {
                        other_rows.push((ch.id, format!("#{}", ch.name), g.name.clone()));
                    }
                }
            }
        }

        let show_headers = !playing_rows.is_empty() && !other_rows.is_empty();
        let app_id = modal.app_id.clone();
        let party_id = modal.party_id.clone();
        let sent_ids = modal.sent_ids.clone();

        let make_row = |idx: usize, (cid, name, sub): (Id, String, String), prefix: &str| {
            let is_sent = sent_ids.contains(&cid);
            let app_c = app_id.clone();
            let party_c = party_id.clone();
            div()
                .id(format!("activity-invite-row-{prefix}-{idx}"))
                .h(px(48.))
                .px(px(8.))
                .rounded(px(8.))
                .hover(|s| s.bg(color("--background-mod-subtle")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_none()
                                .size(px(32.))
                                .rounded_full()
                                .bg(color("--background-mod-strong"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(
                                    name.chars()
                                        .next()
                                        .map(|c| c.to_ascii_uppercase().to_string())
                                        .unwrap_or_else(|| "?".to_owned()),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(name),
                                )
                                .when(!sub.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(sub),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .id(format!("activity-invite-btn-{prefix}-{idx}"))
                        .flex_none()
                        .w(px(72.))
                        .h(px(32.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(if is_sent {
                            color("--background-mod-subtle")
                        } else {
                            color("--control-secondary-background-default")
                        })
                        .when(!is_sent, |d| {
                            d.hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                        })
                        .when(is_sent, |d| d.opacity(0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(if is_sent {
                            color("--text-muted")
                        } else {
                            color("--text-strong")
                        })
                        .child(dc_intl::t(if is_sent { "i6A1Xw" } else { "6F9ivu" }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_sent {
                                return;
                            }
                            let _ = this
                                .requests
                                .send(send_activity_invite_request(cid, &party_c, &app_c));
                            if let Some(m) = this.activity_invite_modal.as_mut() {
                                m.sent_ids.insert(cid);
                            }
                            cx.notify();
                        })),
                )
        };

        let mut list_items: Vec<AnyElement> = Vec::new();
        if !playing_rows.is_empty() {
            if show_headers {
                let hdr = dc_intl::format(
                    "HBJiRR",
                    &[("name", Arg::Str(Cow::Borrowed(&modal.app_name)))],
                );
                list_items.push(
                    div()
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(hdr)
                        .into_any_element(),
                );
            }
            for (idx, row) in playing_rows.into_iter().enumerate() {
                list_items.push(make_row(idx, row, "play").into_any_element());
            }
        }
        if !other_rows.is_empty() {
            if show_headers {
                list_items.push(
                    div()
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("Jf0OOQ"))
                        .into_any_element(),
                );
            }
            for (idx, row) in other_rows.into_iter().enumerate() {
                list_items.push(make_row(idx, row, "other").into_any_element());
            }
        }

        let dialog = div()
            .id("activity-invite-search-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(480.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(20.))
                            .line_height(px(26.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .id("activity-invite-search-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity_invite_modal = None;
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--interactive-text-default")),
                    ),
            )
            .child(
                div().px(px(24.)).pb(px(12.)).child(
                    div()
                        .h(px(38.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("search", 16., "--text-muted"))
                        .child(div().flex_1().min_w_0().child(modal.query.clone())),
                ),
            )
            .child(
                div()
                    .id("activity-invite-results")
                    .px(px(16.))
                    .pb(px(12.))
                    .h(px(300.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(if list_items.is_empty() {
                        vec![
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(8.))
                                .child(icon("search", 32., "--text-muted"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .text_color(color("--text-muted"))
                                        .child("No results found"),
                                )
                                .into_any_element(),
                        ]
                    } else {
                        list_items
                    }),
            );

        Some(
            deferred(
                div()
                    .id("activity-invite-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.activity_invite_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(dialog),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_invite_modal_196492_request_and_keys() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = send_activity_invite_request(777, "party-9", "app-1")
        else {
            panic!("expected Rest");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/channels/777/messages");
        let b = body.unwrap();
        assert_eq!(b["activity"]["type"], ACTIVITY_JOIN);
        assert_eq!(b["activity"]["party_id"], "party-9");
        assert_eq!(b["application_id"], "app-1");

        for k in [
            "2tN7ih", "BddRzS", "MAxtks", "5h0QOP", "6F9ivu", "i6A1Xw", "HBJiRR", "Jf0OOQ",
        ] {
            assert!(dc_intl::has_key(k), "missing 196492 key {k}");
        }
    }

    #[test]
    fn age_restricted_embed_gate_116653_matrix_and_intl() {
        assert_eq!(
            evaluate_age_restricted_embed_gate(None, false, false, false),
            AgeRestrictedEmbedDecision::Display
        );
        assert_eq!(
            evaluate_age_restricted_embed_gate(Some(1 << 1), false, false, false),
            AgeRestrictedEmbedDecision::Display
        );
        assert_eq!(
            evaluate_age_restricted_embed_gate(Some(1 << 3), false, true, true),
            AgeRestrictedEmbedDecision::BlockUnderage
        );
        assert_eq!(
            evaluate_age_restricted_embed_gate(Some(1 << 4), true, false, false),
            AgeRestrictedEmbedDecision::BlockChannelRestriction
        );
        assert_eq!(
            evaluate_age_restricted_embed_gate(Some(1 << 0), true, true, false),
            AgeRestrictedEmbedDecision::Display
        );
        assert_eq!(
            evaluate_age_restricted_embed_gate(Some(1 << 0), true, false, true),
            AgeRestrictedEmbedDecision::Display
        );
        for k in ["pkq6Vq", "bZBN64", "GhU4yl", "B99UMJ", "LPOzxB", "NIZyKq"] {
            assert!(dc_intl::has_key(k), "missing 116653 key {k}");
        }
        assert!(
            age_restricted_embed_copy(
                AgeRestrictedEmbedVariant::GameInvite,
                AgeRestrictedEmbedDecision::BlockUnderage
            )
            .is_some()
        );
        assert!(
            age_restricted_embed_copy(
                AgeRestrictedEmbedVariant::AppEmbed,
                AgeRestrictedEmbedDecision::BlockChannelRestriction
            )
            .is_some()
        );
    }
}
