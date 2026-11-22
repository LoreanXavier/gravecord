//! The Active Now card's menu (`El` of module 596339, `now-playing-menu`, aria label "Actions"): a click on a card
//! opens it beside the card (the popout of `EE`, `position: "left"`, `spacing: 8`); a right click on the card's header
//! opens the priority person's own menu instead (`onUserContextMenu`).
//!
//! Its groups, in order: Message (one person alone); Join Channel and Go to Server for the first voice channel, then
//! Watch {name}'s Stream for every stream; Play on Spotify / Listen Along for a Spotify party; each activity's
//! Joined / Join / Ask to Join, and Connect Xbox; then the people, under "{n} People", or per game ("{game} - n") and
//! "Other Members - n".
//!
//! GAP: Join (the game's `ACTIVITY_JOIN` over RPC, 308368 `join`), Ask to Join (an activity invite message,
//! `sendActivityInviteUser`) and Play Game (the library launcher, `canPlay` of 144914) have no plumbing in this client:
//! the first two rows show with the official state and do nothing, Play Game is never offered.
//! GAP: no DOM capture of this menu; rows reuse the measured context menu rows, the people rows are `memberItem_SISQ-M`
//! (24px avatar, 6px by 8px) and the group labels `groupLabel_M3qk4j` (14px medium `--text-muted`, 8px inside).

use super::{Root, color};
use dc_state::{
    Id,
    now_playing::{Party, Stream},
};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, anchored, deferred, div,
    prelude::*, px,
};
use serde_json::Value;

/// `jUm` of module 652215.
const JOIN: u64 = 2;
const PARTY_PRIVACY_FRIENDS: u64 = 64;
const PARTY_PRIVACY_VOICE_CHANNEL: u64 = 128;
const EMBEDDED: u64 = 256;

/// `o` of module 776677.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Joinability {
    CanJoin,
    CannotJoin,
    Joined,
}

/// What 776677 reads for a desktop, non-embedded activity.
#[derive(Clone, Debug, Default)]
pub(super) struct JoinFacts {
    /// Our own activity of the same application shares its party id (`d.w` over 454292).
    pub same_party_locally: bool,
    pub friend: bool,
    /// The person is in our voice channel; with its kind (1 DM, 3 group DM) and, for a server, whether it is a
    /// Community server and its member count.
    pub shared_voice: Option<(u64, bool, Option<u64>)>,
}

fn flag(activity: &Value, bit: u64) -> bool {
    activity["flags"].as_u64().unwrap_or(0) & bit != 0
}

/// `_C` of module 874546: a `PLAYING` activity with the JOIN flag.
pub(super) fn joinable_kind(activity: &Value) -> bool {
    flag(activity, JOIN) && activity["type"].as_u64() == Some(0)
}

/// `Ay` of module 874546 on desktop: `_C`, and the activity's platform (desktop when none) is desktop or its
/// `supported_platforms` name it.
pub(super) fn joinable_here(activity: &Value) -> bool {
    if !joinable_kind(activity) {
        return false;
    }
    if activity["platform"].as_str().unwrap_or("desktop") == "desktop" {
        return true;
    }
    activity["supported_platforms"]
        .as_array()
        .is_some_and(|list| list.iter().any(|item| item == "desktop"))
}

/// Module 776677 for an activity of the Active Now card (`isEmbedded: false`, no channel).
pub(super) fn joinability(activity: &Value, facts: &JoinFacts) -> Joinability {
    if facts.same_party_locally {
        return Joinability::Joined;
    }
    if !joinable_here(activity) {
        return Joinability::CannotJoin;
    }
    // 125017 `_`, 287613 and 702631: a party size of two numbers, not full.
    let size = activity["party"]["size"].as_array();
    let (count, max) = match size {
        Some(size) if size.len() >= 2 => (
            size[0].as_i64().unwrap_or(-1),
            size[1].as_i64().unwrap_or(-1),
        ),
        _ => (-1, -1),
    };
    if !(count > -1 && max > -1) || (max > 0 && count >= max) {
        return Joinability::CannotJoin;
    }
    if flag(activity, PARTY_PRIVACY_FRIENDS) && facts.friend {
        return Joinability::CanJoin;
    }
    if flag(activity, PARTY_PRIVACY_VOICE_CHANNEL)
        && let Some((kind, community, members)) = facts.shared_voice
    {
        if matches!(kind, 1 | 3) {
            return Joinability::CanJoin;
        }
        if !community && members.is_some_and(|count| count < 100) {
            return Joinability::CanJoin;
        }
    }
    Joinability::CannotJoin
}

