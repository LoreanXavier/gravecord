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
    /// "Channel is full" (module 361698).
    ChannelFull,
    /// "Join voice?" (module 525828, `voice_blocked`).
    JoinBlocked {
        guild: Id,
        channel: Id,
        blocked: Vec<Id>,
        ignored: Vec<Id>,
    },
    /// "Leave voice?" (module 552258, `voice_blocked`).
    BlockedJoined {
        channel: Id,
        user: Id,
    },
    /// "You have been invited to speak." (module 427412).
    InvitedToSpeak {
        channel: Id,
    },
    /// "Ready to start a call?" (module 502054), with its "Don't ask again" box.
    CallConfirm {
        target: CallTarget,
        dont_ask: bool,
    },
    /// "Reload Activity" (module 47294) before the call pops out (or back in) while an Activity runs, with its
    /// "Don't show again" box.
    ReloadActivity {
        dont_show: bool,
    },
}

/// Whom the confirmed call of module 502054 goes to: a person's menu, or a DM row's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallTarget {
    User(Id),
    Dm(Id),
}

/// The picture of "You have been invited to speak." (module 973324), 288 by 192.
const INVITED_ART: super::graphic_modal::Graphic = super::graphic_modal::Graphic {
    url: "https://cdn.discordapp.com/assets/content/ef7fa86f8c519fe9764867c1ba473ee1939292fddb2c897885aab732e3bd31d2.svg",
    width: 288.,
    height: 192.,
};

/// `Pd` of module 147036 for a voice channel that is not a Stage: full when it has video and as many people as the
/// guild's video seats (one more for a `mover`, someone who may move members and connect: `c`), or as many as its
/// `user_limit` unless this account is such a mover (`E && !c`).
pub(super) fn voice_channel_full(
    present: u64,
    user_limit: u64,
    has_video: bool,
    video_seats: u64,
    mover: bool,
) -> bool {
    let video_full = has_video && video_seats > 0 && present >= video_seats + u64::from(mover);
    let limit_full = user_limit > 0 && present >= user_limit;
    video_full || (limit_full && !mover)
}

/// `disableCallUserConfirmationPrompt` of `UnsyncedUserSettingsStore` (module 964404), off until ticked.
pub(super) fn call_confirmation_disabled() -> bool {
    crate::storage::get("UnsyncedUserSettingsStore")
        .and_then(|store| store.get("disableCallUserConfirmationPrompt")?.as_bool())
        .unwrap_or(false)
}

/// `disableEmbeddedActivityPopOutAlert` of `UnsyncedUserSettingsStore` (module 964404), off until ticked.
pub(super) fn activity_popout_alert_disabled() -> bool {
    crate::storage::get("UnsyncedUserSettingsStore")
        .and_then(|store| store.get("disableEmbeddedActivityPopOutAlert")?.as_bool())
        .unwrap_or(false)
}

/// Module 47294's effect: the box's state is kept as it changes (`updatedUnsyncedSettings`), whichever button closes
/// the dialog.
fn set_activity_popout_alert_disabled(disabled: bool) {
    let mut store = crate::storage::get("UnsyncedUserSettingsStore")
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    store["disableEmbeddedActivityPopOutAlert"] = serde_json::json!(disabled);
    crate::storage::set("UnsyncedUserSettingsStore", store);
}

fn disable_call_confirmation() {
    let mut store = crate::storage::get("UnsyncedUserSettingsStore")
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    store["disableCallUserConfirmationPrompt"] = serde_json::json!(true);
    crate::storage::set("UnsyncedUserSettingsStore", store);
}

