//! VoiceUserPopout (module 772800), opened from a voice user row of the channel list (module 340851, with the
//! preview state of module 588224): 100ms after the pointer enters the row (`setTimeout` of 340851) and 50ms more
//! (`J_(50)` of 588224) the popout opens 17px to the right of the row, unless the row is the account on another
//! client or the row's profile popout is open; it closes 175ms after the pointer has left the row and the popout.
//!
//! With the guild in `2026-04_voice_user_duration` (module 402313, `T`) it starts with `F`: the avatar, the name and
//! how long the person has been in the channel (`connectedAt`); then, or without `T` alone, the person's cards: the
//! stream (`S.P`), the Activities running in the call (`R`), what they play or listen to (`K`, each through `L`) and
//! Ask to Stream (`E.B`). With none of them and no `T`, nothing opens. Styles: `*_utcclU` (module 8936, stylesheet
//! 847810) over `popover_81IzEA` (module 489387).

use super::live_activity::{asset_url, millis, party_text};
use super::member_avatars::HIDDEN_STATUS;
use super::tooltip::{Body, Side, Tip};
use super::voice_badge::HoverCard;
use super::voice_invite::{ButtonLook, ds_button};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, ObjectFit, SharedString, div, hsla,
    img, point, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Module 402313: treatment 1 shows how long people have been in voice; the default is off.
pub(super) const DURATION_EXPERIMENT: &str = "2026-04_voice_user_duration";
/// Module 208284: treatment 1 offers Ask to Stream for anything, not only a detected game; the default is off.
pub(super) const REQUEST_TO_STREAM_EXPERIMENT: &str = "2026-10-request-to-stream-non-games";
/// 100ms of the row's `setTimeout`, then 50ms of `J_(50)` in module 588224.
pub(super) const OPEN_AFTER: u64 = 150;
/// `spacing: 17` of the row's popout.
pub(super) const SPACING: f32 = 17.;
/// `ActivityFlags.EMBEDDED` (module 652215 `jUm`).
const EMBEDDED: u64 = 1 << 8;
/// `MessageActivityTypes.STREAM_REQUEST` (module 652215 `xL`).
const STREAM_REQUEST: u64 = 6;
/// Module 820672 `e`: how long Ask to Stream rests after a request (`STREAM_REQUEST_COOLDOWN_END`).
const REQUEST_COOLDOWN: Duration = Duration::from_secs(30);
/// Module 487887 `$W`: an application id that never counts as a game.
const NOT_A_GAME: &str = "438122941302046720";

thread_local! {
    /// `isRequestOnCooldown` of module 628279: who was asked to stream, and when.
    static COOLDOWNS: RefCell<HashMap<Id, Instant>> = RefCell::new(HashMap::new());
}

fn on_cooldown(user: Id) -> bool {
    COOLDOWNS.with(|cooldowns| {
        cooldowns
            .borrow()
            .get(&user)
            .is_some_and(|at| at.elapsed() < REQUEST_COOLDOWN)
    })
}

/// Module 845056 `N`: a game or something watched that says anything about itself, or anything listened to.
pub(super) fn voice_activity_shown(activity: &Value) -> bool {
    match activity["type"].as_u64() {
        Some(0 | 3) => [
            "name",
            "application_id",
            "assets",
            "state",
            "details",
            "party",
        ]
        .iter()
        .any(|key| !activity[*key].is_null()),
        Some(2) => true,
        _ => false,
    }
}

/// Module 765379: an activity flagged `EMBEDDED` (an Activity running in a call) is left to the embedded app cards.
fn embedded(activity: &Value) -> bool {
    activity["flags"].as_u64().unwrap_or(0) & EMBEDDED == EMBEDDED
}

/// Module 845056 `m`: the shown activities less the stream's own (`excludeActivity ?? streamActivity`, matched by
/// application or by name), one per application (or name).
pub(super) fn listed_activities<'a>(
    activities: &[&'a Value],
    exclude: Option<&Value>,
) -> Vec<&'a Value> {
    let same = |activity: &Value, key: &str| {
        exclude.is_some_and(|exclude| !exclude[key].is_null() && activity[key] == exclude[key])
    };
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for activity in activities {
        if !voice_activity_shown(activity)
            || embedded(activity)
            || same(activity, "application_id")
            || same(activity, "name")
        {
            continue;
        }
        let Some(key) = activity["application_id"]
            .as_str()
            .or_else(|| activity["name"].as_str())
        else {
            continue;
        };
        if !seen.iter().any(|known| known == key) {
            seen.push(key.to_owned());
            out.push(*activity);
        }
    }
    out
}