/// `uB.A` of module 682261: an activity with rich presence.
pub(super) fn has_rich_presence(activity: &Value) -> bool {
    let assets = &activity["assets"];
    activity["type"].as_u64() != Some(4)
        && (!activity["details"].is_null()
            || (assets.is_object()
                && (!assets["large_image"].is_null() || !assets["small_text"].is_null()))
            || !activity["party"].is_null()
            || !activity["secrets"].is_null()
            || !activity["state"].is_null())
}

/// The `xbox-connect` row: no activity with rich presence, no Xbox account linked, an Xbox activity among them and
/// none of those joinable here.
pub(super) fn offers_xbox(activities: &[&Value], xbox_linked: bool) -> bool {
    let xbox: Vec<&&Value> = activities
        .iter()
        .filter(|activity| super::platform_presence::is_xbox(activity))
        .collect();
    !activities
        .iter()
        .any(|activity| has_rich_presence(activity))
        && !xbox_linked
        && !xbox.is_empty()
        && !xbox.iter().any(|activity| joinable_here(activity))
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Act {
    Message(Id),
    JoinVoice {
        guild: Id,
        channel: Id,
    },
    GoToServer(Id),
    Watch(Stream),
    Spotify {
        user: Id,
        sync: bool,
    },
    /// Module 468389 / 273626: join a user's joinable activity session.
    JoinActivity {
        user: Id,
        session_id: String,
        app_id: Id,
    },
    /// Module 468389 / 308368: send an Ask to Join (`type: 5`) request message.
    AskToJoinActivity {
        user: Id,
        app_id: Id,
        party_id: Option<String>,
    },
    /// Disabled display-only row (e.g., already Joined).
    Unwired,
    ConnectXbox,
    Profile(Id),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Line {
    Item {
        label: String,
        sub: Option<String>,
        disabled: bool,
        act: Act,
    },
    Heading(String),
    Member(Id),
    Unknown(usize),
    Separator,
}

/// One activity of the party as the menu needs it.
#[derive(Clone, Debug)]
pub(super) struct ActivityFacts {
    pub user: Id,
    /// `i.toString()`: the person's username.
    pub username: String,
    pub name: String,
    pub value: Value,
    pub joinability: Joinability,
    pub joining: bool,
}

/// A Spotify party's first activity (module 286617 through 341335).
#[derive(Clone, Debug)]
pub(super) struct SpotifyFacts {
    pub user: Id,
    pub play_disabled: bool,
    pub play_sub: Option<String>,
    /// `canPlaySpotify`: Listen Along is offered.
    pub can_sync: bool,
    /// Set when Listen Along is disabled, with the reason.
    pub sync_sub: Option<String>,
}

/// Everything the menu shows, read from the stores.
#[derive(Clone, Debug, Default)]
pub(super) struct Facts {
    pub message: Option<Id>,
    /// The first voice channel: guild, channel, whether we may connect.
    pub voice: Option<(Id, Id, bool)>,
    pub streams: Vec<(Stream, String)>,
    pub spotify: Option<SpotifyFacts>,
    pub activities: Vec<ActivityFacts>,
    pub xbox_connect: bool,
    /// People groups: an optional label, the people, the unknown count.
    pub groups: Vec<(Option<String>, Vec<Id>, usize)>,
}

/// `El`'s children: `[l, rX[o, d], rX[c], rX[u], _]`.
pub(super) fn lines(facts: &Facts) -> Vec<Line> {
    let mut out = Vec::new();
    let group = |out: &mut Vec<Line>, items: Vec<Line>| {
        if items.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push(Line::Separator);
        }
        out.extend(items);
    };
    let item = |label: String, sub: Option<String>, disabled: bool, act: Act| Line::Item {
        label,
        sub,
        disabled,
        act,
    };
    if let Some(user) = facts.message {
        group(
            &mut out,
            vec![item(
                dc_intl::t("g33r/P").to_owned(),
                None,
                false,
                Act::Message(user),
            )],
        );
    }
    let mut places = Vec::new();
    if let Some((guild, channel, can)) = facts.voice {
        places.push(item(
            dc_intl::t("8yOlh9").to_owned(),
            None,
            !can,
            Act::JoinVoice { guild, channel },
        ));
        places.push(item(
            dc_intl::t("RGgqm1").to_owned(),
            None,
            false,
            Act::GoToServer(guild),
        ));
    }
    for (stream, name) in &facts.streams {
        places.push(item(
            dc_intl::format("m09d0T", &[("streamerName", name.clone().into())]),
            None,
            false,
            Act::Watch(stream.clone()),
        ));
    }
    group(&mut out, places);
    if let Some(SpotifyFacts {
        user,
        play_disabled: play_off,
        play_sub,
        can_sync: sync_shown,
        sync_sub,
    }) = facts.spotify.clone()
    {
        let mut rows = vec![item(
            dc_intl::t("rRffNz").to_owned(),
            play_sub.filter(|_| play_off),
            play_off,
            Act::Spotify { user, sync: false },
        )];
        if sync_shown {
            let sync_off = sync_sub.is_some();
            rows.push(item(
                dc_intl::t("eU3inB").to_owned(),
                sync_sub,
                sync_off,
                Act::Spotify { user, sync: true },
            ));
        }
        group(&mut out, rows);
    }
    let multiple = facts.activities.len() > 1;
    let mut actions = Vec::new();
    for activity in &facts.activities {
        match activity.joinability {
            Joinability::Joined => actions.push(item(
                dc_intl::t("KC26NR").to_owned(),
                Some(activity.name.clone()),
                true,
                Act::Unwired,
            )),
            Joinability::CanJoin => {
                let session_id = activity.value["session_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let app_id = activity.value["application_id"]
                    .as_str()
                    .and_then(|s| s.parse::<Id>().ok())
                    .unwrap_or(0);
                actions.push(item(
                    dc_intl::t(if activity.joining { "bf6Ci7" } else { "VJlc0S" }).to_owned(),
                    Some(activity.name.clone()),
                    activity.joining,
                    Act::JoinActivity {
                        user: activity.user,
                        session_id,
                        app_id,
                    },
                ));
            }
            Joinability::CannotJoin if joinable_kind(&activity.value) => {
                let app_id = activity.value["application_id"]
                    .as_str()
                    .and_then(|s| s.parse::<Id>().ok())
                    .unwrap_or(0);
                let party_id = activity.value["party"]["id"].as_str().map(str::to_owned);
                actions.push(item(
                    if multiple {
                        dc_intl::format("wFAvxM", &[("name", activity.username.clone().into())])
                    } else {
                        dc_intl::t("OKsSCR").to_owned()
                    },
                    Some(activity.name.clone()),
                    false,
                    Act::AskToJoinActivity {
                        user: activity.user,
                        app_id,
                        party_id,
                    },
                ));
            }
            Joinability::CannotJoin => {}
        }
    }
    if facts.xbox_connect {
        actions.push(item(
            dc_intl::format("XWSHTb", &[("platform", "Xbox".into())]),
            None,
            false,
            Act::ConnectXbox,
        ));
    }
    group(&mut out, actions);
    for (label, members, unknown) in &facts.groups {
        let mut rows = Vec::new();
        if let Some(label) = label {
            rows.push(Line::Heading(label.clone()));
        }
        rows.extend(members.iter().map(|user| Line::Member(*user)));
        if *unknown > 0 {
            rows.push(Line::Unknown(*unknown));
        }
        group(&mut out, rows);
    }
    out
}

/// `_` of `El`: the people of the party, whole or by game.
pub(super) fn people(
    party: &Party,
    app_name: impl Fn(usize) -> String,
) -> Vec<(Option<String>, Vec<Id>, usize)> {
    let priority = &party.priority;
    let others: Vec<Id> = party
        .members
        .iter()
        .copied()
        .filter(|user| !priority.contains(user))
        .collect();
    let playing: Vec<Id> = party
        .activities
        .iter()
        .flat_map(|activity| activity.members.iter().copied())
        .collect();
    let idle: Vec<Id> = party
        .members
        .iter()
        .copied()
        .filter(|user| !playing.contains(user))
        .collect();
    let list = |among: &[Id]| -> Vec<Id> {
        let mut rows: Vec<Id> = priority
            .iter()
            .copied()
            .filter(|user| among.contains(user))
            .collect();
        rows.extend(
            among
                .iter()
                .copied()
                .filter(|user| !priority.contains(user)),
        );
        rows
    };
    if party.activities.len() <= 1 && idle.is_empty() {
        let label = (party.members.len() > 1).then(|| {
            dc_intl::format(
                "C4WXvc",
                &[("memberCount", (party.members.len() as i64).into())],
            )
        });
        let mut rows: Vec<Id> = priority.clone();
        rows.extend(others);
        let unknown = party.members.len().saturating_sub(rows.len());
        return vec![(label, rows, unknown)];
    }
    let mut groups: Vec<(Option<String>, Vec<Id>, usize)> = party
        .activities
        .iter()
        .enumerate()
        .map(|(index, activity)| {
            let rows = list(&activity.members);
            let unknown = activity.members.len().saturating_sub(rows.len());
            (
                Some(format!("{} - {}", app_name(index), activity.members.len())),
                rows,
                unknown,
            )
        })
        .collect();
    groups.push((
        Some(format!("{} - {}", dc_intl::t("mIx+gG"), idle.len())),
        list(&idle),
        0,
    ));
    groups
}

impl Root {
    /// A click on a card: its menu opens, or closes when it was open (the popout's toggle).
    pub(super) fn now_playing_menu_toggle(&mut self, party: &str, cx: &mut Context<Self>) {
        if self.active_now.menu.as_deref() == Some(party) {
            self.active_now.menu = None;
        } else {
            self.active_now.menu = Some(party.to_owned());
        }
        cx.notify();
    }

    /// Escape closes an open card menu (the popout's `closeOnEscape`).
    pub(super) fn now_playing_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.active_now.menu.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    fn now_playing_facts(&self, party: &Party) -> Facts {
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let name_of = |user: Id| {
            self.stores
                .users
                .get(&user)
                .map(|user| user.display_name().to_owned())
                .unwrap_or_default()
        };
        let message =
            (party.members.len() == 1 && party.priority.len() == 1).then(|| party.priority[0]);
        let voice = party.voice.first().map(|voice| {
            let can = self.stores.guild(voice.guild).is_some_and(|guild| {
                guild
                    .channels
                    .iter()
                    .find(|channel| channel.id == voice.channel)
                    .is_some_and(|channel| {
                        self.can_in_channel(channel, dc_state::permissions::CONNECT)
                    })
            });
            (voice.guild, voice.channel, can)
        });
        let streams = party
            .streams
            .iter()
            .map(|stream| (stream.clone(), name_of(stream.owner)))
            .collect();
        let spotify = party
            .activities
            .first()
            .filter(|_| party.spotify)
            .map(|activity| {
                let controls = self
                    .stores
                    .spotify
                    .controls(me, activity.user, &activity.value);
                // 341335 for USER_ACTIVITY_PLAY and USER_ACTIVITY_SYNC.
                let play_sub = if controls.own {
                    Some(dc_intl::t("ZpDeUC").to_owned())
                } else if controls.same_track {
                    Some(dc_intl::t("TYSymS").to_owned())
                } else if controls.not_playable {
                    Some(dc_intl::format("SqJBnN", &[("name", "Spotify".into())]))
                } else {
                    None
                };
                let sync_off = controls.own || controls.same_user || controls.same_party;
                let sync_sub = sync_off.then(|| {
                    if controls.own {
                        dc_intl::t("AOU/CR").to_owned()
                    } else {
                        dc_intl::t("m8CScv").to_owned()
                    }
                });
                SpotifyFacts {
                    user: activity.user,
                    play_disabled: controls.own || controls.not_playable || controls.same_track,
                    play_sub,
                    can_sync: controls.has_account,
                    sync_sub,
                }
            });
        let my_voice = self.call.as_ref().map(|call| (call.guild, call.channel));
        let own_activities: Vec<Value> = self
            .stores
            .presence
            .get(me, None)
            .map(|presence| presence.visible_activities().into_iter().cloned().collect())
            .unwrap_or_default();
        let activities = party
            .activities
            .iter()
            .filter(|activity| !flag(&activity.value, EMBEDDED))
            .map(|activity| {
                let value = &activity.value;
                let party_id = value["party"]["id"].as_str();
                let same_party_locally = party_id.is_some()
                    && own_activities.iter().any(|own| {
                        own["application_id"] == value["application_id"]
                            && own["party"]["id"].as_str() == party_id
                    });
                let shared_voice = my_voice.and_then(|(guild, channel)| {
                    let theirs = if guild == 0 {
                        None
                    } else {
                        self.voice_channel_of(guild, activity.user)
                    };
                    if guild == 0 || theirs != Some(channel) {
                        // A DM call: the person is in it when the call's voice states say so.
                        let dm = self
                            .stores
                            .private_channels
                            .iter()
                            .find(|known| known.id == channel)?;
                        let present = self.stores.voice_states.get(&0).is_some_and(|states| {
                            states.iter().any(|state| {
                                state.user_id == activity.user && state.channel_id == channel
                            })
                        });
                        return present.then_some((dm.kind, false, None));
                    }
                    let server = self.stores.guild(guild)?;
                    Some((
                        0,
                        server.features.iter().any(|feature| feature == "COMMUNITY"),
                        self.stores.member_counts.get_member_count(guild),
                    ))
                });
                let facts = JoinFacts {
                    same_party_locally,
                    friend: self.stores.relationships.get(&activity.user) == Some(&1),
                    shared_voice,
                };
                let app_id_str = value["application_id"].as_str().unwrap_or_default();
                let join_key = format!("{}:{app_id_str}", activity.user);
                ActivityFacts {
                    user: activity.user,
                    username: self
                        .stores
                        .users
                        .get(&activity.user)
                        .map(|user| user.username.clone())
                        .unwrap_or_default(),
                    name: value["name"].as_str().unwrap_or_default().to_owned(),
                    value: value.clone(),
                    joinability: joinability(value, &facts),
                    joining: self.joining_activities.contains(&join_key),
                }
            })
            .collect();
        let values: Vec<&Value> = party
            .activities
            .iter()
            .map(|activity| &activity.value)
            .collect();
        let xbox_connect = offers_xbox(&values, self.linked_platforms().contains(&"xbox"));
        let groups = people(party, |index| {
            party.activities[index].application.name.clone()
        });
        Facts {
            message,
            voice,
            streams,
            spotify,
            activities,
            xbox_connect,
            groups,
        }
    }

    fn now_playing_run(
        &mut self,
        act: Act,
        party: &Party,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.active_now.menu = None;
        match act {
            Act::Message(user) => self.open_dm_with(user, cx),
            Act::JoinVoice { guild, channel } => {
                self.open_voice_view(guild, channel, cx);
                self.request_join_voice(guild, channel, cx);
            }
            Act::GoToServer(guild) => self.go_guild(guild, cx),
            Act::Watch(stream) => {
                if stream.guild == 0 {
                    self.join_dm_call(stream.channel, false, cx);
                } else {
                    self.open_voice_view(stream.guild, stream.channel, cx);
                    self.request_watch_voice(stream.guild, stream.channel, stream.owner, cx);
                }
            }
            Act::Spotify { user, sync } => {
                if let Some(activity) = party.activities.first() {
                    let value = activity.value.clone();
                    self.spotify_clicked(
                        user,
                        &value,
                        sync,
                        super::live_activity::Surface::Popout,
                        cx,
                    );
                }
            }
            Act::JoinActivity {
                user,
                session_id,
                app_id,
            } => {
                self.join_user_activity(user, session_id, app_id, self.channel, cx);
            }
            Act::AskToJoinActivity {
                user,
                app_id,
                party_id,
            } => {
                self.send_activity_invite(user, None, 5, Some(app_id), party_id, cx);
            }
            Act::Unwired => {}
            Act::ConnectXbox => self.open_settings_page("connected-apps", cx),
            // `openUserProfileModal`.
            Act::Profile(user) => self.open_full_profile(user, window, cx),
        }
        cx.notify();
    }

    /// The open card's menu, beside its card.
    pub(super) fn now_playing_menu(
        &self,
        party: &Party,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.active_now.menu.as_deref() != Some(party.id.as_str()) {
            return None;
        }
        let card = self
            .tip_anchors
            .borrow()
            .get(format!("active-now-party-{}", party.id).as_str())
            .copied()?;
        let facts = self.now_playing_facts(party);
        let lines = lines(&facts);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            let key = SharedString::from(format!("now-playing-row-{index}"));
            let row = match line {
                Line::Separator => div()
                    .my(px(8.))
                    .h(px(1.))
                    .bg(color("--border-subtle"))
                    .into_any_element(),
                Line::Heading(label) => div()
                    .min_h(px(32.))
                    .p(px(8.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(label)
                    .into_any_element(),
                Line::Member(user) => {
                    // `_7`: the 24px avatar, with the status dot for the priority people (`uz.eu` with `status`).
                    let avatar =
                        self.active_now_avatar(user, 24., party.priority.contains(&user), cx);
                    let name = self
                        .stores
                        .users
                        .get(&user)
                        .map(|known| known.display_name().to_owned())
                        .unwrap_or_default();
                    let party = party.clone();
                    div()
                        .id(key)
                        .min_h(px(24.))
                        .py(px(6.))
                        .px(px(8.))
                        .rounded(px(2.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(|style| {
                            style
                                .bg(color("--interactive-background-hover"))
                                .rounded(px(4.))
                        })
                        .child(div().size(px(24.)).mr(px(8.)).flex_none().child(avatar))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .truncate()
                                .child(name),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.now_playing_run(Act::Profile(user), &party, window, cx)
                        }))
                        .into_any_element()
                }
                Line::Unknown(count) => div()
                    .min_h(px(24.))
                    .py(px(6.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .size(px(24.))
                            .mr(px(8.))
                            .flex_none()
                            .rounded_full()
                            .bg(color("--background-mod-subtle")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::format(
                                "6Kqn9M",
                                &[("count", (count as i64).into())],
                            )),
                    )
                    .into_any_element(),
                Line::Item {
                    label,
                    sub,
                    disabled,
                    act,
                } => {
                    let party = party.clone();
                    div()
                        .id(key)
                        .min_h(px(if sub.is_some() { 52. } else { 34. }))
                        .p(px(8.))
                        .rounded(px(2.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .when(!disabled, |this| {
                            this.cursor_pointer().hover(|style| {
                                style
                                    .bg(color("--interactive-background-hover"))
                                    .rounded(px(4.))
                            })
                        })
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(if disabled {
                                    "--text-muted"
                                } else {
                                    "--text-strong"
                                }))
                                .child(label),
                        )
                        .children(sub.map(|sub| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(sub)
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if !disabled {
                                this.now_playing_run(act.clone(), &party, window, cx);
                            }
                        }))
                        .into_any_element()
                }
            };
            rows.push(row);
        }
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopRight)
                    .position(gpui::point(card.left() - px(8.), card.top()))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("now-playing-menu")
                            .aria_label(dc_intl::t("ogxXGq"))
                            .occlude()
                            .min_w(px(188.))
                            .max_w(px(320.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-higher"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.24),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .p(px(8.))
                            .flex()
                            .flex_col()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.active_now.menu = None;
                                cx.notify();
                            }))
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                Line::Item { label, .. } => label.clone(),
                Line::Heading(label) => format!("# {label}"),
                Line::Member(user) => format!("@{user}"),
                Line::Unknown(count) => format!("?{count}"),
                Line::Separator => "--".to_owned(),
            })
            .collect()
    }

    #[test]
    fn join_needs_the_flag_a_party_with_room_and_a_privacy_rule() {
        let game = json!({"type": 0, "flags": 2 | 64, "party": {"id": "p", "size": [1, 4]}});
        let friend = JoinFacts {
            friend: true,
            ..JoinFacts::default()
        };
        assert_eq!(joinability(&game, &friend), Joinability::CanJoin);
        assert_eq!(
            joinability(&game, &JoinFacts::default()),
            Joinability::CannotJoin
        );
        let full = json!({"type": 0, "flags": 2 | 64, "party": {"size": [4, 4]}});
        assert_eq!(joinability(&full, &friend), Joinability::CannotJoin);
        let no_size = json!({"type": 0, "flags": 2 | 64, "party": {"id": "p"}});
        assert_eq!(joinability(&no_size, &friend), Joinability::CannotJoin);
        let console =
            json!({"type": 0, "flags": 2 | 64, "platform": "xbox", "party": {"size": [1, 4]}});
        assert_eq!(joinability(&console, &friend), Joinability::CannotJoin);
        let voice = json!({"type": 0, "flags": 2 | 128, "party": {"size": [1, 4]}});
        let dm = JoinFacts {
            shared_voice: Some((1, false, None)),
            ..JoinFacts::default()
        };
        assert_eq!(joinability(&voice, &dm), Joinability::CanJoin);
        let big = JoinFacts {
            shared_voice: Some((0, false, Some(500))),
            ..JoinFacts::default()
        };
        assert_eq!(joinability(&voice, &big), Joinability::CannotJoin);
        let joined = JoinFacts {
            same_party_locally: true,
            ..JoinFacts::default()
        };
        assert_eq!(joinability(&json!({}), &joined), Joinability::Joined);
    }

    #[test]
    fn connect_xbox_is_offered_for_a_plain_xbox_game_without_a_link() {
        let xbox = json!({"type": 0, "name": "Halo", "platform": "xbox"});
        assert!(offers_xbox(&[&xbox], false));
        assert!(!offers_xbox(&[&xbox], true));
        let rich = json!({"type": 0, "name": "Halo", "platform": "xbox", "details": "Ranked"});
        assert!(!offers_xbox(&[&rich], false));
        let pc = json!({"type": 0, "name": "Halo"});
        assert!(!offers_xbox(&[&pc], false));
    }

    #[test]
    fn the_menu_keeps_the_official_group_order() {
        let facts = Facts {
            message: Some(7),
            voice: Some((1, 2, false)),
            streams: Vec::new(),
            spotify: None,
            activities: vec![ActivityFacts {
                user: 7,
                username: "mira".to_owned(),
                name: "Halo".to_owned(),
                value: json!({"type": 0, "flags": 2}),
                joinability: Joinability::CannotJoin,
                joining: false,
            }],
            xbox_connect: true,
            groups: vec![(None, vec![7], 0)],
        };
        let lines = lines(&facts);
        assert_eq!(
            labels(&lines),
            [
                "Message",
                "--",
                "Join Channel",
                "Go to Server",
                "--",
                "Ask to Join",
                "Connect Xbox",
                "--",
                "@7"
            ]
        );
        assert!(matches!(&lines[2], Line::Item { disabled: true, .. }));
        assert!(matches!(&lines[5], Line::Item { sub: Some(name), .. } if name == "Halo"));
    }
}
