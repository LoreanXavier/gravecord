//! The small alerts of voice, Stages and streams, all on the design system's alert (module 189213, the 400 wide
//! dialog `modal::render_small` draws):
//!
//! - "Stream is Full" (module 166789), shown once per stream when `STREAM_DELETE` ends a stream we tried to watch with
//!   `reason: stream_full` (`handleStreamDelete` of module 505293), with the error code of `stream-full` (2010, the
//!   table of module 487329).
//! - "Sorry, this Stage is full." (module 291453), instead of joining a Stage that shows video and has as many people
//!   as the guild's `max_stage_video_channel_users` (`Pd` of module 147036 and `xn` of module 96566, checked by the
//!   voice channel select of module 597798).
//! - "Blocked and Ignored Accounts Present" (module 200629), before joining a Stage where people the account blocked or
//!   ignored are (`j6`/`W0` of module 993838, called by `N` of module 129095); "Got it, thanks" joins.
//! - "Do you want to also end the Stage?" (module 118101), when the last Stage moderator who speaks leaves a live
//!   Stage (module 106044, `j3` of module 993838 from the voice tray's disconnect).

use super::{Root, avatar_source, color, modal};
use crate::net::Request;
use dc_state::{ChannelKind, Id, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

/// `GUILD_STAGE_VOICE`.
const STAGE: ChannelKind = ChannelKind::Other(13);
/// `errorCode` of `stream-full` in module 487329.
const STREAM_FULL_CODE: i64 = 2010;
/// `QY` of module 233993: what makes a Stage moderator.
const STAGE_MODERATOR: u64 = permissions::MANAGE_CHANNELS | (1 << 22) | (1 << 24);
/// `COMMUNITY` guilds may hold up to this many in a video Stage (`S<1e4` of module 291453).
const COMMUNITY_STAGE_MAX: u64 = 10_000;
/// Other guilds below Boost level 3 may grow past this many (`S<=150`).
const PLAIN_STAGE_MAX: u64 = 150;

/// One open alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum VoiceAlert {
    StreamFull,
    StageFull {
        guild: Id,
        channel: Id,
    },
    StageBlocked {
        guild: Id,
        channel: Id,
        blocked: Vec<Id>,
        ignored: Vec<Id>,
    },
    StageEndOnLeave {
        channel: Id,
    },
}

/// What module 291453 says and offers for a full video Stage: the subtitle's key and whether Boost is offered
/// (`O`): a Community guild below 10,000 seats, or another guild below Boost level 3 at no more than 150.
pub(super) fn stage_full_text(
    community: bool,
    premium_tier: u64,
    max_seats: u64,
) -> (&'static str, bool) {
    let boost = if community {
        max_seats < COMMUNITY_STAGE_MAX
    } else {
        premium_tier != 3 && max_seats <= PLAIN_STAGE_MAX
    };
    let subtitle = if community && premium_tier == 3 {
        if max_seats <= COMMUNITY_STAGE_MAX {
            "tJmOuw"
        } else {
            "7FHbPG"
        }
    } else if boost {
        "8/uDSF"
    } else {
        "7FHbPG"
    };
    (subtitle, boost)
}

/// The title and subtitle of module 200629 for these counts, formatted.
pub(super) fn blocked_texts(blocked: usize, ignored: usize) -> (String, String) {
    let number = |count: usize| [("number", (count as i64).into())];
    if blocked > 0 && ignored > 0 {
        (
            dc_intl::format("Uzdyho", &number(blocked + ignored)),
            dc_intl::t("P/KFXz").to_owned(),
        )
    } else if ignored > 0 {
        (
            dc_intl::format("wvygk8", &number(ignored)),
            dc_intl::format("Ri3o33", &number(ignored)),
        )
    } else {
        (
            dc_intl::format("HviVA9", &number(blocked)),
            dc_intl::format("28qZMU", &number(blocked)),
        )
    }
}

/// Module 106044: leaving this live Stage leaves it without a speaking moderator. `me` must be a moderator who speaks,
/// and either no other moderator is in the Stage or none of them speaks.
pub(super) fn last_speaking_moderator(
    me_moderator: bool,
    me_speaker: bool,
    others: &[(bool, bool)],
) -> bool {
    me_moderator
        && me_speaker
        && (!others.iter().any(|(moderator, _)| *moderator)
            || !others
                .iter()
                .any(|(moderator, speaker)| *moderator && *speaker))
}