/// Module 25528 `F9` (with 778383): a game this client can stream, with an application, not on a console.
fn streamable_game(activity: &Value) -> bool {
    activity["type"].as_u64() == Some(0)
        && !embedded(activity)
        && activity["application_id"]
            .as_str()
            .is_some_and(|app| app != NOT_A_GAME)
        && !matches!(activity["platform"].as_str(), Some("xbox" | "ps4" | "ps5"))
}

/// Module 804630 `_Q`: the heading over an activity.
fn heading_of(kind: u64) -> &'static str {
    match kind {
        0 | 5 => dc_intl::t("J4MQFX"),
        3 => dc_intl::t("xeqvTg"),
        2 => dc_intl::t("ep6av7"),
        _ => dc_intl::t("1+boPi"),
    }
}

/// The row's popout state that the card needs: the person, the channel and its guild.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct VoiceUserTarget {
    pub guild: Id,
    pub channel: Id,
    pub user: Id,
}

/// An Activity running in the channel that the person is in (module 685399 `Ay` and `Rz`).
struct EmbeddedApp {
    application: String,
    name: String,
    /// The person's presence activity of that application (`Rz`: the first participant's).
    presence: Option<Value>,
}

/// What `S.P` needs of a stream: its channel and owner.
#[derive(Clone, Copy)]
struct StreamOf {
    guild: Id,
    channel: Id,
    owner: Id,
}

impl Root {
    /// The person whose popout is open, for the live cards' assets (`live_users`).
    pub(super) fn voice_popout_user(&self) -> Option<Id> {
        match self.voice_hover_open()? {
            HoverCard::User(target) => Some(target.user),
            HoverCard::Channel { .. } => None,
        }
    }

    /// The pointer entered (`inside`) or left a voice user row (`ee` and `eI` of module 340851).
    pub(super) fn voice_row_hover(
        &mut self,
        target: VoiceUserTarget,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        let key = SharedString::from(format!("voice-member-{}", target.user));
        if !inside {
            self.voice_hover_leave(&key, cx);
            return;
        }
        // `isSelfOnOtherClient`: the account's own row for another session has no popout.
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other_client = me == Some(target.user)
            && self
                .stores
                .voice_members(target.guild, target.channel)
                .iter()
                .find(|state| state.user_id == target.user)
                .is_some_and(|state| {
                    self.stores.session_id.as_deref() != Some(state.session_id.as_str())
                });
        // `shouldShowHoverPopout && !em`: not while the row's own profile popout is open.
        let profile_open = self
            .profile
            .as_ref()
            .is_some_and(|open| open.user == target.user);
        if other_client || profile_open {
            return;
        }
        // The cards' applications (`useApplication` of 685399 and 729829) are asked for as the popout opens.
        let apps: Vec<String> = self
            .voice_embedded_apps(target.channel, target.user, false)
            .into_iter()
            .map(|app| app.application)
            .collect();
        for app in apps {
            self.request_voice_application(&app);
        }
        self.voice_hover_enter(key, HoverCard::User(target), OPEN_AFTER, cx);
    }

