//! The event cards of the Events modal (module 327241) and the pieces the card and the detail view share: the status
//! line, the three action buttons, the creator's avatar and the interested toggle. Measured in the `events-list-*`
//! captures.

use super::super::tooltip::{Side, Tip};
use super::super::{Root, avatar_source, color, icon};
use super::schedule::{self, Status, Tone};
use super::{CREATE_EVENTS, MANAGE_EVENTS};
use crate::net::Request;
use crate::parity::Parity;
use chrono::NaiveDateTime;
use dc_state::{
    ChannelKind, Id, User, permissions,
    scheduled_events::{
        ENTITY_EXTERNAL, ENTITY_STAGE_INSTANCE, EventUser, RESPONSE_INTERESTED, STATUS_CANCELED,
        STATUS_COMPLETED, ScheduledEvent,
    },
};
use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, Stateful, div, img, prelude::*, px,
};
use serde_json::Value;

/// `rsvp` answers come back under this key, then the guild, the event and the answer (`1` put, `0` delete).
pub(super) const RSVP_KEY: &str = "guild-events-rsvp:";
/// The answers of the signed-in user to every event of a guild (`getGuildEventsForCurrentUser`).
pub(super) const MINE_KEY: &str = "guild-events-mine:";
/// The interested count of one event.
pub(super) const COUNTS_KEY: &str = "guild-events-counts:";
/// The users interested in one event.
pub(super) const USERS_KEY: &str = "guild-events-users:";

/// The tab of the detail view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Tab {
    Info,
    Interested,
}

/// The open detail view of an event.
pub(in super::super) struct Detail {
    pub event: Id,
    pub tab: Tab,
    /// The occurrence of a recurring event picked in "Events in series" (module 893110), by its start in ms.
    pub recurrence: Option<i64>,
    /// The occurrences the series list shows (`er(4, ...)`, four more per "View future events"), worked out when the
    /// view opens and on each View future events, and whether another follows within four years.
    pub series: Vec<chrono::DateTime<chrono::Utc>>,
    pub series_more: bool,
}

/// Which of the two sizes of the design system button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Sm,
    Md,
}

/// The look of an action button: secondary, or the green `active` one of a set Interested.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Look {
    Secondary,
    Connected,
}

/// A design system button: 32 (sm) or 40 (md) high, its icon before its text, 4px between. An icon alone is square.
pub(super) fn action_button(
    id: impl Into<ElementId>,
    size: Size,
    look: Look,
    glyph: &'static str,
    label: Option<&str>,
    disabled: bool,
) -> Stateful<gpui::Div> {
    let (height, glyph_size, text_size, line, pad) = match size {
        Size::Sm => (32., 16., 14., 18., 11.),
        Size::Md => (40., 20., 16., 20., 15.),
    };
    let (background, hover, border, tint) = match look {
        Look::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-background-hover",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
        ),
        Look::Connected => (
            "--control-connected-background-default",
            "--control-connected-background-hover",
            "--control-connected-border-default",
            "--control-connected-text-default",
        ),
    };
    div()
        .id(id)
        .h(px(height))
        .flex_none()
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .when(label.is_none(), |this| this.w(px(height)))
        .when(label.is_some(), |this| this.px(px(pad)))
        .when(!disabled, |this| {
            this.cursor_pointer().hover(|style| style.bg(color(hover)))
        })
        .when(disabled, |this| this.opacity(0.5))
        .when(!glyph.is_empty(), |this| {
            this.child(icon(glyph, glyph_size, tint))
        })
        .children(label.map(|label| {
            div()
                .text_size(px(text_size))
                .line_height(px(line))
                .font_weight(FontWeight(500.))
                .text_color(color(tint))
                .child(label.to_owned())
        }))
}

/// `canManageGuildEvent`: a manager of events, or the creator who may create them.
pub(super) fn can_manage(
    guild: &dc_state::Guild,
    me: permissions::Me,
    event: &ScheduledEvent,
) -> bool {
    permissions::can_in_guild(guild, me, permissions::ADMINISTRATOR)
        || permissions::can_in_guild(guild, me, MANAGE_EVENTS)
        || (event.creator_id == Some(me.id) && permissions::can_in_guild(guild, me, CREATE_EVENTS))
}