/// `T` of module 90386, the line the blocked-people warnings of a call put beside their avatars: "{a} is here",
/// "{a} and {b} are here", "... and 1 other person is here", "... and {n} other people are here". `names` are the
/// first two people's names (`getName`) and `count` how many there are; each name is its own run, tagged with its
/// hook, for the `body-md/semibold` inline name (`username_OObHCR`).
// Its callers, the blocked-people warnings before joining or leaving a call and leaving a group, are still to port.
#[allow(dead_code)]
pub(super) fn here_runs(names: &[String], count: usize) -> Vec<(String, Option<Box<str>>)> {
    let first = names.first().cloned().unwrap_or_default();
    let second = names.get(1).cloned().unwrap_or_default();
    match count {
        0 | 1 => dc_intl::format_runs("4WHCtq", &[("usernameHook", first.into())]),
        2 => dc_intl::format_runs(
            "veV4IN",
            &[
                ("usernameHook1", first.into()),
                ("usernameHook2", second.into()),
            ],
        ),
        3 => dc_intl::format_runs(
            "67ZE+9",
            &[
                ("usernameHook1", first.into()),
                ("usernameHook2", second.into()),
            ],
        ),
        _ => dc_intl::format_runs(
            "qfo6KR",
            &[
                ("usernameHook1", first.into()),
                ("usernameHook2", second.into()),
                (
                    "numberOfOtherUsers",
                    ((count - names.len().min(2)) as i64).into(),
                ),
            ],
        ),
    }
}

impl Root {
    /// `isModerator` (module 312006): the person holds Manage Channels, Mute Members and Move Members in the Stage.
    pub(super) fn is_stage_moderator(&self, guild: Id, channel: Id, user: Id) -> bool {
        let Some(known) = self.stores.guild(guild) else {
            return false;
        };
        let Some(stage) = known.channels.iter().find(|known| known.id == channel) else {
            return false;
        };
        let roles = self
            .stores
            .members
            .get(guild, user)
            .map(|member| member.roles.clone())
            .unwrap_or_default();
        permissions::member_channel_permissions(known, stage, user, &roles) & STAGE_MODERATOR
            == STAGE_MODERATOR
    }

    /// The alert that stands in for joining this channel, if one does: the blocked or ignored people of a Stage first
    /// (module 129095 asks before selecting), then a full video Stage (module 597798's select).
    pub(super) fn stage_join_alert(&self, guild: Id, channel: Id) -> Option<VoiceAlert> {
        let known = self.stores.guild(guild)?;
        let stage = known.channels.iter().find(|known| known.id == channel)?;
        if stage.kind != STAGE {
            return None;
        }
        let me = self.stores.current_user.as_ref()?.id;
        let states = self.stores.voice_members(guild, channel);
        if !self.is_stage_moderator(guild, channel, me) {
            let others = states.iter().filter(|state| state.user_id != me);
            let blocked: Vec<Id> = others
                .clone()
                .filter(|state| self.is_blocked(state.user_id))
                .map(|state| state.user_id)
                .collect();
            let ignored: Vec<Id> = others
                .filter(|state| {
                    !self.is_blocked(state.user_id) && self.stores.is_ignored(state.user_id)
                })
                .map(|state| state.user_id)
                .collect();
            if !blocked.is_empty() || !ignored.is_empty() {
                return Some(VoiceAlert::StageBlocked {
                    guild,
                    channel,
                    blocked,
                    ignored,
                });
            }
        }
        self.stage_is_full(guild, channel)
            .then_some(VoiceAlert::StageFull { guild, channel })
    }

    /// `Pd` of module 147036 for a Stage with video or a speaker's stream (`xn` of module 96566): as many people as
    /// the guild's video Stage seats.
    fn stage_is_full(&self, guild: Id, channel: Id) -> bool {
        let Some(known) = self.stores.guild(guild) else {
            return false;
        };
        let states = self.stores.voice_members(guild, channel);
        let video = states
            .iter()
            .any(|state| state.self_video || state.self_stream);
        let seats = known.max_stage_video_channel_users;
        video && seats > 0 && states.len() as u64 >= seats
    }

