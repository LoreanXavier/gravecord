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

use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::{Id, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use std::borrow::Cow;

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
            ACTIVITY_SPECTATE => ActivityInviteVariant::GameInvite {
                app_name,
                details: String::new(),
                party_size: 1,
                max_party_size: Some(4),
                expired: false,
                can_launch: true,
                can_cloud_try: false,
                spectate: true,
            },
            ACTIVITY_JOIN => ActivityInviteVariant::GameInvite {
                app_name,
                details: String::new(),
                party_size: 1,
                max_party_size: Some(4),
                expired: false,
                can_launch: true,
                can_cloud_try: false,
                spectate: false,
            },
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
}