    /// `GET /applications/public` for an application's record, once (the profile cards' `presence-app:` entry).
    fn request_voice_application(&mut self, app: &str) {
        let key = format!("presence-app:{app}");
        if !self.fetched.contains_key(&key) {
            self.fetched.insert(key.clone(), Value::Null);
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/applications/public?application_ids={app}"),
            });
        }
    }

    /// An application's fetched record, if its lookup answered.
    fn voice_application(&self, app: &str) -> Option<&Value> {
        self.fetched
            .get(&format!("presence-app:{app}"))?
            .as_array()?
            .iter()
            .find(|record| record["id"].as_str() == Some(app))
    }

    /// The person's activities as the presence store has them for this guild (`getActivities(id, guild)`): all of
    /// them, not the profile's one-game list.
    fn voice_activities_of(&self, user: Id, guild: Id) -> Vec<&Value> {
        self.stores
            .presence
            .get(user, Some(guild))
            .map(|presence| presence.activities.iter().collect())
            .unwrap_or_default()
    }

    /// `getStreamForUser(user, guild)`: the person's Go Live in the guild, from their voice state.
    fn voice_stream_of(&self, guild: Id, user: Id) -> Option<StreamOf> {
        let state = self
            .stores
            .voice_states
            .get(&guild)?
            .iter()
            .find(|state| state.user_id == user && state.self_stream)?;
        Some(StreamOf {
            guild,
            channel: state.channel_id,
            owner: user,
        })
    }

    /// Module 685399 `Ay` filtered by the person (`R` of 772800): the Activities running in the channel that they take
    /// part in, each once its application is known (`p` drops one whose application has not loaded).
    fn voice_embedded_apps(&self, channel: Id, user: Id, known_only: bool) -> Vec<EmbeddedApp> {
        let user_text = user.to_string();
        let mut out: Vec<EmbeddedApp> = Vec::new();
        for instance in self.activities.in_channel(channel) {
            if !instance
                .participants
                .iter()
                .any(|participant| participant.user_id == user_text)
            {
                continue;
            }
            let app = instance.application_id.clone();
            if out.iter().any(|known| known.application == app) {
                continue;
            }
            let name = self
                .voice_application(&app)
                .and_then(|record| record["name"].as_str().map(str::to_owned))
                .or_else(|| self.activities.names.get(&app).cloned());
            if known_only && name.is_none() {
                continue;
            }
            // `Rz`: the first participant's presence activity of that application.
            let presence = instance
                .participants
                .first()
                .and_then(|first| first.user_id.parse::<Id>().ok())
                .and_then(|first| self.stores.presence.get(first, self.guild))
                .and_then(|presence| {
                    presence
                        .activities
                        .iter()
                        .find(|activity| activity["application_id"].as_str() == Some(app.as_str()))
                        .cloned()
                });
            out.push(EmbeddedApp {
                application: app,
                name: name.unwrap_or_default(),
                presence,
            });
        }
        out
    }

    /// Module 729829 `u`: whether this account may ask the person to stream in `channel`, with the game it would name
    /// (`G`, their first streamable game). Not for oneself, not while they stream, not on a Stage; the account must be
    /// able to send messages there and the person to stream there; without the non-game experiment they must play a
    /// game whose application has loaded.
    fn request_to_stream(&self, guild: Id, channel: &Channel, user: Id) -> Option<Option<Value>> {
        let non_games = self.guild_experiment_on(REQUEST_TO_STREAM_EXPERIMENT, guild);
        let me = self.stores.current_user.as_ref()?.id;
        let game = self
            .voice_activities_of(user, guild)
            .into_iter()
            .find(|activity| streamable_game(activity))
            .cloned();
        let application = game
            .as_ref()
            .and_then(|game| game["application_id"].as_str())
            .and_then(|app| self.voice_application(app));
        let known = self.stores.guild(guild)?;
        let stage = channel.kind == ChannelKind::Other(13);
        let roles = self
            .stores
            .members
            .get(guild, user)
            .map(|member| member.roles.clone())
            .unwrap_or_default();
        let may = !stage
            && self.my_permission(guild, Some(channel), permissions::SEND_MESSAGES)
            && permissions::member_channel_permissions(known, channel, user, &roles)
                & permissions::STREAM
                == permissions::STREAM;
        (me != user
            && (game.is_some() || non_games)
            && (application.is_some() || non_games)
            && may
            && self.voice_stream_of(guild, user).is_none())
        .then_some(game)
    }

    /// `handleRequestToStream`: a message in the channel's chat naming the person, carrying the request (module
    /// 308368 `sendActivityInvite` through `sendMessage`'s `activityAction`), then the 30 second rest.
    fn send_stream_request(
        &mut self,
        channel: Id,
        user: Id,
        game: Option<Value>,
        cx: &mut Context<Self>,
    ) {
        let session = self.stores.session_id.clone().unwrap_or_default();
        let mut activity = json!({
            "type": STREAM_REQUEST,
            "session_id": session,
            "target_user_id": user.to_string(),
        });
        if let Some(party) = game.as_ref().and_then(|game| game["party"]["id"].as_str()) {
            activity["party_id"] = party.into();
        }
        let mut body = json!({
            "content": format!("<@{user}>"),
            "nonce": crate::net::nonce(),
            "tts": false,
            "flags": 0,
            "activity": activity,
        });
        if let Some(app) = game
            .as_ref()
            .and_then(|game| game["application_id"].as_str())
        {
            body["application_id"] = app.into();
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/messages"),
            body: Some(body),
        });
        COOLDOWNS.with(|cooldowns| cooldowns.borrow_mut().insert(user, Instant::now()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REQUEST_COOLDOWN).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    /// Module 772800 `A`, or `None` when it renders nothing (`!J`).
    ///
    /// GAP: the gifting breadcrumb `L` adds under a card of a store application (module 128790, `withGiftingBreadcrumb`
    /// when the application is in the detectable list) is not drawn.
    pub(super) fn voice_user_card(
        &self,
        target: VoiceUserTarget,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let VoiceUserTarget {
            guild,
            channel,
            user,
        } = target;
        let duration = self.guild_experiment_on(DURATION_EXPERIMENT, guild);
        let known_channel = self
            .stores
            .guild(guild)?
            .channels
            .iter()
            .find(|known| known.id == channel)?
            .clone();
        // `O` and `P`: the person's stream and the game it shows (`nr`: their first non-embedded game).
        let stream = self.voice_stream_of(guild, user);
        let activities = self.voice_activities_of(user, guild);
        let stream_activity = stream.and_then(|_| {
            activities
                .iter()
                .find(|activity| activity["type"].as_u64() == Some(0) && !embedded(activity))
                .map(|activity| (*activity).clone())
        });
        // `w`: an age-gated channel shows no stream. `W`: the row passes `onWatchStream` only for a streaming row.
        let gated = self.age_gated_channel(guild, &known_channel);
        let stream_card = stream.filter(|_| !gated);
        let apps = self.voice_embedded_apps(channel, user, true);
        let listed: Vec<Value> = listed_activities(&activities, stream_activity.as_ref())
            .into_iter()
            .cloned()
            .collect();
        // `q`, `H` and `$`: Ask to Stream shows for someone who plays nothing streamable, with the experiment.
        let request = self.request_to_stream(guild, &known_channel, user);
        let games = activities
            .iter()
            .filter(|activity| streamable_game(activity))
            .count();
        let game_card = self.guild_experiment_on(REQUEST_TO_STREAM_EXPERIMENT, guild)
            && request.is_some()
            && games == 0;
        // `Z`: something to show opens the popout; `J`: or the duration header.
        let has_cards =
            stream_card.is_some() || !listed.is_empty() || !apps.is_empty() || request.is_some();
        if !has_cards && !duration {
            return None;
        }
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some(stream) = stream_card {
            cards.push(self.voice_stream_card(
                stream,
                stream_activity.as_ref(),
                &known_channel,
                cx,
            ));
        }
        for app in &apps {
            cards.push(self.voice_activity_card(
                guild,
                user,
                &known_channel,
                app.presence.as_ref(),
                Some(app),
                None,
                true,
                cx,
            ));
        }
        for activity in &listed {
            cards.push(self.voice_activity_card(
                guild,
                user,
                &known_channel,
                Some(activity),
                None,
                None,
                true,
                cx,
            ));
        }
        if game_card && let Some(game) = request.clone() {
            cards.push(self.ask_to_stream_button(&known_channel, user, game, true, cx));
        }
        // `scroller_utcclU` (inside `experimentContainer_utcclU`) or `container_utcclU`: the cards 12px apart, each
        // after the first under a 1px rule with 12px above it.
        let header_layout = duration || game_card;
        let rule = if header_layout {
            "--border-subtle"
        } else {
            "--border-muted"
        };
        let list = |cards: Vec<AnyElement>| {
            cards.into_iter().enumerate().map(move |(index, card)| {
                div()
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(color(rule)).pt(px(12.))
                    })
                    .child(card)
                    .into_any_element()
            })
        };
        let popover = || {
            // `popover_81IzEA`: `--background-surface-high`, `--radius-md`, a 1px `--border-subtle` ring inside and
            // `--shadow-high`.
            div()
                .bg(color("--background-surface-high"))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
        };
        if !header_layout {
            // `container_utcclU`: 280px, 16px inside, at most 90% of the window tall.
            return Some(
                popover()
                    .parity("voice-user-popout")
                    .w(px(280.))
                    .p(px(16.))
                    .gap(px(12.))
                    .children(list(cards))
                    .into_any_element(),
            );
        }
        let header = self.voice_user_duration(guild, channel, user, cx)?;
        let any = !cards.is_empty();
        Some(
            popover()
                .parity("voice-user-popout")
                .w(px(280.))
                .child(header)
                .when(has_cards && any, |this| {
                    // `scroller_utcclU`: a popover of its own with square top corners, 16px inside, at most 404px, scrolling.
                    this.child(
                        popover()
                            .id("voice-user-scroller")
                            .overflow_y_scroll()
                            .rounded_t(px(0.))
                            .max_h(px(404.))
                            .p(px(16.))
                            .gap(px(12.))
                            .children(list(cards)),
                    )
                })
                .into_any_element(),
        )
    }

    /// Module 727353 `P` with `showHeader` (`streamPreviewWrapper_1v6-1l`): "Streaming Now" over the LIVE mark, the
    /// 142px preview (`previewContainerUserActivity_1v6-1l`) that watches the stream on a click (`onWatchStream`, `eS`
    /// of module 340851) with the action over it on hover (`previewHover_1v6-1l`), then the stream's game card without
    /// its heading, or the stream's actions alone (module 961272 for `STREAM_PREVIEW`).
    fn voice_stream_card(
        &self,
        stream: StreamOf,
        activity: Option<&Value>,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (action, can_watch, _, _) = self.voice_watch_state(stream, channel);
        let key = super::stream_previews::key_of(stream.guild, stream.channel, stream.owner);
        let preview = self
            .stream_previews
            .url(&key.to_string())
            .and_then(|url| self.images.get(url))
            .cloned();
        let root = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = root.update(cx, |root, _| root.stream_preview_want(key));
        });
        let connect = self.my_permission(stream.guild, Some(channel), permissions::CONNECT);
        let group = SharedString::from(format!("voice-stream-preview-{}", stream.owner));
        // Module 607407 with `noImage`: the picture fills the box (`object-fit: contain`); before one has arrived, the
        // line under the empty picture (`emptyPreviewText_hELJz2`) alone.
        let picture: AnyElement = match preview {
            Some(picture) => img(picture)
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(if connect {
                            dc_intl::t("uQZTBV")
                        } else {
                            dc_intl::t("pgUTZC")
                        }),
                )
                .into_any_element(),
        };
        let owner = stream.owner;
        let (guild, channel_id) = (stream.guild, stream.channel);
        let tile = div()
            .id(group.clone())
            .group(group.clone())
            .parity("voice-user-stream-preview")
            .relative()
            .w_full()
            .h(px(142.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-secondary-alt"))
            .cursor_pointer()
            .child(picture)
            .when(can_watch, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(color("--opacity-black-60"))
                        .opacity(0.)
                        .group_hover(group.clone(), |style| style.opacity(1.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(action),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.voice_watch_stream(guild, channel_id, owner, cx);
            }));
        let body: AnyElement = match activity {
            Some(activity) => self.voice_activity_card(
                stream.guild,
                owner,
                channel,
                Some(activity),
                None,
                Some(stream),
                false,
                cx,
            ),
            None => div()
                .mt(px(12.))
                .child(self.voice_watch_row(stream, channel, cx))
                .into_any_element(),
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                // `streamHeaderVoiceUserActivities_1v6-1l`: the two ends of a row, `--space-xs` (8px at the default density) over the preview.
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .mb(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("HV97U/")),
                    )
                    .child(super::voice_invite::live_pill()),
            )
            .child(tile)
            // `bodyUserActivity_1v6-1l`: `--space-xs` over the card.
            .child(div().pt(px(8.)).child(body))
            .into_any_element()
    }

    /// Module 711589 `K` for a stream: the action's words, whether it can be watched, whether it is this account's own
    /// and whether this client watches it.
    ///
    /// GAP: of the reasons of module 279250 `zP` only a missing Connect ("No permission") is read; remote mode, a full
    /// channel and an age-restricted one are not.
    fn voice_watch_state(
        &self,
        stream: StreamOf,
        channel: &Channel,
    ) -> (&'static str, bool, bool, bool) {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let own = me == Some(stream.owner);
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| (call.guild, call.channel) == (stream.guild, stream.channel));
        let watching = in_call
            && (own
                || self
                    .video_stream_of(stream.owner)
                    .is_some_and(|info| info.role == dc_voice::streams::Role::Watch));
        let can_watch = self.my_permission(stream.guild, Some(channel), permissions::CONNECT);
        let action = if own {
            dc_intl::t("XvBdeT")
        } else if watching {
            dc_intl::t("JH1SJ+")
        } else if !can_watch {
            dc_intl::t("pgUTZC")
        } else {
            dc_intl::t("7Xq/nV")
        };
        (action, can_watch, own, watching)
    }

    /// `eS` of module 340851: select the stream's channel (joining it, unless the stream is one's own) and watch.
    fn voice_watch_stream(&mut self, guild: Id, channel: Id, owner: Id, cx: &mut Context<Self>) {
        self.voice_hover_dismiss(cx);
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        if me == Some(owner) {
            self.open_voice_view(guild, channel, cx);
            return;
        }
        self.request_watch_voice(guild, channel, owner, cx);
        self.open_voice_view(guild, channel, cx);
    }

    /// Module 961272 `eA` (`watchStreamRow_NzHADT`): the full-width Watch Stream button, `active` (or `secondary` while
    /// watching), off for one's own stream, a watched one or one that cannot be watched; it selects the channel and
    /// watches (`Nl`) and closes the popout.
    ///
    /// GAP: the "Start watching multiple streams" icon button (EyePlusIcon, shown while another stream of the channel
    /// is live) is not drawn.
    fn voice_watch_row(
        &self,
        stream: StreamOf,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (action, can_watch, own, watching) = self.voice_watch_state(stream, channel);
        let disabled = own || watching || !can_watch;
        let StreamOf {
            guild,
            channel: channel_id,
            owner,
        } = stream;
        let button = ds_button(
            format!("voice-user-watch-{owner}"),
            action,
            if watching {
                ButtonLook::Secondary
            } else {
                ButtonLook::Active
            },
            true,
            Some("conn_screen_stream"),
        );
        let button = if disabled {
            button.opacity(0.5).cursor_default()
        } else {
            button.on_click(cx.listener(move |this, _, _, cx| {
                this.voice_watch_stream(guild, channel_id, owner, cx);
            }))
        };
        div().flex().gap(px(8.)).child(button).into_any_element()
    }

    /// Module 655724 `B`: Ask to Stream, full width and small (`primary`, or `secondary` among an activity's buttons),
    /// off with "Not in voice channel" over it unless this client is in the channel, and with "Request sent too
    /// recently" while the last request rests (module 756977).
    fn ask_to_stream_button(
        &self,
        channel: &Channel,
        user: Id,
        game: Option<Value>,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cooldown = on_cooldown(user);
        let inside = self.in_voice_channel(channel.id);
        let disabled = cooldown || !inside;
        let tip = if cooldown {
            dc_intl::t("mVFTUg")
        } else {
            dc_intl::t("qRXats")
        };
        let channel_id = channel.id;
        let button = ds_button(
            format!("voice-user-ask-to-stream-{user}"),
            dc_intl::t("DKHhec"),
            if primary {
                ButtonLook::Primary
            } else {
                ButtonLook::Secondary
            },
            true,
            None,
        );
        let button = if disabled {
            button.opacity(0.5).cursor_default()
        } else {
            button.on_click(cx.listener(move |this, _, _, cx| {
                this.send_stream_request(channel_id, user, game.clone(), cx);
            }))
        };
        if disabled {
            self.with_tip(
                format!("voice-user-ask-to-stream-tip-{user}"),
                Tip::body(Body::Text(tip.into()), Side::Top),
                button,
                cx,
            )
            .into_any_element()
        } else {
            button.into_any_element()
        }
    }

    /// Module 329554 `y` with `v` (`flexColumn_-kuxsn`): the heading ("Playing Now", …) unless hidden, the 32px picture
    /// 8px before the name (`heading-xs/medium`, `text-strong`), the details and the state (`body-xs/normal`,
    /// `text-muted`, one line each; no state beside a party) and the chips of module 78871 (the elapsed time in green,
    /// the party), then the actions of module 961272 (`VOICE_CHANNEL`, or `STREAM_PREVIEW` for a stream's game).
    ///
    /// GAP: Spotify's own layout (`vP`: the artists as links and the song's time bar) is drawn as a plain card, and a
    /// countdown timer or one with an end, the topics chip (`Rq`) and the game profile link (`d`) are left out. Of the
    /// actions only Ask to Stream, Join for an Activity, Watch Stream and the activity's own buttons are drawn: the
    /// console Connect buttons, Spotify's Play and Listen Along, Join Voice Channel, Join and Ask to Join for a game
    /// that takes joins (module 513772 with 776677), Notify and the store's call-to-action are not; an Activity's Join
    /// does not ask whether this session may join (module 550151 `vG`), it launches the Activity.
    #[allow(clippy::too_many_arguments)]
    fn voice_activity_card(
        &self,
        guild: Id,
        user: Id,
        channel: &Channel,
        activity: Option<&Value>,
        app: Option<&EmbeddedApp>,
        stream: Option<StreamOf>,
        show_header: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let empty = Value::Null;
        let activity_value = activity.unwrap_or(&empty);
        let spotify = activity.is_some_and(dc_state::now_playing::is_spotify);
        // `Gf`: Spotify reads its song from the details, an Activity its application, anything else itself.
        let (name, kind) = if spotify {
            (
                activity_value["details"]
                    .as_str()
                    .or_else(|| activity_value["name"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
                2,
            )
        } else if let Some(app) = app {
            (
                app.name.clone(),
                if activity_value["type"].as_u64() == Some(3) {
                    3
                } else {
                    0
                },
            )
        } else {
            (
                activity_value["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                activity_value["type"].as_u64().unwrap_or(0),
            )
        };
        let text_of = |key: &str| {
            activity_value[key]
                .as_str()
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let details = (!spotify).then(|| text_of("details")).flatten();
        let party = !embedded(activity_value) && !activity_value["party"].is_null();
        let state = text_of("state").filter(|_| spotify || !party);
        // `Tf`: the large (or small) asset; else an Activity's icon, or a game's icon.
        let application = app
            .map(|app| app.application.clone())
            .or_else(|| activity_value["application_id"].as_str().map(str::to_owned));
        let asset = asset_url(activity_value, "large_image")
            .or_else(|| asset_url(activity_value, "small_image"));
        let picture = match asset {
            Some(url) => self.images.get(&url).cloned(),
            None if app.is_some() => application
                .as_ref()
                .and_then(|id| self.images.get(&format!("shelf-icon:{id}")))
                .or_else(|| {
                    application
                        .as_ref()
                        .and_then(|id| self.images.get(&format!("app-icon:{id}")))
                })
                .cloned(),
            None if activity_value["type"].as_u64() == Some(0) => application
                .as_ref()
                .and_then(|id| self.images.get(&format!("app-icon:{id}")))
                .cloned(),
            None => None,
        };
        // Module 78871: `QA` (the time since the start, or since the activity was created) and `cy` (the party).
        let start = millis(&activity_value["timestamps"]["start"])
            .or_else(|| millis(&activity_value["created_at"]))
            .filter(|_| {
                activity.is_some() && millis(&activity_value["timestamps"]["end"]).is_none()
            });
        let glyph = match kind {
            2 => "activity_music",
            3 => "activity_tv",
            _ => "activity_game",
        };
        let chip_party = party
            .then(|| party_text(activity_value["state"].as_str(), &activity_value["party"]))
            .flatten();
        let chips = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(8.))
            .children(start.map(|start| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon(glyph, 12., "--text-feedback-positive"))
                    .child(self.elapsed_timer(start.max(0) as u64, cx))
            }))
            .children(chip_party.map(|text| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("activity_party", 12., "--text-subtle"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(text),
                    )
            }));
        let line = |text: String, size: f32, weight: f32, tone: &str| {
            div()
                .min_w(px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(size))
                .line_height(px(16.))
                .font_weight(FontWeight(weight))
                .text_color(color(tone))
                .child(text)
        };
        let row = div()
            .flex()
            .gap(px(8.))
            .children(picture.map(|picture| {
                img(picture)
                    .flex_none()
                    .size(px(32.))
                    .rounded(px(8.))
                    .object_fit(ObjectFit::Cover)
            }))
            .child(
                // `detailsContainer_-kuxsn`.
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(line(name, 12., 500., "--text-strong"))
                    .children(details.map(|text| line(text, 12., 400., "--text-muted")))
                    .children(state.map(|text| line(text, 12., 400., "--text-muted")))
                    .when(activity.is_some() && !spotify, |this| this.child(chips)),
            );
        let actions = self.voice_activity_actions(guild, user, channel, activity, app, stream, cx);
        div()
            .parity("voice-activity-card")
            .flex()
            .flex_col()
            .when(show_header, |this| {
                // `headerContainer_-kuxsn`: `--space-xs` (8px at the default density) over the row.
                this.child(
                    div()
                        .mb(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-muted"))
                        .child(heading_of(kind)),
                )
            })
            .child(row)
            .children(actions)
            .into_any_element()
    }

    /// Module 961272 `ep` as the voice cards use it (`buttonVariant: "primary"`): `buttonsWrapper_nwnlPG`, 12px under
    /// the card, the buttons 8px apart in a column; nothing when no button shows.
    #[allow(clippy::too_many_arguments)]
    fn voice_activity_actions(
        &self,
        guild: Id,
        user: Id,
        channel: &Channel,
        activity: Option<&Value>,
        app: Option<&EmbeddedApp>,
        stream: Option<StreamOf>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut buttons: Vec<AnyElement> = Vec::new();
        if let Some(stream) = stream {
            buttons.push(self.voice_watch_row(stream, channel, cx));
        } else {
            // `B`: a streamable game in a channel offers Ask to Stream first, and the rest turn secondary.
            let game = activity.is_some_and(streamable_game);
            if game && let Some(request) = self.request_to_stream(guild, channel, user) {
                buttons.push(self.ask_to_stream_button(channel, user, request, true, cx));
            }
            let look = if game {
                ButtonLook::Secondary
            } else {
                ButtonLook::Primary
            };
            // `W` (module 284525 through 513772): Join for an Activity running in the call, "Joined" (off) once this
            // session is in it.
            if let Some(app) = app {
                let joined = self.activities.running.as_ref().is_some_and(|running| {
                    running.application_id == app.application && running.channel == channel.id
                });
                let id = app.application.clone();
                let button = ds_button(
                    format!("voice-user-activity-join-{}", app.application),
                    if joined {
                        dc_intl::t("DPfdsq")
                    } else {
                        dc_intl::t("VJlc0S")
                    },
                    look,
                    true,
                    None,
                );
                buttons.push(if joined {
                    button.opacity(0.5).cursor_default().into_any_element()
                } else {
                    button
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.voice_hover_dismiss(cx);
                            this.launch_activity(&id, cx);
                        }))
                        .into_any_element()
                });
            }
            // `M`: the activity's own buttons, which open their link.
            if let Some(activity) = activity.filter(|_| app.is_none()) {
                for (index, label) in activity["buttons"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .enumerate()
                {
                    let value = activity.clone();
                    buttons.push(
                        ds_button(
                            format!("voice-user-activity-button-{user}-{index}"),
                            label.to_owned(),
                            look,
                            true,
                            None,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.voice_hover_dismiss(cx);
                            this.live_button_clicked(user, &value, index, cx);
                        }))
                        .into_any_element(),
                    );
                }
            }
        }
        if buttons.is_empty() {
            return None;
        }
        Some(
            div()
                .mt(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(buttons)
                .into_any_element(),
        )
    }

    /// `F`: `durationContainer_utcclU` on `--opacity-4` with the top corners round, 12px by 16px inside: the 32px avatar
    /// (streaming, or the status unless offline), then the name (`body-sm/medium`, the member's nick or the display
    /// name, one line) and the time since `connectedAt`, 12px apart and pushed to the two ends.
    fn voice_user_duration(
        &self,
        guild: Id,
        channel: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let known = self
            .stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| self.stores.current_user.clone().filter(|me| me.id == user))?;
        let name = self
            .stores
            .members
            .get(guild, user)
            .and_then(|member| member.nick.clone())
            .unwrap_or_else(|| known.display_name().to_owned());
        let status = match self.stores.status(user) {
            "offline" | "invisible" => HIDDEN_STATUS,
            status => status,
        };
        let connected = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .find(|state| state.user_id == user)
            .and_then(|state| state.connected_at)
            .map(|seconds| seconds * 1000);
        Some(
            div()
                .parity("voice-user-duration")
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .py(px(12.))
                .px(px(16.))
                .rounded_t(px(12.))
                .bg(color("--opacity-4"))
                .child(self.member_avatar(&known, status, false, false, cx))
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(SharedString::from(name)),
                        )
                        .children(connected.map(|start| self.elapsed_timer(start, cx))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{listed_activities, streamable_game, voice_activity_shown};
    use serde_json::json;

    #[test]
    fn games_and_watching_need_something_listening_needs_nothing() {
        assert!(voice_activity_shown(&json!({"type": 0, "name": "Game"})));
        assert!(!voice_activity_shown(&json!({"type": 3})));
        assert!(voice_activity_shown(&json!({"type": 2})));
        assert!(!voice_activity_shown(
            &json!({"type": 4, "state": "custom"})
        ));
        assert!(!voice_activity_shown(&json!({"type": 1, "name": "Stream"})));
    }

    #[test]
    fn the_streams_game_and_repeats_and_embedded_apps_are_left_out() {
        let game = json!({"type": 0, "name": "Game", "application_id": "1"});
        let again = json!({"type": 0, "name": "Game again", "application_id": "1"});
        let music = json!({"type": 2, "name": "Spotify"});
        let activity = json!({"type": 0, "name": "Activity", "application_id": "2", "flags": 256});
        let all = [&game, &again, &music, &activity];
        let names = |list: Vec<&serde_json::Value>| -> Vec<String> {
            list.iter()
                .map(|one| one["name"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(names(listed_activities(&all, None)), ["Game", "Spotify"]);
        assert_eq!(names(listed_activities(&all, Some(&game))), ["Spotify"]);
        let by_name = json!({"type": 0, "name": "Spotify"});
        assert_eq!(names(listed_activities(&all, Some(&by_name))), ["Game"]);
    }

    #[test]
    fn a_streamable_game_has_an_application_and_no_console() {
        assert!(streamable_game(&json!({"type": 0, "application_id": "5"})));
        assert!(!streamable_game(&json!({"type": 0, "name": "No app"})));
        assert!(!streamable_game(
            &json!({"type": 0, "application_id": "5", "platform": "ps5"})
        ));
        assert!(!streamable_game(
            &json!({"type": 0, "application_id": "438122941302046720"})
        ));
        assert!(!streamable_game(&json!({"type": 2, "application_id": "5"})));
    }
}