/// The tint of a status line's icon and text.
fn tones(tone: Tone) -> (&'static str, &'static str) {
    match tone {
        Tone::Plain => ("--icon-brand", "--text-default"),
        Tone::Soon => ("--icon-brand", "--text-brand"),
        Tone::Live => ("--text-feedback-positive", "--text-feedback-positive"),
        Tone::Over => ("--icon-muted", "--text-muted"),
        Tone::Canceled => ("--text-feedback-critical", "--text-default"),
    }
}

/// The status line: the calendar, then the words of `status_line` in 14/18 semibold.
pub(super) fn status_row(status: &Status) -> gpui::Div {
    let (glyph, text) = tones(status.tone);
    div()
        .flex()
        .items_center()
        .h(px(20.))
        .child(icon("events", 20., glyph))
        .child(
            div()
                .ml(px(8.))
                .flex()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color(text))
                .children(status.lead.as_ref().map(|lead| {
                    div()
                        .text_color(color("--text-feedback-positive"))
                        .child(lead.clone())
                }))
                .child(status.text.clone()),
        )
}

/// The status of an event for the card (`with_end` false) or the detail (true).
pub(super) fn event_status(
    event: &ScheduledEvent,
    now: NaiveDateTime,
    with_end: bool,
) -> Option<Status> {
    let start = schedule::local_time(&event.scheduled_start_time)?;
    let end = event
        .scheduled_end_time
        .as_deref()
        .and_then(schedule::local_time);
    let strings = schedule::time_strings(start, end, now);
    Some(schedule::status_line(
        &strings,
        event.status,
        event.entity_type == ENTITY_EXTERNAL,
        with_end,
    ))
}

/// `1 person is interested`, `2 people are interested`: the plural rule of the detail's count line.
pub(super) fn interested_sentence(count: i64) -> String {
    if count == 1 {
        "1 person is interested".to_owned()
    } else {
        format!("{count} people are interested")
    }
}

impl Root {
    /// A user of an event: the account's own, or one the client has met.
    pub(super) fn event_user(&self, id: Id) -> Option<User> {
        self.stores
            .current_user
            .as_ref()
            .filter(|me| me.id == id)
            .or_else(|| self.stores.users.get(&id))
            .cloned()
    }

    /// A round avatar of `size`, or an empty disc while the picture loads.
    pub(in crate::ui) fn event_avatar(&self, user: Option<&User>, size: f32) -> AnyElement {
        let image = user.and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        match image {
            Some(image) => img(image).size(px(size)).rounded_full().into_any_element(),
            None => div()
                .size(px(size))
                .rounded_full()
                .bg(color("--background-brand"))
                .into_any_element(),
        }
    }

    /// Asks for the pictures of the users an event shows, and for the creators the client has not met.
    pub(super) fn event_prepare_users(&mut self, guild: Id, events: &[Id]) {
        let mut wanted: Vec<Id> = Vec::new();
        for event in events {
            if let Some(creator) = self
                .stores
                .scheduled_events
                .get_guild_scheduled_event(*event)
                .and_then(|event| event.creator_id)
            {
                wanted.push(creator);
            }
            wanted.extend(
                self.stores
                    .scheduled_events
                    .get_users_for_guild_event(*event, None)
                    .iter()
                    .map(|user| user.user_id),
            );
        }
        let mut unknown = Vec::new();
        for id in wanted {
            match self.event_user(id) {
                Some(user) => {
                    let (key, url) = avatar_source(&user);
                    self.request_image(key, url);
                }
                None => unknown.push(id),
            }
        }
        unknown.sort_unstable();
        unknown.dedup();
        let asked: Vec<Id> = match self.guild_events.as_mut() {
            Some(open) => unknown
                .into_iter()
                .filter(|id| open.asked.insert(*id))
                .collect(),
            None => Vec::new(),
        };
        if !asked.is_empty() {
            let _ = self.requests.send(Request::GuildMembers {
                guilds: vec![guild],
                query: String::new(),
                limit: 0,
                user_ids: asked,
            });
        }
    }