    /// Module 106044 for the Stage being left: live, and this account its last speaking moderator.
    pub(super) fn leaving_ends_stage(&self, guild: Id, channel: Id) -> bool {
        if self
            .stores
            .stage_instances
            .get_by_channel(channel)
            .is_none()
        {
            return false;
        }
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return false;
        };
        let states = self.stores.voice_members(guild, channel);
        let speaker = |user: Id| {
            states
                .iter()
                .any(|state| state.user_id == user && !state.suppress)
        };
        let others: Vec<(bool, bool)> = states
            .iter()
            .filter(|state| state.user_id != me)
            .map(|state| {
                (
                    self.is_stage_moderator(guild, channel, state.user_id),
                    !state.suppress,
                )
            })
            .collect();
        last_speaking_moderator(
            self.is_stage_moderator(guild, channel, me),
            speaker(me),
            &others,
        )
    }

    /// The Stage view's leave button: the question of module 118101 when leaving would leave the Stage without a
    /// speaking moderator, otherwise the plain disconnect.
    pub(super) fn leave_stage_or_ask(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if self.leaving_ends_stage(guild, channel) {
            self.voice_alert = Some(VoiceAlert::StageEndOnLeave { channel });
            cx.notify();
        } else {
            self.leave_voice(cx);
        }
    }

    /// The voice panel's and the call bar's Disconnect (`Ta` of module 596339, the tray of module 717965): a Stage
    /// asks first when leaving would leave it without a speaking moderator.
    pub(super) fn disconnect_from_call(&mut self, cx: &mut Context<Self>) {
        let stage = self.call.as_ref().and_then(|call| {
            let channel = self
                .stores
                .guild(call.guild)?
                .channels
                .iter()
                .find(|known| known.id == call.channel)?;
            (channel.kind == STAGE).then_some((call.guild, call.channel))
        });
        match stage {
            Some((guild, channel)) => self.leave_stage_or_ask(guild, channel, cx),
            None => self.leave_voice(cx),
        }
    }

    /// `handleStreamDelete` of module 505293 with `reason: stream_full`: the alert, once per stream.
    pub(super) fn stream_was_full(&mut self, key: String, cx: &mut Context<Self>) {
        if self.stream_full_seen.insert(key) {
            self.voice_alert = Some(VoiceAlert::StreamFull);
            cx.notify();
        }
    }

    /// Opens the alert, asking for the pictures it shows.
    pub(super) fn open_voice_alert(&mut self, alert: VoiceAlert, cx: &mut Context<Self>) {
        if let VoiceAlert::StageBlocked {
            blocked, ignored, ..
        } = &alert
        {
            for user in blocked.iter().chain(ignored) {
                if let Some(known) = self.stores.users.get(user).cloned() {
                    let (key, url) = avatar_source(&known);
                    self.request_image(key, url);
                }
            }
        }
        self.voice_alert = Some(alert);
        cx.notify();
    }

    pub(super) fn voice_alert_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let alert = self.voice_alert.clone()?;
        let close = || -> modal::Handler { Box::new(|this, _, _| this.voice_alert = None) };
        let spec = match alert {
            VoiceAlert::StreamFull => modal::Spec {
                id: "stream-full-modal",
                title: dc_intl::t("GzjdO5").into(),
                subtitle: Some(div().child(dc_intl::t("VVZDBL")).into_any_element()),
                // `body-md/medium`, `text-muted`, selectable.
                body: Some(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::format(
                            "ejOT95",
                            &[("errorCode", STREAM_FULL_CODE.into())],
                        ))
                        .into_any_element(),
                ),
                buttons: vec![modal::button(
                    "stream-full-okay",
                    dc_intl::t("BddRzS"),
                    modal::Tone::Primary,
                    |this, _, _| this.voice_alert = None,
                )],
                close: close(),
            },
            VoiceAlert::StageFull { guild, .. } => {
                let known = self.stores.guild(guild);
                let community = known.is_some_and(|known| {
                    known.features.iter().any(|feature| feature == "COMMUNITY")
                });
                let tier = known.map_or(0, |known| known.premium_tier);
                let seats = known.map_or(0, |known| known.max_stage_video_channel_users);
                let (subtitle, boost) = stage_full_text(community, tier, seats);
                let buttons = if boost {
                    vec![
                        modal::button(
                            "stage-full-no-thanks",
                            dc_intl::t("f3Pet9"),
                            modal::Tone::Secondary,
                            |this, _, _| this.voice_alert = None,
                        ),
                        // `expressive` in the module; the boost dialog of `K4` (module 233824).
                        modal::button(
                            "stage-full-boost",
                            dc_intl::t("Uj0md3"),
                            modal::Tone::Primary,
                            move |this, _, cx| {
                                this.voice_alert = None;
                                this.boost_this_server(guild, cx);
                            },
                        ),
                    ]
                } else {
                    vec![modal::button(
                        "stage-full-got-it",
                        dc_intl::t("NX+WJN"),
                        modal::Tone::Primary,
                        |this, _, _| this.voice_alert = None,
                    )]
                };
                modal::Spec {
                    id: "stage-full-modal",
                    title: dc_intl::t("pqPQL0").into(),
                    subtitle: Some(div().child(dc_intl::t(subtitle)).into_any_element()),
                    body: None,
                    buttons,
                    close: close(),
                }
            }
            VoiceAlert::StageBlocked {
                guild,
                channel,
                blocked,
                ignored,
            } => {
                let (title, subtitle) = blocked_texts(blocked.len(), ignored.len());
                let rows: Vec<AnyElement> = blocked
                    .iter()
                    .map(|user| (*user, true))
                    .chain(ignored.iter().map(|user| (*user, false)))
                    .map(|(user, is_blocked)| {
                        self.stage_person_row(guild, channel, user, is_blocked)
                    })
                    .collect();
                modal::Spec {
                    id: "stage-blocked-modal",
                    title: title.into(),
                    subtitle: Some(div().child(subtitle).into_any_element()),
                    body: Some(div().flex().flex_col().children(rows).into_any_element()),
                    buttons: vec![
                        modal::button(
                            "stage-blocked-never-mind",
                            dc_intl::t("CZGqeT"),
                            modal::Tone::Secondary,
                            |this, _, _| this.voice_alert = None,
                        ),
                        modal::button(
                            "stage-blocked-got-it",
                            dc_intl::t("mbD50D"),
                            modal::Tone::Primary,
                            move |this, _, cx| {
                                this.voice_alert = None;
                                // `onAccept` selects the channel, whose select still checks for a full Stage.
                                if this.stage_is_full(guild, channel) {
                                    this.open_voice_alert(
                                        VoiceAlert::StageFull { guild, channel },
                                        cx,
                                    );
                                } else {
                                    this.join_voice_now(guild, channel, cx);
                                }
                            },
                        ),
                    ],
                    close: close(),
                }
            }
            VoiceAlert::StageEndOnLeave { channel } => modal::Spec {
                id: "stage-end-on-leave-modal",
                title: dc_intl::t("pADdJu").into(),
                subtitle: Some(div().child(dc_intl::t("mT7jwN")).into_any_element()),
                body: None,
                buttons: vec![
                    modal::button(
                        "stage-leave-quietly",
                        dc_intl::t("xTwqz2"),
                        modal::Tone::Secondary,
                        |this, _, cx| {
                            this.voice_alert = None;
                            this.leave_voice(cx);
                        },
                    ),
                    modal::button(
                        "stage-leave-and-end",
                        dc_intl::t("sBP/1s"),
                        modal::Tone::Critical,
                        move |this, _, cx| {
                            this.voice_alert = None;
                            // `OE` of module 849736: `DELETE /stage-instances/<channel>`, then the disconnect.
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/stage-instances/{channel}"),
                                body: None,
                            });
                            this.leave_voice(cx);
                        },
                    ),
                ],
                close: close(),
            },
        };
        Some(modal::render_small(spec, cx))
    }

    /// `k` of module 918192 with `showStatus` (`user_jBt7uB`: 8px padding; the 32px avatar; `userInfo` 12px after it):
    /// the name and `#discriminator` (`body-md/normal`, the tag `text-subtle`), then Blocked (`text-feedback-critical`)
    /// or Ignored and `| Speaker`, `| Stage Moderator` or `| Audience` (`body-xs/normal`, `text-muted`).
    fn stage_person_row(&self, guild: Id, channel: Id, user: Id, is_blocked: bool) -> AnyElement {
        let known = self.stores.users.get(&user);
        let picture = known.and_then(|known| self.images.get(&avatar_source(known).0).cloned());
        let name = known
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        let discriminator = known
            .map(|known| known.discriminator.clone())
            .unwrap_or_default();
        let speaker = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .any(|state| state.user_id == user && !state.suppress);
        let role = if speaker {
            dc_intl::t("LqMmG2")
        } else if self.is_stage_moderator(guild, channel, user) {
            dc_intl::t("GMZqSi")
        } else {
            dc_intl::t("suRApw")
        };
        div()
            .p(px(8.))
            .flex()
            .items_center()
            .justify_start()
            .child(
                div()
                    .flex_none()
                    .size(px(32.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(color("--background-mod-strong"))
                    .children(picture.map(|picture| img(picture).size(px(32.)).rounded_full())),
            )
            .child(
                div()
                    .ml(px(12.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .child(div().text_color(color("--text-default")).child(name))
                            .child(
                                div()
                                    .text_color(color("--text-subtle"))
                                    .child(format!("#{discriminator}")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child(if is_blocked {
                                div()
                                    .text_color(color("--text-feedback-critical"))
                                    .child(dc_intl::t("4bDptI"))
                            } else {
                                div()
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("tFY5Zb"))
                            })
                            .child(
                                div()
                                    .text_color(color("--text-muted"))
                                    .child(format!("| {role}")),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_here_line_names_two_people_and_counts_the_rest() {
        let names = vec!["Ann".to_owned(), "Bo".to_owned()];
        let text = |count: usize| -> String {
            here_runs(&names[..count.min(2)], count)
                .into_iter()
                .map(|(text, _)| text)
                .collect()
        };
        assert_eq!(text(1), "Ann is here");
        assert_eq!(text(2), "Ann and Bo are here");
        assert_eq!(text(3), "Ann and Bo and 1 other person is here");
        assert_eq!(text(5), "Ann and Bo and 3 other people are here");
        let runs = here_runs(&names, 2);
        assert_eq!(runs[0], ("Ann".to_owned(), Some("usernameHook1".into())));
    }

    #[test]
    fn a_full_stage_offers_boost_only_when_more_seats_can_be_bought() {
        assert_eq!(stage_full_text(true, 3, 10_000), ("tJmOuw", false));
        assert_eq!(stage_full_text(true, 3, 10_001), ("7FHbPG", false));
        assert_eq!(stage_full_text(true, 1, 300), ("8/uDSF", true));
        assert_eq!(stage_full_text(false, 1, 150), ("8/uDSF", true));
        assert_eq!(stage_full_text(false, 1, 300), ("7FHbPG", false));
        assert_eq!(stage_full_text(false, 3, 50), ("7FHbPG", false));
    }

    #[test]
    fn the_blocked_warning_names_blocked_ignored_or_both() {
        assert_eq!(
            blocked_texts(1, 1).0,
            "Blocked and Ignored Accounts Present"
        );
        assert_eq!(blocked_texts(1, 1).1, dc_intl::t("P/KFXz"));
        assert_eq!(
            blocked_texts(2, 0).0,
            dc_intl::format("HviVA9", &[("number", 2i64.into())])
        );
        assert_eq!(
            blocked_texts(0, 3).0,
            dc_intl::format("wvygk8", &[("number", 3i64.into())])
        );
    }

    #[test]
    fn only_the_last_speaking_moderator_is_asked_to_end_the_stage() {
        // Another moderator who speaks stays: no question.
        assert!(!last_speaking_moderator(true, true, &[(true, true)]));
        // The only other moderator is in the audience.
        assert!(last_speaking_moderator(
            true,
            true,
            &[(true, false), (false, true)]
        ));
        // No other moderator at all.
        assert!(last_speaking_moderator(true, true, &[(false, true)]));
        // A moderator in the audience, or someone who is no moderator, just leaves.
        assert!(!last_speaking_moderator(true, false, &[]));
        assert!(!last_speaking_moderator(false, true, &[]));
    }
}