/// `eY` of module 105530 is `REQUESTED_TO_SPEAK_AND_AWAITING_USER_ACK`: no longer suppressed, but the hand is still
/// up (a moderator invited this person).
pub(super) fn invited_to_speak(suppress: bool, requested: bool) -> bool {
    !suppress && requested
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
/// hook, for the `body-md/semibold` inline name (`username_OObHCR`). Used by "Join voice?" and "Leave voice?"
/// (`voice_blocked`).
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
            .any(|state| state.self_video || (state.self_stream && !state.suppress));
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

    /// `Pd` of module 147036 for a channel that is not a Stage (`voice_channel_full`).
    pub(super) fn voice_channel_is_full(&self, guild: Id, channel: Id) -> bool {
        let Some(known) = self.stores.guild(guild) else {
            return false;
        };
        let Some(target) = known.channels.iter().find(|known| known.id == channel) else {
            return false;
        };
        if target.kind == STAGE {
            return false;
        }
        let Some(me) = self.stores.me() else {
            return false;
        };
        let can = |bit| permissions::can_in_channel(known, target, me, bit);
        let states = self.stores.voice_members(guild, channel);
        voice_channel_full(
            states.len() as u64,
            target.user_limit,
            states.iter().any(|state| state.self_video),
            known.max_video_channel_users,
            can(permissions::MOVE_MEMBERS) && can(permissions::CONNECT),
        )
    }

    /// Module 596339's watcher of this session's own voice state (`b4`, modal key `STAGE_INVITED_TO_SPEAK_MODAL`):
    /// invited to speak in a Stage, "You have been invited to speak." opens once; leaving voice closes it.
    pub(super) fn stage_invite_check(&mut self, data: &serde_json::Value, cx: &mut Context<Self>) {
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return;
        };
        if data["user_id"].as_str() != Some(me.to_string().as_str())
            || data["session_id"].as_str() != self.stores.session_id.as_deref()
        {
            return;
        }
        let open = matches!(self.voice_alert, Some(VoiceAlert::InvitedToSpeak { .. }));
        let Some(channel) = data["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            if open {
                self.voice_alert = None;
                cx.notify();
            }
            return;
        };
        let Some(guild) = self.guild_of_channel(channel) else {
            return;
        };
        if !self.is_stage_voice_channel(guild, channel) || open {
            return;
        }
        let suppress = data["suppress"].as_bool().unwrap_or(false);
        let requested = !data["request_to_speak_timestamp"].is_null();
        if invited_to_speak(suppress, requested) {
            self.request_graphic(INVITED_ART);
            self.voice_alert = Some(VoiceAlert::InvitedToSpeak { channel });
            cx.notify();
        }
    }

    /// `e7` of module 849736 (`p`): the answer to an invitation to speak, `PATCH /guilds/<id>/voice-states/@me` with
    /// `suppress` (true declines) and the hand put down.
    fn answer_speak_invite(&mut self, channel: Id, decline: bool) {
        self.voice_alert = None;
        let Some(guild) = self.guild_of_channel(channel) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/voice-states/@me"),
            body: Some(serde_json::json!({
                "suppress": decline,
                "request_to_speak_timestamp": null,
                "channel_id": channel.to_string(),
            })),
        });
    }

    /// The "Start a Call" rows (`runVoiceCallAction` of module 949674): "Ready to start a call?" first, unless
    /// `disableCallUserConfirmationPrompt` of the unsynced settings is on.
    pub(super) fn start_call_or_ask(&mut self, target: CallTarget, cx: &mut Context<Self>) {
        if call_confirmation_disabled() {
            self.start_confirmed_call(target, cx);
        } else {
            self.voice_alert = Some(VoiceAlert::CallConfirm {
                target,
                dont_ask: false,
            });
            cx.notify();
        }
    }

    fn start_confirmed_call(&mut self, target: CallTarget, cx: &mut Context<Self>) {
        match target {
            CallTarget::User(user) => self.start_dm_call_with(user, false, cx),
            CallTarget::Dm(channel) => {
                self.select_home(Some(channel));
                let active = self.stores.calls.is_call_active(channel, None);
                self.start_call(channel, false, !active, cx);
            }
        }
    }

    /// The dialog's `onClose`: a ticked "Don't ask again" is kept (`updatedUnsyncedSettings`).
    fn close_call_confirm(&mut self) {
        if let Some(VoiceAlert::CallConfirm { dont_ask: true, .. }) = self.voice_alert {
            disable_call_confirmation();
        }
        self.voice_alert = None;
    }

    /// The alert's own close: what Escape, the close button and the scrim do.
    pub(super) fn dismiss_voice_alert(&mut self, cx: &mut Context<Self>) {
        match self.voice_alert.clone() {
            Some(VoiceAlert::JoinBlocked {
                guild,
                channel,
                blocked,
                ignored,
            }) => {
                let people = blocked.into_iter().chain(ignored).collect();
                self.join_past_blocked(guild, channel, people, cx);
            }
            Some(VoiceAlert::BlockedJoined { user, .. }) => self.stay_past_blocked(user),
            Some(VoiceAlert::InvitedToSpeak { channel }) => self.answer_speak_invite(channel, true),
            Some(VoiceAlert::CallConfirm { .. }) => self.close_call_confirm(),
            _ => self.voice_alert = None,
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
        match &alert {
            VoiceAlert::JoinBlocked {
                guild,
                channel,
                blocked,
                ignored,
            } => return Some(self.join_blocked_modal(*guild, *channel, blocked, ignored, cx)),
            VoiceAlert::BlockedJoined { channel, user } => {
                return Some(self.blocked_joined_modal(*channel, *user, cx));
            }
            VoiceAlert::InvitedToSpeak { channel } => {
                return Some(self.invited_to_speak_modal(*channel, cx));
            }
            VoiceAlert::CallConfirm { target, dont_ask } => {
                return Some(self.call_confirm_modal(*target, *dont_ask, cx));
            }
            VoiceAlert::ReloadActivity { dont_show, .. } => {
                return Some(self.reload_activity_modal(*dont_show, cx));
            }
            _ => {}
        }
        let close = || -> modal::Handler { Box::new(|this, _, _| this.voice_alert = None) };
        let spec = match alert {
            VoiceAlert::JoinBlocked { .. }
            | VoiceAlert::BlockedJoined { .. }
            | VoiceAlert::InvitedToSpeak { .. }
            | VoiceAlert::CallConfirm { .. }
            | VoiceAlert::ReloadActivity { .. } => return None,
            VoiceAlert::ChannelFull => modal::Spec {
                id: "channel-full-modal",
                title: dc_intl::t("elyVbv").into(),
                subtitle: Some(div().child(dc_intl::t("nIeRep")).into_any_element()),
                body: None,
                buttons: vec![modal::button(
                    "channel-full-okay",
                    dc_intl::t("BddRzS"),
                    modal::Tone::Primary,
                    |this, _, _| this.voice_alert = None,
                )],
                close: close(),
            },
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

    /// Module 427412 on the dialog with a header graphic: "Join the speakers in" the Stage's icon (14px,
    /// `channelIcon_mNVMq4`: 2px after it) and name; Not Now (secondary) and Accept (`active`). Its close declines.
    fn invited_to_speak_modal(&self, channel: Id, cx: &mut Context<Self>) -> AnyElement {
        let name = self
            .guild_of_channel(channel)
            .and_then(|guild| self.stores.guild(guild))
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
            .map(|known| known.name.clone())
            .unwrap_or_default();
        let lead = dc_intl::format_runs("Z+3bWy", &[("stageChannelHook", String::new().into())])
            .into_iter()
            .filter(|(_, hook)| hook.is_none())
            .map(|(text, _)| text)
            .collect::<String>();
        let subtitle = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_center()
            .child(lead)
            .child(
                div()
                    .mr(px(2.))
                    .child(super::icon("stage_channel", 14., "--text-subtle")),
            )
            .child(name);
        self.graphic_modal(
            super::graphic_modal::Spec {
                id: "stage-invited-to-speak-modal",
                graphic: Some(INVITED_ART),
                title: dc_intl::t("Ul1RJQ").into(),
                subtitle: Some(subtitle.into_any_element()),
                body: None,
                buttons: vec![
                    modal::button(
                        "stage-invited-not-now",
                        dc_intl::t("L5eIZ2"),
                        modal::Tone::Secondary,
                        |this, _, _| this.voice_alert = None,
                    ),
                    modal::button(
                        "stage-invited-accept",
                        dc_intl::t("MMlhsr"),
                        modal::Tone::Active,
                        move |this, _, _| this.answer_speak_invite(channel, false),
                    ),
                ],
                close: Box::new(move |this, _, _| this.answer_speak_invite(channel, true)),
            },
            cx,
        )
    }

    /// Module 502054 on the alert of module 732159 (`variant: "active"`): Cancel and Start Call, "Don't ask again" in
    /// the action bar (the checkbox of module 568385, `labelType: "secondary"`: `body-md/normal`, `text-subtle`, 12px
    /// after the box).
    /// Module 47294 on the dialog of module 189213 (`size: "md"`): "Reload Activity", its subtitle, Cancel and Reload,
    /// and "Don't show again" in the action bar.
    fn reload_activity_modal(&self, dont_show: bool, cx: &mut Context<Self>) -> AnyElement {
        let checkbox = div()
            .id("reload-activity-dont-show")
            .flex()
            .items_start()
            .gap(px(12.))
            .cursor_pointer()
            .child(super::server_onboarding::checkbox(dont_show, false))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t("wGAwOS")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(VoiceAlert::ReloadActivity { dont_show, .. }) =
                    this.voice_alert.as_mut()
                {
                    *dont_show = !*dont_show;
                    set_activity_popout_alert_disabled(*dont_show);
                }
                cx.notify();
            }))
            .into_any_element();
        let spec = modal::Spec {
            id: "reload-activity-modal",
            title: dc_intl::t("ye21jI").into(),
            subtitle: Some(div().child(dc_intl::t("hbagWD")).into_any_element()),
            body: None,
            buttons: vec![
                modal::button(
                    "reload-activity-cancel",
                    dc_intl::t("B8pz37"),
                    modal::Tone::Secondary,
                    |this, _, _| this.voice_alert = None,
                ),
                modal::button(
                    "reload-activity-reload",
                    dc_intl::t("makWn9"),
                    modal::Tone::Primary,
                    |this, window, cx| {
                        this.voice_alert = None;
                        this.toggle_call_popout_now(window, cx);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.voice_alert = None),
        };
        modal::render_with_input(spec, checkbox, cx)
    }

    fn call_confirm_modal(
        &self,
        target: CallTarget,
        dont_ask: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let checkbox = div()
            .id("call-confirm-dont-ask")
            .flex()
            .items_start()
            .gap(px(12.))
            .cursor_pointer()
            .child(super::server_onboarding::checkbox(dont_ask, false))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t("JdIQ/Y")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(VoiceAlert::CallConfirm { dont_ask, .. }) = this.voice_alert.as_mut() {
                    *dont_ask = !*dont_ask;
                }
                cx.notify();
            }))
            .into_any_element();
        let spec = modal::Spec {
            id: "call-confirm-modal",
            title: dc_intl::t("HlAPoq").into(),
            subtitle: Some(div().child(dc_intl::t("cRW4D/")).into_any_element()),
            body: None,
            buttons: vec![
                modal::button(
                    "call-confirm-cancel",
                    dc_intl::t("ETE/oC"),
                    modal::Tone::Secondary,
                    |this, _, _| this.close_call_confirm(),
                ),
                modal::button(
                    "call-confirm-start",
                    dc_intl::t("rimG2R"),
                    modal::Tone::Active,
                    move |this, _, cx| {
                        this.close_call_confirm();
                        this.start_confirmed_call(target, cx);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.close_call_confirm()),
        };
        modal::render_small_with_input(spec, checkbox, cx)
    }

    /// `k` of module 918192 with `showStatus` (`user_jBt7uB`: 8px padding; the 32px avatar; `userInfo` 12px after it):
    /// the name and `#discriminator` (`body-md/normal`, the tag `text-subtle`), then Blocked (`text-feedback-critical`)
    /// or Ignored and `| Speaker`, `| Stage Moderator` or `| Audience` (`body-xs/normal`, `text-muted`).
    pub(super) fn stage_person_row(
        &self,
        guild: Id,
        channel: Id,
        user: Id,
        is_blocked: bool,
    ) -> AnyElement {
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

impl Root {
    /// Capture only: `voice-alert reload-activity`, what Pop Out asks while an Activity runs.
    #[cfg(feature = "capture")]
    pub(crate) fn script_voice_alert(&mut self, name: &str) -> String {
        match name {
            "reload-activity" => {
                self.voice_alert = Some(VoiceAlert::ReloadActivity { dont_show: false });
                "voice alert: reload activity".to_owned()
            }
            _ => "usage: voice-alert reload-activity".to_owned(),
        }
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
    fn a_voice_channel_is_full_by_its_limit_or_its_video_seats() {
        // 5 of a limit of 5: full, unless this account may move members.
        assert!(voice_channel_full(5, 5, false, 25, false));
        assert!(!voice_channel_full(5, 5, false, 25, true));
        assert!(!voice_channel_full(4, 5, false, 25, false));
        // No limit: only video seats count, and only while someone shows video.
        assert!(!voice_channel_full(25, 0, false, 25, false));
        assert!(voice_channel_full(25, 0, true, 25, false));
        // A member mover gets one seat more.
        assert!(!voice_channel_full(25, 0, true, 25, true));
        assert!(voice_channel_full(26, 0, true, 25, true));
    }

    #[test]
    fn an_invitation_to_speak_is_an_unsuppressed_raised_hand() {
        assert!(invited_to_speak(false, true));
        assert!(!invited_to_speak(true, true));
        assert!(!invited_to_speak(false, false));
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