    /// The answers that complete the list: the count of every event, and what the user said to them.
    pub(super) fn event_fetch_details(&mut self, guild: Id, events: &[Id]) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{MINE_KEY}{guild}"),
            route: format!("/users/@me/scheduled-events?guild_ids={guild}"),
        });
        for event in events {
            self.event_fetch_counts(guild, *event);
        }
    }

    pub(super) fn event_fetch_counts(&mut self, guild: Id, event: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{COUNTS_KEY}{guild}:{event}"),
            route: format!("/guilds/{guild}/scheduled-events/{event}/users/counts"),
        });
    }

    pub(super) fn event_fetch_users(&mut self, guild: Id, event: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{USERS_KEY}{guild}:{event}"),
            route: format!(
                "/guilds/{guild}/scheduled-events/{event}/users?limit=100&with_member=true"
            ),
        });
    }

    /// Takes the answers of the keys above into the store. Returns whether the key was one of them.
    pub(super) fn event_details_fetched(&mut self, key: &str, body: &Value) -> bool {
        if key.starts_with(MINE_KEY) {
            if body.is_array() {
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_RSVPS_FETCH_SUCESS", body);
            }
            return true;
        }
        if let Some(ids) = key.strip_prefix(COUNTS_KEY) {
            if let Some((_, event)) = ids.split_once(':') {
                let mut counts = body.clone();
                counts["event_id"] = Value::String(event.to_owned());
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_USER_COUNTS_FETCH_SUCCESS", &counts);
            }
            return true;
        }
        if let Some(ids) = key.strip_prefix(USERS_KEY) {
            let Some((guild, event)) = ids.split_once(':') else {
                return true;
            };
            let list = body
                .as_array()
                .or_else(|| body["users"].as_array())
                .cloned()
                .unwrap_or_default();
            let mut answers = Vec::new();
            for mut entry in list {
                if let Some(user) = User::from_value(&entry["user"]) {
                    self.stores.users.entry(user.id).or_insert(user);
                }
                entry["guild_scheduled_event_id"] = Value::String(event.to_owned());
                answers.push(entry);
            }
            self.stores.scheduled_events.dispatch(
                "GUILD_SCHEDULED_EVENT_USERS_FETCH_SUCCESS",
                &Value::Array(answers),
            );
            if let (Ok(guild), Ok(event)) = (guild.parse::<Id>(), event.parse::<Id>()) {
                self.event_prepare_users(guild, &[event]);
            }
            return true;
        }
        false
    }

    /// Whether the user is interested in the event (`isInterestedInEventRecurrence`).
    pub(super) fn event_interested(&self, event: Id) -> bool {
        self.stores.me().is_some_and(|me| {
            self.stores
                .scheduled_events
                .is_interested_in_event_recurrence(event, None, me.id)
        })
    }

    /// `updateRsvp` of a series: the answer is set or taken back at once, then asked of the server; a refusal puts
    /// it back (`RSVP_KEY` answer).
    pub(super) fn toggle_event_interest(&mut self, event: Id, cx: &mut Context<Self>) {
        let (Some(guild), Some(me)) = (
            self.guild_events.as_ref().map(|open| open.guild),
            self.stores.me(),
        ) else {
            return;
        };
        let interested = self.event_interested(event);
        let (method, body, answer) = if interested {
            self.stores
                .scheduled_events
                .remove_rsvp(event, None, me.id, RESPONSE_INTERESTED);
            (reqwest::Method::DELETE, None, 0)
        } else {
            self.stores.scheduled_events.add_rsvp(
                event,
                None,
                EventUser {
                    user_id: me.id,
                    response: RESPONSE_INTERESTED,
                    nick: None,
                },
            );
            (
                reqwest::Method::PUT,
                Some(serde_json::json!({"response": RESPONSE_INTERESTED})),
                1,
            )
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{RSVP_KEY}{guild}:{event}:{answer}")),
            method,
            route: format!("/guilds/{guild}/scheduled-events/{event}/users/@me"),
            body,
        });
        cx.notify();
    }

    /// The answer to an interested request: a refusal undoes what was shown.
    pub(super) fn event_rsvp_answered(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        let Some(ids) = key.strip_prefix(RSVP_KEY) else {
            return;
        };
        if body["ok"].as_bool() == Some(true) {
            return;
        }
        let mut parts = ids.split(':');
        let (Some(_guild), Some(event), Some(answer), Some(me)) = (
            parts.next(),
            parts.next().and_then(|event| event.parse::<Id>().ok()),
            parts.next(),
            self.stores.me(),
        ) else {
            return;
        };
        if answer == "1" {
            self.stores
                .scheduled_events
                .remove_rsvp(event, None, me.id, RESPONSE_INTERESTED);
        } else {
            self.stores.scheduled_events.add_rsvp(
                event,
                None,
                EventUser {
                    user_id: me.id,
                    response: RESPONSE_INTERESTED,
                    nick: None,
                },
            );
        }
        cx.notify();
    }

    /// The Interested button of the card and the detail: green with a check when set, else secondary with a bell.
    /// A finished event shows "Event is over", disabled.
    pub(super) fn interested_button(
        &self,
        id: &'static str,
        event: &ScheduledEvent,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = event.id;
        if matches!(event.status, STATUS_COMPLETED | STATUS_CANCELED) {
            return action_button(id, size, Look::Secondary, "", Some("Event is over"), true)
                .into_any_element();
        }
        let interested = self.event_interested(target);
        action_button(
            id,
            size,
            if interested {
                Look::Connected
            } else {
                Look::Secondary
            },
            if interested { "event_check" } else { "bell" },
            Some("Interested"),
            false,
        )
        .parity_with_border(id, 1.)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_event_interest(target, cx);
        }))
        .into_any_element()
    }

    /// The "..." button; the menu it opens sits at the click.
    pub(super) fn event_more_button(
        &self,
        id: &'static str,
        event: Id,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        action_button(id, size, Look::Secondary, "more_horizontal", None, false)
            .parity_with_border(id, 1.)
            .on_click(cx.listener(move |this, click: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                this.open_event_menu(event, click.position(), cx);
            }))
            .into_any_element()
    }

    pub(super) fn event_copy_link_button(
        &self,
        id: &'static str,
        event: Id,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        action_button(
            id,
            size,
            Look::Secondary,
            "share_link",
            Some("Copy Link"),
            false,
        )
        .parity_with_border(id, 1.)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.copy_event_link(event, cx);
        }))
        .into_any_element()
    }

    /// The place line of an event: its icon and name, an outside location or the channel.
    pub(super) fn event_place(&self, event: &ScheduledEvent) -> (&'static str, String) {
        if event.entity_type == ENTITY_EXTERNAL {
            return ("event_location", event.location.clone().unwrap_or_default());
        }
        let channel = event.channel_id.and_then(|id| {
            self.stores
                .guild(event.guild_id)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
                .map(|channel| channel.name.clone())
        });
        (
            if event.entity_type == ENTITY_STAGE_INSTANCE {
                "stage_channel"
            } else {
                "channel_type_voice"
            },
            channel.unwrap_or_default(),
        )
    }

    /// One card of the list (`card__wH91U`).
    pub(super) fn event_card(
        &self,
        event: &ScheduledEvent,
        now: NaiveDateTime,
        first: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = event.id;
        let status = event_status(event, now, false);
        let creator = event
            .creator_id
            .and_then(|creator| self.event_user(creator));
        let count = self.stores.scheduled_events.get_user_count(id, None);
        let (place_icon, place) = self.event_place(event);
        let mut card = div()
            .id(("event-card", id))
            .w(px(554.))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .hover(|style| style.bg(color("--background-surface-highest")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open_event_detail(id, cx)));
        if first {
            card = card.parity_with_border("events-card", 1.);
        }
        let mut status_line = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(20.))
            .children(status.as_ref().map(status_row))
            .child(div().flex_1())
            .child(self.event_avatar(creator.as_ref(), 20.))
            .child(
                div()
                    .ml(px(8.))
                    .h(px(20.))
                    .px(px(8.))
                    .rounded(px(12.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("event_rsvp", 12., "--text-default"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(count.to_string()),
                    ),
            );
        if first {
            status_line = status_line.parity("events-card-status");
        }
        // Module 567550 `S`: a channel the viewer may enter is a link (`channelContainerEnabled__wH91U`: underlined,
        // `--interactive-text-active` on hover) that joins it (`onJoinClick` of module 482857, given while the viewer
        // may connect); a private one tells event managers who can see the event (tooltip on the icon).
        let place_channel = event.channel_id.and_then(|id| {
            self.stores
                .guild(event.guild_id)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
        });
        // Events take place in voice channels and Stages, the only channels `onJoinClick` enters.
        let joinable = place_channel.filter(|channel| {
            matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13))
                && self.my_permission(event.guild_id, Some(channel), permissions::CONNECT)
        });
        let private_note = joinable.is_some_and(|channel| {
            let manager = self
                .stores
                .me()
                .zip(self.stores.guild(event.guild_id))
                .is_some_and(|(me, known)| can_manage(known, me, event));
            manager
                && self
                    .stores
                    .guild(event.guild_id)
                    .is_some_and(|known| permissions::is_private(known, channel))
        });
        let join_target = joinable.map(|channel| (event.guild_id, channel.id));
        // A pending member (membership screening, `H` of module 51758) who may not connect: the row still answers a
        // click with the screening (`Ze`), under "Missing permission to connect." (`channelContainerDisabled__wH91U`).
        let screening_only = joinable.is_none()
            && place_channel.is_some()
            && self
                .stores
                .guild(event.guild_id)
                .is_some_and(permissions::pending_gate);
        let place_icon_element = div().child(icon(place_icon, 20., "--text-default"));
        let place_icon_element = if private_note {
            self.with_tip(
                format!("event-card-private-tip-{id}"),
                Tip::text(dc_intl::t("48WXaW"), Side::Top),
                place_icon_element,
                cx,
            )
            .into_any_element()
        } else {
            place_icon_element.into_any_element()
        };
        let place_row = div()
            .id(("event-card-place", id))
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w(px(0.))
            .text_color(color("--text-default"))
            .when_some(join_target, |this, (guild, channel)| {
                this.cursor_pointer()
                    .hover(|style| {
                        style
                            .text_color(color("--interactive-text-active"))
                            .underline()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.request_join_voice(guild, channel, cx);
                    }))
            })
            .when(screening_only, |this| {
                this.cursor_not_allowed()
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_screening(cx);
                    }))
            })
            .child(place_icon_element)
            .child(
                div()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .child(place),
            );
        let place_row = if screening_only {
            self.with_tip(
                format!("event-card-connect-tip-{id}"),
                Tip::text(dc_intl::t("nHjY9C"), Side::Top),
                place_row,
                cx,
            )
            .into_any_element()
        } else {
            place_row.into_any_element()
        };
        let footer = div()
            .mt(px(12.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_between()
            .child(place_row)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.event_more_button("event-card-more", id, Size::Sm, cx))
                    .child(self.event_copy_link_button("event-card-copy", id, Size::Sm, cx))
                    .child(self.interested_button("event-card-interested", event, Size::Sm, cx)),
            );
        card.child(
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .child(status_line)
                .child(
                    div()
                        .mt(px(12.))
                        .child(
                            div()
                                .h(px(24.))
                                .truncate()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(event.name.clone()),
                        )
                        .children(
                            event
                                .description
                                .clone()
                                .filter(|text| !text.is_empty())
                                .map(|text| {
                                    div()
                                        .mt(px(8.))
                                        .line_clamp(2)
                                        .text_ellipsis()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(text)
                                }),
                        ),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .mx(px(-16.))
                        .h(px(1.))
                        .bg(color("--border-subtle")),
                )
                .child(footer),
        )
        .into_any_element()
    }
}
